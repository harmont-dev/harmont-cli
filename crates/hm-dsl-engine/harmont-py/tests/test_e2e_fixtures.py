"""E2E fixture generation + validation.

Renders 4 complex pipeline scenarios to v0 IR JSON and writes
committed fixtures for Rust deserialization tests.

Regenerate: UPDATE_E2E_FIXTURES=1 pytest tests/test_e2e_fixtures.py -v
"""

from __future__ import annotations

import json
import os
from datetime import timedelta
from pathlib import Path

import pytest

import harmont as hm
from harmont._cmake import cmake
from harmont._go import go
from harmont._js import js
from harmont._python import python as python_tc
from harmont._rust import rust
from harmont._zig import zig

REPO_ROOT = Path(__file__).resolve().parents[4]
FIXTURES_DIR = REPO_ROOT / "tests" / "e2e" / "fixtures" / "python"


def _render(ir: dict) -> str:
    return json.dumps(ir, indent=2, sort_keys=True, ensure_ascii=False)


def _assert_fixture(name: str, ir: dict) -> None:
    rendered = _render(ir)
    fixture_path = FIXTURES_DIR / f"{name}.json"

    if os.environ.get("UPDATE_E2E_FIXTURES"):
        fixture_path.write_text(rendered + "\n")
        return

    assert fixture_path.exists(), (
        f"Fixture {fixture_path} missing — run with UPDATE_E2E_FIXTURES=1"
    )
    expected = json.loads(fixture_path.read_text())
    actual = json.loads(rendered)
    assert actual == expected, f"Fixture drift for {name}. Regenerate with UPDATE_E2E_FIXTURES=1"


def _build_monorepo_ci() -> dict:
    go_project = go(path="services/api")
    py_project = python_tc(path="services/ml")
    web_project = js.project(path="web")

    return hm.pipeline(
        [
            go_project.build(),
            go_project.test(),
            go_project.vet(),
            py_project.test(),
            py_project.lint(),
            py_project.typecheck(),
            web_project.run("build"),
            web_project.run("test"),
            web_project.run("lint"),
        ],
        env={"CI": "true"},
    )


def _build_rust_release() -> dict:
    project = rust.toolchain(path=".")

    return hm.pipeline(
        [project.build(), project.test(), project.clippy(), project.fmt(), project.doc()],
        env={"CI": "true"},
    )


def _build_zig_node_polyglot() -> dict:
    base = hm.sh(
        "apt-get update && apt-get install -y --no-install-recommends "
        "curl ca-certificates xz-utils",
        label=":apt: base",
        cache=hm.ttl(timedelta(days=1)),
        image="ubuntu:24.04",
    )
    zig_tc = zig(base=base)
    proj_a = zig_tc.project(path="zig-a")
    proj_b = zig_tc.project(path="zig-b")
    web = js.project(path="web", base=base)

    return hm.pipeline(
        [
            proj_a.build(),
            proj_a.test(),
            proj_b.build(),
            proj_b.test(),
            web.run("build"),
            web.run("test"),
            web.run("lint"),
        ],
        env={"CI": "true"},
    )


def _build_kitchen_sink() -> dict:
    c_project = cmake(path="infra/agent")
    py_web = python_tc(path="services/web")

    return hm.pipeline(
        [
            c_project.build(),
            c_project.test(),
            c_project.fmt(),
            py_web.test(),
            py_web.lint(),
        ],
        env={"CI": "true"},
    )


def _build_cmake_advanced() -> dict:
    project = cmake(
        path=".",
        compiler="clang-18",
        defines={
            "CMAKE_BUILD_TYPE": "Release",
            "CMAKE_CXX_STANDARD": "20",
        },
    )
    return hm.pipeline(
        [project.test(), project.lint(), project.fmt()],
        env={"CI": "true"},
    )


SCENARIOS = {
    "monorepo-ci": _build_monorepo_ci,
    "rust-release": _build_rust_release,
    "zig-node-polyglot": _build_zig_node_polyglot,
    "kitchen-sink": _build_kitchen_sink,
    "cmake-advanced": _build_cmake_advanced,
}


@pytest.mark.parametrize("name", SCENARIOS.keys())
def test_e2e_fixture(name: str) -> None:
    ir = SCENARIOS[name]()

    assert ir["version"] == "0"
    assert len(ir["graph"]["nodes"]) > 0
    # Root steps (no builds_in parent) should carry the ubuntu:24.04 default image.
    nodes = ir["graph"]["nodes"]
    edges = ir["graph"]["edges"]
    child_idxs = {e[1] for e in edges if e[2] == "builds_in"}
    roots = [n for i, n in enumerate(nodes) if i not in child_idxs]
    assert all(n["step"].get("image") == "ubuntu:24.04" for n in roots)
    assert ir["graph"]["edge_property"] == "directed"

    for node in ir["graph"]["nodes"]:
        assert "key" in node["step"]
        assert "cmd" in node["step"]
        assert isinstance(node["env"], dict)

    for src, dst, kind in ir["graph"]["edges"]:
        assert kind in ("builds_in", "depends_on")
        assert src < len(ir["graph"]["nodes"])
        assert dst < len(ir["graph"]["nodes"])

    _assert_fixture(name, ir)
