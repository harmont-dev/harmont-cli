//! Regression test: root steps with no per-step `image` must inherit
//! the pipeline's `default_image`. Without this, the docker runner's
//! `resolve_image` falls back to `alpine:latest` and any apt-get
//! command in a ubuntu-targeted example dies with
//! `sh: apt-get: not found`.

#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration test pinning a tiny invariant"
)]

use daggy::petgraph::visit::IntoNodeReferences;
use hm_pipeline_ir::PipelineGraph;
use rstest::rstest;

fn decode(json: &[u8]) -> PipelineGraph {
    serde_json::from_slice::<PipelineGraph>(json).unwrap()
}

fn find_step<'a>(g: &'a PipelineGraph, key: &str) -> &'a hm_pipeline_ir::Step {
    let dag = g.dag();
    let (_, t) = dag
        .graph()
        .node_references()
        .find(|(_, t)| t.step.key == key)
        .unwrap();
    &t.step
}

// Children boot from the parent's committed snapshot at runtime, not from an
// image tag — leaving `child`'s image=None is the correct wire state for chain
// steps. Likewise, an absent `default_image` must never synthesize an image.
#[rstest]
#[case::root_inherits(
    br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "apt-base", "action": {"cmd": "apt-get update"}, "image": "ubuntu:24.04"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": []
        }
    }"#,
    "apt-base",
    Some("ubuntu:24.04")
)]
#[case::explicit_wins(
    br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "rust", "action": {"cmd": "cargo build"}, "image": "rust:1.82"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": []
        }
    }"#,
    "rust",
    Some("rust:1.82")
)]
#[case::child_none(
    br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "parent", "action": {"cmd": "echo p"}, "image": "ubuntu:24.04"}, "env": {}},
                {"step": {"key": "child",  "action": {"cmd": "echo c"}}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": [
                [0, 1, "builds_in"]
            ]
        }
    }"#,
    "child",
    None
)]
#[case::no_default(
    br#"{
        "version": "0",
        "graph": {
            "nodes": [
                {"step": {"key": "k", "action": {"cmd": "true"}}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": []
        }
    }"#,
    "k",
    None
)]
fn default_image_resolves(
    #[case] json: &[u8],
    #[case] step_key: &str,
    #[case] expected: Option<&str>,
) {
    let g = decode(json);
    let step = find_step(&g, step_key);
    assert_eq!(step.image.as_deref(), expected);
}
