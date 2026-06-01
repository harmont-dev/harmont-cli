"""Tests for ZigToolchain (the multi-project entry point for hm.zig)."""

from __future__ import annotations

import json

import harmont as hm
from harmont._zig import ZigProject, ZigToolchain


def test_zig_no_path_returns_toolchain() -> None:
    """hm.zig() (without path=) returns a ZigToolchain -- not a ZigProject."""
    tc = hm.zig()
    assert isinstance(tc, ZigToolchain)


def test_zig_with_path_still_returns_project() -> None:
    """hm.zig(path=".") preserves the current ZigProject return for back-compat."""
    proj = hm.zig(path=".")
    assert isinstance(proj, ZigProject)


def test_toolchain_project_returns_zig_project() -> None:
    tc = hm.zig()
    proj = tc.project(path="lib-a")
    assert isinstance(proj, ZigProject)
    assert proj.path == "lib-a"


def test_two_projects_share_install_step() -> None:
    """Critical: two .project() calls on the same toolchain reuse the
    same installed Step. This is what makes ONE zig install fan out to
    N project chains in the v0 IR."""
    tc = hm.zig()
    a = tc.project(path="lib-a")
    b = tc.project(path="lib-b")
    assert a.installed is b.installed


def test_pipeline_with_shared_toolchain_emits_one_install() -> None:
    """End-to-end: a pipeline that pulls two ZigProjects off the same
    ZigToolchain must emit exactly one :zig: install node in the IR."""
    import harmont._registry as reg
    import harmont._target as targets

    reg.clear_registry()
    targets.clear_target_cache()

    @hm.target()
    def zig() -> ZigToolchain:
        return hm.zig()

    @hm.target()
    def lib_a(zig: hm.Target[ZigToolchain]) -> ZigProject:
        return zig.project(path="lib-a")

    @hm.target()
    def lib_b(zig: hm.Target[ZigToolchain]) -> ZigProject:
        return zig.project(path="lib-b")

    @hm.pipeline("ci", default_image="ubuntu:24.04")
    def ci(
        lib_a: hm.Target[ZigProject],
        lib_b: hm.Target[ZigProject],
    ) -> tuple[hm.Step, ...]:
        return (lib_a.build(), lib_b.build())

    envelope = json.loads(hm.dump_registry_json())
    definition = envelope["pipelines"][0]["definition"]
    nodes = definition["graph"]["nodes"]
    edges = definition["graph"]["edges"]

    zig_installs = [n for n in nodes if n["step"].get("label") == ":zig: install"]
    assert len(zig_installs) == 1, (
        f"expected exactly one :zig: install node, got {[n['step']['key'] for n in zig_installs]}"
    )

    install_key = zig_installs[0]["step"]["key"]
    lib_a_build = next(n for n in nodes if "lib-a" in (n["step"].get("label") or ""))
    lib_b_build = next(n for n in nodes if "lib-b" in (n["step"].get("label") or ""))

    # Verify builds_in edges connect install to both builds.
    key_by_idx = {i: n["step"]["key"] for i, n in enumerate(nodes)}
    idx_by_key = {v: k for k, v in key_by_idx.items()}

    install_idx = idx_by_key[install_key]
    lib_a_idx = idx_by_key[lib_a_build["step"]["key"]]
    lib_b_idx = idx_by_key[lib_b_build["step"]["key"]]

    builds_in_edges = [(s, d) for s, d, k in edges if k == "builds_in"]
    assert (install_idx, lib_a_idx) in builds_in_edges
    assert (install_idx, lib_b_idx) in builds_in_edges

    reg.clear_registry()
    targets.clear_target_cache()
