#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use daggy::Walker;
use daggy::petgraph::visit::IntoNodeReferences;
use hm_pipeline_ir::PipelineGraph;

fn graph(json: &[u8]) -> PipelineGraph {
    serde_json::from_slice(json).unwrap()
}

fn find_by_key<'a>(g: &'a PipelineGraph, key: &str) -> &'a hm_pipeline_ir::Transition {
    let dag = g.dag();
    let (_, t) = dag
        .graph()
        .node_references()
        .find(|(_, t)| t.step.key == key)
        .unwrap();
    t
}

#[test]
fn builds_simple_chain() {
    let g = graph(
        br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "a", "cmd": "echo a", "image": "ubuntu:24.04"}, "env": {}},
                {"step": {"key": "b", "cmd": "echo b"}, "env": {}},
                {"step": {"key": "c", "cmd": "echo c"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": [
                [0, 1, "builds_in"],
                [1, 2, "builds_in"]
            ]
        }
    }"#,
    );
    assert_eq!(g.node_count(), 3);
    assert_eq!(g.default_image(), Some("ubuntu:24.04"));
}

#[test]
fn root_inherits_default_image() {
    let g = graph(
        br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "a", "cmd": "echo a", "image": "ubuntu:24.04"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": []
        }
    }"#,
    );
    let t = find_by_key(&g, "a");
    assert_eq!(t.step.image.as_deref(), Some("ubuntu:24.04"));
}

#[test]
fn child_does_not_inherit_default_image() {
    let g = graph(
        br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "a", "cmd": "echo a", "image": "ubuntu:24.04"}, "env": {}},
                {"step": {"key": "b", "cmd": "echo b"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": [
                [0, 1, "builds_in"]
            ]
        }
    }"#,
    );
    let b = find_by_key(&g, "b");
    assert!(b.step.image.is_none());
}

#[test]
fn wait_inserts_implicit_deps() {
    let g = graph(
        br#"{
        "version": "0",
        "graph": {
            "nodes": [
                {"step": {"key": "a", "cmd": "echo a"}, "env": {}},
                {"step": {"key": "b", "cmd": "echo b"}, "env": {}},
                {"step": {"key": "c", "cmd": "echo c"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": [
                [0, 2, "depends_on"],
                [1, 2, "depends_on"]
            ]
        }
    }"#,
    );
    let dag = g.dag();
    let c_idx = dag
        .graph()
        .node_references()
        .find(|(_, t)| t.step.key == "c")
        .map(|(idx, _)| idx)
        .unwrap();
    let parent_keys: Vec<String> = dag
        .parents(c_idx)
        .iter(dag)
        .map(|(_, p)| dag[p].step.key.clone())
        .collect();
    assert!(parent_keys.contains(&"a".to_string()));
    assert!(parent_keys.contains(&"b".to_string()));
}
