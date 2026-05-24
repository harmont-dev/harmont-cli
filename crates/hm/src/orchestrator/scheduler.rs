//! Dataflow scheduler.
//!
//! Walks the pipeline DAG in topological order, spawning a shared
//! future per step. Each future awaits its predecessors, acquires a
//! parallelism permit, and dispatches the step to its registered
//! executor plugin (Docker by default).

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
    clippy::missing_panics_doc,
    // `significant_drop_tightening`: the registry MutexGuard in the
    // --format validation block is held only across constant-time
    // hash-map lookups; the lint would have us scatter `drop(reg)`
    // calls that add no clarity.
    clippy::significant_drop_tightening
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use daggy::{Dag, NodeIndex, Walker};
use daggy::petgraph::algo::toposort;
use futures::future::{BoxFuture, FutureExt, join_all};

use anyhow::{Context, Result};
use hm_plugin_protocol::{
    ArchiveId, BuildEvent, ExecutorInput, PlanSummary, SnapshotRef, StepResult,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use hm_pipeline_ir::{EdgeKind, PipelineGraph, Transition};

use crate::error::HmError;
use crate::orchestrator::docker_client::DockerClient;
use crate::orchestrator::source::build_archive_bytes;
use crate::plugin::{PluginRegistry, RegistryConfig};

use super::archive::ArchiveStore;
use super::cache;
use tokio_util::sync::CancellationToken;
use super::events::EventBus;
use super::state::{self, OrchestratorState};

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
/// Returns an error if plugin discovery fails, the source archive
/// cannot be built, the Docker daemon is unreachable, or any
/// scheduler-level failure occurs. Non-zero step exit codes are
/// surfaced via the returned `i32`, not as an Err.
pub async fn run(
    graph: PipelineGraph,
    repo_root: PathBuf,
    parallelism: usize,
    format_name: String,
) -> Result<i32> {
    // Set up per-run state.
    let bus = EventBus::new();
    let archives = ArchiveStore::new();
    let cancel = CancellationToken::new();
    let _ctrlc = crate::plugin::signal::install_ctrlc(cancel.clone());
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

    // Install per-run state for host fns to read.
    let state_arc = Arc::new(OrchestratorState {
        event_bus: bus.clone(),
        archives,
        cancel: cancel.clone(),
        docker: docker.clone(),
        run_id,
    });
    state::install(state_arc.clone());

    let parallelism = parallelism.max(1);

    // Load the plugin registry with the embedded docker plugin.
    // The docker runner's pool gets pre-sized to `parallelism` so
    // concurrent chains can run truly in parallel rather than
    // serialising on a single plugin instance.
    let mut pool_sizes: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    pool_sizes.insert("docker".to_string(), parallelism);
    let registry = Arc::new(Mutex::new(
        PluginRegistry::load(RegistryConfig {
            auto_discover: true,
            extra_paths: vec![],
            embedded: vec![
                (
                    "harmont-docker",
                    crate::plugin::embedded::DOCKER_PLUGIN_WASM,
                ),
                (
                    "harmont-output-human",
                    crate::plugin::embedded::OUTPUT_HUMAN_PLUGIN_WASM,
                ),
                (
                    "harmont-output-json",
                    crate::plugin::embedded::OUTPUT_JSON_PLUGIN_WASM,
                ),
            ],
            pool_sizes,
        })
        .context("load plugin registry")?,
    ));

    // Validate the requested output format BEFORE emitting BuildStart
    // so an invalid `--format` fails fast without producing any output.
    // We materialise the available list under the lock and then drop
    // the guard before the (rare) bail to satisfy
    // `clippy::significant_drop_tightening`.
    let bad_format: Option<Vec<String>> = {
        let reg = registry.lock().await;
        if reg.output_formatter_index.contains_key(&format_name) {
            None
        } else {
            let mut names: Vec<String> = reg.output_formatter_index.keys().cloned().collect();
            names.sort();
            Some(names)
        }
    };
    if let Some(available) = bad_format {
        anyhow::bail!(
            "unknown --format '{format_name}'; available: {}",
            available.join(", ")
        );
    }

    let semaphore = Arc::new(tokio::sync::Semaphore::new(parallelism));

    // Spawn the output subscriber. Dispatches every BuildEvent to the
    // selected output-formatter plugin (default: `human`).
    let sink_handle =
        super::output_subscriber::spawn(bus.clone(), registry.clone(), format_name.clone());

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
            default_runner: "docker".into(),
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
        let reg = registry.clone();
        let bus = bus.clone();
        let cancel = cancel.clone();

        let fut: StepFuture = async move {
            // Await all predecessors.
            let pred_outcomes: Vec<StepOutcome> =
                join_all(preds.iter().map(|(_, f)| f.clone())).await;

            // Early exit if any predecessor failed or the build was cancelled.
            if cancel.is_cancelled()
                || pred_outcomes.iter().any(|o| o.exit_code != 0)
            {
                return StepOutcome { exit_code: 0, snapshot: None };
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
                reg,
                bus,
                cancel,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(e) => {
                    tracing::error!(%e, "step execution failed");
                    StepOutcome { exit_code: 1, snapshot: None }
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

    state::clear();
    drop(state_arc);
    Ok(overall)
}

/// Execute a single step, returning its outcome (exit code + snapshot).
///
/// On cache hit the function returns early with exit code 0 and the
/// cached snapshot so downstream nodes receive the correct
/// `parent_snapshot` without running the plugin at all.
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
    registry: Arc<Mutex<PluginRegistry>>,
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
    let decision = {
        let s = state::current().context("no orchestrator state")?;
        cache::decide(&s.docker, &step_wire).await?
    };
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
        // there is nothing for the executor plugin to do. Return the
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

    // Resolve the runner plugin name. Steps that didn't declare a
    // runner fall back to whichever plugin registered as
    // `default: true` (docker, in the embedded binary).
    let runner = if let Some(name) = input.step.runner.clone() {
        name
    } else {
        let reg = registry.lock().await;
        reg.default_runner_name()
            .map_or_else(|| "docker".into(), str::to_string)
    };
    let started = Instant::now();
    bus.emit(BuildEvent::StepStart {
        step_id,
        runner: runner.clone(),
        image: input.step.image.clone(),
    });

    // Dispatch to the runner-named plugin. Look up the Arc under the
    // registry lock, drop the lock BEFORE awaiting so other tasks can
    // dispatch concurrently.
    let plugin = {
        let reg = registry.lock().await;
        let idx = reg
            .runner_index
            .get(&runner)
            .copied()
            .or(reg.default_runner)
            .ok_or_else(|| HmError::UnknownRunner {
                step_key: input.step.key.clone(),
                runner: runner.clone(),
                available: reg.runner_index.keys().cloned().collect(),
            })?;
        reg.get(idx).context("plugin moved away under us")?
    };
    crate::plugin::host_fns::set_current_step_id(step_id);
    let result: Result<StepResult> = plugin.call_capability("hm_executor_run", &input).await;
    crate::plugin::host_fns::clear_current_step_id();

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
