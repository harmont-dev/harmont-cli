"""JSON emitter -- v0 IR output shape goldens.

The wire format uses petgraph-serde graph encoding.  Cache keys are
resolved at render time and embedded in cache.key."""

from __future__ import annotations

import json
from datetime import timedelta
from pathlib import Path

from harmont import (
    forever,
    on_change,
    pipeline,
    scratch,
    ttl,
    wait,
)
from harmont.json_emit import pipeline_to_json


def _emit(p, **kw):
    kw.setdefault("env", {})
    return json.loads(pipeline_to_json(p, now=0, base_path=Path("/tmp"), **kw))  # noqa: S108


def _nodes(out):
    return out["graph"]["nodes"]


def _edges(out):
    return out["graph"]["edges"]


def _step_by_key(out, key):
    for n in _nodes(out):
        if n["step"]["key"] == key:
            return n["step"]
    msg = f"no node with key {key!r}"
    raise AssertionError(msg)


def _node_by_key(out, key):
    for n in _nodes(out):
        if n["step"]["key"] == key:
            return n
    msg = f"no node with key {key!r}"
    raise AssertionError(msg)


def _builds_in_parent_key(out, child_key):
    """Return the parent key for a child_key via builds_in edges, or None."""
    key_by_idx = {i: n["step"]["key"] for i, n in enumerate(_nodes(out))}
    idx_by_key = {v: k for k, v in key_by_idx.items()}
    child_idx = idx_by_key[child_key]
    for src, dst, kind in _edges(out):
        if kind == "builds_in" and dst == child_idx:
            return key_by_idx[src]
    return None


def test_minimal_command():
    p = pipeline([scratch().sh("echo hi", label="hello")])
    out = _emit(p)
    assert out["version"] == "0"
    assert len(_nodes(out)) == 1
    step = _nodes(out)[0]["step"]
    assert step["key"] == "hello"
    assert step["label"] == "hello"
    assert step["cmd"] == "echo hi"
    # No "type" or "builds_in" field on step dicts.
    assert "type" not in step
    assert "builds_in" not in step
    # No builds_in edges for a root step.
    assert _builds_in_parent_key(out, "hello") is None


def test_chain_parent_key_in_builds_in_edge():
    a = scratch().sh("install", label="install")
    b = a.sh("build", label="build")
    out = _emit(pipeline([b]))
    assert _builds_in_parent_key(out, "install") is None
    assert _builds_in_parent_key(out, "build") == "install"


def test_wait_step_becomes_depends_on_edges():
    out = _emit(pipeline([scratch().sh("a", label="a"), wait()]))
    # Wait produces no nodes; only the command step "a" is present.
    # (No post-wait steps in this case, so no depends_on edges either.)
    assert len(_nodes(out)) == 1
    assert _nodes(out)[0]["step"]["key"] == "a"


def test_wait_emits_depends_on_edges():
    a = scratch().sh("a", label="a")
    b = scratch().sh("b", label="b")
    out = _emit(pipeline([a, wait(), b]))
    keys = [n["step"]["key"] for n in _nodes(out)]
    idx_a = keys.index("a")
    idx_b = keys.index("b")
    depends_on = [(s, d) for s, d, k in _edges(out) if k == "depends_on"]
    assert (idx_a, idx_b) in depends_on


def test_pipeline_env_merged_into_node_env():
    out = _emit(pipeline([scratch().sh("a", label="a")], env={"CI": "true"}))
    assert _nodes(out)[0]["env"] == {"CI": "true"}


def test_default_image_emitted_when_set():
    out = _emit(pipeline([scratch().sh("a", label="a")], default_image="alpine:3"))
    assert out["default_image"] == "alpine:3"


def test_cache_ttl_resolves_key():
    p = pipeline(
        [scratch().sh("apt-get install -y curl", label="apt", cache=ttl(timedelta(days=1)))]
    )
    out = _emit(p)
    s = _nodes(out)[0]["step"]
    assert s["cache"]["policy"] == "ttl"
    assert s["cache"]["duration_seconds"] == 86400
    assert isinstance(s["cache"]["key"], str)
    assert len(s["cache"]["key"]) == 64


def test_cache_forever_with_env_keys_emitted():
    out = _emit(
        pipeline([scratch().sh("x", label="x", cache=forever(env_keys=("FOO", "BAR")))]),
        env={"FOO": "1", "BAR": "2"},
    )
    s = _nodes(out)[0]["step"]
    assert s["cache"]["policy"] == "forever"
    assert s["cache"]["env_keys"] == ["FOO", "BAR"]
    assert "key" in s["cache"]


def test_cache_on_change_paths_round_trip(tmp_path):
    (tmp_path / "a.txt").write_bytes(b"contents")
    (tmp_path / "b.txt").write_bytes(b"other")
    out = json.loads(
        pipeline_to_json(
            pipeline([scratch().sh("make", label="m", cache=on_change("a.txt", "b.txt"))]),
            now=0,
            base_path=tmp_path,
            env={},
        )
    )
    s = _nodes(out)[0]["step"]
    assert s["cache"]["policy"] == "on_change"
    assert s["cache"]["paths"] == ["a.txt", "b.txt"]
    assert "key" in s["cache"]


def test_no_optional_fields_when_not_set():
    out = _emit(pipeline([scratch().sh("x", label="x")]))
    s = _nodes(out)[0]["step"]
    assert "image" not in s
    assert "timeout_seconds" not in s
    assert "cache" not in s


def test_timeout_seconds_emitted_when_set():
    out = _emit(pipeline([scratch().sh("x", label="x", timeout_seconds=300)]))
    assert _nodes(out)[0]["step"]["timeout_seconds"] == 300


def test_image_emitted_when_set():
    out = _emit(pipeline([scratch().sh("x", label="x", image="alpine:3.19")]))
    assert _nodes(out)[0]["step"]["image"] == "alpine:3.19"


def test_command_emits_runner_and_runner_args():
    out = _emit(
        pipeline(
            [
                scratch().sh(
                    "cargo test",
                    label="t",
                    image="rust:1.82",
                    runner="freestyle",
                    runner_args={"region": "us"},
                )
            ]
        )
    )
    step = _nodes(out)[0]["step"]
    assert step["runner"] == "freestyle"
    assert step["runner_args"] == {"region": "us"}


def test_command_omits_runner_when_unset():
    out = _emit(pipeline([scratch().sh("echo hi", label="hi")]))
    step = _nodes(out)[0]["step"]
    assert "runner" not in step
    assert "runner_args" not in step


def test_multi_leaf_pipeline_emits_all_command_steps():
    a = scratch().sh("a", label="a")
    b = scratch().sh("b", label="b")
    out = _emit(pipeline([a, b]))
    keys = sorted(n["step"]["key"] for n in _nodes(out))
    assert keys == ["a", "b"]


def test_pipeline_org_and_slug_threaded_through_to_cache_key():
    """Different (org, slug) pairs produce different cache keys for the
    same step. Mirrors the namespacing in harmont_macros.scm."""
    p = pipeline([scratch().sh("x", label="x", cache=forever())])
    k1 = json.loads(
        pipeline_to_json(
            p,
            now=0,
            base_path=Path("/tmp"),  # noqa: S108
            env={},
            pipeline_org="acme",
            pipeline_slug="api",
        )
    )["graph"]["nodes"][0]["step"]["cache"]["key"]
    k2 = json.loads(
        pipeline_to_json(
            p,
            now=0,
            base_path=Path("/tmp"),  # noqa: S108
            env={},
            pipeline_org="acme",
            pipeline_slug="web",
        )
    )["graph"]["nodes"][0]["step"]["cache"]["key"]
    assert k1 != k2
