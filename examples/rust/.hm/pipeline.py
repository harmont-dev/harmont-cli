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
    triggers=[hm.push(branch="main")],
)
def ci(project: hm.Target[RustProject]) -> tuple[hm.Step, ...]:
    # ci() is the zero-config DAG: test + clippy + fmt, all sharing one warmup.
    # Pass nextest=True if cargo-nextest is available to also split out doctests.
    return project.ci()
