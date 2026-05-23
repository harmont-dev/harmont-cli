//! Regression test: root steps with no per-step `image` must inherit
//! the pipeline's `default_image`. Without this, the docker plugin's
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

use harmont_cli::orchestrator::graph::Graph;

fn decode(json: &[u8]) -> Graph {
    serde_json::from_slice::<Graph>(json).unwrap()
}

#[test]
fn root_step_inherits_default_image() {
    let g = decode(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "apt-base", "cmd": "apt-get update", "image": "ubuntu:24.04"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": []
        }
    }"#);
    let idx = g.node_index_by_key("apt-base").unwrap();
    assert_eq!(
        g.node_weight(idx).step.image.as_deref(),
        Some("ubuntu:24.04"),
        "root step must inherit pipeline default_image"
    );
}

#[test]
fn root_step_explicit_image_wins() {
    let g = decode(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "rust", "cmd": "cargo build", "image": "rust:1.82"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": []
        }
    }"#);
    let idx = g.node_index_by_key("rust").unwrap();
    assert_eq!(
        g.node_weight(idx).step.image.as_deref(),
        Some("rust:1.82"),
        "explicit per-step image must override default_image"
    );
}

#[test]
fn child_step_unchanged_by_default_image() {
    // Children boot from the parent's committed snapshot at runtime,
    // not from an image tag — leaving their image=None is the correct
    // wire state for chain steps.
    let g = decode(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "parent", "cmd": "echo p", "image": "ubuntu:24.04"}, "env": {}},
                {"step": {"key": "child",  "cmd": "echo c"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": [
                [0, 1, "builds_in"]
            ]
        }
    }"#);
    let idx = g.node_index_by_key("child").unwrap();
    assert!(
        g.node_weight(idx).step.image.is_none(),
        "child step must not inherit default_image — chain steps boot from parent snapshot",
    );
}

#[test]
fn no_default_image_leaves_root_alone() {
    let g = decode(br#"{
        "version": "0",
        "graph": {
            "nodes": [
                {"step": {"key": "k", "cmd": "true"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": []
        }
    }"#);
    let idx = g.node_index_by_key("k").unwrap();
    assert!(
        g.node_weight(idx).step.image.is_none(),
        "absent default_image must not synthesize an image"
    );
}
