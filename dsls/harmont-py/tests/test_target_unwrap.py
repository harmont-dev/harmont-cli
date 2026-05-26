"""as_leaves unwraps toolchain return values to (Step, ...) (HAR-28)."""

from __future__ import annotations

import pytest

import harmont as hm
from harmont._step import Step
from harmont._unwrap import as_leaves


def test_step_passes_through():
    s = hm.sh("echo hi")
    out = as_leaves(s)
    assert out == (s,)


def test_tuple_of_steps_passes_through():
    a = hm.sh("a")
    b = hm.sh("b")
    out = as_leaves((a, b))
    assert out == (a, b)


def test_list_of_steps_is_normalized_to_tuple():
    a = hm.sh("a")
    out = as_leaves([a])
    assert out == (a,)


def test_haskell_package_unwraps_to_build(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "api").mkdir()
    pkg = hm.haskell(ghc="9.6.7").cabal(path="api")
    leaves = as_leaves(pkg)
    assert len(leaves) == 1
    assert isinstance(leaves[0], Step)
    assert "cabal build all" in leaves[0].cmd


def test_rust_toolchain_unwraps_to_build():
    tc = hm.rust.toolchain(path="cli", version="stable")
    leaves = as_leaves(tc)
    assert len(leaves) == 1
    assert "cargo build" in leaves[0].cmd


def test_rust_project_unwraps_to_test_clippy_fmt():
    proj = hm.rust.project(path="cli")
    leaves = as_leaves(proj)
    assert len(leaves) == 3
    assert "cargo test" in leaves[0].cmd
    assert "cargo clippy" in leaves[1].cmd
    assert "cargo fmt" in leaves[2].cmd


def test_npm_project_unwraps_to_install():
    proj = hm.npm(path="app", version="20")
    leaves = as_leaves(proj)
    assert len(leaves) == 1
    # The default leaf is the npm-ci step itself.
    assert "npm ci" in leaves[0].cmd


def test_elm_project_unwraps_to_make_main():
    proj = hm.elm(path="src")
    leaves = as_leaves(proj)
    assert len(leaves) == 1
    assert "elm make src/Main.elm" in leaves[0].cmd


def test_nested_tuple_is_flattened(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "api").mkdir()
    pkg = hm.haskell(ghc="9.6.7").cabal(path="api")
    a = hm.sh("a")
    out = as_leaves((a, pkg, (a, a)))
    # Order preserved; pkg unwrapped to its build leaf.
    assert len(out) == 4


def test_unknown_type_raises_typeerror():
    with pytest.raises(TypeError, match=r"hm\.target: cannot use"):
        as_leaves(42)  # type: ignore[arg-type]


def test_unknown_type_message_lists_supported_types():
    with pytest.raises(TypeError, match=r"Step.*RustProject.*RustToolchain.*HaskellPackage"):
        as_leaves("oops")  # type: ignore[arg-type]
