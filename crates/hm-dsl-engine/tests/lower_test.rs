#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use daggy::petgraph::visit::{EdgeRef, IntoNodeReferences};
use hm_dsl_engine::lower::lower;
use hm_dsl_engine::step_chain::RawStepChain;
use hm_pipeline_ir::{EdgeKind, PipelineGraph};

const fn kind_str(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::BuildsIn => "builds_in",
        EdgeKind::DependsOn => "depends_on",
    }
}

fn chain(json: &str) -> RawStepChain {
    serde_json::from_str(json).expect("parse RawStepChain")
}

/// Map of `key -> Transition`-ish accessors, keyed for order-independent asserts.
fn nodes_by_key(g: &PipelineGraph) -> BTreeMap<String, NodeView> {
    g.dag()
        .graph()
        .node_references()
        .map(|(_, t)| {
            (
                t.step.key.clone(),
                NodeView {
                    image: t.step.image.clone(),
                    label: t.step.label.clone(),
                    cmd: t.step.cmd.clone(),
                    env: t.env.clone(),
                    cache_policy: t.step.cache.as_ref().map(|c| c.policy.clone()),
                },
            )
        })
        .collect()
}

struct NodeView {
    image: Option<String>,
    label: Option<String>,
    cmd: String,
    env: BTreeMap<String, String>,
    cache_policy: Option<String>,
}

/// `(source_key, target_key, kind)` for every edge.
fn edges_by_key(g: &PipelineGraph) -> BTreeSet<(String, String, &'static str)> {
    let dag = g.dag();
    let graph = dag.graph();
    let key = |idx| graph.node_weight(idx).unwrap().step.key.clone();
    graph
        .edge_references()
        .map(|e| (key(e.source()), key(e.target()), kind_str(*e.weight())))
        .collect()
}

fn keys(g: &PipelineGraph) -> BTreeSet<String> {
    nodes_by_key(g).into_keys().collect()
}

#[test]
fn linear_chain_builds_in_edge() {
    // scratch -> a -> b
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": null, "parent_idx": null},
                {"cmd": "echo a", "parent_idx": 0, "label": "a"},
                {"cmd": "echo b", "parent_idx": 1, "label": "b"}
            ],
            "leaf_indices": [2]
        }"#,
    ))
    .unwrap();

    assert_eq!(g.node_count(), 2);
    let nodes = nodes_by_key(&g);
    // Root a is imageless -> stamped; child b inherits parent snapshot.
    assert_eq!(nodes["a"].image.as_deref(), Some("ubuntu:24.04"));
    assert_eq!(nodes["b"].image, None);
    assert_eq!(nodes["a"].cmd, "echo a");

    assert_eq!(
        edges_by_key(&g),
        BTreeSet::from([("a".into(), "b".into(), "builds_in")])
    );
}

#[test]
fn fork_produces_two_independent_roots() {
    // scratch -> a, scratch -> b
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": null, "parent_idx": null},
                {"cmd": "echo a", "parent_idx": 0, "label": "a"},
                {"cmd": "echo b", "parent_idx": 0, "label": "b"}
            ],
            "leaf_indices": [1, 2]
        }"#,
    ))
    .unwrap();

    assert_eq!(g.node_count(), 2);
    let nodes = nodes_by_key(&g);
    assert_eq!(nodes["a"].image.as_deref(), Some("ubuntu:24.04"));
    assert_eq!(nodes["b"].image.as_deref(), Some("ubuntu:24.04"));
    assert!(edges_by_key(&g).is_empty(), "forked roots share no edges");
}

#[test]
fn wait_barrier_creates_depends_on() {
    // pipeline([a, wait, b]): wait sits between two roots in leaf order.
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "echo a", "parent_idx": null, "label": "a"},
                {"cmd": null, "parent_idx": null, "is_wait": true},
                {"cmd": "echo b", "parent_idx": null, "label": "b"}
            ],
            "leaf_indices": [0, 1, 2]
        }"#,
    ))
    .unwrap();

    assert_eq!(g.node_count(), 2);
    assert_eq!(
        edges_by_key(&g),
        BTreeSet::from([("a".into(), "b".into(), "depends_on")])
    );
}

#[test]
fn wait_fans_out_and_in() {
    // pipeline([a, b, wait, c, d]): a,b before the barrier; c,d after.
    // Every post-wait step depends on every pre-wait step.
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "a", "parent_idx": null, "label": "a"},
                {"cmd": "b", "parent_idx": null, "label": "b"},
                {"cmd": null, "parent_idx": null, "is_wait": true},
                {"cmd": "c", "parent_idx": null, "label": "c"},
                {"cmd": "d", "parent_idx": null, "label": "d"}
            ],
            "leaf_indices": [0, 1, 2, 3, 4]
        }"#,
    ))
    .unwrap();

    assert_eq!(
        edges_by_key(&g),
        BTreeSet::from([
            ("a".into(), "c".into(), "depends_on"),
            ("a".into(), "d".into(), "depends_on"),
            ("b".into(), "c".into(), "depends_on"),
            ("b".into(), "d".into(), "depends_on"),
        ])
    );
}

#[test]
fn explicit_key_override_wins() {
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "x", "parent_idx": null, "label": "Nice Label", "key_override": "custom"}
            ],
            "leaf_indices": [0]
        }"#,
    ))
    .unwrap();
    assert_eq!(keys(&g), BTreeSet::from(["custom".to_string()]));
}

#[test]
fn slug_derived_from_label() {
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "x", "parent_idx": null, "label": ":rocket: Build & Test!"}
            ],
            "leaf_indices": [0]
        }"#,
    ))
    .unwrap();
    // ":rocket:" stripped, "&"/"!"/spaces collapse to dashes, trimmed.
    assert_eq!(keys(&g), BTreeSet::from(["build-test".to_string()]));
}

#[test]
fn labelless_step_gets_hash_key() {
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "make", "parent_idx": null}
            ],
            "leaf_indices": [0]
        }"#,
    ))
    .unwrap();
    let k = keys(&g).into_iter().next().unwrap();
    assert_eq!(k.len(), 12, "hash key is a 12-char hex prefix");
    assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
    // Byte-exact match with Python's hash_key("", "make", 0).
    assert_eq!(k, "9f3f33e47e82");
}

#[test]
fn empty_slug_falls_back_to_hash() {
    // A label that slugifies to "" (non-ASCII only) must not yield an empty key.
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "build", "parent_idx": null, "label": "构建"}
            ],
            "leaf_indices": [0]
        }"#,
    ))
    .unwrap();
    let k = keys(&g).into_iter().next().unwrap();
    assert_eq!(k.len(), 12);
    let nodes = nodes_by_key(&g);
    // Display label is preserved even though the key is hash-based.
    assert_eq!(nodes[&k].label.as_deref(), Some("构建"));
}

#[test]
fn slug_collision_both_fall_back_to_hash() {
    // Two steps share a label; neither claimed it via override -> both hash.
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "one", "parent_idx": null, "label": "Test"},
                {"cmd": "two", "parent_idx": null, "label": "Test"}
            ],
            "leaf_indices": [0, 1]
        }"#,
    ))
    .unwrap();
    let ks = keys(&g);
    assert!(!ks.contains("test"), "colliding slug must not be claimed");
    assert_eq!(ks.len(), 2, "distinct hash keys");
    assert!(ks.iter().all(|k| k.len() == 12));
}

#[test]
fn override_reserves_slug_against_peer() {
    // A step whose natural slug is "test" but key=override still "claims"
    // "test"; a peer with label "Test" cannot take the slug and hashes.
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "one", "parent_idx": null, "label": "Test", "key_override": "explicit"},
                {"cmd": "two", "parent_idx": null, "label": "Test"}
            ],
            "leaf_indices": [0, 1]
        }"#,
    ))
    .unwrap();
    let ks = keys(&g);
    assert!(ks.contains("explicit"));
    assert!(!ks.contains("test"), "peer cannot claim the reserved slug");
    // The peer got a hash, not the slug.
    let peer = ks.iter().find(|k| *k != "explicit").unwrap();
    assert_eq!(peer.len(), 12);
}

#[test]
fn natural_slug_matching_reserved_override_collides() {
    // Step 0 overrides its key to "build"; step 1's natural slug is also
    // "build" -> reserved, so step 1 hashes.
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "one", "parent_idx": null, "key_override": "build"},
                {"cmd": "two", "parent_idx": null, "label": "Build"}
            ],
            "leaf_indices": [0, 1]
        }"#,
    ))
    .unwrap();
    let ks = keys(&g);
    assert!(ks.contains("build"));
    let other = ks.iter().find(|k| *k != "build").unwrap();
    assert_eq!(other.len(), 12, "natural slug lost to the reserved override");
}

#[test]
fn env_merges_baseline_pipeline_and_step_layers() {
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "x", "parent_idx": null, "label": "a",
                 "env": {"TERM": "xterm", "STEP": "1"}}
            ],
            "leaf_indices": [0],
            "pipeline_env": {"CI": "true", "STEP": "0"}
        }"#,
    ))
    .unwrap();
    let env = &nodes_by_key(&g)["a"].env;
    assert_eq!(env.get("DEBIAN_FRONTEND").map(String::as_str), Some("noninteractive"));
    assert_eq!(env.get("CI").map(String::as_str), Some("true"));
    // Step layer overrides both baseline (TERM) and pipeline (STEP).
    assert_eq!(env.get("TERM").map(String::as_str), Some("xterm"));
    assert_eq!(env.get("STEP").map(String::as_str), Some("1"));
}

#[test]
fn scratch_and_fork_nodes_are_not_emitted() {
    // scratch(0) -> fork(1) -> a(2); fork nodes carry no cmd.
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": null, "parent_idx": null},
                {"cmd": null, "parent_idx": 0},
                {"cmd": "echo a", "parent_idx": 1, "label": "a"}
            ],
            "leaf_indices": [2]
        }"#,
    ))
    .unwrap();
    assert_eq!(g.node_count(), 1);
    assert_eq!(keys(&g), BTreeSet::from(["a".to_string()]));
    // a walks back through both passthrough nodes to no command ancestor,
    // so it is a root and gets the default image.
    assert_eq!(nodes_by_key(&g)["a"].image.as_deref(), Some("ubuntu:24.04"));
}

#[test]
fn builds_in_walks_through_scratch_nodes() {
    // a(1) -> scratch(2) -> b(3): b's nearest command ancestor is a.
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": null, "parent_idx": null},
                {"cmd": "echo a", "parent_idx": 0, "label": "a"},
                {"cmd": null, "parent_idx": 1},
                {"cmd": "echo b", "parent_idx": 2, "label": "b"}
            ],
            "leaf_indices": [3]
        }"#,
    ))
    .unwrap();
    assert_eq!(g.node_count(), 2);
    assert_eq!(
        edges_by_key(&g),
        BTreeSet::from([("a".into(), "b".into(), "builds_in")])
    );
    // b has a command ancestor -> imageless (inherits snapshot).
    assert_eq!(nodes_by_key(&g)["b"].image, None);
}

#[test]
fn explicit_image_on_root_is_not_overwritten() {
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "x", "parent_idx": null, "label": "a", "image": "alpine:3"}
            ],
            "leaf_indices": [0]
        }"#,
    ))
    .unwrap();
    assert_eq!(nodes_by_key(&g)["a"].image.as_deref(), Some("alpine:3"));
}

#[test]
fn cache_policy_lowered_to_name() {
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": "x", "parent_idx": null, "label": "a",
                 "cache": {"policy": "ttl", "duration_seconds": 3600, "env_keys": ["K"]}}
            ],
            "leaf_indices": [0]
        }"#,
    ))
    .unwrap();
    assert_eq!(nodes_by_key(&g)["a"].cache_policy.as_deref(), Some("ttl"));
}

#[test]
fn pipeline_timeout_is_carried_through() {
    let g = lower(&chain(
        r#"{
            "steps": [{"cmd": "x", "parent_idx": null, "label": "a"}],
            "leaf_indices": [0],
            "pipeline_timeout_seconds": 1800
        }"#,
    ))
    .unwrap();
    assert_eq!(g.timeout_seconds().map(std::num::NonZeroU32::get), Some(1800));
}

#[test]
fn zero_step_timeout_is_rejected_at_wire_boundary() {
    let err = lower(&chain(
        r#"{
            "steps": [{"cmd": "x", "parent_idx": null, "label": "a", "timeout_seconds": 0}],
            "leaf_indices": [0]
        }"#,
    ));
    assert!(err.is_err(), "a zero-second step timeout must be rejected");
}

#[test]
fn diamond_shares_no_duplicate_ancestors() {
    // scratch -> a -> {b, c}; both b and c build_in a exactly once.
    let g = lower(&chain(
        r#"{
            "steps": [
                {"cmd": null, "parent_idx": null},
                {"cmd": "a", "parent_idx": 0, "label": "a"},
                {"cmd": "b", "parent_idx": 1, "label": "b"},
                {"cmd": "c", "parent_idx": 1, "label": "c"}
            ],
            "leaf_indices": [2, 3]
        }"#,
    ))
    .unwrap();
    assert_eq!(g.node_count(), 3);
    assert_eq!(
        edges_by_key(&g),
        BTreeSet::from([
            ("a".into(), "b".into(), "builds_in"),
            ("a".into(), "c".into(), "builds_in"),
        ])
    );
    // a appears once in the topo order despite being reached via two leaves.
    assert_eq!(nodes_by_key(&g)["a"].image.as_deref(), Some("ubuntu:24.04"));
}
