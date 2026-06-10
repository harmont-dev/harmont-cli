//! Dataflow scheduler.
//!
//! Walks the pipeline DAG in topological order, spawning a shared
//! future per step. Each future awaits its predecessors, acquires a
//! parallelism permit, and dispatches the step to its registered
//! runner (VM by default).

// Pedantic-bucket nags accepted at module scope:
// - `cast_possible_truncation`: every `as u64` here is a millisecond
//   wall-clock duration; `u128 -> u64` cannot overflow for any
//   conceivable build runtime (584 million years).
// - `expect_used`: semaphore acquire and DAG edge-weight lookups on
//   edges that are guaranteed to exist by construction.
// - `too_many_lines` on `run`: setup + dataflow loop form one
//   cohesive unit; splitting would obscure the spawn/join symmetry.
// - `missing_panics_doc`: the only panic paths are the semaphore and
//   edge-weight expects described above.
#![allow(
    clippy::cast_possible_truncation,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::missing_panics_doc
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use daggy::petgraph::algo::toposort;
use daggy::{Dag, NodeIndex, Walker};
use futures::future::{BoxFuture, FutureExt, join_all};

use anyhow::Context as _;
use hm_plugin_protocol::events::BuildRef;
use hm_plugin_protocol::{
    ArchiveId, BuildEvent, CacheDecision, ExecutorInput, PlanSummary, SnapshotRef, StepResult,
};
use uuid::Uuid;

use hm_pipeline_ir::{EdgeKind, PipelineGraph, Transition};

use crate::local::runner::{RunnerRegistry, StepContext};
use crate::local::source::build_archive_bytes;
use crate::{BuildOutcome, BuildStatus, StepResultSummary, StepStatus};

use super::archive::ArchiveStore;
use super::cache;
use super::events::EventBus;
use tokio_util::sync::CancellationToken;

/// What one finished step contributes to the scheduler's bookkeeping:
/// the snapshot it produced (for downstream container lineage) plus a
/// terminal [`StepResultSummary`] for the run's [`BuildOutcome`].
#[derive(Clone)]
struct StepOutcome {
    exit_code: i32,
    snapshot: Option<SnapshotRef>,
    /// `None` only for steps short-circuited because a predecessor failed
    /// or the build was cancelled before they could run.
    summary: Option<StepResultSummary>,
}

type StepFuture = futures::future::Shared<BoxFuture<'static, StepOutcome>>;

/// Entry point: run a parsed pipeline locally end-to-end.
///
/// Emits every [`BuildEvent`] to `tx` (via an internal broadcast bus that
/// the many concurrent step tasks publish to) and returns a typed
/// [`BuildOutcome`]. Non-zero step exit codes are reflected in the outcome's
/// [`BuildStatus`], not surfaced as an `Err`.
///
/// `cancel` is supplied by the caller (the CLI owns Ctrl-C handling); the
/// scheduler observes it cooperatively and never installs a signal handler.
///
/// # Errors
/// Returns an error if the source archive cannot be built or any
/// scheduler-level failure occurs. Non-zero step exit codes are
/// surfaced via the returned [`BuildOutcome`], not as an `Err`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run(
    graph: PipelineGraph,
    repo_root: PathBuf,
    pipeline_slug: String,
    parallelism: usize,
    runner_registry: Arc<RunnerRegistry>,
    tx: tokio::sync::mpsc::Sender<BuildEvent>,
    cancel: CancellationToken,
    keep_going: bool,
) -> crate::Result<BuildOutcome> {
    // Set up per-run state.
    let bus = EventBus::new();
    let archives = Arc::new(ArchiveStore::new());

    // Forward every bus event onto the caller's mpsc channel. The bus is a
    // lossy broadcast that the concurrent step tasks emit into; the mpsc
    // forward gives the renderer backpressure. If the renderer goes away
    // (`tx` closed) we stop forwarding; a lagging subscriber drops events
    // but the build keeps running.
    let forward = {
        let mut sub = bus.subscribe();
        let tx = tx.clone();
        tokio::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match sub.recv().await {
                    Ok(ev) => {
                        // Renderer went away: stop forwarding.
                        if tx.send(ev).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                    // Lossy broadcast: a slow renderer drops events but the
                    // build keeps running. Skip the gap and keep forwarding.
                    Err(RecvError::Lagged(_)) => {}
                }
            }
        })
    };

    let run_id = Uuid::new_v4();

    // Build the source archive once.
    let archive_bytes = build_archive_bytes(&repo_root)
        .context("build source archive")
        .map_err(|e| crate::BackendError::Local(format!("{e:#}")))?;
    let archive_id = archives.register(archive_bytes);

    let run_ctx = StepContext {
        event_bus: bus.clone(),
        archives: archives.clone(),
        cancel: cancel.clone(),
    };

    let parallelism = parallelism.max(1);

    let semaphore = Arc::new(tokio::sync::Semaphore::new(parallelism));

    let dag = graph.dag();
    let pipeline_timeout = graph.timeout_seconds();
    let chain_info = compute_chain_info(dag);

    let order = toposort(dag.graph(), None).map_err(|c| {
        crate::BackendError::Local(format!("pipeline graph has a cycle at {:?}", c.node_id()))
    })?;

    let started_at = chrono::Utc::now();
    bus.emit(BuildEvent::BuildStart {
        run_id,
        plan: PlanSummary {
            step_count: graph.node_count(),
            chain_count: chain_info.chain_count,
            default_runner: runner_registry.default_runner_name().unwrap_or("vm").into(),
        },
        started_at,
    });

    let started_total = Instant::now();

    let mut done: HashMap<NodeIndex, StepFuture> = HashMap::new();

    for &n in &order {
        let preds: Vec<(EdgeKind, StepFuture)> = dag
            .parents(n)
            .iter(dag)
            .map(|(e, p)| (*dag.edge_weight(e).expect("edge in DAG"), done[&p].clone()))
            .collect();

        let transition = dag[n].clone();
        let node_key = transition.step.key.clone();
        let chain_id = chain_info.node_chain_id[&n];
        let chain_pos = chain_info.node_chain_pos[&n];
        let parent_key: Option<String> = dag
            .parents(n)
            .iter(dag)
            .find(|(e, _)| dag.edge_weight(*e).copied() == Some(EdgeKind::BuildsIn))
            .map(|(_, p)| dag[p].step.key.clone());
        let sem = semaphore.clone();
        let reg = runner_registry.clone();
        let bus = bus.clone();
        let cancel = cancel.clone();
        let run_ctx = run_ctx.clone();

        let fut: StepFuture = async move {
            // Await all predecessors.
            let pred_outcomes: Vec<StepOutcome> =
                join_all(preds.iter().map(|(_, f)| f.clone())).await;

            // Early exit if any predecessor failed or the build was cancelled.
            if cancel.is_cancelled() || pred_outcomes.iter().any(|o| o.exit_code != 0) {
                let status = if cancel.is_cancelled() {
                    StepStatus::Canceled
                } else {
                    StepStatus::Skipped
                };
                return StepOutcome {
                    exit_code: 0,
                    snapshot: None,
                    summary: Some(StepResultSummary {
                        step_id: Uuid::new_v4(),
                        key: node_key,
                        status,
                        exit_code: None,
                        duration_ms: 0,
                    }),
                };
            }

            // Acquire parallelism permit.
            let _permit = sem
                .acquire_owned()
                .await
                .expect("semaphore closed unexpectedly");

            // Find the BuildsIn parent's snapshot for container lineage.
            let parent_snapshot = preds
                .iter()
                .zip(&pred_outcomes)
                .find(|((ek, _), _)| *ek == EdgeKind::BuildsIn)
                .and_then(|(_, outcome)| outcome.snapshot.clone());

            match execute_step(
                n,
                transition,
                parent_snapshot,
                chain_id,
                chain_pos,
                parent_key,
                archive_id,
                run_id,
                run_ctx,
                reg,
                bus,
                cancel,
                keep_going,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(e) => {
                    tracing::error!(%e, "step execution failed");
                    StepOutcome {
                        exit_code: 1,
                        snapshot: None,
                        summary: Some(StepResultSummary {
                            step_id: Uuid::new_v4(),
                            key: node_key,
                            status: StepStatus::Failed,
                            exit_code: Some(1),
                            duration_ms: 0,
                        }),
                    }
                }
            }
        }
        .boxed()
        .shared();

        tokio::spawn(fut.clone());
        done.insert(n, fut);
    }

    // The step futures are Shared + already spawned, so we can await the join
    // set twice: once racing the deadline (to fire cancellation promptly), then
    // again to drain every step to completion before tearing down.
    let pending: Vec<StepFuture> = done.into_values().collect();
    let timed_out = match pipeline_timeout {
        Some(secs) if secs > 0 => {
            let join_fut = join_all(pending.clone());
            tokio::pin!(join_fut);
            tokio::select! {
                _ = &mut join_fut => false,
                () = tokio::time::sleep(Duration::from_secs(u64::from(secs))) => {
                    // Whole-build budget blown: signal every step to stop. New
                    // steps short-circuit via the `cancel.is_cancelled()` check
                    // in the spawn closure; in-flight runners observe
                    // run_ctx.cancel.
                    cancel.cancel();
                    true
                }
            }
        }
        _ => {
            let _ = join_all(pending.clone()).await;
            false
        }
    };
    let outcomes: Vec<StepOutcome> = join_all(pending).await;
    let any_failed = outcomes.iter().any(|o| o.exit_code != 0);

    // Derive the overall verdict. Timeout wins (it also fired cancellation);
    // then cancellation; then any failed step; otherwise the build passed.
    let status = if timed_out {
        BuildStatus::TimedOut
    } else if cancel.is_cancelled() {
        BuildStatus::Canceled
    } else if any_failed {
        BuildStatus::Failed
    } else {
        BuildStatus::Passed
    };

    if timed_out {
        tracing::warn!(
            timeout_seconds = pipeline_timeout,
            "pipeline wall-clock timeout exceeded; build failed"
        );
    }

    let steps: Vec<StepResultSummary> = outcomes.iter().filter_map(|o| o.summary.clone()).collect();

    let dur = started_total.elapsed().as_millis() as u64;

    bus.emit(BuildEvent::BuildEnd {
        exit_code: status.exit_code(),
        duration_ms: dur,
    });

    // Drop every remaining bus sender (the template `StepContext` still holds
    // one) so the forwarder observes `Closed` and drains, then await it so the
    // renderer sees `BuildEnd` before we return.
    drop(run_ctx);
    drop(bus);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), forward).await;

    Ok(BuildOutcome {
        build: BuildRef {
            run_id,
            number: None,
            org: None,
            pipeline: pipeline_slug,
        },
        status,
        steps,
        started_at,
        finished_at: chrono::Utc::now(),
        watch_url: None,
    })
}

/// Execute a single step, returning its outcome (exit code + snapshot).
///
/// On cache hit the function returns early with exit code 0 and the
/// cached snapshot so downstream nodes receive the correct
/// `parent_snapshot` without running the runner at all.
///
/// On non-zero exit the cancellation token is cancelled so sibling
/// tasks observe the failure promptly.
#[allow(clippy::too_many_arguments)]
async fn execute_step(
    _node_idx: NodeIndex,
    transition: Transition,
    parent_snapshot: Option<SnapshotRef>,
    chain_id: usize,
    chain_pos: usize,
    parent_key: Option<String>,
    archive_id: ArchiveId,
    run_id: Uuid,
    run_ctx: StepContext,
    runner_registry: Arc<RunnerRegistry>,
    bus: Arc<EventBus>,
    cancel: CancellationToken,
    keep_going: bool,
) -> anyhow::Result<StepOutcome> {
    let step_wire = transition.step;
    let step_key = step_wire.key.clone();
    let display_name = step_wire.label.clone().unwrap_or_else(|| {
        let cmd = step_wire.cmd.trim();
        if cmd.len() <= 40 {
            cmd.to_owned()
        } else {
            format!("{}…", &cmd[..39])
        }
    });
    let env_map = transition.env;
    let step_id = Uuid::new_v4();

    bus.emit(BuildEvent::StepQueued {
        step_id,
        key: step_key.clone(),
        chain_idx: chain_pos,
        parent_key: parent_key.clone(),
        display_name: display_name.clone(),
    });

    // Compute the cache lookup for the runner. The runner (VmRunner)
    // handles cache hit/miss internally via ImageRegistry.
    let cache_tag = cache::stable_cache_tag(&step_wire);
    let cache_lookup = cache_tag
        .as_ref()
        .map_or(CacheDecision::MissNoCommit, |tag| {
            CacheDecision::MissBuildAs {
                tag: SnapshotRef::from(tag.clone()),
            }
        });

    let input = ExecutorInput {
        step: step_wire,
        workspace_archive_id: archive_id,
        env: env_map,
        workdir: "/workspace".to_string(),
        run_id,
        step_id,
        cache_lookup,
        parent_snapshot,
    };

    // Resolve the runner by name. Steps that didn't declare a runner
    // fall back to whichever runner was registered as default (vm).
    let runner_name = input
        .step
        .runner
        .as_deref()
        .or_else(|| runner_registry.default_runner_name())
        .unwrap_or("vm")
        .to_owned();

    // Capture the per-step wall-clock budget before `input` is moved
    // into the runner below.
    let step_timeout_secs = input.step.timeout_seconds;

    let started = Instant::now();
    bus.emit(BuildEvent::StepStart {
        step_id,
        runner: runner_name.clone(),
        image: input.step.image.clone(),
    });

    let available: Vec<String> = runner_registry
        .runner_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let runner = runner_registry
        .resolve(input.step.runner.as_deref())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "step '{}' requested runner '{}', but no runner provides it (available: {:?})",
                input.step.key,
                runner_name,
                available,
            )
        })?;

    let exec = runner.execute(&run_ctx, input);
    let result: anyhow::Result<StepResult> = match step_timeout_secs {
        Some(secs) if secs > 0 => {
            match tokio::time::timeout(Duration::from_secs(u64::from(secs)), exec).await {
                Ok(r) => r,
                Err(_elapsed) => {
                    // Per-step wall-clock budget exceeded. Emit a step-end with the
                    // conventional timeout exit code (124), fail the chain, and
                    // cancel siblings — same shape as a non-zero exit below.
                    let dur_ms = started.elapsed().as_millis() as u64;
                    bus.emit(BuildEvent::StepEnd {
                        step_id,
                        exit_code: 124,
                        duration_ms: dur_ms,
                        snapshot: None,
                    });
                    bus.emit(BuildEvent::ChainFailed {
                        chain_idx: chain_id,
                        failed_step_id: step_id,
                        failed_step_key: step_key.clone(),
                        exit_code: 124,
                        message: format!("step '{step_key}' timed out after {secs}s"),
                        ts: chrono::Utc::now(),
                    });
                    if !keep_going {
                        cancel.cancel();
                    }
                    return Ok(StepOutcome {
                        exit_code: 124,
                        snapshot: None,
                        summary: Some(StepResultSummary {
                            step_id,
                            key: step_key.clone(),
                            status: StepStatus::TimedOut,
                            exit_code: Some(124),
                            duration_ms: dur_ms,
                        }),
                    });
                }
            }
        }
        _ => exec.await,
    };

    let dur_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(sr) => {
            bus.emit(BuildEvent::StepEnd {
                step_id,
                exit_code: sr.exit_code,
                duration_ms: dur_ms,
                snapshot: sr.committed_snapshot.clone(),
            });
            if sr.exit_code != 0 {
                bus.emit(BuildEvent::ChainFailed {
                    chain_idx: chain_id,
                    failed_step_id: step_id,
                    failed_step_key: step_key.clone(),
                    exit_code: sr.exit_code,
                    message: format!("step '{}' exited with code {}", step_key, sr.exit_code),
                    ts: chrono::Utc::now(),
                });
                if !keep_going {
                    cancel.cancel();
                }
            }
            let status = match sr.exit_code {
                0 => StepStatus::Passed,
                // The Docker runner returns 130 when a step is cut short by
                // cooperative cancellation (Ctrl-C / sibling failure).
                130 => StepStatus::Canceled,
                _ => StepStatus::Failed,
            };
            Ok(StepOutcome {
                exit_code: sr.exit_code,
                snapshot: sr.committed_snapshot,
                summary: Some(StepResultSummary {
                    step_id,
                    key: step_key.clone(),
                    status,
                    exit_code: Some(sr.exit_code),
                    duration_ms: dur_ms,
                }),
            })
        }
        Err(e) => {
            bus.emit(BuildEvent::StepEnd {
                step_id,
                exit_code: 1,
                duration_ms: dur_ms,
                snapshot: None,
            });
            Err(e)
        }
    }
}

/// Per-node chain membership used for event enrichment. Maps every
/// node in the DAG to (`chain_id`, `position_within_chain`).
struct ChainInfo {
    chain_count: usize,
    node_chain_id: HashMap<NodeIndex, usize>,
    node_chain_pos: HashMap<NodeIndex, usize>,
}

/// Return the number of linear `BuildsIn` chains in the pipeline DAG.
///
/// This is the authoritative implementation shared by the scheduler and the
/// [`crate::request`] plan-summarizer. See [`compute_chain_info`] for the
/// full per-node mapping used during a live run.
pub(crate) fn chain_count(dag: &Dag<Transition, EdgeKind>) -> usize {
    compute_chain_info(dag).chain_count
}

/// Walk the DAG and assign each node to a linear chain. A chain starts
/// at any node not yet assigned and extends forward through single
/// `BuildsIn` children where the child has exactly one parent total.
/// This mirrors `PipelineGraph::chains()` but lives as a free function
/// operating on the raw `Dag`.
fn compute_chain_info(dag: &Dag<Transition, EdgeKind>) -> ChainInfo {
    let mut node_chain_id: HashMap<NodeIndex, usize> = HashMap::new();
    let mut node_chain_pos: HashMap<NodeIndex, usize> = HashMap::new();
    let mut chain_count: usize = 0;

    // Walk nodes in index order.
    let mut indices: Vec<NodeIndex> = dag.graph().node_indices().collect();
    indices.sort();

    for idx in indices {
        if node_chain_id.contains_key(&idx) {
            continue;
        }

        // Start a new chain rooted at this unvisited node.
        let chain_id = chain_count;
        chain_count += 1;

        let mut cur = idx;
        let mut pos: usize = 0;
        loop {
            node_chain_id.insert(cur, chain_id);
            node_chain_pos.insert(cur, pos);
            pos += 1;

            // Collect BuildsIn children of `cur`.
            let builds_in_children: Vec<NodeIndex> = dag
                .children(cur)
                .iter(dag)
                .filter(|(e, _)| dag.edge_weight(*e).copied() == Some(EdgeKind::BuildsIn))
                .map(|(_, child)| child)
                .collect();

            // Follow the chain only if there's exactly one BuildsIn child...
            if builds_in_children.len() != 1 {
                break;
            }
            let child = builds_in_children[0];

            // ...that hasn't been assigned yet...
            if node_chain_id.contains_key(&child) {
                break;
            }

            // ...and that child has exactly one parent total.
            let parent_count = dag.parents(child).iter(dag).count();
            if parent_count != 1 {
                break;
            }

            cur = child;
        }
    }

    ChainInfo {
        chain_count,
        node_chain_id,
        node_chain_pos,
    }
}
