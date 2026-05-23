//! Chain-bounded scheduler. Dispatches each step to its registered
//! step-executor plugin (Docker by default) via the plugin host.

// Pedantic-bucket nags accepted at module scope:
// - `cast_possible_truncation`: every `as u64` here is a millisecond
//   wall-clock duration; `u128 -> u64` cannot overflow for any
//   conceivable build runtime (584 million years).
// - `expect_used` on the semaphore: `acquire_owned` only errors if the
//   semaphore is closed, which we never close.
// - `too_many_lines` on `run`: the scheduler body is one cohesive
//   loop; splitting it would obscure the spawn/join symmetry.
// - `missing_panics_doc`: the only panic path is the semaphore expect
//   described above; the function docstring already explains its
//   error surface.
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

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use hm_plugin_protocol::{
    ArchiveId, BuildEvent, ExecutorInput, PlanSummary, SnapshotRef, StepResult,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::HmError;
use crate::orchestrator::docker_client::DockerClient;
use crate::orchestrator::graph::Graph;
use crate::orchestrator::source::build_archive_bytes;
use crate::plugin::{PluginRegistry, RegistryConfig};

use super::archive::ArchiveStore;
use super::cache;
use tokio_util::sync::CancellationToken;
use super::events::EventBus;
use super::state::{self, OrchestratorState};

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
    pipeline: hm_plugin_protocol::Pipeline,
    repo_root: PathBuf,
    parallelism: usize,
    format_name: String,
) -> Result<i32> {
    // Build graph + chains directly from the wire-typed pipeline.
    let graph = Graph::build(&pipeline).context("build graph")?;
    let chains = graph.chains();
    let chain_deps = graph.chain_deps(&chains);

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

    // Cross-chain snapshot lineage. When a step completes, we stash
    // its `committed_snapshot` under its node index. A fork-child
    // chain looks up its `builds_in` parent here to know what base
    // image to boot from. Mirrors legacy `SharedState::node_image`.
    let node_image: Arc<Mutex<HashMap<usize, SnapshotRef>>> = Arc::new(Mutex::new(HashMap::new()));

    // Spawn the output subscriber. Dispatches every BuildEvent to the
    // selected output-formatter plugin (default: `human`).
    let sink_handle =
        super::output_subscriber::spawn(bus.clone(), registry.clone(), format_name.clone());

    // Announce build start.
    let started_at = chrono::Utc::now();
    let plan_summary = PlanSummary {
        step_count: graph.nodes.len(),
        chain_count: chains.len(),
        default_runner: "docker".into(),
    };
    bus.emit(BuildEvent::BuildStart {
        run_id,
        plan: plan_summary,
        started_at,
    });

    // Schedule chains. Each chain runs sequentially internally; chains
    // run concurrently subject to the semaphore and the chain_deps DAG.
    let started_total = Instant::now();
    let mut overall = 0i32;
    let mut completed: HashSet<usize> = HashSet::new();
    let mut pending: Vec<usize> = (0..chains.len()).collect();
    let mut in_flight: tokio::task::JoinSet<(usize, Result<i32>)> = tokio::task::JoinSet::new();

    loop {
        // Spawn ready chains.
        let mut still_pending = Vec::with_capacity(pending.len());
        for ci in std::mem::take(&mut pending) {
            let ready = chain_deps[ci].iter().all(|d| completed.contains(d));
            if !ready {
                still_pending.push(ci);
                continue;
            }
            let semaphore = semaphore.clone();
            let registry = registry.clone();
            let graph = graph.clone();
            let cancel = cancel.clone();
            let chain_nodes = chains[ci].clone();
            let bus = bus.clone();
            let node_image = node_image.clone();
            in_flight.spawn(async move {
                let _permit = semaphore.acquire_owned().await.expect("semaphore");
                if cancel.is_cancelled() {
                    return (ci, Ok(0));
                }
                let rc = run_chain(
                    ci,
                    &graph,
                    &chain_nodes,
                    archive_id,
                    run_id,
                    &registry,
                    &bus,
                    &cancel,
                    &node_image,
                )
                .await;
                (ci, rc)
            });
        }
        pending = still_pending;

        if in_flight.is_empty() {
            break;
        }

        match in_flight.join_next().await {
            Some(Ok((ci, Ok(0)))) => {
                completed.insert(ci);
            }
            Some(Ok((ci, Ok(_rc)))) => {
                overall = crate::error::EXIT_BUILD_FAILED;
                cancel.cancel();
                completed.insert(ci);
                // ChainFailed already emitted by run_chain; no stderr write here.
            }
            Some(Ok((_, Err(e)))) => {
                cancel.cancel();
                bus.emit(BuildEvent::BuildEnd {
                    exit_code: crate::error::EXIT_BUILD_FAILED,
                    duration_ms: started_total.elapsed().as_millis() as u64,
                });
                return Err(e);
            }
            Some(Err(je)) => {
                cancel.cancel();
                bus.emit(BuildEvent::BuildEnd {
                    exit_code: crate::error::EXIT_BUILD_FAILED,
                    duration_ms: started_total.elapsed().as_millis() as u64,
                });
                return Err(anyhow::anyhow!("chain task panicked: {je}"));
            }
            None => break,
        }
    }

    let dur = started_total.elapsed().as_millis() as u64;
    bus.emit(BuildEvent::BuildEnd {
        exit_code: overall,
        duration_ms: dur,
    });

    // Wait briefly for the sink to drain the BuildEnd event. It exits
    // when it sees BuildEnd, so this completes quickly.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), sink_handle).await;

    state::clear();
    drop(state_arc);
    Ok(overall)
}

/// Drive one chain end-to-end. Each step within a chain runs
/// sequentially, with the previous step's snapshot becoming the next
/// step's `parent_snapshot` input.
///
/// `node_image` is the cross-chain lineage map: when this chain's
/// root is a fork-child (its `builds_in` parent lives in another
/// chain), we look up the parent's committed snapshot there to seed
/// our initial `parent_snapshot`. Each step we run records its
/// committed snapshot back so downstream fork-children can find it.
#[allow(
    clippy::too_many_arguments,
    reason = "tightly-coupled per-run state — splitting into a struct would just rename the bag"
)]
async fn run_chain(
    chain_idx: usize,
    graph: &Graph,
    chain_nodes: &[usize],
    archive_id: ArchiveId,
    run_id: Uuid,
    registry: &Arc<Mutex<PluginRegistry>>,
    bus: &Arc<EventBus>,
    cancel: &CancellationToken,
    node_image: &Arc<Mutex<HashMap<usize, SnapshotRef>>>,
) -> Result<i32> {
    // Seed from the cross-chain lineage map: if this chain's root has
    // a `builds_in` parent that already committed a snapshot, boot
    // from it. Otherwise this is a chain-root proper and starts from
    // the step's image.
    let chain_root = chain_nodes[0];
    let mut parent_snapshot: Option<SnapshotRef> = {
        let g = node_image.lock().await;
        graph.nodes[chain_root]
            .builds_in
            .and_then(|p| g.get(&p).cloned())
    };

    for (pos, &i) in chain_nodes.iter().enumerate() {
        if cancel.is_cancelled() {
            return Ok(0);
        }
        let step_wire = graph.nodes[i].step.clone();
        // Keep a copy of the step key for diagnostics — `step_wire` is
        // moved into `ExecutorInput` below.
        let step_key = step_wire.key.clone();
        let env_map: std::collections::BTreeMap<String, String> =
            graph.nodes[i].env.clone().into_iter().collect();
        let step_id = Uuid::new_v4();

        bus.emit(BuildEvent::StepQueued {
            step_id,
            key: step_key.clone(),
            chain_idx: pos,
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
        }

        let input = ExecutorInput {
            step: step_wire,
            workspace_archive_id: archive_id,
            env: env_map,
            workdir: "/workspace".to_string(),
            run_id,
            step_id,
            cache_lookup: decision,
            parent_snapshot: parent_snapshot.clone(),
        };

        // `input.step.runner` is the IR field as-declared. Steps that
        // didn't declare a runner fall back to whichever plugin
        // registered as `default: true` (docker, in the embedded
        // binary). The hardcoded `"docker"` is only a last-resort
        // fallback when no plugin claims default — practically
        // unreachable, but cheap to keep so the dispatch lookup below
        // still has a string to look up.
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

        // Dispatch to the runner-named plugin. Look up the Arc under
        // the registry lock, drop the lock BEFORE awaiting so other
        // chains can dispatch concurrently — the per-plugin pool
        // serialises (or parallelises, up to its capacity) calls
        // internally.
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
                // Publish this step's committed snapshot to the
                // cross-chain map so fork-children rooted at this
                // node can boot from it.
                if let Some(snap) = sr.committed_snapshot.clone() {
                    let mut g = node_image.lock().await;
                    g.insert(i, snap);
                }
                parent_snapshot = sr.committed_snapshot;
                if sr.exit_code != 0 {
                    bus.emit(BuildEvent::ChainFailed {
                        chain_idx,
                        failed_step_id: step_id,
                        failed_step_key: step_key.clone(),
                        exit_code: sr.exit_code,
                        message: format!("step '{}' exited with code {}", step_key, sr.exit_code),
                        ts: chrono::Utc::now(),
                    });
                    return Ok(sr.exit_code);
                }
            }
            Err(e) => {
                bus.emit(BuildEvent::StepEnd {
                    step_id,
                    exit_code: 1,
                    duration_ms: dur_ms,
                    snapshot: None,
                });
                return Err(e);
            }
        }
    }
    Ok(0)
}
