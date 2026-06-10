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

use std::collections::{BTreeMap, HashMap};
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

use hm_pipeline_ir::{EdgeKind, PipelineGraph, StepEval, Transition};

use crate::local::runner::{RunnerRegistry, StepContext};
use crate::local::source::build_archive_bytes;
use crate::{BuildOutcome, BuildStatus, DynamicEvaluator, StepResultSummary, StepStatus};

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
    summaries: Vec<StepResultSummary>,
}

type StepFuture = futures::future::Shared<BoxFuture<'static, StepOutcome>>;

pub(crate) struct LocalRunContext {
    pub repo_root: PathBuf,
    pub pipeline_slug: String,
    pub runtime_env: BTreeMap<String, String>,
    pub dynamic_evaluator: Option<Arc<dyn DynamicEvaluator>>,
}

#[derive(Debug)]
struct ExecutionBatch {
    transitions: Vec<Transition>,
    parents: Vec<Vec<(EdgeKind, usize)>>,
    terminals: Vec<usize>,
}

async fn resolve_execution_batch(
    transition: Transition,
    repo_root: &std::path::Path,
    runtime_env: &BTreeMap<String, String>,
    evaluator: Option<&dyn DynamicEvaluator>,
) -> anyhow::Result<ExecutionBatch> {
    let StepEval::Dynamic { target_name } = &transition.step.eval else {
        return Ok(ExecutionBatch {
            transitions: vec![transition],
            parents: vec![Vec::new()],
            terminals: vec![0],
        });
    };
    let evaluator = evaluator.ok_or_else(|| {
        anyhow::anyhow!("dynamic target {target_name:?} cannot run without a DSL evaluator")
    })?;

    let fragment = evaluator
        .evaluate(repo_root, target_name, runtime_env)
        .await
        .map_err(|e| anyhow::anyhow!("{e:#}"))?;
    if fragment.node_count() == 0 {
        anyhow::bail!("dynamic target {target_name:?} produced no steps");
    }

    let dag = fragment.dag();
    let order = toposort(dag.graph(), None)
        .map_err(|_| anyhow::anyhow!("dynamic target {target_name:?} produced a cyclic graph"))?;
    let position_by_node: HashMap<NodeIndex, usize> = order
        .iter()
        .enumerate()
        .map(|(position, &node)| (node, position))
        .collect();
    let mut parents = Vec::with_capacity(order.len());
    let mut terminals = Vec::new();

    for &node in &order {
        if !matches!(dag[node].step.eval, StepEval::Cmd { .. }) {
            anyhow::bail!("dynamic target {target_name:?} returned another dynamic step");
        }

        let node_parents: Vec<(EdgeKind, usize)> = dag
            .parents(node)
            .iter(dag)
            .map(|(edge, parent)| {
                (
                    *dag.edge_weight(edge).expect("edge in dynamic DAG"),
                    position_by_node[&parent],
                )
            })
            .collect();
        if node_parents
            .iter()
            .filter(|(kind, _)| *kind == EdgeKind::BuildsIn)
            .count()
            > 1
        {
            anyhow::bail!(
                "dynamic target {target_name:?} produced a step with multiple snapshot parents"
            );
        }
        parents.push(node_parents);
        if dag.children(node).iter(dag).next().is_none() {
            terminals.push(position_by_node[&node]);
        }
    }

    let placeholder = transition.step;
    let single_terminal = terminals
        .as_slice()
        .first()
        .copied()
        .filter(|_| terminals.len() == 1);
    let mut transitions = Vec::with_capacity(order.len());
    for (position, node) in order.into_iter().enumerate() {
        let mut concrete = dag[node].clone();
        concrete.step.key = if Some(position) == single_terminal {
            placeholder.key.clone()
        } else {
            format!("{}/{}", placeholder.key, concrete.step.key)
        };
        if Some(position) == single_terminal {
            if concrete.step.label.is_none() {
                concrete.step.label.clone_from(&placeholder.label);
            }
            if concrete.step.timeout_seconds.is_none() {
                concrete.step.timeout_seconds = placeholder.timeout_seconds;
            }
            if concrete.step.cache.is_none() {
                concrete.step.cache.clone_from(&placeholder.cache);
            }
            if concrete.step.runner.is_none() {
                concrete.step.runner.clone_from(&placeholder.runner);
            }
            if concrete.step.runner_args.is_none() {
                concrete
                    .step
                    .runner_args
                    .clone_from(&placeholder.runner_args);
            }
        }
        if parents[position].is_empty() && concrete.step.image.is_none() {
            concrete.step.image.clone_from(&placeholder.image);
        }

        let mut env = transition.env.clone();
        env.extend(concrete.env);
        concrete.env = env;
        transitions.push(concrete);
    }
    Ok(ExecutionBatch {
        transitions,
        parents,
        terminals,
    })
}

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
pub(crate) async fn run(
    graph: PipelineGraph,
    context: LocalRunContext,
    parallelism: usize,
    runner_registry: Arc<RunnerRegistry>,
    tx: tokio::sync::mpsc::Sender<BuildEvent>,
    cancel: CancellationToken,
    keep_going: bool,
) -> crate::Result<BuildOutcome> {
    let LocalRunContext {
        repo_root,
        pipeline_slug,
        runtime_env,
        dynamic_evaluator,
    } = context;
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

    let default_image = graph.default_image().map(str::to_owned);
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
        let repo_root = repo_root.clone();
        let runtime_env = runtime_env.clone();
        let dynamic_evaluator = dynamic_evaluator.clone();
        let default_image = default_image.clone();

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
                    summaries: vec![StepResultSummary {
                        step_id: Uuid::new_v4(),
                        key: node_key,
                        status,
                        exit_code: None,
                        duration_ms: 0,
                    }],
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
                repo_root,
                runtime_env,
                dynamic_evaluator,
                default_image,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(e) => {
                    tracing::error!(%e, "step execution failed");
                    StepOutcome {
                        exit_code: 1,
                        snapshot: None,
                        summaries: vec![StepResultSummary {
                            step_id: Uuid::new_v4(),
                            key: node_key,
                            status: StepStatus::Failed,
                            exit_code: Some(1),
                            duration_ms: 0,
                        }],
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

    let steps: Vec<StepResultSummary> = outcomes
        .iter()
        .flat_map(|outcome| outcome.summaries.clone())
        .collect();

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
    repo_root: PathBuf,
    runtime_env: BTreeMap<String, String>,
    dynamic_evaluator: Option<Arc<dyn DynamicEvaluator>>,
    default_image: Option<String>,
) -> anyhow::Result<StepOutcome> {
    let batch = resolve_execution_batch(
        transition,
        &repo_root,
        &runtime_env,
        dynamic_evaluator.as_deref(),
    )
    .await?;
    let mut outcomes: Vec<Option<StepOutcome>> = vec![None; batch.transitions.len()];
    let mut summaries = Vec::with_capacity(batch.transitions.len());
    let mut first_failure = None;
    let step_keys: Vec<String> = batch
        .transitions
        .iter()
        .map(|transition| transition.step.key.clone())
        .collect();

    for (offset, mut transition) in batch.transitions.into_iter().enumerate() {
        let node_parents = &batch.parents[offset];
        let step_key = transition.step.key.clone();
        let blocked = cancel.is_cancelled()
            || node_parents.iter().any(|(_, parent)| {
                outcomes[*parent]
                    .as_ref()
                    .is_some_and(|outcome| outcome.exit_code != 0)
            });
        if blocked {
            let status = if cancel.is_cancelled() {
                StepStatus::Canceled
            } else {
                StepStatus::Skipped
            };
            let summary = StepResultSummary {
                step_id: Uuid::new_v4(),
                key: step_key,
                status,
                exit_code: None,
                duration_ms: 0,
            };
            summaries.push(summary.clone());
            outcomes[offset] = Some(StepOutcome {
                exit_code: 0,
                snapshot: None,
                summaries: vec![summary],
            });
            continue;
        }

        let builds_in_parent = node_parents
            .iter()
            .find(|(kind, _)| *kind == EdgeKind::BuildsIn)
            .map(|(_, parent)| *parent);
        let node_parent_snapshot = builds_in_parent
            .and_then(|parent| outcomes[parent].as_ref())
            .and_then(|outcome| outcome.snapshot.clone())
            .or_else(|| {
                if node_parents.is_empty() {
                    parent_snapshot.clone()
                } else {
                    None
                }
            });
        let node_parent_key = builds_in_parent
            .map(|parent| step_keys[parent].clone())
            .or_else(|| {
                if node_parents.is_empty() {
                    parent_key.clone()
                } else {
                    None
                }
            });
        if node_parent_snapshot.is_none() && transition.step.image.is_none() {
            transition.step.image.clone_from(&default_image);
        }

        let outcome = execute_command(
            transition,
            node_parent_snapshot,
            chain_id,
            chain_pos + offset,
            node_parent_key,
            archive_id,
            run_id,
            run_ctx.clone(),
            runner_registry.clone(),
            bus.clone(),
            cancel.clone(),
            keep_going,
        )
        .await?;
        summaries.extend(outcome.summaries.clone());
        if outcome.exit_code != 0 && first_failure.is_none() {
            first_failure = Some(outcome.exit_code);
        }
        outcomes[offset] = Some(outcome);
    }

    let snapshot = if batch.terminals.len() == 1 {
        outcomes[batch.terminals[0]]
            .as_ref()
            .and_then(|outcome| outcome.snapshot.clone())
    } else {
        None
    };
    Ok(StepOutcome {
        exit_code: first_failure.unwrap_or(0),
        snapshot,
        summaries,
    })
}

#[allow(clippy::too_many_arguments)]
async fn execute_command(
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
    let step_wire = transition
        .step
        .into_command()
        .ok_or_else(|| anyhow::anyhow!("dynamic target returned another dynamic step"))?;
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
                        summaries: vec![StepResultSummary {
                            step_id,
                            key: step_key.clone(),
                            status: StepStatus::TimedOut,
                            exit_code: Some(124),
                            duration_ms: dur_ms,
                        }],
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
                summaries: vec![StepResultSummary {
                    step_id,
                    key: step_key.clone(),
                    status,
                    exit_code: Some(sr.exit_code),
                    duration_ms: dur_ms,
                }],
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod dynamic_tests {
    use super::*;
    use hm_pipeline_ir::{PipelineStep, StepEval};

    #[derive(Debug)]
    struct FakeEvaluator {
        graph: PipelineGraph,
    }

    #[async_trait::async_trait]
    impl DynamicEvaluator for FakeEvaluator {
        async fn evaluate(
            &self,
            _repo_root: &std::path::Path,
            _target_name: &str,
            _env: &BTreeMap<String, String>,
        ) -> crate::Result<PipelineGraph> {
            Ok(self.graph.clone())
        }
    }

    fn graph(json: &str) -> PipelineGraph {
        serde_json::from_str(json).unwrap()
    }

    fn dynamic_transition() -> Transition {
        Transition {
            step: PipelineStep {
                key: "choose-build".into(),
                label: Some("Choose build".into()),
                eval: StepEval::Dynamic {
                    target_name: "choose_build".into(),
                },
                image: Some("ubuntu:24.04".into()),
                env: None,
                timeout_seconds: None,
                cache: None,
                runner: None,
                runner_args: None,
            },
            env: BTreeMap::from([("PIPELINE_ENV".into(), "present".into())]),
        }
    }

    #[tokio::test]
    async fn dynamic_transition_resolves_to_one_concrete_step() {
        let evaluator = FakeEvaluator {
            graph: graph(
                r#"{
                    "version":"0",
                    "graph":{
                        "nodes":[
                            {"step":{"key":"generated","eval":{"type":"cmd","cmd":"go test ./..."}}, "env":{"LANGUAGE":"go"}}
                        ],
                        "node_holes":[],
                        "edge_property":"directed",
                        "edges":[]
                    }
                }"#,
            ),
        };

        let resolved = resolve_execution_batch(
            dynamic_transition(),
            std::path::Path::new("/repo"),
            &BTreeMap::from([("LANGUAGE".into(), "go".into())]),
            Some(&evaluator),
        )
        .await
        .unwrap();
        let resolved = &resolved.transitions[0];

        assert_eq!(resolved.step.key, "choose-build");
        assert_eq!(resolved.step.label.as_deref(), Some("Choose build"));
        assert_eq!(resolved.step.image.as_deref(), Some("ubuntu:24.04"));
        assert_eq!(
            resolved.step.eval,
            StepEval::Cmd {
                cmd: "go test ./...".into()
            }
        );
        assert_eq!(resolved.env["PIPELINE_ENV"], "present");
        assert_eq!(resolved.env["LANGUAGE"], "go");
    }

    #[tokio::test]
    async fn dynamic_transition_requires_an_evaluator() {
        let error = resolve_execution_batch(
            dynamic_transition(),
            std::path::Path::new("/repo"),
            &BTreeMap::new(),
            None,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("without a DSL evaluator"));
    }

    #[tokio::test]
    async fn dynamic_transition_accepts_linear_multi_step_fragment() {
        let evaluator = FakeEvaluator {
            graph: graph(
                r#"{
                    "version":"0",
                    "graph":{
                        "nodes":[
                            {"step":{"key":"a","eval":{"type":"cmd","cmd":"echo a"}}, "env":{}},
                            {"step":{"key":"b","eval":{"type":"cmd","cmd":"echo b"}}, "env":{}}
                        ],
                        "node_holes":[],
                        "edge_property":"directed",
                        "edges":[[0,1,"builds_in"]]
                    }
                }"#,
            ),
        };

        let resolved = resolve_execution_batch(
            dynamic_transition(),
            std::path::Path::new("/repo"),
            &BTreeMap::new(),
            Some(&evaluator),
        )
        .await
        .unwrap();

        assert_eq!(resolved.transitions.len(), 2);
        assert_eq!(resolved.transitions[0].step.key, "choose-build/a");
        assert_eq!(resolved.transitions[1].step.key, "choose-build");
        assert_eq!(
            resolved.parents,
            vec![vec![], vec![(EdgeKind::BuildsIn, 0)]]
        );
        assert_eq!(resolved.terminals, vec![1]);
        assert_eq!(
            resolved.transitions[0].step.eval,
            StepEval::Cmd {
                cmd: "echo a".into()
            }
        );
        assert_eq!(
            resolved.transitions[1].step.eval,
            StepEval::Cmd {
                cmd: "echo b".into()
            }
        );
    }

    #[tokio::test]
    async fn dynamic_transition_accepts_group_with_multiple_terminals() {
        let evaluator = FakeEvaluator {
            graph: graph(
                r#"{
                    "version":"0",
                    "graph":{
                        "nodes":[
                            {"step":{"key":"a","eval":{"type":"cmd","cmd":"echo a"}}, "env":{}},
                            {"step":{"key":"b","eval":{"type":"cmd","cmd":"echo b"}}, "env":{}}
                        ],
                        "node_holes":[],
                        "edge_property":"directed",
                        "edges":[]
                    }
                }"#,
            ),
        };

        let resolved = resolve_execution_batch(
            dynamic_transition(),
            std::path::Path::new("/repo"),
            &BTreeMap::new(),
            Some(&evaluator),
        )
        .await
        .unwrap();

        assert_eq!(resolved.transitions.len(), 2);
        let mut keys: Vec<&str> = resolved
            .transitions
            .iter()
            .map(|transition| transition.step.key.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["choose-build/a", "choose-build/b"]);
        assert_eq!(resolved.parents, vec![vec![], vec![]]);
        assert_eq!(resolved.terminals, vec![0, 1]);
    }

    #[tokio::test]
    async fn dynamic_transition_accepts_explicit_group_continuation() {
        let evaluator = FakeEvaluator {
            graph: graph(
                r#"{
                    "version":"0",
                    "graph":{
                        "nodes":[
                            {"step":{"key":"a","eval":{"type":"cmd","cmd":"echo a"}}, "env":{}},
                            {"step":{"key":"b","eval":{"type":"cmd","cmd":"echo b"}}, "env":{}},
                            {"step":{"key":"merge","eval":{"type":"cmd","cmd":"echo merge"}}, "env":{}}
                        ],
                        "node_holes":[],
                        "edge_property":"directed",
                        "edges":[[0,2,"depends_on"],[1,2,"depends_on"]]
                    }
                }"#,
            ),
        };

        let resolved = resolve_execution_batch(
            dynamic_transition(),
            std::path::Path::new("/repo"),
            &BTreeMap::new(),
            Some(&evaluator),
        )
        .await
        .unwrap();

        let terminal = resolved.terminals[0];
        assert_eq!(resolved.terminals.len(), 1);
        assert_eq!(resolved.transitions[terminal].step.key, "choose-build");
        assert_eq!(resolved.parents[terminal].len(), 2);
        assert!(
            resolved.parents[terminal]
                .iter()
                .all(|(kind, _)| *kind == EdgeKind::DependsOn)
        );
    }
}
