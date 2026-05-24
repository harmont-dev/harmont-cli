"""dotnet (C#) toolchain tests."""
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


def test_dotnet_object_form_full_chain():
    dn = hm.dotnet(path="svc")
    p = hm.pipeline(dn.build(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("dot.net/v1/dotnet-install.sh" in c for c in cmds)
    assert any("cd svc && dotnet build" in c for c in cmds)


def test_dotnet_actions_share_install():
    dn = hm.dotnet(path="svc")
    p = hm.pipeline(dn.build(), dn.test(), dn.fmt(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "dotnet-install" in c]) == 1
    assert any("dotnet build" in c for c in cmds)
    assert any("dotnet test" in c for c in cmds)
    assert any("dotnet format --verify-no-changes" in c for c in cmds)


def test_dotnet_channel_in_install_cmd():
    dn = hm.dotnet(path=".", channel="8.0")
    p = hm.pipeline(dn.build())
    install = _step_by_substring(p, "dotnet-install")
    assert "--channel 8.0" in install["cmd"]


def test_dotnet_invalid_channel_rejected():
    with pytest.raises(ValueError, match="channel"):
        hm.dotnet(channel="bogus; rm -rf /")


def test_dotnet_action_labels_auto_generated():
    dn = hm.dotnet(path=".")
    assert dn.build().label == ":dotnet: build"
    assert dn.test().label == ":dotnet: test"
    assert dn.fmt().label == ":dotnet: fmt"


def test_dotnet_bare_form_actions():
    p = hm.pipeline(hm.dotnet.build(), hm.dotnet.test(), hm.dotnet.fmt())
    cmds = _cmds(p)
    assert any("dotnet build" in c for c in cmds)
    assert any("dotnet test" in c for c in cmds)
    assert any("dotnet format" in c for c in cmds)


def test_dotnet_install_cache_forever():
    dn = hm.dotnet(path=".")
    p = hm.pipeline(dn.build())
    install = _step_by_substring(p, "dotnet-install")
    assert install["cache"]["policy"] == "forever"


def test_dotnet_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    dn = hm.dotnet(path="svc", base=base)
    p = hm.pipeline(dn.build(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert not any("apt-get install" in c for c in cmds)
    assert any("custom base" in c for c in cmds)
