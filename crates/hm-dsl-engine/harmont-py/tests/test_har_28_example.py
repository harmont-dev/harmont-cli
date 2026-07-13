"""End-to-end: HAR-28 issue example renders to a valid envelope."""

from __future__ import annotations

import json

import pytest

import harmont as hm
from harmont._deps import clear_target_names
from harmont._registry import clear_registry
from harmont._target import clear_target_cache


@pytest.fixture(autouse=True)
def _reset(tmp_path, monkeypatch):
    clear_registry()
    clear_target_cache()
    clear_target_names()
    monkeypatch.chdir(tmp_path)
    yield
    clear_registry()
    clear_target_cache()
    clear_target_names()


def _steps(step_chain):
    return step_chain["steps"]


def test_har_28_example_renders():
    @hm.target()
    def apt_base():
        return hm.sh("apt-get update").sh("apt-get install -y python3 python3-venv python3-pip")

    @hm.target()
    def venv():
        return (
            apt_base()
            .sh("python3 -m venv .venv", cwd="cidsl/py")
            .sh("pip install -e '.[dev]'", cwd="cidsl/py")
            .sh("pytest -v", cwd="cidsl/py")
        )

    @hm.target()
    def api():
        return hm.go(path="api").build()

    @hm.target()
    def web():
        return hm.js.project(path="web").run("build")

    @hm.pipeline("ci")
    def ci():
        return (venv(), api(), web())

    out = json.loads(hm.dump_registry_json())
    p = out["pipelines"][0]
    steps = _steps(p["step_chain"])

    cmds = [s.get("cmd") for s in steps]
    assert any("pytest -v" in (c or "") for c in cmds)
    assert any("go build" in (c or "") for c in cmds)
    assert any("npm" in (c or "") for c in cmds)

    # apt-base used by the venv chain appears exactly once (memoized).
    apt_update_steps = [s for s in steps if s.get("cmd") == "apt-get update"]
    assert len(apt_update_steps) == 1


def test_har_28_cwd_kwarg_renders_to_cd_prefix():
    @hm.pipeline("ci")
    def ci():
        return hm.sh("pytest -v", cwd="cidsl/py")

    out = json.loads(hm.dump_registry_json())
    steps = _steps(out["pipelines"][0]["step_chain"])
    cmds = [s.get("cmd") for s in steps]
    assert "cd cidsl/py && pytest -v" in cmds
