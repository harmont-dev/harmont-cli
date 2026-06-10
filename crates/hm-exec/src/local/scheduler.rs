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
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    /// Host-side workspace path produced by this step's runner, if any.
    /// The scheduler propagates this to downstream `BuildsIn` children
    /// so they can COW-copy instead of re-extracting. By construction it
    /// is always a run-owned kept tempdir of a step that executed this
    /// run with exit 0; cache hits, skips and failures carry `None`.
    /// The dir is deleted as soon as the step's last `BuildsIn` child
    /// finishes (refcounted), with an end-of-run sweep as backstop.
    workspace_dir: Option<String>,
    /// True when the snapshot is ephemeral and must be cleaned up after
    /// all downstream steps finish.
    ephemeral_snapshot: bool,
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
    parallelism: NonZeroUsize,
    runner_registry: Arc<RunnerRegistry>,
    tx: tokio::sync::mpsc::Sender<BuildEvent>,
    cancel: CancellationToken,
    vm: Option<Arc<hm_vm::HmVm>>,
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
        parent_workspace_dir: None,
        source_base: Arc::new(tokio::sync::OnceCell::new()),
    };

    let semaphore = Arc::new(tokio::sync::Semaphore::new(parallelism.get()));

    let dag = graph.dag();
    let pipeline_timeout = graph.timeout_seconds();
    let chain_info = compute_chain_info(dag);

    let order = toposort(dag.graph(), None).map_err(|c| {
        crate::BackendError::Local(format!("pipeline graph has a cycle at {:?}", c.node_id()))
    })?;

    // Per-node refcount of `BuildsIn` children: the only consumers of a
    // step's kept workspace dir. Each child decrements its parent's count
    // when it finishes (whether it ran, was skipped, or failed); the child
    // that drops the count to zero deletes the parent's dir. Steps with no
    // `BuildsIn` children free their own dir immediately. This caps the
    // run's temp-space footprint at the live DAG frontier instead of
    // accumulating one workspace copy per executed step until the end of
    // the run (full byte copies on non-reflink filesystems, RAM on
    // tmpfs-mounted /tmp).
    let ws_consumers: HashMap<NodeIndex, Arc<AtomicUsize>> = order
        .iter()
        .map(|&n| {
            let count = dag
                .children(n)
                .iter(dag)
                .filter(|(e, _)| dag.edge_weight(*e).copied() == Some(EdgeKind::BuildsIn))
                .count();
            (n, Arc::new(AtomicUsize::new(count)))
        })
        .collect();

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
        // (edge kind, parent future, parent's workspace-consumer refcount)
        let preds: Vec<(EdgeKind, StepFuture, Arc<AtomicUsize>)> = dag
            .parents(n)
            .iter(dag)
            .map(|(e, p)| {
                (
                    *dag.edge_weight(e).expect("edge in DAG"),
                    done[&p].clone(),
                    Arc::clone(&ws_consumers[&p]),
                )
            })
            .collect();
        let own_ws_consumers = Arc::clone(&ws_consumers[&n]);

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
                join_all(preds.iter().map(|(_, f, _)| f.clone())).await;

            // Run the step (or short-circuit). All exit paths of this inner
            // block flow into the workspace refcount release below.
            let outcome = async {
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
                            key: node_key.clone(),
                            status,
                            exit_code: None,
                            duration_ms: 0,
                        }),
                        ephemeral_snapshot: false,
                        workspace_dir: None,
                    };
                }

                // Acquire parallelism permit.
                let _permit = sem
                    .acquire_owned()
                    .await
                    .expect("semaphore closed unexpectedly");

                // Find the BuildsIn parent's snapshot and workspace dir for
                // container lineage and COW workspace propagation.
                let (parent_snapshot, parent_workspace_dir) = preds
                    .iter()
                    .zip(&pred_outcomes)
                    .find(|((ek, _, _), _)| *ek == EdgeKind::BuildsIn)
                    .map_or((None, None), |(_, outcome)| {
                        (outcome.snapshot.clone(), outcome.workspace_dir.clone())
                    });

                let mut step_ctx = run_ctx.clone();
                step_ctx.parent_workspace_dir = parent_workspace_dir;

                match execute_step(
                    n,
                    transition,
                    parent_snapshot,
                    chain_id,
                    chain_pos,
                    parent_key,
                    archive_id,
                    run_id,
                    step_ctx,
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
                                key: node_key.clone(),
                                status: StepStatus::Failed,
                                exit_code: Some(1),
                                duration_ms: 0,
                            }),
                            workspace_dir: None,
                            ephemeral_snapshot: false,
                        }
                    }
                }
            }
            .await;

            // This step is done with its parents' workspaces (the COW copy,
            // if any, happened inside the runner). Decrement each BuildsIn
            // parent's consumer count; the last child to finish deletes the
            // parent's kept dir so temp space tracks the live DAG frontier.
            for ((kind, _, counter), pred_outcome) in preds.iter().zip(&pred_outcomes) {
                if *kind == EdgeKind::BuildsIn
                    && counter.fetch_sub(1, Ordering::AcqRel) == 1
                    && let Some(ws) = pred_outcome.workspace_dir.clone()
                {
                    tokio::task::spawn_blocking(move || std::fs::remove_dir_all(ws).ok())
                        .await
                        .ok();
                }
            }
            // No BuildsIn children will ever read this step's workspace:
            // free it now. (Children, if any, observe this outcome only
            // after this future resolves, so the load cannot race a
            // decrement.)
            if own_ws_consumers.load(Ordering::Acquire) == 0
                && let Some(ws) = outcome.workspace_dir.clone()
            {
                tokio::task::spawn_blocking(move || std::fs::remove_dir_all(ws).ok())
                    .await
                    .ok();
            }

            outcome
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
    let timed_out = if let Some(secs) = pipeline_timeout {
        let join_fut = join_all(pending.clone());
        tokio::pin!(join_fut);
        tokio::select! {
            _ = &mut join_fut => false,
            () = tokio::time::sleep(Duration::from_secs(u64::from(secs.get()))) => {
                // Whole-build budget blown: signal every step to stop. New
                // steps short-circuit via the `cancel.is_cancelled()` check
                // in the spawn closure; in-flight runners observe
                // run_ctx.cancel.
                cancel.cancel();
                true
            }
        }
    } else {
        let _ = join_all(pending.clone()).await;
        false
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
            timeout_seconds = ?pipeline_timeout,
            "pipeline wall-clock timeout exceeded; build failed"
        );
    }

    let steps: Vec<StepResultSummary> = outcomes.iter().filter_map(|o| o.summary.clone()).collect();

    // Clean up ephemeral Docker snapshots and kept temp workspace dirs.
    // Workspace state is strictly run-scoped: every `Some(workspace_dir)`
    // names a tempdir kept alive (TempDir::keep) by a step that executed
    // this run so children could COW-copy from it. Most dirs were already
    // freed incrementally by the last-consumer refcount above; this pass is
    // a backstop (`remove_dir_all` on an already-deleted path is a no-op)
    // now that all steps have drained.
    for outcome in &outcomes {
        if outcome.ephemeral_snapshot
            && let (Some(vm), Some(snap)) = (vm.as_ref(), outcome.snapshot.as_ref())
        {
            // Guarded removal: a demoted-to-ephemeral `harmont-cache/*` tag
            // may have been re-registered by a concurrent run since this
            // step marked it ephemeral; destroying it would kill that run's
            // live cache entry.
            vm.remove_snapshot_unless_registered(&hm_vm::SnapshotId::new(snap.0.clone()))
                .await;
        }
        if let Some(ref ws) = outcome.workspace_dir {
            let ws = ws.clone();
            tokio::task::spawn_blocking(move || std::fs::remove_dir_all(ws).ok())
                .await
                .ok();
        }
    }

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

    // Give the runner a step-scoped cancellation token (a child of the
    // build token, so build-level cancellation still propagates). The
    // per-step timeout FIRES this token instead of dropping the runner
    // future: the runner tears down cooperatively — the VM layer destroys
    // the container and reclaims bind-mount ownership of root-written
    // files before the workspace tempdir is touched — so a timed-out step
    // can never leak a workspace dir or race the host-side cleanup.
    let mut run_ctx = run_ctx;
    let step_cancel = run_ctx.cancel.child_token();
    run_ctx.cancel = step_cancel.clone();

    let exec = runner.execute(&run_ctx, input);
    let (result, step_timed_out): (anyhow::Result<StepResult>, bool) = match step_timeout_secs {
        Some(secs) => {
            tokio::pin!(exec);
            tokio::select! {
                r = &mut exec => (r, false),
                () = tokio::time::sleep(Duration::from_secs(u64::from(secs.get()))) => {
                    step_cancel.cancel();
                    // Await the cooperative teardown to completion; never
                    // drop the in-flight future.
                    (exec.await, true)
                }
            }
        }
        _ => (exec.await, false),
    };

    if step_timed_out {
        // Per-step wall-clock budget exceeded. Emit a step-end with the
        // conventional timeout exit code (124), fail the chain, and cancel
        // siblings — same shape as a non-zero exit below. Whatever the
        // post-cancel teardown returned is superseded by the timeout
        // verdict, but any resources it reports (a kept workspace dir or
        // an ephemeral snapshot, if the step happened to finish in the
        // cancellation race window) are carried into the outcome so the
        // scheduler's cleanup passes reclaim them.
        let dur_ms = started.elapsed().as_millis() as u64;
        let (snapshot, workspace_dir, ephemeral_snapshot) = match result {
            Ok(sr) => (
                sr.committed_snapshot,
                sr.workspace_dir,
                sr.ephemeral_snapshot,
            ),
            Err(_) => (None, None, false),
        };
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
            message: format!(
                "step '{step_key}' timed out after {}s",
                step_timeout_secs.map_or(0, std::num::NonZeroU32::get)
            ),
            ts: chrono::Utc::now(),
        });
        if !keep_going {
            cancel.cancel();
        }
        return Ok(StepOutcome {
            exit_code: 124,
            snapshot,
            summary: Some(StepResultSummary {
                step_id,
                key: step_key.clone(),
                status: StepStatus::TimedOut,
                exit_code: Some(124),
                duration_ms: dur_ms,
            }),
            workspace_dir,
            ephemeral_snapshot,
        });
    }

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
                workspace_dir: sr.workspace_dir,
                ephemeral_snapshot: sr.ephemeral_snapshot,
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::local::runner::{StepContext, StepRunner};
    use hm_plugin_protocol::{ExecutorInput, StepResult};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;

    /// Runner stub that materializes a kept tempdir per step (mirroring the
    /// VM runner's workspace contract) and records, at the start of each
    /// step, which previously produced workspace dirs still exist on disk.
    /// This makes the scheduler's incremental workspace reclamation
    /// observable mid-run.
    #[derive(Debug, Default)]
    struct WorkspaceProbeRunner {
        /// `(step key, kept workspace dir)` in execution order.
        dirs: Mutex<Vec<(String, PathBuf)>>,
        /// step key -> keys of earlier steps whose dirs were still on disk
        /// when this step started.
        observed: Mutex<HashMap<String, Vec<String>>>,
    }

    impl StepRunner for WorkspaceProbeRunner {
        fn name(&self) -> &'static str {
            "probe"
        }

        fn execute(
            &self,
            _ctx: &StepContext,
            input: ExecutorInput,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<StepResult>> + Send + '_>> {
            Box::pin(async move {
                let key = input.step.key.clone();
                let alive: Vec<String> = self
                    .dirs
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, p)| p.exists())
                    .map(|(k, _)| k.clone())
                    .collect();
                self.observed.lock().unwrap().insert(key.clone(), alive);

                let ws = tempfile::tempdir().unwrap().keep();
                self.dirs.lock().unwrap().push((key, ws.clone()));
                Ok(StepResult {
                    exit_code: 0,
                    committed_snapshot: None,
                    artifacts: vec![],
                    workspace_dir: Some(ws.display().to_string()),
                    ephemeral_snapshot: false,
                })
            })
        }
    }

    /// Build a [`PipelineGraph`] from step keys plus `(from, to)` `BuildsIn`
    /// edges (indices into `keys`).
    fn graph_with_edges(keys: &[&str], edges: &[(usize, usize)]) -> PipelineGraph {
        let nodes: Vec<serde_json::Value> = keys
            .iter()
            .map(|k| serde_json::json!({ "step": { "key": k, "cmd": "true" }, "env": {} }))
            .collect();
        let edges: Vec<serde_json::Value> = edges
            .iter()
            .map(|(a, b)| serde_json::json!([a, b, "builds_in"]))
            .collect();
        serde_json::from_value(serde_json::json!({
            "version": "0",
            "graph": {
                "nodes": nodes,
                "node_holes": [],
                "edge_property": "directed",
                "edges": edges,
            }
        }))
        .unwrap()
    }

    async fn run_probe(
        graph: PipelineGraph,
        runner: Arc<WorkspaceProbeRunner>,
    ) -> crate::BuildOutcome {
        let repo = tempfile::tempdir().unwrap();
        std::fs::write(repo.path().join("marker.txt"), "v1").unwrap();
        let mut registry = RunnerRegistry::new();
        registry.register(runner, true);
        // Hold `_rx` so the event forwarder keeps a live receiver; the
        // handful of events a tiny pipeline emits fits the channel.
        let (tx, _rx) = tokio::sync::mpsc::channel(1024);
        run(
            graph,
            repo.path().to_path_buf(),
            "test-pipeline".into(),
            NonZeroUsize::new(1).unwrap(),
            Arc::new(registry),
            tx,
            CancellationToken::new(),
            None,
            false,
        )
        .await
        .unwrap()
    }

    /// Chain a -> b -> c: a's kept workspace must be deleted as soon as its
    /// only `BuildsIn` child (b) finishes — i.e. before c starts — not at the
    /// end of the run. This caps temp-space at the live DAG frontier.
    #[tokio::test]
    async fn chain_frees_parent_workspace_when_last_child_finishes() {
        let runner = Arc::new(WorkspaceProbeRunner::default());
        let graph = graph_with_edges(&["a", "b", "c"], &[(0, 1), (1, 2)]);

        let outcome = run_probe(graph, Arc::clone(&runner)).await;
        assert_eq!(outcome.status, crate::BuildStatus::Passed);

        let observed = runner.observed.lock().unwrap().clone();
        // b starts while a's dir is alive (it COWs from it)...
        assert_eq!(observed["b"], vec!["a".to_owned()]);
        // ...but by the time c starts, b (a's last consumer) has finished
        // and a's dir is already gone. Only b's dir is alive.
        assert_eq!(observed["c"], vec!["b".to_owned()]);

        // Backstop: nothing survives the run.
        for (_, dir) in runner.dirs.lock().unwrap().iter() {
            assert!(!dir.exists(), "workspace dir leaked: {}", dir.display());
        }
    }

    /// Fork a -> {b, c}: the first child to finish must NOT free a's dir
    /// (its sibling still needs it); the last one does. A leaf's own dir
    /// (b has no `BuildsIn` children) is freed as soon as the leaf finishes.
    #[tokio::test]
    async fn fork_frees_parent_workspace_only_after_last_sibling() {
        let runner = Arc::new(WorkspaceProbeRunner::default());
        let graph = graph_with_edges(&["a", "b", "c"], &[(0, 1), (0, 2)]);

        let outcome = run_probe(graph, Arc::clone(&runner)).await;
        assert_eq!(outcome.status, crate::BuildStatus::Passed);

        let observed = runner.observed.lock().unwrap().clone();
        let exec_order: Vec<String> = runner
            .dirs
            .lock()
            .unwrap()
            .iter()
            .map(|(k, _)| k.clone())
            .collect();
        assert_eq!(exec_order[0], "a");
        // The first sibling sees a alive. The second sibling must STILL see
        // a alive: it is a's remaining consumer, so the first sibling's
        // completion must not have freed a's dir. (The first sibling's own
        // leaf dir is reclaimed concurrently with the second sibling's
        // start — the permit is released before the cleanup runs — so no
        // assertion is made about it mid-run; the leak check below covers
        // it.)
        assert_eq!(observed[&exec_order[1]], vec!["a".to_owned()]);
        assert!(observed[&exec_order[2]].contains(&"a".to_owned()));

        for (_, dir) in runner.dirs.lock().unwrap().iter() {
            assert!(!dir.exists(), "workspace dir leaked: {}", dir.display());
        }
    }

    /// Runner that hangs until its step-scoped cancellation token fires,
    /// then performs (simulated) teardown work before returning — mirroring
    /// the VM runner, whose post-cancel path destroys the container and
    /// reclaims workspace ownership before resolving.
    #[derive(Debug, Default)]
    struct CooperativeHangRunner {
        /// Keys of steps whose futures ran to completion (were awaited
        /// through teardown rather than dropped).
        torn_down: Mutex<Vec<String>>,
    }

    impl StepRunner for CooperativeHangRunner {
        fn name(&self) -> &'static str {
            "hang"
        }

        fn execute(
            &self,
            ctx: &StepContext,
            input: ExecutorInput,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<StepResult>> + Send + '_>> {
            let cancel = ctx.cancel.clone();
            Box::pin(async move {
                #[allow(
                    clippy::duration_suboptimal_units,
                    reason = "from_hours is nightly-only"
                )]
                const HANG: Duration = Duration::from_secs(3600);
                tokio::select! {
                    () = cancel.cancelled() => {}
                    () = tokio::time::sleep(HANG) => {}
                }
                // Teardown must be awaited by the scheduler, never cut
                // short by a dropped future.
                tokio::time::sleep(Duration::from_millis(100)).await;
                self.torn_down.lock().unwrap().push(input.step.key.clone());
                Ok(StepResult {
                    exit_code: 130,
                    committed_snapshot: None,
                    artifacts: vec![],
                    workspace_dir: None,
                    ephemeral_snapshot: false,
                })
            })
        }
    }

    /// A per-step timeout must cancel the runner COOPERATIVELY and await
    /// its teardown to completion (dropping the in-flight future would
    /// race the container's bind-mount ownership reclaim and leak the
    /// workspace dir), while still reporting the step as timed out.
    #[tokio::test]
    async fn step_timeout_awaits_cooperative_teardown() {
        let runner = Arc::new(CooperativeHangRunner::default());
        let graph: PipelineGraph = serde_json::from_value(serde_json::json!({
            "version": "0",
            "graph": {
                "nodes": [{
                    "step": { "key": "slow", "cmd": "true", "timeout_seconds": 1 },
                    "env": {}
                }],
                "node_holes": [],
                "edge_property": "directed",
                "edges": [],
            }
        }))
        .unwrap();

        let repo = tempfile::tempdir().unwrap();
        std::fs::write(repo.path().join("marker.txt"), "v1").unwrap();
        let mut registry = RunnerRegistry::new();
        registry.register(Arc::clone(&runner) as Arc<dyn StepRunner>, true);
        let (tx, _rx) = tokio::sync::mpsc::channel(1024);
        let outcome = run(
            graph,
            repo.path().to_path_buf(),
            "test-pipeline".into(),
            NonZeroUsize::new(1).unwrap(),
            Arc::new(registry),
            tx,
            CancellationToken::new(),
            None,
            false,
        )
        .await
        .unwrap();

        // The runner's future was awaited through its teardown...
        assert_eq!(
            runner.torn_down.lock().unwrap().clone(),
            vec!["slow".to_owned()]
        );
        // ...and the step is still reported as timed out.
        assert_eq!(outcome.steps.len(), 1);
        assert_eq!(outcome.steps[0].status, StepStatus::TimedOut);
        assert_eq!(outcome.steps[0].exit_code, Some(124));
    }
}
