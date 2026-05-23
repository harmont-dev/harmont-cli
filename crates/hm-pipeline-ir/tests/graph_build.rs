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

#[test]
fn chain_detection() {
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
    let a = g.node_index_by_key("a").unwrap();
    let b = g.node_index_by_key("b").unwrap();
    let c = g.node_index_by_key("c").unwrap();
    assert!(!g.is_chain_step(a));
    assert!(g.is_chain_step(b));
    assert!(g.is_chain_step(c));
}

#[test]
fn fork_breaks_chain() {
    let p = decode(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "steps": [
            {"type": "command", "key": "a", "cmd": "echo a"},
            {"type": "command", "key": "b", "cmd": "echo b", "builds_in": "a"},
            {"type": "command", "key": "c", "cmd": "echo c", "builds_in": "a"}
        ]
    }"#);
    let g = PipelineGraph::build(&p).unwrap();
    let b = g.node_index_by_key("b").unwrap();
    let c = g.node_index_by_key("c").unwrap();
    assert!(!g.is_chain_step(b));
    assert!(!g.is_chain_step(c));
}

#[test]
fn chains_partition_includes_every_node_once() {
    let p = decode(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "steps": [
            {"type": "command", "key": "a", "cmd": "echo a"},
            {"type": "command", "key": "b", "cmd": "echo b", "builds_in": "a"},
            {"type": "command", "key": "c", "cmd": "echo c", "builds_in": "b"},
            {"type": "command", "key": "d", "cmd": "echo d", "builds_in": "a"},
            {"type": "command", "key": "e", "cmd": "echo e"}
        ]
    }"#);
    let g = PipelineGraph::build(&p).unwrap();
    let chains = g.chains();
    let mut all_nodes: Vec<_> = chains.iter().flatten().copied().collect();
    all_nodes.sort();
    assert_eq!(all_nodes.len(), 5, "every node in exactly one chain");

    let b = g.node_index_by_key("b").unwrap();
    let c = g.node_index_by_key("c").unwrap();
    let bc_chain = chains.iter().find(|ch| ch.contains(&b)).unwrap();
    assert_eq!(*bc_chain, vec![b, c]);
}

#[test]
fn chain_deps_cross_chain() {
    let p = decode(br#"{
        "version": "0",
        "steps": [
            {"type": "command", "key": "a", "cmd": "echo a"},
            {"type": "command", "key": "b", "cmd": "echo b", "builds_in": "a"},
            {"type": "command", "key": "c", "cmd": "echo c", "builds_in": "b"},
            {"type": "command", "key": "d", "cmd": "echo d", "builds_in": "a"},
            {"type": "command", "key": "e", "cmd": "echo e"}
        ]
    }"#);
    let g = PipelineGraph::build(&p).unwrap();
    let chains = g.chains();
    let deps = g.chain_deps(&chains);

    let find_chain = |key: &str| -> usize {
        let idx = g.node_index_by_key(key).unwrap();
        chains.iter().position(|ch| ch.contains(&idx)).unwrap()
    };
    let a_ci = find_chain("a");
    let bc_ci = find_chain("b");
    let d_ci = find_chain("d");
    let e_ci = find_chain("e");

    assert!(deps[a_ci].is_empty());
    assert_eq!(deps[bc_ci], vec![a_ci]);
    assert_eq!(deps[d_ci], vec![a_ci]);
    assert!(deps[e_ci].is_empty());
}

#[test]
fn chain_deps_subsumes_wait_barriers() {
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
    let chains = g.chains();
    let deps = g.chain_deps(&chains);
    let find_chain = |key: &str| -> usize {
        let idx = g.node_index_by_key(key).unwrap();
        chains.iter().position(|ch| ch.contains(&idx)).unwrap()
    };
    let a_ci = find_chain("a");
    let b_ci = find_chain("b");
    let c_ci = find_chain("c");
    let mut c_deps = deps[c_ci].clone();
    c_deps.sort_unstable();
    let mut want = vec![a_ci, b_ci];
    want.sort_unstable();
    assert_eq!(c_deps, want);
}
