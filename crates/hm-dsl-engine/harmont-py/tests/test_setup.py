"""`.setup()` splices a prep step into a toolchain's install chain so that
action leaves fork from it. One parametrized test over every install-bearing
toolchain object."""
from __future__ import annotations

import json

import pytest

import harmont as hm

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


def _render_keys_and_edges(leaf: hm.Step) -> tuple[dict, list]:
    """Render a one-leaf pipeline and return (nodes-by-index-key, edges)."""
    doc = json.loads(hm.pipeline_to_json(hm.pipeline([leaf])))
    graph = doc["graph"]
    keys = [n["step"]["key"] for n in graph["nodes"]]
    cmds = [n["step"].get("cmd") for n in graph["nodes"]]
    return {"keys": keys, "cmds": cmds}, graph["edges"]


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
    info, _edges = _render_keys_and_edges(advanced.installed)
    assert any(c and "__SETUP_MARKER__" in c for c in info["cmds"]), info


def test_setup_is_chainable() -> None:
    proj = hm.elixir(path=".").setup("echo __ONE__").setup("echo __TWO__")
    info, _edges = _render_keys_and_edges(proj.installed)
    assert any(c and "__ONE__" in c for c in info["cmds"])
    assert any(c and "__TWO__" in c for c in info["cmds"])
