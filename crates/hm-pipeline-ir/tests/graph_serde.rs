#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeMap;

use hm_pipeline_ir::graph::{EdgeKind, NodeWeight};
use hm_pipeline_ir::CommandStep;

#[test]
fn node_weight_round_trips() {
    let nw = NodeWeight {
        step: CommandStep {
            key: "a".into(),
            label: Some("step A".into()),
            cmd: "echo a".into(),
            builds_in: None,
            image: Some("ubuntu:24.04".into()),
            env: None,
            timeout_seconds: None,
            cache: None,
            runner: None,
            runner_args: None,
        },
        env: BTreeMap::from([("FOO".into(), "bar".into())]),
    };
    let json = serde_json::to_string(&nw).unwrap();
    let back: NodeWeight = serde_json::from_str(&json).unwrap();
    assert_eq!(back.step.key, "a");
    assert_eq!(back.env.get("FOO").unwrap(), "bar");
}

#[test]
fn edge_kind_serializes_as_snake_case() {
    assert_eq!(serde_json::to_string(&EdgeKind::BuildsIn).unwrap(), "\"builds_in\"");
    assert_eq!(serde_json::to_string(&EdgeKind::DependsOn).unwrap(), "\"depends_on\"");
}

#[test]
fn edge_kind_round_trips() {
    let bi: EdgeKind = serde_json::from_str("\"builds_in\"").unwrap();
    assert_eq!(bi, EdgeKind::BuildsIn);
    let dep: EdgeKind = serde_json::from_str("\"depends_on\"").unwrap();
    assert_eq!(dep, EdgeKind::DependsOn);
}
