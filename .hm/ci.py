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
    # Build esbuild into the image first: hm-dsl-engine's build.rs shells out
    # to esbuild at compile time to embed the TypeScript SDK bundle. Without it
    # the build emits an 18-byte stub and the bundled_sources tests fail (CLI-37).
    # Installing Node here also lets the JS-runtime-gated render tests actually
    # run instead of self-skipping.
    ts_deps = hm.js.project(
        path="crates/hm-dsl-engine/harmont-ts",
        base=shared_base,
    ).install()
    project = hm.rust.project(path=".", base=ts_deps)
    return hm.group([
        project.test(),  # cargo test --workspace --locked — every package
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


@hm.target()
def ts_project(shared_base: hm.Target[hm.Step]) -> tuple[hm.Step, ...]:
    project = hm.js.project(
        path="crates/hm-dsl-engine/harmont-ts",
        base=shared_base,
    )
    return hm.group([
        project.run("typecheck", label=":typescript: tsc"),
        project.run("test", label=":test_tube: vitest"),
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
    ts_project: hm.Target[tuple[hm.Step, ...]],
) -> list:
    return [rust_project, py_project, ts_project]
