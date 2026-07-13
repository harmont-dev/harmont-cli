"""`.setup()` splices a prep step into a toolchain's install chain so that
action leaves fork from it. One parametrized test over every install-bearing
toolchain object."""

from __future__ import annotations

import pytest

import harmont as hm
from harmont._serialize import serialize_step_chain

# (label, factory) for every toolchain object that owns an `installed` chain.
# Each factory returns an object exposing `.installed` and `.setup()`.
TOOLCHAINS = [
    ("elixir", lambda: hm.elixir(path=".")),
    ("python", lambda: hm.python(path=".")),
    ("go", lambda: hm.go(path=".")),
    ("js", lambda: hm.js.project(path=".")),
    ("zig_project", lambda: hm.zig(path=".")),
    ("zig_toolchain", lambda: hm.zig()),
    ("rust_toolchain", lambda: hm.rust.toolchain()),  # RustEntry is NOT callable
    ("cmake_toolchain", lambda: hm.cmake()),
]


def _cmds(leaf: hm.Step) -> list[str | None]:
    """Serialize a one-leaf chain and return its per-step commands."""
    chain = serialize_step_chain([leaf])
    return [s.get("cmd") for s in chain["steps"]]


@pytest.mark.parametrize(
    ("label", "factory"), TOOLCHAINS, ids=lambda v: v if isinstance(v, str) else ""
)
def test_setup_advances_install_chain(label: str, factory) -> None:
    proj = factory()
    before = proj.installed
    advanced = proj.setup("echo __SETUP_MARKER__", label="setup-marker")

    # Immutable: original object's cursor is untouched; a new object is returned.
    assert proj.installed is before
    assert advanced is not proj
    assert advanced.installed is not before
    assert type(advanced) is type(proj)

    # The setup command renders, as an ancestor of the install cursor.
    cmds = _cmds(advanced.installed)
    assert any(c and "__SETUP_MARKER__" in c for c in cmds), cmds


def test_setup_is_chainable() -> None:
    proj = hm.elixir(path=".").setup("echo __ONE__").setup("echo __TWO__")
    cmds = _cmds(proj.installed)
    assert any(c and "__ONE__" in c for c in cmds)
    assert any(c and "__TWO__" in c for c in cmds)
