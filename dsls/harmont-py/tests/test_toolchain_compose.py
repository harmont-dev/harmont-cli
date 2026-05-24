"""Cross-cutting toolchain composition tests (HAR-15)."""
from __future__ import annotations

import harmont as hm

# Several tests construct `ghc.package("api")`, whose default cache_paths
# globs `api/*.cabal` relative to cwd. The autouse fixture in
# tests/conftest.py pins cwd to the repo root so that glob resolves to
# the real `api/harmont-api.cabal`.


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


def test_stack_elm_on_npm():
    """npm -> elm composition. Elm forks off node-installed Step."""
    node = hm.npm(path="app/codegen")
    elm = hm.elm(path="app", base=node.installed)
    p = hm.pipeline(elm.make("src/Main.elm"), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    # One apt-base (from npm)
    assert len([c for c in cmds if "ca-certificates" in c]) == 1
    # node install (from npm) + elm install
    assert any("npm ci" in c for c in cmds)
    assert any("elm/compiler/releases" in c for c in cmds)


def test_escape_hatch_consistent_across_toolchains():
    """Every toolchain exposes .installed as a public Step."""
    rust = hm.rust(path=".")
    ghc = hm.haskell(ghc="9.6.7")
    api = ghc.package("api")
    node = hm.npm(path=".")
    elm = hm.elm(path=".")
    assert isinstance(rust.installed, hm.Step)
    assert isinstance(ghc.installed, hm.Step)
    assert isinstance(api.installed, hm.Step)
    assert isinstance(node.installed, hm.Step)
    assert isinstance(elm.installed, hm.Step)


def test_deterministic_emission():
    """Two identical pipeline constructions emit equal IR dicts."""
    def build() -> dict:
        rust = hm.rust(path="cli")
        return hm.pipeline(rust.build(), rust.test(),
                           default_image="ubuntu:24.04")

    assert build() == build()


def test_mixed_pipeline_compiles():
    """A pipeline mixing all four toolchains lowers without error."""
    ghc = hm.haskell(ghc="9.6.7")
    rust = hm.rust(path="cli")
    node = hm.npm(path="app/codegen")
    elm = hm.elm(path="app", base=node.installed)
    p = hm.pipeline(
        ghc.package("api").test(),
        rust.test(), rust.clippy(),
        node.install(),
        elm.make("src/Main.elm"),
        default_image="ubuntu:24.04",
    )
    assert p["version"] == "0"
    assert len(p["graph"]["nodes"]) > 0
