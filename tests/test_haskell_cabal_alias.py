"""HaskellToolchain.cabal alias for .package (HAR-28)."""
from __future__ import annotations

import harmont as hm
from harmont.haskell import HaskellPackage


def test_cabal_returns_haskell_package(tmp_path, monkeypatch):
    # Run from tmp_path so the default cabal_paths glob doesn't try to
    # read the real api/ directory.
    monkeypatch.chdir(tmp_path)
    (tmp_path / "api").mkdir()
    pkg = hm.haskell(ghc="9.6.7").cabal(path="api")
    assert isinstance(pkg, HaskellPackage)
    assert pkg.path == "api"


def test_cabal_accepts_cache_paths(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    pkg = hm.haskell(ghc="9.6.7").cabal(
        path="api", cache_paths=("api/api.cabal", "api/cabal.project")
    )
    assert isinstance(pkg, HaskellPackage)


def test_cabal_equivalent_to_package(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "api").mkdir()
    tc = hm.haskell(ghc="9.6.7")
    via_cabal = tc.cabal(path="api")
    via_package = tc.package(path="api")
    # Same path, same shape (different Step instances since each call
    # builds a new chain — but the .installed cmd should match).
    assert via_cabal.path == via_package.path
    assert via_cabal.installed.cmd == via_package.installed.cmd
