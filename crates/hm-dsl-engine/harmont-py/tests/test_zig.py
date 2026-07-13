"""Zig toolchain tests."""

from __future__ import annotations

import pytest

import harmont as hm
from harmont._serialize import serialize_step_chain


def _cmds(leaves: list) -> list[str]:
    chain = serialize_step_chain(list(leaves))
    return [s["cmd"] for s in chain["steps"] if s.get("cmd") is not None]


def _step_by_substring(leaves: list, needle: str) -> dict:
    chain = serialize_step_chain(list(leaves))
    for s in chain["steps"]:
        if needle in (s.get("cmd") or ""):
            return s
    raise AssertionError(needle)


def test_zig_object_form_full_chain():
    z = hm.zig(path="svc")
    cmds = _cmds([z.build()])
    assert any("ziglang.org" in c for c in cmds)
    assert any("cd svc && zig build" in c for c in cmds)


def test_zig_actions_share_install():
    z = hm.zig(path="svc")
    cmds = _cmds([z.build(), z.test(), z.fmt()])
    assert len([c for c in cmds if "ziglang.org" in c]) == 1
    assert any("zig build test" in c for c in cmds)
    assert any("zig fmt --check ." in c for c in cmds)


def test_zig_version_in_install_cmd():
    z = hm.zig(path=".", version="0.14.1")
    install = _step_by_substring([z.build()], "ziglang.org")
    assert "0.14.1" in install["cmd"]


def test_zig_invalid_version_rejected():
    with pytest.raises(ValueError, match="version"):
        hm.zig(version="oops!")


def test_zig_action_labels_auto_generated():
    z = hm.zig(path=".")
    assert z.build().label == ":zig: . build"
    assert z.test().label == ":zig: . test"
    assert z.fmt().label == ":zig: . fmt"


def test_zig_bare_form_actions():
    cmds = _cmds([hm.zig.build(), hm.zig.test(), hm.zig.fmt()])
    assert any("zig build" in c for c in cmds)
    assert any("zig fmt --check ." in c for c in cmds)


def test_zig_old_version_uses_old_url_format():
    """Versions < 0.14.1 use zig-linux-x86_64-{v} format."""
    z = hm.zig(path=".", version="0.13.0")
    install = _step_by_substring([z.build()], "ziglang.org")
    assert "zig-linux-x86_64-0.13.0" in install["cmd"]


def test_zig_new_version_uses_new_url_format():
    """Versions >= 0.14.1 use zig-x86_64-linux-{v} format."""
    z = hm.zig(path=".", version="0.14.1")
    install = _step_by_substring([z.build()], "ziglang.org")
    assert "zig-x86_64-linux-0.14.1" in install["cmd"]


def test_zig_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    z = hm.zig(path="svc", base=base)
    assert not any("apt-get install" in c for c in _cmds([z.build()]))
