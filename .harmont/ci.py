"""Harmont CI pipeline — dogfood."""
from __future__ import annotations

import harmont as hm
from harmont.py.uv import UvProject
from harmont.rust import RustToolchain


@hm.target()
def rust_project() -> RustToolchain:
    return hm.rust(path=".")


@hm.target()
def py_project() -> UvProject:
    return hm.py.uv(path="dsls/harmont-py")


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[
        hm.push(branch="main"),
        hm.pull_request(branches="main"),
    ],
)
def ci(
    rust_project: hm.Target[RustToolchain],
    py_project: hm.Target[UvProject],
) -> tuple[hm.Step, ...]:
    return (
        rust_project.build(),
        rust_project.installed.sh(
            ". $HOME/.cargo/env && cd . && cargo test --lib",
            label=":rust: test",
        ),
        rust_project.clippy(),
        rust_project.fmt(),
        py_project.lint(),
        py_project.fmt(),
        py_project.typecheck(paths="harmont"),
        py_project.run(
            "pytest -v"
            " --deselect tests/test_gradle.py"
            " --deselect tests/test_haskell.py",
            label=":python: test",
        ),
    )
