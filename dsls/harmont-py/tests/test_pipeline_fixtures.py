"""@hm.pipeline fixture-style param resolution (HAR-28 follow-up)."""
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


def _graph_nodes(definition):
    return definition["graph"]["nodes"]


def test_zero_param_pipeline_still_works():
    @hm.pipeline("ci")
    def ci() -> hm.Step:
        return hm.sh("echo hi")

    out = json.loads(hm.dump_registry_json())
    nodes = _graph_nodes(out["pipelines"][0]["definition"])
    assert any(n["step"].get("cmd") == "echo hi" for n in nodes)


def test_pipeline_receives_target_as_param():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    @hm.pipeline("ci")
    def ci(apt_base: hm.Target[hm.Step]) -> hm.Step:
        return apt_base.sh("smoke")

    out = json.loads(hm.dump_registry_json())
    nodes = _graph_nodes(out["pipelines"][0]["definition"])
    cmds = [n["step"].get("cmd") for n in nodes]
    assert "apt-get update" in cmds
    assert "smoke" in cmds


def test_pipeline_multi_param_composes_targets():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    @hm.target()
    def api(apt_base: hm.Target[hm.Step]) -> hm.Step:
        return apt_base.sh("cabal build")

    @hm.target()
    def py_test(apt_base: hm.Target[hm.Step]) -> hm.Step:
        return apt_base.sh("pytest")

    @hm.pipeline("ci")
    def ci(
        api: hm.Target[hm.Step],
        py_test: hm.Target[hm.Step],
    ) -> tuple[hm.Step, ...]:
        return (api, py_test)

    out = json.loads(hm.dump_registry_json())
    nodes = _graph_nodes(out["pipelines"][0]["definition"])
    apt = [n for n in nodes if n["step"].get("cmd") == "apt-get update"]
    assert len(apt) == 1  # apt_base deduped via target memoization
    cmds = sorted(n["step"].get("cmd") for n in nodes)
    assert "cabal build" in cmds
    assert "pytest" in cmds


def test_pipeline_with_unknown_param_raises():
    @hm.pipeline("ci")
    def ci(no_such_target: hm.Target[hm.Step]) -> hm.Step:
        return hm.sh("never reached")

    with pytest.raises(TypeError, match="target 'no_such_target' not found"):
        hm.dump_registry_json()
