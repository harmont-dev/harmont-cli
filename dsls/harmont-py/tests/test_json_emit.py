"""JSON emitter — v0 IR output shape goldens.

The wire format mirrors harmont-pipeline/src/Harmont/Pipeline/Schema.hs.
Optional fields are omitted (not null); `builds_in: null` only when
the step has no parent (scratch). Cache keys are resolved at render
time and embedded in cache.key."""

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


def test_minimal_command():
    p = pipeline(scratch().sh("echo hi", label="hello"))
    out = _emit(p)
    assert out == {
        "version": "0",
        "steps": [
            {
                "type": "command",
                "key": "hello",
                "label": "hello",
                "cmd": "echo hi",
                "builds_in": None,
            },
        ],
    }


def test_chain_parent_key_in_builds_in():
    a = scratch().sh("install", label="install")
    b = a.sh("build", label="build")
    out = _emit(pipeline(b))
    by_key = {s["key"]: s for s in out["steps"]}
    assert by_key["install"]["builds_in"] is None
    assert by_key["build"]["builds_in"] == "install"


def test_wait_step():
    out = _emit(pipeline(scratch().sh("a", label="a"), wait()))
    types = [s["type"] for s in out["steps"]]
    assert types == ["command", "wait"]


def test_wait_continue_on_failure_emitted():
    out = _emit(pipeline(scratch().sh("a", label="a"), wait(continue_on_failure=True)))
    assert out["steps"][-1] == {"type": "wait", "continue_on_failure": True}


def test_pipeline_env_emitted_as_object():
    out = _emit(pipeline(scratch().sh("a", label="a"), env={"CI": "true"}))
    assert out["env"] == {"CI": "true"}


def test_default_image_emitted_when_set():
    out = _emit(pipeline(scratch().sh("a", label="a"), default_image="alpine:3"))
    assert out["default_image"] == "alpine:3"


def test_cache_ttl_resolves_key():
    p = pipeline(
        scratch().sh("apt-get install -y curl", label="apt", cache=ttl(timedelta(days=1)))
    )
    out = _emit(p)
    s = out["steps"][0]
    assert s["cache"]["policy"] == "ttl"
    assert s["cache"]["duration_seconds"] == 86400
    assert isinstance(s["cache"]["key"], str)
    assert len(s["cache"]["key"]) == 64


def test_cache_forever_with_env_keys_emitted():
    out = _emit(
        pipeline(scratch().sh("x", label="x", cache=forever(env_keys=("FOO", "BAR")))),
        env={"FOO": "1", "BAR": "2"},
    )
    s = out["steps"][0]
    assert s["cache"]["policy"] == "forever"
    assert s["cache"]["env_keys"] == ["FOO", "BAR"]
    assert "key" in s["cache"]


def test_cache_on_change_paths_round_trip(tmp_path):
    (tmp_path / "a.txt").write_bytes(b"contents")
    (tmp_path / "b.txt").write_bytes(b"other")
    out = json.loads(
        pipeline_to_json(
            pipeline(scratch().sh("make", label="m", cache=on_change("a.txt", "b.txt"))),
            now=0,
            base_path=tmp_path,
            env={},
        )
    )
    s = out["steps"][0]
    assert s["cache"]["policy"] == "on_change"
    assert s["cache"]["paths"] == ["a.txt", "b.txt"]
    assert "key" in s["cache"]


def test_no_optional_fields_when_not_set():
    out = _emit(pipeline(scratch().sh("x", label="x")))
    s = out["steps"][0]
    assert "image" not in s
    assert "env" not in s
    assert "timeout_seconds" not in s
    assert "cache" not in s


def test_timeout_seconds_emitted_when_set():
    out = _emit(pipeline(scratch().sh("x", label="x", timeout_seconds=300)))
    assert out["steps"][0]["timeout_seconds"] == 300


def test_image_emitted_when_set():
    out = _emit(pipeline(scratch().sh("x", label="x", image="alpine:3.19")))
    assert out["steps"][0]["image"] == "alpine:3.19"


def test_command_emits_runner_and_runner_args():
    out = _emit(
        pipeline(
            scratch().sh(
                "cargo test",
                label="t",
                image="rust:1.82",
                runner="freestyle",
                runner_args={"region": "us"},
            )
        )
    )
    cmd = next(s for s in out["steps"] if s["type"] == "command")
    assert cmd["runner"] == "freestyle"
    assert cmd["runner_args"] == {"region": "us"}


def test_command_omits_runner_when_unset():
    out = _emit(pipeline(scratch().sh("echo hi", label="hi")))
    cmd = next(s for s in out["steps"] if s["type"] == "command")
    assert "runner" not in cmd
    assert "runner_args" not in cmd


def test_multi_leaf_pipeline_emits_all_command_steps():
    a = scratch().sh("a", label="a")
    b = scratch().sh("b", label="b")
    out = _emit(pipeline(a, b))
    keys = sorted(s["key"] for s in out["steps"] if s["type"] == "command")
    assert keys == ["a", "b"]


def test_pipeline_org_and_slug_threaded_through_to_cache_key():
    """Different (org, slug) pairs produce different cache keys for the
    same step. Mirrors the namespacing in harmont_macros.scm."""
    p = pipeline(scratch().sh("x", label="x", cache=forever()))
    k1 = json.loads(
        pipeline_to_json(
            p,
            now=0,
            base_path=Path("/tmp"),  # noqa: S108
            env={},
            pipeline_org="acme",
            pipeline_slug="api",
        )
    )["steps"][0]["cache"]["key"]
    k2 = json.loads(
        pipeline_to_json(
            p,
            now=0,
            base_path=Path("/tmp"),  # noqa: S108
            env={},
            pipeline_org="acme",
            pipeline_slug="web",
        )
    )["steps"][0]["cache"]["key"]
    assert k1 != k2
