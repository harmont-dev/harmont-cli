"""@hm.target() decorator — memoization + composition (HAR-28)."""

from __future__ import annotations

import json

import pytest

import harmont as hm
from harmont._deps import clear_target_names
from harmont._target import (
    clear_target_cache,
    evaluate_dynamic_target,
    render_dynamic_target_json,
)


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


def test_target_with_toolchain_return_passes_through():
    @hm.target()
    def api():
        return hm.go(path="api")

    from harmont._go import GoToolchain

    result = api()
    assert isinstance(result, GoToolchain)
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


def test_dynamic_target_returns_placeholder_without_evaluating_body():
    call_count = 0

    @hm.target(dynamic=True)
    def choose_build() -> hm.Step:
        nonlocal call_count
        call_count += 1
        return hm.sh("cargo test")

    placeholder = choose_build()

    assert call_count == 0
    assert placeholder.dynamic_target_name == "choose_build"
    assert placeholder.cmd is None


def test_dynamic_target_body_can_be_evaluated_by_name():
    @hm.target(dynamic=True)
    def choose_build() -> hm.Step:
        return hm.sh("cargo test")

    leaves = evaluate_dynamic_target("choose_build")

    assert len(leaves) == 1
    assert leaves[0].cmd == "cargo test"


def test_dynamic_target_returns_group_leaves():
    @hm.target(dynamic=True)
    def checks() -> tuple[hm.Step, ...]:
        return hm.group([hm.sh("cargo test"), hm.sh("cargo clippy")])

    leaves = evaluate_dynamic_target("checks")

    assert [leaf.cmd for leaf in leaves] == ["cargo test", "cargo clippy"]

    fragment = json.loads(render_dynamic_target_json("checks"))
    assert len(fragment["graph"]["nodes"]) == 2
    assert fragment["graph"]["edges"] == []


def test_dynamic_group_can_define_explicit_continuation():
    @hm.target(dynamic=True)
    def checks() -> tuple[hm.Step, ...]:
        return hm.group(
            [
                hm.sh("cargo test", label="test"),
                hm.sh("cargo clippy", label="clippy"),
                hm.wait(),
                hm.sh("prepare deploy", label="merge"),
            ]
        )

    fragment = json.loads(render_dynamic_target_json("checks"))
    keys = [node["step"]["key"] for node in fragment["graph"]["nodes"]]
    merge = keys.index("merge")
    depends_on = {
        (source, target)
        for source, target, kind in fragment["graph"]["edges"]
        if kind == "depends_on"
    }

    assert depends_on == {(keys.index("test"), merge), (keys.index("clippy"), merge)}


def test_dynamic_target_json_contains_concrete_fragment_and_runtime_env():
    @hm.target(dynamic=True)
    def choose_build() -> hm.Step:
        return hm.sh("cargo test", label="selected build")

    fragment = json.loads(
        render_dynamic_target_json("choose_build", env={"PROFILE": "release"})
    )
    node = fragment["graph"]["nodes"][0]

    assert node["step"]["eval"] == {"type": "cmd", "cmd": "cargo test"}
    assert node["env"]["PROFILE"] == "release"
