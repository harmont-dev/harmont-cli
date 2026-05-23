//! Capability indexing.
//!
//! After loading `noop_executor` + `recording_hook` + `failing_subcommand`,
//! the registry has the expected indices and we can dispatch through them.

#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

pub mod common;

use common::fixtures;
use harmont_cli::plugin::{PluginRegistry, RegistryConfig};
use hm_plugin_protocol::{
    ArchiveId, CacheDecision, CommandStep, ExecutorInput, ExitInfo, StepResult,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn loads_three_fixtures_and_builds_indices() {
    let reg = PluginRegistry::load(RegistryConfig {
        auto_discover: false,
        extra_paths: vec![
            fixtures::fixture_path("noop_executor"),
            fixtures::fixture_path("recording_hook"),
            fixtures::fixture_path("failing_subcommand"),
        ],
        embedded: vec![],
        ..Default::default()
    })
    .expect("load");
    assert!(reg.runner_index.contains_key("noop"));
    assert!(reg.subcommand_index.contains_key("fixture-fail"));
    assert_eq!(reg.manifests().count(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatches_subcommand_with_nonzero_exit_info() {
    let reg = PluginRegistry::load(RegistryConfig {
        auto_discover: false,
        extra_paths: vec![fixtures::fixture_path("failing_subcommand")],
        embedded: vec![],
        ..Default::default()
    })
    .unwrap();
    let idx = reg.subcommand_index["fixture-fail"];
    let plugin = reg.get(idx).unwrap();
    let info: ExitInfo = plugin
        .call_capability(
            "hm_subcommand_run",
            &json!({"verb_path": ["fixture-fail"], "args": {}, "env": {}}),
        )
        .await
        .unwrap();
    assert_eq!(info.exit_code, 7);
    assert_eq!(
        info.message.as_deref(),
        Some("intentional failure for tests")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatches_step_executor() {
    let reg = PluginRegistry::load(RegistryConfig {
        auto_discover: false,
        extra_paths: vec![fixtures::fixture_path("noop_executor")],
        embedded: vec![],
        ..Default::default()
    })
    .unwrap();
    let idx = reg.runner_index["noop"];
    let plugin = reg.get(idx).unwrap();
    let input = ExecutorInput {
        step: CommandStep {
            key: "build".into(),
            label: None,
            cmd: "true".into(),
            image: None,
            env: None,
            timeout_seconds: None,
            cache: None,
            runner: Some("noop".into()),
            runner_args: None,
        },
        workspace_archive_id: ArchiveId(Uuid::nil()),
        env: std::collections::BTreeMap::new(),
        workdir: "/workspace".into(),
        run_id: Uuid::nil(),
        step_id: Uuid::nil(),
        cache_lookup: CacheDecision::MissNoCommit,
        parent_snapshot: None,
    };
    let result: StepResult = plugin
        .call_capability("hm_executor_run", &input)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.committed_snapshot.is_none());
}
