"""Lowering: walk leaves back to scratch, topo-sort, emit graph-format dicts.

The lowering pass returns an intermediate Python dict (the petgraph-serde
graph shape the JSON IR will have). This test asserts on that
intermediate graph structure.
"""

from __future__ import annotations

import pytest

import harmont as hm
from harmont._pipeline import _lower_to_graph, pipeline
from harmont._step import scratch, wait


def _nodes(graph: dict) -> list[dict]:
    return graph["nodes"]


def _edges(graph: dict) -> list[list]:
    return graph["edges"]


def _step_keys(graph: dict) -> list[str]:
    return [n["step"]["key"] for n in graph["nodes"]]


def _builds_in_edges(graph: dict) -> list[tuple[int, int]]:
    return [(src, dst) for src, dst, kind in graph["edges"] if kind == "builds_in"]


def _depends_on_edges(graph: dict) -> list[tuple[int, int]]:
    return [(src, dst) for src, dst, kind in graph["edges"] if kind == "depends_on"]


def _parent_key_map(graph: dict) -> dict[str, str | None]:
    """Return {child_key: parent_key} for builds_in edges."""
    key_by_idx = {i: n["step"]["key"] for i, n in enumerate(graph["nodes"])}
    result: dict[str, str | None] = {}
    # Start with all keys having no parent.
    for n in graph["nodes"]:
        result[n["step"]["key"]] = None
    for src, dst, kind in graph["edges"]:
        if kind == "builds_in":
            result[key_by_idx[dst]] = key_by_idx[src]
    return result


def test_single_chain_emits_three_command_nodes_in_parent_order():
    a = scratch().sh("step a", label="a")
    b = a.sh("step b", label="b")
    c = b.sh("step c", label="c")
    graph = _lower_to_graph([c])
    assert _step_keys(graph) == ["a", "b", "c"]
    parents = _parent_key_map(graph)
    assert parents["a"] is None
    assert parents["b"] == "a"
    assert parents["c"] == "b"


def test_fork_node_is_not_emitted_children_inherit_grandparent():
    base = scratch().sh("install", label="install")
    branch = base.fork(label="branch-a")
    leaf = branch.sh("test", label="test")
    graph = _lower_to_graph([leaf])
    keys = _step_keys(graph)
    parents = _parent_key_map(graph)
    assert keys == ["install", "test"]
    assert parents["install"] is None
    assert parents["test"] == "install"


def test_two_branches_share_parent_key():
    base = scratch().sh("install", label="install")
    a = base.fork(label="a").sh("test-a", label="test-a")
    b = base.fork(label="b").sh("test-b", label="test-b")
    graph = _lower_to_graph([a, b])
    parents = _parent_key_map(graph)
    assert parents["test-a"] == "install"
    assert parents["test-b"] == "install"


def test_wait_step_emitted_as_depends_on_edges():
    a = scratch().sh("a", label="a")
    b = scratch().sh("b", label="b")
    c = scratch().sh("c", label="c")
    graph = _lower_to_graph([a, b, wait(), c])
    keys = _step_keys(graph)
    assert "a" in keys
    assert "b" in keys
    assert "c" in keys
    # c should have depends_on edges from a and b.
    depends_on = _depends_on_edges(graph)
    idx_a = keys.index("a")
    idx_b = keys.index("b")
    idx_c = keys.index("c")
    assert (idx_a, idx_c) in depends_on
    assert (idx_b, idx_c) in depends_on


def test_command_includes_label_env_timeout_when_set():
    s = hm.timeout(
        600,
        scratch().sh("make", label="build", env={"CI": "true"}),
    )
    graph = _lower_to_graph([s])
    node = graph["nodes"][0]
    assert node["step"]["label"] == "build"
    assert node["env"]["CI"] == "true"
    assert node["env"]["DEBIAN_FRONTEND"] == "noninteractive"
    assert node["step"]["timeout_seconds"] == 600


def test_command_omits_optional_fields_when_unset():
    s = scratch().sh("make")
    graph = _lower_to_graph([s])
    step = graph["nodes"][0]["step"]
    # Required fields present.
    assert "key" in step
    assert "cmd" in step
    # No "type" or "builds_in" fields in the new format.
    assert "type" not in step
    assert "builds_in" not in step
    # Optional fields omitted (not None) when unset.
    assert "label" not in step
    assert "timeout_seconds" not in step
    assert "cache" not in step


def test_pipeline_factory_collects_reachable_via_parent():
    base = scratch().sh("install", label="install")
    leaf_a = base.fork(label="a").sh("test-a", label="test-a")
    leaf_b = base.fork(label="b").sh("test-b", label="test-b")
    p = pipeline([leaf_a, leaf_b], env={"CI": "true"})
    keys = _step_keys(p["graph"])
    assert set(keys) == {"install", "test-a", "test-b"}
    # Pipeline-level env is merged into every node.
    for node in p["graph"]["nodes"]:
        assert "CI" in node["env"]
    assert p["version"] == "0"


def test_pipeline_with_no_leaves_raises():
    with pytest.raises(ValueError, match="at least one leaf"):
        pipeline([])


def test_dedup_when_step_reachable_from_multiple_leaves():
    base = scratch().sh("install", label="install")
    a = base.sh("a", label="a")
    b = base.sh("b", label="b")
    p = pipeline([a, b])
    keys = _step_keys(p["graph"])
    # `install` appears once even though it's reachable from both leaves.
    assert keys.count("install") == 1
