#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use hm_pipeline_ir::graph::PipelineGraph;
use hm_pipeline_ir::Pipeline;

fn decode(json: &[u8]) -> Pipeline {
    serde_json::from_slice(json).unwrap()
}

#[test]
fn builds_simple_chain() {
    let p = decode(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "steps": [
            {"type": "command", "key": "a", "cmd": "echo a"},
            {"type": "command", "key": "b", "cmd": "echo b", "builds_in": "a"},
            {"type": "command", "key": "c", "cmd": "echo c", "builds_in": "b"}
        ]
    }"#);
    let g = PipelineGraph::build(&p).unwrap();
    assert_eq!(g.node_count(), 3);
    assert_eq!(g.default_image(), Some("ubuntu:24.04"));
}

#[test]
fn rejects_unknown_builds_in() {
    let p = decode(br#"{
        "version": "0",
        "steps": [
            {"type": "command", "key": "b", "cmd": "echo b", "builds_in": "missing"}
        ]
    }"#);
    let err = PipelineGraph::build(&p).unwrap_err();
    assert!(
        err.to_string().contains("missing") || err.to_string().contains("unknown"),
        "error should mention the missing key: {err}"
    );
}

#[test]
fn root_inherits_default_image() {
    let p = decode(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "steps": [
            {"type": "command", "key": "a", "cmd": "echo a"}
        ]
    }"#);
    let g = PipelineGraph::build(&p).unwrap();
    let node = g.node_weight(g.node_index_by_key("a").unwrap());
    assert_eq!(node.step.image.as_deref(), Some("ubuntu:24.04"));
}

#[test]
fn child_does_not_inherit_default_image() {
    let p = decode(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "steps": [
            {"type": "command", "key": "a", "cmd": "echo a"},
            {"type": "command", "key": "b", "cmd": "echo b", "builds_in": "a"}
        ]
    }"#);
    let g = PipelineGraph::build(&p).unwrap();
    let b = g.node_weight(g.node_index_by_key("b").unwrap());
    assert!(b.step.image.is_none());
}

#[test]
fn wait_inserts_implicit_deps() {
    let p = decode(br#"{
        "version": "0",
        "steps": [
            {"type": "command", "key": "a", "cmd": "echo a"},
            {"type": "command", "key": "b", "cmd": "echo b"},
            {"type": "wait"},
            {"type": "command", "key": "c", "cmd": "echo c"}
        ]
    }"#);
    let g = PipelineGraph::build(&p).unwrap();
    let c = g.node_index_by_key("c").unwrap();
    let parents = g.parent_keys(c);
    assert!(parents.contains(&"a".to_string()));
    assert!(parents.contains(&"b".to_string()));
}
