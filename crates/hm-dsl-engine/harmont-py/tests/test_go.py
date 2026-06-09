"""Go toolchain abstraction tests."""

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


def test_go_object_form_full_chain():
    go = hm.go(path="svc")
    p = hm.pipeline([go.build()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("apt-get install" in c for c in cmds)
    assert any("go.dev/dl/" in c for c in cmds)
    assert any("cd svc && go build ./..." in c for c in cmds)


def test_go_actions_share_install_step():
    go = hm.go(path="svc")
    p = hm.pipeline([go.build(), go.test(), go.vet(), go.fmt()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "go.dev/dl/" in c]) == 1
    assert any("go build ./..." in c for c in cmds)
    assert any("go test ./..." in c for c in cmds)
    assert any("go vet ./..." in c for c in cmds)
    assert any("gofmt -l" in c for c in cmds)


def test_go_install_cache_forever():
    go = hm.go(path=".")
    p = hm.pipeline([go.build()])
    install = _step_by_substring(p, "go.dev/dl/")
    assert install["cache"]["policy"] == "forever"


def test_go_version_in_install_cmd():
    go = hm.go(path=".", version="1.23.2")
    p = hm.pipeline([go.build()])
    install = _step_by_substring(p, "go.dev/dl/")
    assert "go1.23.2" in install["cmd"]


def test_go_invalid_version_rejected():
    with pytest.raises(ValueError, match="version"):
        hm.go(version="bogus; rm -rf /")


def test_go_bare_form_actions():
    p = hm.pipeline([hm.go.build(), hm.go.test(), hm.go.vet(), hm.go.fmt()])
    cmds = _cmds(p)
    assert any("go build" in c for c in cmds)
    assert any("go test" in c for c in cmds)
    assert any("go vet" in c for c in cmds)
    assert any("gofmt" in c for c in cmds)


def test_go_action_labels_auto_generated():
    go = hm.go(path=".")
    assert go.build().label == ":go: build"
    assert go.test().label == ":go: test"
    assert go.vet().label == ":go: vet"
    assert go.fmt().label == ":go: fmt"


def test_go_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    go = hm.go(path="svc", base=base)
    p = hm.pipeline([go.build()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert not any("apt-get install" in c for c in cmds)
    assert any("custom base" in c for c in cmds)


def test_go_installed_escape_hatch_chains():
    go = hm.go(path="svc")
    custom = go.installed.sh("cd svc && go generate ./...", label=":go: gen")
    p = hm.pipeline([custom])
    assert any("go generate" in c for c in _cmds(p))
