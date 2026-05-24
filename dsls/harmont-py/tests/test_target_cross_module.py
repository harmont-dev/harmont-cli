"""Cross-module target deps via global registry (HAR-28 follow-up)."""
from __future__ import annotations

import json

import pytest

import harmont as hm
from harmont._registry import clear_registry
from harmont._target import clear_target_cache


@pytest.fixture(autouse=True)
def _reset():
    clear_registry()
    clear_target_cache()
    yield
    clear_registry()
    clear_target_cache()


def test_target_in_module_a_consumed_by_target_in_module_b():
    """Simulate two .harmont/*.py files registering targets in one
    envelope render. Module A defines apt_base; module B's target
    depends on it by parameter name."""
    # Module A — defines apt_base.
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    # Module B — declares apt_base as a param (cross-module by name).
    @hm.target()
    def py_test(apt_base: hm.Target[hm.Step]) -> hm.Step:
        return apt_base.sh("pytest -v", cwd="cidsl/py")

    # Module C — pipeline composes module B's target.
    @hm.pipeline("ci")
    def ci(py_test: hm.Target[hm.Step]) -> hm.Step:
        return py_test

    out = json.loads(hm.dump_registry_json())
    nodes = out["pipelines"][0]["definition"]["graph"]["nodes"]
    cmds = sorted(n["step"].get("cmd") for n in nodes)
    assert "apt-get update" in cmds
    assert "cd cidsl/py && pytest -v" in cmds


def test_duplicate_name_across_modules_raises():
    """Same target name registered twice (e.g. two modules both define
    apt_base) raises at decoration time."""
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("first")

    with pytest.raises(ValueError, match="duplicate target name 'apt_base'"):
        @hm.target()
        def apt_base() -> hm.Step:
            return hm.sh("second")


def test_disambiguate_via_explicit_name():
    """Two modules with same fn name can coexist via name=."""
    @hm.target(name="apt_base_a")
    def apt_base() -> hm.Step:
        return hm.sh("first")

    @hm.target(name="apt_base_b")
    def apt_base() -> hm.Step:  # noqa: F811
        return hm.sh("second")

    from harmont._deps import _TARGETS_BY_NAME
    assert "apt_base_a" in _TARGETS_BY_NAME
    assert "apt_base_b" in _TARGETS_BY_NAME
