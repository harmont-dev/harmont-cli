#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use hm_pipeline_ir::PipelineGraph;

fn graph(json: &[u8]) -> PipelineGraph {
    serde_json::from_slice(json).unwrap()
}

#[test]
fn builds_simple_chain() {
    let g = graph(br#"{
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
    }"#);
    assert_eq!(g.node_count(), 3);
    assert_eq!(g.default_image(), Some("ubuntu:24.04"));
}

#[test]
fn root_inherits_default_image() {
    let g = graph(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "a", "cmd": "echo a", "image": "ubuntu:24.04"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": []
        }
    }"#);
    let node = g.get_transition(g.node_index_by_key("a").unwrap());
    assert_eq!(node.step.image.as_deref(), Some("ubuntu:24.04"));
}

#[test]
fn child_does_not_inherit_default_image() {
    let g = graph(br#"{
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
    }"#);
    let b = g.get_transition(g.node_index_by_key("b").unwrap());
    assert!(b.step.image.is_none());
}

#[test]
fn wait_inserts_implicit_deps() {
    let g = graph(br#"{
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
    }"#);
    let c = g.node_index_by_key("c").unwrap();
    let parents = g.parent_keys(c);
    assert!(parents.contains(&"a".to_string()));
    assert!(parents.contains(&"b".to_string()));
}

#[test]
fn chain_detection() {
    let g = graph(br#"{
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
    }"#);
    let a = g.node_index_by_key("a").unwrap();
    let b = g.node_index_by_key("b").unwrap();
    let c = g.node_index_by_key("c").unwrap();
    assert!(!g.is_chain_step(a));
    assert!(g.is_chain_step(b));
    assert!(g.is_chain_step(c));
}

#[test]
fn fork_breaks_chain() {
    let g = graph(br#"{
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
                [0, 2, "builds_in"]
            ]
        }
    }"#);
    let b = g.node_index_by_key("b").unwrap();
    let c = g.node_index_by_key("c").unwrap();
    assert!(!g.is_chain_step(b));
    assert!(!g.is_chain_step(c));
}

#[test]
fn chains_partition_includes_every_node_once() {
    let g = graph(br#"{
        "version": "0",
        "default_image": "ubuntu:24.04",
        "graph": {
            "nodes": [
                {"step": {"key": "a", "cmd": "echo a", "image": "ubuntu:24.04"}, "env": {}},
                {"step": {"key": "b", "cmd": "echo b"}, "env": {}},
                {"step": {"key": "c", "cmd": "echo c"}, "env": {}},
                {"step": {"key": "d", "cmd": "echo d"}, "env": {}},
                {"step": {"key": "e", "cmd": "echo e", "image": "ubuntu:24.04"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": [
                [0, 1, "builds_in"],
                [1, 2, "builds_in"],
                [0, 3, "builds_in"]
            ]
        }
    }"#);
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
    let g = graph(br#"{
        "version": "0",
        "graph": {
            "nodes": [
                {"step": {"key": "a", "cmd": "echo a"}, "env": {}},
                {"step": {"key": "b", "cmd": "echo b"}, "env": {}},
                {"step": {"key": "c", "cmd": "echo c"}, "env": {}},
                {"step": {"key": "d", "cmd": "echo d"}, "env": {}},
                {"step": {"key": "e", "cmd": "echo e"}, "env": {}}
            ],
            "edge_property": "directed",
            "edges": [
                [0, 1, "builds_in"],
                [1, 2, "builds_in"],
                [0, 3, "builds_in"]
            ]
        }
    }"#);
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
    let g = graph(br#"{
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
    }"#);
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
