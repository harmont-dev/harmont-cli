"""Npm project abstraction tests."""
from __future__ import annotations

import pytest

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


def test_npm_full_chain():
    node = hm.npm(path="app/codegen")
    p = hm.pipeline(node.install(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("apt-get install" in c for c in cmds)
    assert any("deb.nodesource.com/setup_20" in c for c in cmds)
    assert any("cd app/codegen && npm ci" in c for c in cmds)


def test_npm_actions_share_install():
    node = hm.npm(path="app/codegen")
    p = hm.pipeline(
        node.run("build"), node.test(), node.lint(), node.fmt(),
        default_image="ubuntu:24.04",
    )
    cmds = _cmds(p)
    assert len([c for c in cmds if "npm ci" in c]) == 1
    assert any("cd app/codegen && npm run build" in c for c in cmds)
    assert any("cd app/codegen && npm test" in c for c in cmds)
    assert any("cd app/codegen && npm run lint" in c for c in cmds)
    assert any("cd app/codegen && npm run fmt" in c for c in cmds)


def test_npm_run_script():
    node = hm.npm(path=".")
    s = node.run("typecheck")
    assert s.cmd is not None
    assert "npm run typecheck" in s.cmd


def test_npm_version_in_install_cmd():
    node = hm.npm(path=".", version="22")
    p = hm.pipeline(node.install())
    install = _step_by_substring(p, "deb.nodesource.com")
    assert "setup_22.x" in install["cmd"]


def test_npm_invalid_version():
    with pytest.raises(ValueError, match="version"):
        hm.npm(version="latest")


def test_npm_node_install_cache_forever():
    node = hm.npm(path="app/codegen")
    p = hm.pipeline(node.install())
    install = _step_by_substring(p, "deb.nodesource.com")
    assert install["cache"]["policy"] == "forever"


def test_npm_ci_cache_on_package_lock():
    node = hm.npm(path="app/codegen")
    p = hm.pipeline(node.install())
    npm_ci = _step_by_substring(p, "npm ci")
    assert npm_ci["cache"]["policy"] == "on_change"
    assert "app/codegen/package-lock.json" in npm_ci["cache"]["paths"]


def test_npm_action_labels():
    node = hm.npm(path="app")
    assert node.run("build").label == ":node: build"
    assert node.test().label == ":node: test"
    assert node.lint().label == ":node: lint"
    assert node.fmt().label == ":node: fmt"


def test_npm_with_base_skips_apt():
    base = hm.scratch().sh("base step", label="base")
    node = hm.npm(path="app", base=base)
    p = hm.pipeline(node.install(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    # apt-base step (installing curl + ca-certificates) is skipped; the
    # nodesource install still runs `apt-get install -y nodejs` though.
    assert not any("ca-certificates" in c for c in cmds)
    assert any("deb.nodesource.com" in c for c in cmds)


def test_npm_installed_is_npm_ci_step():
    node = hm.npm(path="app")
    assert node.installed.cmd is not None
    assert "npm ci" in node.installed.cmd


def test_npm_bare_form_install():
    p = hm.pipeline(hm.npm.install())
    cmds = _cmds(p)
    assert any("cd . && npm ci" in c for c in cmds)


def test_npm_bare_form_test():
    p = hm.pipeline(hm.npm.test(path="app"))
    cmds = _cmds(p)
    assert any("cd app && npm test" in c for c in cmds)


def test_npm_bare_form_forwards_action_kwargs():
    s = hm.npm.test(path=".", label=":node: custom")
    assert s.label == ":node: custom"
