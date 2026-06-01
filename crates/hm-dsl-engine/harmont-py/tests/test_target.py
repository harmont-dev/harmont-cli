"""@hm.target() decorator — memoization + composition (HAR-28)."""

from __future__ import annotations

import pytest

import harmont as hm
from harmont._deps import clear_target_names
from harmont._target import clear_target_cache


@pytest.fixture(autouse=True)
def _reset_target_cache():
    clear_target_cache()
    clear_target_names()
    yield
    clear_target_cache()
    clear_target_names()


def test_target_returns_function_unchanged_in_signature():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    # callable with no args, returns a Step
    result = apt_base()
    assert isinstance(result, hm.Step)
    assert result.cmd == "apt-get update"


def test_target_memoizes_within_one_render():
    call_count = 0

    @hm.target()
    def apt_base() -> hm.Step:
        nonlocal call_count
        call_count += 1
        return hm.sh("apt-get update")

    a = apt_base()
    b = apt_base()
    assert a is b
    assert call_count == 1


def test_clear_target_cache_resets_memoization():
    call_count = 0

    @hm.target()
    def apt_base() -> hm.Step:
        nonlocal call_count
        call_count += 1
        return hm.sh("apt-get update")

    apt_base()
    clear_target_cache()
    apt_base()
    assert call_count == 2


def test_composition_via_chaining_off_a_target():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    @hm.target()
    def venv() -> hm.Step:
        return apt_base().sh("python3 -m venv .venv")

    @hm.target()
    def api() -> hm.Step:
        return apt_base().sh("cabal build")

    v = venv()
    a = api()
    # Both targets chained off the SAME apt-base step (memoized).
    assert v.parent is a.parent
    assert v.parent is not None
    assert v.parent.cmd == "apt-get update"


def test_target_with_toolchain_return_passes_through(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "api").mkdir()

    @hm.target()
    def api():
        return hm.haskell(ghc="9.6.7").cabal(path="api")

    from harmont._haskell import HaskellPackage

    result = api()
    assert isinstance(result, HaskellPackage)
    assert result.path == "api"


def test_target_called_inside_pipeline_uses_cached_value():
    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    @hm.target()
    def venv() -> hm.Step:
        return apt_base().sh("venv setup")

    # Direct invocation: same call returns same Step.
    v1 = venv()
    v2 = venv()
    assert v1 is v2
