"""Lowering: walk leaves back to scratch, topo-sort, emit JSON-shaped dicts.

The lowering pass returns intermediate Python dicts (the same shape
the JSON IR will have, before the codegen pass produces Scheme). This
test asserts on that intermediate, not on Scheme strings — Scheme
output is covered by test_codegen.py.
"""

from __future__ import annotations

import pytest

from harmont._step import scratch, wait
from harmont.pipeline import _lower_to_dicts, pipeline


def test_single_chain_emits_three_command_dicts_in_parent_order():
    a = scratch().sh("step a", label="a")
    b = a.sh("step b", label="b")
    c = b.sh("step c", label="c")
    dicts = _lower_to_dicts([c])
    assert [d["type"] for d in dicts] == ["command", "command", "command"]
    assert [d["key"] for d in dicts] == ["a", "b", "c"]
    assert dicts[0]["builds_in"] is None
    assert dicts[1]["builds_in"] == "a"
    assert dicts[2]["builds_in"] == "b"


def test_fork_node_is_not_emitted_children_inherit_grandparent():
    base = scratch().sh("install", label="install")
    branch = base.fork(label="branch-a")
    leaf = branch.sh("test", label="test")
    dicts = _lower_to_dicts([leaf])
    keys = [d["key"] for d in dicts]
    parents = {d["key"]: d["builds_in"] for d in dicts}
    assert keys == ["install", "test"]
    assert parents["install"] is None
    assert parents["test"] == "install"


def test_two_branches_share_parent_key():
    base = scratch().sh("install", label="install")
    a = base.fork(label="a").sh("test-a", label="test-a")
    b = base.fork(label="b").sh("test-b", label="test-b")
    dicts = _lower_to_dicts([a, b])
    parents = {d["key"]: d["builds_in"] for d in dicts}
    assert parents["test-a"] == "install"
    assert parents["test-b"] == "install"


def test_wait_step_emitted_in_position():
    a = scratch().sh("a", label="a")
    b = scratch().sh("b", label="b")
    c = scratch().sh("c", label="c")
    dicts = _lower_to_dicts([a, b, wait(), c])
    types = [d["type"] for d in dicts]
    assert "wait" in types
    wait_idx = types.index("wait")
    keys_before = [d["key"] for d in dicts[:wait_idx]]
    keys_after = [d["key"] for d in dicts[wait_idx + 1 :]]
    assert "a" in keys_before
    assert "b" in keys_before
    assert "c" in keys_after


def test_wait_continue_on_failure_carried_through():
    a = scratch().sh("a", label="a")
    dicts = _lower_to_dicts([a, wait(continue_on_failure=True)])
    wait_dict = next(d for d in dicts if d["type"] == "wait")
    assert wait_dict["continue_on_failure"] is True


def test_command_includes_label_env_timeout_when_set():
    s = scratch().sh(
        "make",
        label="build",
        env={"CI": "true"},
        timeout_seconds=600,
    )
    dicts = _lower_to_dicts([s])
    assert dicts[0]["label"] == "build"
    assert dicts[0]["env"] == {"CI": "true"}
    assert dicts[0]["timeout_seconds"] == 600


def test_command_omits_optional_fields_when_unset():
    s = scratch().sh("make")
    d = _lower_to_dicts([s])[0]
    # Required fields present.
    assert d["type"] == "command"
    assert "key" in d
    assert "cmd" in d
    assert "builds_in" in d
    # Optional fields omitted (not None) when unset.
    assert "label" not in d
    assert "env" not in d
    assert "timeout_seconds" not in d
    assert "cache" not in d


def test_pipeline_factory_collects_reachable_via_parent():
    base = scratch().sh("install", label="install")
    leaf_a = base.fork(label="a").sh("test-a", label="test-a")
    leaf_b = base.fork(label="b").sh("test-b", label="test-b")
    p = pipeline(leaf_a, leaf_b, env={"CI": "true"})
    keys = [s["key"] for s in p["steps"]]
    assert set(keys) == {"install", "test-a", "test-b"}
    assert p["env"] == {"CI": "true"}
    assert p["version"] == "0"


def test_pipeline_with_no_leaves_raises():
    with pytest.raises(ValueError, match="at least one leaf"):
        pipeline()


def test_dedup_when_step_reachable_from_multiple_leaves():
    base = scratch().sh("install", label="install")
    a = base.sh("a", label="a")
    b = base.sh("b", label="b")
    p = pipeline(a, b)
    keys = [s["key"] for s in p["steps"]]
    # `install` appears once even though it's reachable from both leaves.
    assert keys.count("install") == 1
