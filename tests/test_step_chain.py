"""Pure mechanics of the chain DSL — no codegen, no JSON."""

from __future__ import annotations

from dataclasses import FrozenInstanceError

import pytest

from harmont._step import scratch, wait
from harmont.cache import CacheNone


def test_scratch_has_no_parent_no_cmd():
    s = scratch()
    assert s.parent is None
    assert s.cmd is None
    assert s.is_wait is False


def test_sh_links_parent_and_sets_cmd():
    parent = scratch()
    child = parent.sh("echo hi")
    assert child.parent is parent
    assert child.cmd == "echo hi"
    assert child.is_wait is False


def test_sh_returns_new_instance_parent_unchanged():
    parent = scratch()
    parent.sh("a")
    parent.sh("b")
    # parent must be untouched (frozen dataclass)
    assert parent.parent is None
    assert parent.cmd is None


def test_fork_makes_branded_passthrough():
    parent = scratch().sh("install")
    branch = parent.fork(label="branch-a")
    assert branch.parent is parent
    assert branch.cmd is None
    assert branch.label == "branch-a"
    assert branch.is_wait is False


def test_fork_can_be_called_many_times_off_same_parent():
    parent = scratch().sh("install")
    a = parent.fork(label="a")
    b = parent.fork(label="b")
    c = parent.fork()
    assert {a.label, b.label, c.label} == {"a", "b", None}
    assert a.parent is parent
    assert b.parent is parent
    assert c.parent is parent


def test_sh_kwargs_carried_through():
    s = scratch().sh(
        "make",
        label="build",
        cache=CacheNone(),
        env={"CI": "true"},
        timeout_seconds=600,
        key="explicit-key",
    )
    assert s.label == "build"
    assert s.cache == CacheNone()
    assert s.env == {"CI": "true"}
    assert s.timeout_seconds == 600
    assert s.key_override == "explicit-key"


def test_step_is_frozen():
    s = scratch()
    with pytest.raises(FrozenInstanceError):
        s.cmd = "mutated"  # type: ignore[misc]


def test_wait_has_no_cmd_no_parent_and_is_wait_true():
    w = wait()
    assert w.parent is None
    assert w.cmd is None
    assert w.is_wait is True


def test_wait_continue_on_failure_recorded():
    w_default = wait()
    w_continue = wait(continue_on_failure=True)
    assert w_default.continue_on_failure is False
    assert w_continue.continue_on_failure is True
