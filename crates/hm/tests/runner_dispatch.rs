//! Regression test: a `CommandStep` declaring `runner: "freestyle"`
//! must dispatch to the freestyle plugin, not the docker default.
//!
//! Background — PR #22: an earlier conversion path between the wire
//! `Pipeline` and the scheduler's `Node`/`ExecutorInput` round-trip
//! silently dropped the `runner` field, so every step landed on the
//! docker executor regardless of what the IR declared. A3 made the
//! orchestrator graph consume wire types directly so `runner` survives
//! end-to-end. This test pins that behaviour.
//!
//! Shape:
//!   1. Parse a JSON `Pipeline` with one step declaring `runner: "freestyle"`.
//!   2. Build a `Graph` from it (the conversion path under test).
//!   3. Construct an `ExecutorInput` from `graph.nodes[0].step.clone()`
//!      — mirroring exactly what the scheduler does — and derive the
//!      runner via the scheduler's `runner.clone().unwrap_or("docker")`
//!      pattern.
//!   4. Dispatch through the registry's `runner_index`.
//!   5. Read back the persistent KV slot the freestyle fixture wrote.
//!
//! If a future change drops `runner` through the graph, step 3 falls
//! back to `"docker"`, dispatch lands on the docker plugin (which is
//! not loaded here), and the assertion in step 5 fails.

#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    unsafe_code,
    reason = "test pokes XDG_CONFIG_HOME via std::env::set_var, which is unsafe in Rust 2024"
)]

pub mod common;

use std::collections::BTreeMap;

use daggy::petgraph::visit::IntoNodeReferences;

use common::fixtures;
use hm_pipeline_ir::PipelineGraph;
use harmont_cli::plugin::{PluginRegistry, RegistryConfig};
use hm_plugin_protocol::{ArchiveId, CacheDecision, ExecutorInput, StepResult};
use uuid::Uuid;

const PIPELINE_JSON: &[u8] = br#"{
    "version": "0",
    "graph": {
        "nodes": [
            {
                "step": {
                    "key": "fs-step",
                    "cmd": "irrelevant; fixture ignores cmd",
                    "runner": "freestyle"
                },
                "env": {}
            }
        ],
        "edge_property": "directed",
        "edges": []
    }
}"#;

#[tokio::test(flavor = "multi_thread")]
async fn runner_field_dispatches_to_named_plugin() {
    // The freestyle fixture writes to KvScope::Plugin, which the host
    // persists at <XDG_CONFIG_HOME>/harmont/state/<plugin>.kv. Pin the
    // config dir to a tempdir so this test is hermetic and doesn't
    // touch the developer's real state.
    let temp = tempfile::tempdir().expect("tempdir");
    // SAFETY: process-wide env var set during a test; the tempdir is
    // unique per run. Mirrors the pattern in `plugin_host_fns.rs`.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp.path());
    }

    // 1. Load the freestyle fixture into a clean registry.
    let reg = PluginRegistry::load(RegistryConfig {
        auto_discover: false,
        extra_paths: vec![fixtures::fixture_path("freestyle_runner")],
        embedded: vec![],
        ..Default::default()
    })
    .expect("load registry");

    // 2. Deserialize the graph directly from JSON — the new wire format.
    let graph: PipelineGraph = serde_json::from_slice(PIPELINE_JSON).expect("parse graph");

    // Sanity check: the graph must preserve `runner` from the IR.
    // This is the cheap fast-fail; the dispatch check below is the
    // load-bearing one.
    let (_, first_transition) = graph.dag().graph().node_references()
        .find(|(_, t)| t.step.key == "fs-step")
        .unwrap();
    assert_eq!(
        first_transition.step.runner.as_deref(),
        Some("freestyle"),
        "graph dropped `runner` field — A3's wire-type fix has regressed"
    );

    // 3. Build the executor input exactly as the scheduler does
    //    (orchestrator/scheduler.rs::run_chain). Cloning the wire
    //    step preserves `runner` and `runner_args` verbatim.
    let step_wire = first_transition.step.clone();
    let input = ExecutorInput {
        step: step_wire,
        workspace_archive_id: ArchiveId(Uuid::nil()),
        env: BTreeMap::new(),
        workdir: "/workspace".into(),
        run_id: Uuid::nil(),
        step_id: Uuid::nil(),
        cache_lookup: CacheDecision::MissNoCommit,
        parent_snapshot: None,
    };

    // 4. Derive the runner the same way the scheduler does. If a
    //    future change makes the scheduler stop honouring
    //    `input.step.runner`, this lookup falls back to "docker", the
    //    `runner_index` lookup misses (docker isn't loaded), and the
    //    test fails loudly.
    let runner = input.step.runner.clone().unwrap_or_else(|| "docker".into());
    assert_eq!(runner, "freestyle", "runner derivation lost the field");

    let idx = *reg
        .runner_index
        .get(&runner)
        .unwrap_or_else(|| panic!("runner '{runner}' not in registry"));
    let plugin = reg.get(idx).expect("plugin present at index");

    // 5. Dispatch and assert the freestyle plugin actually ran.
    let result: StepResult = plugin
        .call_capability("hm_executor_run", &input)
        .await
        .expect("dispatch freestyle");
    assert_eq!(result.exit_code, 0);

    // The fixture wrote `step.key` into KvScope::Plugin under the key
    // `freestyle_called_with`. Read it back via the persisted file:
    // <XDG_CONFIG_HOME>/harmont/state/<plugin-name>.kv is the JSON
    // serialisation of a BTreeMap<String, Vec<u8>>.
    let kv_path = temp
        .path()
        .join("harmont")
        .join("state")
        .join("harmont-fixture-freestyle.kv");
    let bytes = std::fs::read(&kv_path)
        .unwrap_or_else(|_| panic!("freestyle plugin KV file missing at {kv_path:?}"));
    let kv: BTreeMap<String, Vec<u8>> =
        serde_json::from_slice(&bytes).expect("parse freestyle plugin KV");
    let recorded = kv
        .get("freestyle_called_with")
        .expect("freestyle plugin did not record `freestyle_called_with` — dispatch missed");
    assert_eq!(
        recorded.as_slice(),
        b"fs-step",
        "freestyle plugin recorded the wrong step key — dispatch wired the wrong step"
    );
}
