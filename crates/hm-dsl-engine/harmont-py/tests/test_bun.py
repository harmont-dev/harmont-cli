"""Bun project abstraction tests."""

from __future__ import annotations

import pytest

import harmont as hm
from harmont._toolchain import bun_install_cmd


def test_bun_install_cmd_latest():
    cmd = bun_install_cmd()
    assert "https://bun.sh/install" in cmd
    assert "BUN_INSTALL=/usr/local" in cmd
    assert "bun-v" not in cmd


def test_bun_install_cmd_version():
    cmd = bun_install_cmd("1.2.0")
    assert "bun-v1.2.0" in cmd


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


def test_bun_full_chain():
    b = hm.bun(path="app/frontend")
    p = hm.pipeline([b.install()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("apt-get install" in c for c in cmds)
    assert any("unzip" in c for c in cmds)
    assert any("bun.sh/install" in c for c in cmds)
    assert any("cd app/frontend && bun install --frozen-lockfile" in c for c in cmds)


def test_bun_actions_share_install():
    b = hm.bun(path="app/frontend")
    p = hm.pipeline(
        [b.run("build"), b.test(), b.lint(), b.fmt()],
        default_image="ubuntu:24.04",
    )
    cmds = _cmds(p)
    assert len([c for c in cmds if "bun install" in c]) == 1
    assert any("cd app/frontend && bun run build" in c for c in cmds)
    assert any("cd app/frontend && bun test" in c for c in cmds)
    assert any("cd app/frontend && bun run lint" in c for c in cmds)
    assert any("cd app/frontend && bun run fmt" in c for c in cmds)


def test_bun_run_script():
    b = hm.bun(path=".")
    s = b.run("typecheck")
    assert s.cmd is not None
    assert "bun run typecheck" in s.cmd


def test_bun_version_in_install_cmd():
    b = hm.bun(path=".", version="1.2.0")
    p = hm.pipeline([b.install()])
    install = _step_by_substring(p, "bun.sh/install")
    assert "bun-v1.2.0" in install["cmd"]


def test_bun_invalid_version():
    with pytest.raises(ValueError, match="version"):
        hm.bun(version="latest")


def test_bun_install_cache_forever():
    b = hm.bun(path="app")
    p = hm.pipeline([b.install()])
    install = _step_by_substring(p, "bun.sh/install")
    assert install["cache"]["policy"] == "forever"


def test_bun_deps_cache_on_lockfile():
    b = hm.bun(path="app/frontend")
    p = hm.pipeline([b.install()])
    deps = _step_by_substring(p, "bun install")
    assert deps["cache"]["policy"] == "on_change"
    assert "app/frontend/bun.lock" in deps["cache"]["paths"]


def test_bun_action_labels():
    b = hm.bun(path="app")
    assert b.run("build").label == ":bun: build"
    assert b.test().label == ":bun: test"
    assert b.lint().label == ":bun: lint"
    assert b.fmt().label == ":bun: fmt"


def test_bun_with_base_skips_apt():
    base = hm.scratch().sh("base step", label="base")
    b = hm.bun(path="app", base=base)
    p = hm.pipeline([b.install()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert not any("ca-certificates" in c for c in cmds)
    assert any("bun.sh/install" in c for c in cmds)


def test_bun_installed_is_deps_step():
    b = hm.bun(path="app")
    assert b.installed.cmd is not None
    assert "bun install" in b.installed.cmd


def test_bun_bare_form_install():
    p = hm.pipeline([hm.bun.install()])
    cmds = _cmds(p)
    assert any("cd . && bun install --frozen-lockfile" in c for c in cmds)


def test_bun_bare_form_test():
    p = hm.pipeline([hm.bun.test(path="app")])
    cmds = _cmds(p)
    assert any("cd app && bun test" in c for c in cmds)


def test_bun_bare_form_forwards_action_kwargs():
    s = hm.bun.test(path=".", label=":bun: custom")
    assert s.label == ":bun: custom"
