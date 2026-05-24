"""Shared toolchain helpers — apt-install template and chain builder."""

from __future__ import annotations

from datetime import timedelta

from harmont._step import scratch
from harmont._toolchain import (
    APT_TTL,
    apt_install_cmd,
    make_install_chain,
    node_install_cmd,
)
from harmont.cache import CacheNone, CacheOnChange, CacheTTL


def test_apt_install_cmd_runs_update_and_install():
    out = apt_install_cmd(("curl", "git"))
    assert "apt-get update" in out
    assert "apt-get install -y curl git" in out


def test_apt_install_cmd_preserves_package_order():
    out = apt_install_cmd(("a", "b", "c"))
    assert "a b c" in out


def test_apt_ttl_is_one_day():
    assert timedelta(days=1) == APT_TTL


def test_make_install_chain_default_emits_apt_then_tool():
    tool = make_install_chain(
        apt_packages=("curl",),
        install_cmd="install_tool.sh",
        install_cache=CacheOnChange(paths=("lockfile",)),
        lang_tag="lang",
        install_tag="tool",
        image=None,
        base=None,
    )
    apt = tool.parent
    assert apt is not None
    assert "apt-get install -y curl" in (apt.cmd or "")
    assert apt.label == ":lang: apt-base"
    assert isinstance(apt.cache, CacheTTL)
    assert apt.cache.duration == APT_TTL
    assert tool.cmd == "install_tool.sh"
    assert tool.label == ":lang: tool"
    assert isinstance(tool.cache, CacheOnChange)
    assert tool.cache.paths == ("lockfile",)


def test_make_install_chain_with_base_skips_apt():
    base = scratch().sh("custom base", label="base")
    tool = make_install_chain(
        apt_packages=("curl",),
        install_cmd="install.sh",
        install_cache=CacheNone(),
        lang_tag="lang",
        install_tag="tool",
        image=None,
        base=base,
    )
    assert tool.parent is base
    assert tool.cmd == "install.sh"
    assert tool.label == ":lang: tool"


def test_make_install_chain_image_set_on_apt_step_only():
    tool = make_install_chain(
        apt_packages=("curl",),
        install_cmd="install.sh",
        install_cache=CacheNone(),
        lang_tag="lang",
        install_tag="tool",
        image="ubuntu:24.04",
        base=None,
    )
    apt = tool.parent
    assert apt is not None
    assert apt.image == "ubuntu:24.04"
    assert tool.image is None


def test_make_install_chain_image_ignored_with_base():
    base = scratch().sh("base")
    tool = make_install_chain(
        apt_packages=("curl",),
        install_cmd="install.sh",
        install_cache=CacheNone(),
        lang_tag="lang",
        install_tag="tool",
        image="ubuntu:24.04",
        base=base,
    )
    assert tool.parent is base
    assert tool.image is None


def test_node_install_cmd_setup_major():
    out = node_install_cmd("20")
    assert "deb.nodesource.com/setup_20.x" in out
    assert "apt-get install -y nodejs" in out


def test_node_install_cmd_strips_dot_x_suffix():
    out = node_install_cmd("20.x")
    assert "deb.nodesource.com/setup_20.x" in out
