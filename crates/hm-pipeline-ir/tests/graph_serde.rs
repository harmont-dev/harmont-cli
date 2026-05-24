#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeMap;

use hm_pipeline_ir::{CommandStep, EdgeKind, Transition};

#[test]
fn transition_round_trips() {
    let nw = Transition {
        step: CommandStep {
            key: "a".into(),
            label: Some("step A".into()),
            cmd: "echo a".into(),
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
    let back: Transition = serde_json::from_str(&json).unwrap();
    assert_eq!(back.step.key, "a");
    assert_eq!(back.env.get("FOO").unwrap(), "bar");
}

#[test]
fn edge_kind_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&EdgeKind::BuildsIn).unwrap(),
        "\"builds_in\""
    );
    assert_eq!(
        serde_json::to_string(&EdgeKind::DependsOn).unwrap(),
        "\"depends_on\""
    );
}

#[test]
fn edge_kind_round_trips() {
    let bi: EdgeKind = serde_json::from_str("\"builds_in\"").unwrap();
    assert_eq!(bi, EdgeKind::BuildsIn);
    let dep: EdgeKind = serde_json::from_str("\"depends_on\"").unwrap();
    assert_eq!(dep, EdgeKind::DependsOn);
}

use hm_pipeline_ir::PipelineGraph;

fn build_test_graph() -> PipelineGraph {
    serde_json::from_value(serde_json::json!({
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "a", "cmd": "echo a", "image": "ubuntu:24.04"}, "env": {}},
                {"step": {"key": "b", "cmd": "echo b"}, "env": {}},
                {"step": {"key": "c", "cmd": "echo c", "image": "ubuntu:24.04"}, "env": {}}
            ],
            "node_holes": [],
            "edge_property": "directed",
            "edges": [
                [0, 1, "builds_in"]
            ]
        }
    }))
    .unwrap()
}

#[test]
fn pipeline_graph_round_trips_through_json() {
    use daggy::{Walker, petgraph::visit::IntoNodeReferences};

    let g = build_test_graph();
    let json = serde_json::to_string_pretty(&g).unwrap();
    let back: PipelineGraph = serde_json::from_str(&json).unwrap();
    assert_eq!(back.node_count(), 3);
    assert_eq!(back.default_image(), Some("ubuntu:24.04"));

    let a_idx = back
        .dag()
        .graph()
        .node_references()
        .find(|(_, t)| t.step.key == "a")
        .map(|(idx, _)| idx)
        .unwrap();
    assert_eq!(
        back.dag()[a_idx].step.image.as_deref(),
        Some("ubuntu:24.04")
    );

    let b_idx = back
        .dag()
        .graph()
        .node_references()
        .find(|(_, t)| t.step.key == "b")
        .map(|(idx, _)| idx)
        .unwrap();
    let has_builds_in_parent = back
        .dag()
        .parents(b_idx)
        .iter(back.dag())
        .any(|(e, _)| *back.dag().edge_weight(e).unwrap() == EdgeKind::BuildsIn);
    assert!(has_builds_in_parent);
}

#[test]
fn dag_accessor_exposes_node_count() {
    let g = build_test_graph();
    assert_eq!(g.dag().node_count(), 3);
}

#[test]
fn pipeline_graph_snapshot() {
    let g = build_test_graph();
    let json = serde_json::to_value(&g).unwrap();
    insta::assert_json_snapshot!(json);
}
