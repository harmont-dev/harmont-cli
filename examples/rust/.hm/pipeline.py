"""Rust example pipeline — the one-call CI DAG."""

from __future__ import annotations

import harmont as hm
from harmont._rust import RustProject


@hm.target()
def project() -> RustProject:
    # project() warms a shared dependency cache keyed on Cargo.lock + sources,
    # so test/clippy/fmt reuse one compile.
    return hm.rust.project(path=".")


@hm.pipeline(
    "ci",
    env={"CI": "true", "RUST_BACKTRACE": "1"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def ci(project: hm.Target[RustProject]) -> tuple[hm.Step, ...]:
    # ci(nextest=True) runs `cargo nextest run` and adds a doctest step
    # (nextest can't run doctests), plus clippy + fmt.
    return project.ci(nextest=True)
