"""Cross-cutting toolchain composition tests (HAR-15)."""

from __future__ import annotations

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def test_stack_npm_on_spec_step():
    """spec -> node install -> npm ci -> codegen. Used by dogfood."""
    spec = hm.scratch().sh("make openapi", label=":lock: spec")
    node = hm.npm(path="app/codegen", base=spec)
    p = hm.pipeline(node.install())
    cmds = _cmds(p)
    assert any("make openapi" in c for c in cmds)
    assert any("deb.nodesource.com" in c for c in cmds)
    assert any("npm ci" in c for c in cmds)
    # No apt-base step: base= skipped it. (Note: nodesource installer
    # itself runs `apt-get install -y nodejs`, so don't assert on
    # apt-get; check the apt-base sentinel `ca-certificates`.)
    assert not any("ca-certificates" in c for c in cmds)


def test_escape_hatch_consistent_across_toolchains():
    """Every toolchain exposes .installed as a public Step."""
    rust = hm.rust.toolchain(path=".")
    node = hm.npm(path=".")
    assert isinstance(rust.installed, hm.Step)
    assert isinstance(node.installed, hm.Step)


def test_deterministic_emission():
    """Two identical pipeline constructions emit equal IR dicts."""

    def build() -> dict:
        rust = hm.rust.toolchain(path="cli")
        return hm.pipeline(rust.build(), rust.test(), default_image="ubuntu:24.04")

    assert build() == build()


def test_mixed_pipeline_compiles():
    """A pipeline mixing multiple toolchains lowers without error."""
    rust = hm.rust.toolchain(path="cli")
    node = hm.npm(path="app/codegen")
    go = hm.go(path="services/api")
    p = hm.pipeline(
        rust.test(),
        rust.clippy(),
        node.install(),
        go.build(),
        go.test(),
        default_image="ubuntu:24.04",
    )
    assert p["version"] == "0"
    assert len(p["graph"]["nodes"]) > 0


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


def test_apt_base_shared_across_toolchains():
    """Single apt-base feeds both rust and python toolchains."""
    base = hm.apt_base(
        packages=(
            "curl",
            "ca-certificates",
            "build-essential",
            "pkg-config",
            "libssl-dev",
            "python3",
            "python3-venv",
        ),
    )
    rust = hm.rust.toolchain(path=".", base=base)
    py = hm.py.uv(path="dsls/harmont-py", base=base)
    p = hm.pipeline(
        rust.build(),
        py.test(),
        default_image="ubuntu:24.04",
    )
    cmds = _cmds(p)
    assert len([c for c in cmds if "apt-get install" in c]) == 1
    assert any("sh.rustup.rs" in c for c in cmds)
    assert any("uv" in c for c in cmds)


def test_apt_base_default_label():
    base = hm.apt_base(packages=("curl",))
    assert base.label == ":apt: base"


def test_apt_base_custom_image():
    base = hm.apt_base(packages=("curl",), image="debian:bookworm")
    rust = hm.rust.toolchain(path=".", base=base)
    p = hm.pipeline(rust.build(), default_image="ubuntu:24.04")
    apt_step = _step_by_substring(p, "apt-get install")
    assert apt_step.get("image") == "debian:bookworm"


def test_apt_base_custom_label():
    base = hm.apt_base(packages=("curl",), label=":lock: deps")
    assert base.label == ":lock: deps"
