"""Step-chain serializer: Step chains -> RawStepChain wire dict."""

from __future__ import annotations

from datetime import timedelta

import harmont as hm
from harmont._serialize import serialize_step_chain
from harmont._step import scratch, wait
from harmont.cache import (
    CacheCompose,
    CacheForever,
    CacheNone,
    CacheOnChange,
    CacheTTL,
)


def _by_cmd(out, cmd):
    """Return the single serialized step whose cmd matches."""
    matches = [s for s in out["steps"] if s["cmd"] == cmd]
    assert len(matches) == 1, f"expected exactly one step with cmd={cmd!r}"
    return matches[0]


def test_linear_chain_parent_indices():
    a = scratch().sh("a")
    b = a.sh("b")
    out = serialize_step_chain([b])

    # scratch root + a + b
    assert len(out["steps"]) == 3
    assert out["leaf_indices"] == [out["steps"].index(_by_cmd(out, "b"))]

    root = _by_cmd(out, None)
    step_a = _by_cmd(out, "a")
    step_b = _by_cmd(out, "b")
    root_idx = out["steps"].index(root)
    a_idx = out["steps"].index(step_a)

    assert root["parent_idx"] is None
    assert step_a["parent_idx"] == root_idx
    assert step_b["parent_idx"] == a_idx


def test_parent_before_child_ordering():
    b = scratch().sh("a").sh("b")
    out = serialize_step_chain([b])
    # Each step's parent_idx must reference an earlier entry.
    for i, s in enumerate(out["steps"]):
        if s["parent_idx"] is not None:
            assert s["parent_idx"] < i


def test_fork_shares_ancestor_dedup():
    base = scratch().sh("install")
    left = base.fork().sh("test-left")
    right = base.fork().sh("test-right")
    out = serialize_step_chain([left, right])

    # 'install' and its scratch root appear exactly once each.
    assert len([s for s in out["steps"] if s["cmd"] == "install"]) == 1
    assert len(out["leaf_indices"]) == 2

    install_idx = out["steps"].index(_by_cmd(out, "install"))
    # Both fork nodes descend (transitively) from the shared install step.
    for cmd in ("test-left", "test-right"):
        leaf = _by_cmd(out, cmd)
        # leaf.parent is the fork passthrough; walk one hop up.
        fork_idx = leaf["parent_idx"]
        assert out["steps"][fork_idx]["parent_idx"] == install_idx


def test_distinct_objects_keep_distinct_indices():
    # Structurally identical but distinct leaves must not be deduped.
    a = scratch().sh("same")
    b = scratch().sh("same")
    out = serialize_step_chain([a, b])
    assert len([s for s in out["steps"] if s["cmd"] == "same"]) == 2
    assert len(out["leaf_indices"]) == 2
    assert len(set(out["leaf_indices"])) == 2


def test_wait_barrier_serialized():
    build = hm.sh("make build")
    barrier = wait()
    deploy = hm.sh("make deploy")
    out = serialize_step_chain([build, barrier, deploy])

    wait_steps = [s for s in out["steps"] if s.get("is_wait")]
    assert len(wait_steps) == 1
    w = wait_steps[0]
    assert w["is_wait"] is True
    assert w["cmd"] is None
    # continue_on_failure defaults false -> omitted.
    assert "continue_on_failure" not in w


def test_wait_continue_on_failure():
    out = serialize_step_chain([wait(continue_on_failure=True)])
    w = out["steps"][out["leaf_indices"][0]]
    assert w["is_wait"] is True
    assert w["continue_on_failure"] is True


def test_cache_none():
    s = scratch().sh("x", cache=CacheNone())
    out = serialize_step_chain([s])
    assert _by_cmd(out, "x")["cache"] == {"policy": "none"}


def test_cache_forever_with_env_keys():
    s = scratch().sh("x", cache=CacheForever(env_keys=("A", "B")))
    out = serialize_step_chain([s])
    assert _by_cmd(out, "x")["cache"] == {
        "policy": "forever",
        "env_keys": ["A", "B"],
    }


def test_cache_ttl():
    s = scratch().sh("x", cache=CacheTTL(duration=timedelta(hours=2), env_keys=("K",)))
    out = serialize_step_chain([s])
    assert _by_cmd(out, "x")["cache"] == {
        "policy": "ttl",
        "duration_seconds": 7200,
        "env_keys": ["K"],
    }


def test_cache_on_change():
    s = scratch().sh("x", cache=CacheOnChange(paths=("Cargo.toml", "Cargo.lock")))
    out = serialize_step_chain([s])
    assert _by_cmd(out, "x")["cache"] == {
        "policy": "on_change",
        "paths": ["Cargo.toml", "Cargo.lock"],
    }


def test_cache_compose():
    s = scratch().sh(
        "x",
        cache=CacheCompose(
            policies=(
                CacheTTL(duration=timedelta(days=1)),
                CacheOnChange(paths=("api/cabal.project",)),
            )
        ),
    )
    out = serialize_step_chain([s])
    assert _by_cmd(out, "x")["cache"] == {
        "policy": "compose",
        "sub_policies": [
            {"policy": "ttl", "duration_seconds": 86400, "env_keys": []},
            {"policy": "on_change", "paths": ["api/cabal.project"]},
        ],
    }


def test_no_cache_field_when_unset():
    out = serialize_step_chain([scratch().sh("x")])
    assert "cache" not in _by_cmd(out, "x")


def test_pipeline_env_propagated():
    out = serialize_step_chain([hm.sh("x")], env={"FOO": "bar"})
    assert out["pipeline_env"] == {"FOO": "bar"}


def test_pipeline_env_omitted_when_none():
    out = serialize_step_chain([hm.sh("x")])
    assert "pipeline_env" not in out


def test_pipeline_timeout_string():
    out = serialize_step_chain([hm.sh("x")], timeout="30m")
    assert out["pipeline_timeout_seconds"] == 1800


def test_pipeline_timeout_int():
    out = serialize_step_chain([hm.sh("x")], timeout=45)
    assert out["pipeline_timeout_seconds"] == 45


def test_pipeline_timeout_omitted_when_none():
    out = serialize_step_chain([hm.sh("x")])
    assert "pipeline_timeout_seconds" not in out


def test_step_env_propagated():
    out = serialize_step_chain([hm.sh("x", env={"A": "1"})])
    assert _by_cmd(out, "x")["env"] == {"A": "1"}


def test_step_timeout_propagated():
    s = hm.timeout("10s", hm.sh("x"))
    out = serialize_step_chain([s])
    assert _by_cmd(out, "x")["timeout_seconds"] == 10


def test_key_override_propagated():
    out = serialize_step_chain([hm.sh("x", key="my-key")])
    assert _by_cmd(out, "x")["key_override"] == "my-key"


def test_label_propagated():
    out = serialize_step_chain([hm.sh("x", label="My Step")])
    assert _by_cmd(out, "x")["label"] == "My Step"


def test_image_runner_runner_args_propagated():
    out = serialize_step_chain(
        [
            scratch().sh(
                "x",
                image="alpine:3.20",
                runner="fly",
                runner_args={"size": "large", "n": 2},
            )
        ]
    )
    step = _by_cmd(out, "x")
    assert step["image"] == "alpine:3.20"
    assert step["runner"] == "fly"
    assert step["runner_args"] == {"size": "large", "n": 2}


def test_optional_fields_omitted_by_default():
    out = serialize_step_chain([hm.sh("x")])
    step = _by_cmd(out, "x")
    for field in (
        "label",
        "cache",
        "env",
        "timeout_seconds",
        "runner",
        "runner_args",
        "key_override",
        "is_wait",
        "continue_on_failure",
    ):
        assert field not in step, f"{field} should be omitted when unset"
    # cmd and parent_idx are always present.
    assert "cmd" in step
    assert "parent_idx" in step


def test_empty_leaves_still_serializes_empty():
    # Guard: serializer itself does not enforce non-empty (the pipeline
    # factory does); an empty forest yields empty steps.
    out = serialize_step_chain([])
    assert out["steps"] == []
    assert out["leaf_indices"] == []
