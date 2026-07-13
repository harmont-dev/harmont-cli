"""Cross-cutting toolchain composition tests (HAR-15)."""

from __future__ import annotations

import harmont as hm
from harmont._serialize import serialize_step_chain


def _cmds(leaves: list) -> list[str]:
    chain = serialize_step_chain(list(leaves))
    return [s["cmd"] for s in chain["steps"] if s.get("cmd") is not None]


def test_stack_npm_on_spec_step():
    """spec -> node install -> npm ci -> codegen. Used by dogfood."""
    spec = hm.scratch().sh("make openapi", label=":lock: spec")
    node = hm.js.project(path="app/codegen", base=spec)
    cmds = _cmds([node.install()])
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
    node = hm.js.project(path=".")
    assert isinstance(rust.installed, hm.Step)
    assert isinstance(node.installed, hm.Step)


def test_deterministic_emission():
    """Two identical pipeline constructions emit equal IR dicts."""

    def build() -> dict:
        rust = hm.rust.toolchain(path="cli")
        return serialize_step_chain([rust.build(), rust.test()])

    assert build() == build()


def test_mixed_pipeline_compiles():
    """A pipeline mixing multiple toolchains lowers without error."""
    rust = hm.rust.toolchain(path="cli")
    node = hm.js.project(path="app/codegen")
    go = hm.go(path="services/api")
    chain = serialize_step_chain(
        [rust.test(), rust.clippy(), node.install(), go.build(), go.test()],
    )
    assert len(chain["steps"]) > 0


def _step_by_substring(leaves: list, needle: str) -> dict:
    chain = serialize_step_chain(list(leaves))
    for s in chain["steps"]:
        if needle in (s.get("cmd") or ""):
            return s
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
    cmds = _cmds([rust.build(), py.test()])
    assert len([c for c in cmds if "apt-get install" in c]) == 1
    assert any("sh.rustup.rs" in c for c in cmds)
    assert any("uv" in c for c in cmds)


def test_apt_base_default_label():
    base = hm.apt_base(packages=("curl",))
    assert base.label == ":apt: base"


def test_apt_base_custom_image():
    base = hm.apt_base(packages=("curl",), image="debian:bookworm")
    rust = hm.rust.toolchain(path=".", base=base)
    apt_step = _step_by_substring([rust.build()], "apt-get install")
    assert apt_step.get("image") == "debian:bookworm"


def test_apt_base_custom_label():
    base = hm.apt_base(packages=("curl",), label=":lock: deps")
    assert base.label == ":lock: deps"
