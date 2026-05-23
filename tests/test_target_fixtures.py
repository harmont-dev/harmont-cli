"""@hm.target fixture-style param resolution (HAR-28 follow-up)."""
from __future__ import annotations

import pytest

import harmont as hm
from harmont._deps import clear_target_names
from harmont._target import clear_target_cache


@pytest.fixture(autouse=True)
def _reset():
    clear_target_cache()
    clear_target_names()
    yield
    clear_target_cache()
    clear_target_names()


def test_zero_param_target_still_works():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    s = apt_base()
    assert s.cmd == "apt-get update"


def test_target_param_receives_dependency_value():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    @hm.target()
    def venv(apt_base: hm.Target[hm.Step]) -> hm.Step:
        return apt_base.sh("python3 -m venv .venv")

    v = venv()
    assert v.parent is not None
    assert v.parent.cmd == "apt-get update"


def test_multi_param_target():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    @hm.target()
    def node_install() -> hm.Step:
        return apt_base().sh("curl ... | bash")

    @hm.target()
    def project(
        apt_base: hm.Target[hm.Step],
        node_install: hm.Target[hm.Step],
    ):
        # Both injected; we just verify both flow through.
        return (apt_base, node_install)

    base, node = project()
    assert base.cmd == "apt-get update"
    assert "curl" in node.cmd


def test_param_named_after_unregistered_target_raises():
    @hm.target()
    def venv(missing: hm.Target[hm.Step]) -> hm.Step:
        return hm.sh("never reached")

    with pytest.raises(TypeError, match="target 'missing' not found"):
        venv()


def test_duplicate_target_name_raises_at_decoration():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("a")

    with pytest.raises(ValueError, match="duplicate target name 'apt_base'"):
        @hm.target()
        def apt_base() -> hm.Step:
            return hm.sh("b")


def test_explicit_name_override():
    # name= overrides the default (fn.__name__) registry key. A dash
    # in the key is fine because we resolve via the registry directly,
    # not via Python identifier rules.
    @hm.target(name="apt-base")
    def whatever() -> hm.Step:
        return hm.sh("apt-get update")

    from harmont._deps import _TARGETS_BY_NAME
    assert "apt-base" in _TARGETS_BY_NAME
    assert "whatever" not in _TARGETS_BY_NAME


def test_default_value_used_when_no_target_registered():
    @hm.target()
    def maybe_extra(image_tag: str = "ubuntu:24.04") -> hm.Step:
        return hm.sh(f"echo {image_tag}")

    s = maybe_extra()
    assert s.cmd == "echo ubuntu:24.04"


def test_memoization_still_works_with_params():
    call_count = 0

    @hm.target()
    def apt_base() -> hm.Step:
        nonlocal call_count
        call_count += 1
        return hm.sh("apt-get update")

    @hm.target()
    def venv(apt_base: hm.Target[hm.Step]) -> hm.Step:
        return apt_base.sh("v")

    @hm.target()
    def api(apt_base: hm.Target[hm.Step]) -> hm.Step:
        return apt_base.sh("a")

    v = venv()
    a = api()
    # apt_base ran once; venv and api share its Step.
    assert call_count == 1
    assert v.parent is a.parent


def test_cycle_between_two_targets_raises():
    # Hand-construct a cycle: a takes b, b takes a.
    @hm.target()
    def a(b: hm.Target[hm.Step]) -> hm.Step:
        return b.sh("a")

    @hm.target()
    def b(a: hm.Target[hm.Step]) -> hm.Step:
        return a.sh("b")

    with pytest.raises(RuntimeError, match="dependency cycle"):
        a()


def test_clear_target_cache_also_clears_name_registry():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("a")

    from harmont._deps import _TARGETS_BY_NAME

    assert "apt_base" in _TARGETS_BY_NAME
    clear_target_cache()
    assert "apt_base" not in _TARGETS_BY_NAME
