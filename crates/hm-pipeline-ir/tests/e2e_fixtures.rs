#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use daggy::petgraph::visit::{EdgeRef, IntoNodeReferences};
use hm_pipeline_ir::{EdgeKind, PipelineGraph};

const SCENARIOS: &[&str] = &[
    "monorepo-ci",
    "rust-release",
    "zig-node-polyglot",
    "kitchen-sink",
];

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

#[test]
fn python_monorepo_ci() {
    let g = load_fixture("monorepo-ci");
    assert_eq!(g.default_image(), None);
    assert!(g.node_count() >= 15, "nodes: {}", g.node_count());
    let labels = step_labels(&g);
    assert!(labels.iter().any(|l| l.contains("go")));
    assert!(
        labels
            .iter()
            .any(|l| l.contains("python") || l.contains("uv"))
    );
    assert!(
        labels
            .iter()
            .any(|l| l.contains("node") || l.contains("npm"))
    );
}

#[test]
fn python_rust_release() {
    let g = load_fixture("rust-release");
    assert_eq!(g.default_image(), None);
    assert!(g.node_count() >= 5, "nodes: {}", g.node_count());
    let labels = step_labels(&g);
    assert!(labels.iter().any(|l| l.contains("rust")));
    // The DSL now injects image on each imageless root step directly.
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

#[test]
fn python_zig_node_polyglot() {
    let g = load_fixture("zig-node-polyglot");
    assert_eq!(g.default_image(), None);
    assert!(g.node_count() >= 10, "nodes: {}", g.node_count());
    let labels = step_labels(&g);
    assert!(labels.iter().any(|l| l.contains("zig")));
    assert!(
        labels
            .iter()
            .any(|l| l.contains("node") || l.contains("npm"))
    );
}

#[test]
fn python_kitchen_sink() {
    let g = load_fixture("kitchen-sink");
    assert_eq!(g.default_image(), None);
    assert!(g.node_count() >= 10, "nodes: {}", g.node_count());
    let labels = step_labels(&g);
    assert!(labels.iter().any(|l| l.contains("python")));
    assert!(
        labels
            .iter()
            .any(|l| l.contains("cmake") || l.contains(":c:"))
    );
    for (_, t) in g.dag().graph().node_references() {
        assert!(
            t.env.contains_key("CI"),
            "node {} missing CI env",
            t.step.key
        );
    }
}

#[test]
fn all_fixtures_have_valid_structure() {
    for scenario in SCENARIOS {
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
}
