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
    # Toolchain `.cabal` glob reads disk for *.cabal files — give it an
    # empty workspace so the test is hermetic.
    monkeypatch.chdir(tmp_path)
    (tmp_path / "api").mkdir()
    (tmp_path / "freestyle").mkdir()
    (tmp_path / "src").mkdir()
    yield
    clear_registry()
    clear_target_cache()
    clear_target_names()


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
        return hm.haskell(ghc="9.6.7").cabal(path="api")

    @hm.target()
    def freestyle():
        return hm.haskell(ghc="9.6.7").cabal(path="freestyle")

    @hm.target()
    def frontend():
        return hm.elm(path="src")

    @hm.pipeline("ci")
    def ci():
        return (venv(), api(), freestyle(), frontend())

    out = json.loads(hm.dump_registry_json())
    p = out["pipelines"][0]
    steps = p["definition"]["steps"]

    cmds = [s.get("cmd") for s in steps if s.get("type") == "command"]
    # Each leaf landed in the IR.
    assert any("pytest -v" in (c or "") for c in cmds)
    assert any("cabal build all" in (c or "") for c in cmds)
    assert any("elm make src/Main.elm" in (c or "") for c in cmds)

    # apt-base used by the venv chain appears exactly once (memoized).
    apt_update_steps = [s for s in steps if s.get("cmd") == "apt-get update"]
    assert len(apt_update_steps) == 1


def test_har_28_cwd_kwarg_renders_to_cd_prefix():
    @hm.pipeline("ci")
    def ci():
        return hm.sh("pytest -v", cwd="cidsl/py")

    out = json.loads(hm.dump_registry_json())
    steps = out["pipelines"][0]["definition"]["steps"]
    cmds = [s["cmd"] for s in steps if s.get("type") == "command"]
    assert "cd cidsl/py && pytest -v" in cmds
