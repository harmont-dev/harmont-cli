"""Haskell toolchain + package abstraction tests."""
from __future__ import annotations

import pytest

import harmont as hm
from harmont.haskell import HaskellPackage, HaskellToolchain

# The repo-root cwd these tests need (so default cache_paths globs
# `<path>/*.cabal` against real files) is supplied by the autouse
# fixture in tests/conftest.py.


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


def test_haskell_constructor_returns_toolchain():
    ghc = hm.haskell(ghc="9.6.7")
    assert isinstance(ghc, HaskellToolchain)


def test_haskell_with_path_returns_package():
    pkg = hm.haskell(ghc="9.6.7", path="freestyle")
    assert isinstance(pkg, HaskellPackage)


def test_haskell_package_full_chain():
    ghc = hm.haskell(ghc="9.6.7")
    api = ghc.package("api")
    p = hm.pipeline(api.test(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("apt-get install" in c for c in cmds)
    assert any("ghcup install ghc 9.6.7" in c for c in cmds)
    assert any("cabal build all --only-dependencies" in c for c in cmds)
    assert any("cd api && cabal test all" in c for c in cmds)


def test_haskell_multi_package_shares_ghcup():
    ghc = hm.haskell(ghc="9.6.7")
    api = ghc.package("api")
    fs = ghc.package("freestyle")
    p = hm.pipeline(api.build(), fs.build(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "ghcup install" in c]) == 1
    assert len([c for c in cmds if "apt-get install" in c]) == 1
    deps = [c for c in cmds if "cabal build all --only-dependencies" in c]
    assert len(deps) == 2
    assert any("cd api && cabal build all" in c for c in cmds)
    assert any("cd freestyle && cabal build all" in c for c in cmds)


def test_haskell_ghcup_cache_forever():
    ghc = hm.haskell(ghc="9.6.7")
    api = ghc.package("api")
    p = hm.pipeline(api.test())
    ghcup = _step_by_substring(p, "ghcup install")
    assert ghcup["cache"]["policy"] == "forever"


def test_haskell_ghcup_version_in_cmd():
    ghc = hm.haskell(ghc="9.6.7")
    api = ghc.package("api")
    p = hm.pipeline(api.test())
    ghcup = _step_by_substring(p, "ghcup install")
    assert "ghc 9.6.7" in ghcup["cmd"]


def test_haskell_package_deps_cache_default():
    ghc = hm.haskell(ghc="9.6.7")
    api = ghc.package("api")
    p = hm.pipeline(api.test())
    deps = _step_by_substring(p, "cabal build all --only-dependencies")
    assert deps["cache"]["policy"] == "on_change"
    assert deps["cache"]["paths"] == ["api/harmont-api.cabal", "api/cabal.project"]


def test_haskell_package_deps_cache_default_no_cabal_project():
    ghc = hm.haskell(ghc="9.6.7")
    fs = ghc.package("freestyle")
    p = hm.pipeline(fs.test())
    deps = _step_by_substring(p, "cabal build all --only-dependencies")
    assert deps["cache"]["policy"] == "on_change"
    assert deps["cache"]["paths"] == ["freestyle/freestyle.cabal"]


def test_haskell_package_deps_cache_explicit_paths():
    ghc = hm.haskell(ghc="9.6.7")
    api = ghc.package("api", cache_paths=("api/cabal.project", "api/harmont-api.cabal"))
    p = hm.pipeline(api.test())
    deps = _step_by_substring(p, "cabal build all --only-dependencies")
    assert deps["cache"]["paths"] == ["api/cabal.project", "api/harmont-api.cabal"]


def test_haskell_actions():
    ghc = hm.haskell(ghc="9.6.7")
    api = ghc.package("api")
    assert "cd api && cabal build all" in (api.build().cmd or "")
    assert "cd api && cabal test all" in (api.test().cmd or "")
    assert "cd api && cabal build all --flag werror" in (api.lint().cmd or "")
    assert "hlint api" in (api.hlint().cmd or "")
    assert "fourmolu --mode check api" in (api.fmt().cmd or "")


def test_haskell_action_labels():
    ghc = hm.haskell(ghc="9.6.7")
    api = ghc.package("api")
    assert api.build().label == ":haskell: api build"
    assert api.test().label == ":haskell: api test"
    assert api.lint().label == ":haskell: api lint"
    assert api.hlint().label == ":haskell: api hlint"
    assert api.fmt().label == ":haskell: api fmt"


def test_haskell_ghc_required():
    with pytest.raises(ValueError, match="ghc is required"):
        hm.haskell()  # type: ignore[call-overload]


def test_haskell_invalid_ghc_format():
    with pytest.raises(ValueError, match="invalid ghc"):
        hm.haskell(ghc="9.6.7;rm")


def test_haskell_accepts_meta_tags():
    # ghcup accepts meta-tags; the DSL should not pre-reject them.
    hm.haskell(ghc="latest")
    hm.haskell(ghc="recommended")


def test_haskell_accepts_prerelease():
    hm.haskell(ghc="9.10.1-alpha1")


def test_haskell_image_set_on_apt_step():
    ghc = hm.haskell(ghc="9.6.7", image="ubuntu:22.04")
    api = ghc.package("api")
    p = hm.pipeline(api.test())
    apt = _step_by_substring(p, "apt-get install")
    assert apt.get("image") == "ubuntu:22.04"


def test_haskell_with_base_skips_apt():
    base = hm.scratch().sh("base", label="base")
    ghc = hm.haskell(ghc="9.6.7", base=base)
    api = ghc.package("api")
    p = hm.pipeline(api.test(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert not any("apt-get install" in c for c in cmds)
    assert any(c == "base" for c in cmds)
    assert any("ghcup install" in c for c in cmds)


def test_haskell_installed_escape_hatch():
    ghc = hm.haskell(ghc="9.6.7")
    custom = ghc.installed.sh("make openapi", label=":lock: openapi")
    p = hm.pipeline(custom)
    cmds = _cmds(p)
    assert any("make openapi" in c for c in cmds)


def test_haskell_bare_form_single_package():
    p = hm.pipeline(hm.haskell.test(path="freestyle", ghc="9.6.7"))
    cmds = _cmds(p)
    assert any("cd freestyle && cabal test all" in c for c in cmds)


def test_haskell_bare_form_returns_step():
    s = hm.haskell.build(path="freestyle", ghc="9.6.7")
    assert isinstance(s, hm.Step)


def test_haskell_bare_form_forwards_action_kwargs():
    s = hm.haskell.build(path="freestyle", ghc="9.6.7", label=":haskell: custom")
    assert s.label == ":haskell: custom"
