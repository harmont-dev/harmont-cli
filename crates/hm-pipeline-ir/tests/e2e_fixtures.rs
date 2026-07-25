#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration test fixtures and assertions"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use daggy::petgraph::visit::{EdgeRef, IntoNodeReferences};
use hm_pipeline_ir::{EdgeKind, PipelineGraph};
use rstest::rstest;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/fixtures")
}

fn load_fixture(scenario: &str) -> PipelineGraph {
    let path = fixtures_dir()
        .join("python")
        .join(format!("{scenario}.json"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse py/{scenario}: {e}"))
}

fn step_labels(g: &PipelineGraph) -> BTreeSet<String> {
    g.dag()
        .graph()
        .node_references()
        .filter_map(|(_, t)| t.step.label.clone())
        .collect()
}

fn edge_kinds(g: &PipelineGraph) -> (usize, usize) {
    let mut builds_in = 0usize;
    let mut depends_on = 0usize;
    for e in g.dag().graph().edge_references() {
        match e.weight() {
            EdgeKind::BuildsIn => builds_in += 1,
            EdgeKind::DependsOn => depends_on += 1,
        }
    }
    (builds_in, depends_on)
}

/// Shared shape check: no pipeline `default_image`, a minimum node count, and
/// at least one label matching each required group of substring alternatives.
#[rstest]
#[case::monorepo_ci("monorepo-ci", 15, &[&["go"] as &[&str], &["python", "uv"], &["node", "npm"]])]
#[case::rust_release("rust-release", 5, &[&["rust"] as &[&str]])]
#[case::zig_node_polyglot("zig-node-polyglot", 10, &[&["zig"] as &[&str], &["node", "npm"]])]
#[case::kitchen_sink("kitchen-sink", 10, &[&["python"] as &[&str], &["cmake", ":c:"]])]
fn fixture_has_expected_shape(
    #[case] scenario: &str,
    #[case] min_nodes: usize,
    #[case] required_label_substrings: &[&[&str]],
) {
    let g = load_fixture(scenario);
    assert_eq!(g.default_image(), None);
    assert!(g.node_count() >= min_nodes, "nodes: {}", g.node_count());
    let labels = step_labels(&g);
    for group in required_label_substrings {
        assert!(
            labels
                .iter()
                .any(|l| group.iter().any(|needle| l.contains(needle))),
            "py/{scenario}: no label matched any of {group:?}"
        );
    }
}

// The DSL now injects image on each imageless root step directly; unique to the
// rust-release fixture, so kept out of the shared shape check.
#[rstest]
fn python_rust_release_root_step_carries_explicit_image() {
    let g = load_fixture("rust-release");
    let apt_base = g
        .dag()
        .graph()
        .node_references()
        .find(|(_, t)| t.step.key == "apt-base")
        .map(|(_, t)| t.step.image.as_deref());
    assert_eq!(
        apt_base,
        Some(Some("ubuntu:24.04")),
        "root step apt-base must carry explicit image"
    );
}

// Unique to the kitchen-sink fixture: every node inherits a `CI` env var. Loops
// over dynamic graph nodes (not fixed literals), so this stays a loop.
#[rstest]
fn python_kitchen_sink_all_nodes_have_ci_env() {
    let g = load_fixture("kitchen-sink");
    for (_, t) in g.dag().graph().node_references() {
        assert!(
            t.env.contains_key("CI"),
            "node {} missing CI env",
            t.step.key
        );
    }
}

#[rstest]
#[case::monorepo_ci("monorepo-ci")]
#[case::rust_release("rust-release")]
#[case::zig_node_polyglot("zig-node-polyglot")]
#[case::kitchen_sink("kitchen-sink")]
fn fixture_has_valid_structure(#[case] scenario: &str) {
    let g = load_fixture(scenario);

    for (_, t) in g.dag().graph().node_references() {
        assert!(!t.step.key.is_empty(), "py/{scenario}: empty key");
        assert!(
            !t.step.cmd.is_empty(),
            "py/{scenario}: empty cmd for {}",
            t.step.key,
        );
    }

    let (bi, dep) = edge_kinds(&g);
    assert!(bi + dep > 0, "py/{scenario}: no edges");

    for e in g.dag().graph().edge_references() {
        assert_ne!(e.source(), e.target(), "py/{scenario}: self-loop");
    }
}
