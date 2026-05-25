"""Zig toolchain tests."""

from __future__ import annotations

import pytest

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    raise AssertionError(needle)


def test_zig_object_form_full_chain():
    z = hm.zig(path="svc")
    p = hm.pipeline(z.build(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("ziglang.org" in c for c in cmds)
    assert any("cd svc && zig build" in c for c in cmds)


def test_zig_actions_share_install():
    z = hm.zig(path="svc")
    p = hm.pipeline(z.build(), z.test(), z.fmt(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "ziglang.org" in c]) == 1
    assert any("zig build test" in c for c in cmds)
    assert any("zig fmt --check ." in c for c in cmds)


def test_zig_version_in_install_cmd():
    z = hm.zig(path=".", version="0.13.0")
    p = hm.pipeline(z.build())
    install = _step_by_substring(p, "ziglang.org")
    assert "0.13.0" in install["cmd"]


def test_zig_invalid_version_rejected():
    with pytest.raises(ValueError, match="version"):
        hm.zig(version="oops!")


def test_zig_action_labels_auto_generated():
    z = hm.zig(path=".")
    assert z.build().label == ":zig: . build"
    assert z.test().label == ":zig: . test"
    assert z.fmt().label == ":zig: . fmt"


def test_zig_bare_form_actions():
    p = hm.pipeline(hm.zig.build(), hm.zig.test(), hm.zig.fmt())
    cmds = _cmds(p)
    assert any("zig build" in c for c in cmds)
    assert any("zig fmt --check ." in c for c in cmds)


def test_zig_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    z = hm.zig(path="svc", base=base)
    p = hm.pipeline(z.build(), default_image="ubuntu:24.04")
    assert not any("apt-get install" in c for c in _cmds(p))
