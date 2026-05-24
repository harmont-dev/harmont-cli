//! Dataflow scheduler.
//!
//! Walks the pipeline DAG in topological order, spawning a shared
//! future per step. Each future awaits its predecessors, acquires a
//! parallelism permit, and dispatches the step to its registered
//! runner (Docker by default).

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
use std::time::Instant;

use daggy::petgraph::algo::toposort;
use daggy::{Dag, NodeIndex, Walker};
use futures::future::{BoxFuture, FutureExt, join_all};

use anyhow::{Context, Result};
use hm_plugin_protocol::{
    ArchiveId, BuildEvent, ExecutorInput, PlanSummary, SnapshotRef, StepResult,
};
use uuid::Uuid;

use hm_pipeline_ir::{EdgeKind, PipelineGraph, Transition};

use crate::error::HmError;
use crate::orchestrator::docker_client::DockerClient;
use crate::orchestrator::source::build_archive_bytes;
use crate::runner::{OutputRenderer, RunContext, RunnerRegistry};

use super::archive::ArchiveStore;
use super::cache;
use super::events::EventBus;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct StepOutcome {
    exit_code: i32,
    snapshot: Option<SnapshotRef>,
}

type StepFuture = futures::future::Shared<BoxFuture<'static, StepOutcome>>;

/// Entry point: run a parsed pipeline locally end-to-end. Returns
/// the overall exit code (0 = success, [`crate::error::EXIT_BUILD_FAILED`]
/// when any step exited non-zero).
///
/// # Errors
/// Returns an error if the source archive cannot be built, the Docker
/// daemon is unreachable, or any scheduler-level failure occurs.
/// Non-zero step exit codes are surfaced via the returned `i32`, not
/// as an Err.
pub async fn run(
    graph: PipelineGraph,
    repo_root: PathBuf,
    parallelism: usize,
    runner_registry: Arc<RunnerRegistry>,
    renderer: Box<dyn OutputRenderer>,
) -> Result<i32> {
    // Set up per-run state.
    let bus = EventBus::new();
    let archives = Arc::new(ArchiveStore::new());
    let cancel = CancellationToken::new();
    let _ctrlc = super::signal::install_ctrlc(cancel.clone());
    // _ctrlc dropped at end of `run`; runtime tear-down kills the task.
    let docker = DockerClient::connect()
        .map_err(|e| HmError::Docker(format!("daemon unreachable — is Docker running? ({e})")))?;
    docker
        .ping()
        .await
        .map_err(|e| HmError::Docker(format!("daemon ping failed: {e}")))?;
    let run_id = Uuid::new_v4();

    // Build the source archive once.
    let archive_bytes = build_archive_bytes(&repo_root).context("build source archive")?;
    let archive_id = archives.register(archive_bytes);

    let run_ctx = RunContext {
        docker: docker.clone(),
        event_bus: bus.clone(),
        archives: archives.clone(),
        cancel: cancel.clone(),
    };

    let parallelism = parallelism.max(1);

    let semaphore = Arc::new(tokio::sync::Semaphore::new(parallelism));

    // Spawn the output subscriber. Dispatches every BuildEvent to the
    // pre-constructed renderer.
    let sink_handle = super::output_subscriber::spawn(bus.clone(), renderer);

    let dag = graph.dag();
    let chain_info = compute_chain_info(dag);

    let order = toposort(dag.graph(), None)
        .map_err(|c| anyhow::anyhow!("pipeline graph has a cycle at {:?}", c.node_id()))?;

    let started_at = chrono::Utc::now();
    bus.emit(BuildEvent::BuildStart {
        run_id,
        plan: PlanSummary {
            step_count: graph.node_count(),
            chain_count: chain_info.chain_count,
            default_runner: runner_registry
                .default_runner_name()
                .unwrap_or("docker")
                .into(),
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
        let chain_id = chain_info.node_chain_id[&n];
        let chain_pos = chain_info.node_chain_pos[&n];
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
                return StepOutcome {
                    exit_code: 0,
                    snapshot: None,
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
                archive_id,
                run_id,
                run_ctx,
                reg,
                bus,
                cancel,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(e) => {
                    tracing::error!(%e, "step execution failed");
                    StepOutcome {
                        exit_code: 1,
                        snapshot: None,
                    }
                }
            }
        }
        .boxed()
        .shared();

        tokio::spawn(fut.clone());
        done.insert(n, fut);
    }

    let outcomes: Vec<StepOutcome> = join_all(done.into_values()).await;
    let overall = if outcomes.iter().any(|o| o.exit_code != 0) {
        crate::error::EXIT_BUILD_FAILED
    } else {
        0
    };

    let dur = started_total.elapsed().as_millis() as u64;
    bus.emit(BuildEvent::BuildEnd {
        exit_code: overall,
        duration_ms: dur,
    });

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), sink_handle).await;

    Ok(overall)
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
    archive_id: ArchiveId,
    run_id: Uuid,
    run_ctx: RunContext,
    runner_registry: Arc<RunnerRegistry>,
    bus: Arc<EventBus>,
    cancel: CancellationToken,
) -> Result<StepOutcome> {
    let step_wire = transition.step;
    let step_key = step_wire.key.clone();
    let env_map = transition.env;
    let step_id = Uuid::new_v4();

    bus.emit(BuildEvent::StepQueued {
        step_id,
        key: step_key.clone(),
        chain_idx: chain_pos,
    });

    // Decide cache outcome host-side.
    let decision = cache::decide(&run_ctx.docker, &step_wire).await?;
    if let hm_plugin_protocol::CacheDecision::Hit { tag } = &decision {
        bus.emit(BuildEvent::StepCacheHit {
            step_id,
            key: step_wire
                .cache
                .as_ref()
                .and_then(|c| c.key.clone())
                .unwrap_or_default(),
            tag: tag.0.clone(),
        });
        // Short-circuit: the cached image already exists locally, so
        // there is nothing for the executor to do. Return the
        // snapshot so downstream nodes can use it as their parent.
        return Ok(StepOutcome {
            exit_code: 0,
            snapshot: Some(tag.clone()),
        });
    }

    let input = ExecutorInput {
        step: step_wire,
        workspace_archive_id: archive_id,
        env: env_map,
        workdir: "/workspace".to_string(),
        run_id,
        step_id,
        cache_lookup: decision,
        parent_snapshot,
    };

    // Resolve the runner by name. Steps that didn't declare a runner
    // fall back to whichever runner was registered as default (docker).
    let runner_name = input
        .step
        .runner
        .as_deref()
        .or_else(|| runner_registry.default_runner_name())
        .unwrap_or("docker")
        .to_owned();

    let started = Instant::now();
    bus.emit(BuildEvent::StepStart {
        step_id,
        runner: runner_name.clone(),
        image: input.step.image.clone(),
    });

    let runner = runner_registry
        .resolve(input.step.runner.as_deref())
        .ok_or_else(|| HmError::UnknownRunner {
            step_key: input.step.key.clone(),
            runner: runner_name.clone(),
            available: runner_registry
                .runner_names()
                .into_iter()
                .map(str::to_owned)
                .collect(),
        })?;

    let result: Result<StepResult> = runner.execute(&run_ctx, input).await;

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
                cancel.cancel();
            }
            Ok(StepOutcome {
                exit_code: sr.exit_code,
                snapshot: sr.committed_snapshot,
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
