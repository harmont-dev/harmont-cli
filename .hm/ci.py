"""Harmont CI pipeline — dogfood."""
from __future__ import annotations

import harmont as hm


@hm.target()
def shared_base() -> hm.Step:
    return hm.apt_base(packages=(
        "curl",
        "ca-certificates",
        "build-essential",
        "pkg-config",
        "libssl-dev",
        "protobuf-compiler",
        "python3",
        "python3-venv",
    ))


@hm.target()
def rust_project(shared_base: hm.Target[hm.Step]) -> tuple[hm.Step, ...]:
    project = hm.rust.project(path=".", base=shared_base)
    return hm.group([
        project.test(flags=("--lib",), packages=("harmont-cli",)),
        project.test(
            flags=("--test", "cmd_init"),
            packages=("harmont-cli",),
            label=":test_tube: init template roundtrip",
        ),
        project.clippy(),
        project.fmt(),
    ])


@hm.target()
def py_project(shared_base: hm.Target[hm.Step]) -> tuple[hm.Step, ...]:
    project = hm.py.uv(path="crates/hm-dsl-engine/harmont-py", base=shared_base)
    return hm.group([
        project.lint(),
        project.fmt(),
        project.typecheck(paths="harmont"),
        project.run(
            "pytest -v",
            label=":python: test",
        ),
    ])


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[
        hm.push(branch="main"),
        hm.pr(branches="main"),
    ],
)
def ci(
    rust_project: hm.Target[tuple[hm.Step, ...]],
    py_project: hm.Target[tuple[hm.Step, ...]],
) -> list:
    return [rust_project, py_project]
