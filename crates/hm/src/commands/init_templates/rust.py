"""Rust CI pipeline."""

from __future__ import annotations

import harmont as hm
from harmont._rust import RustProject


@hm.target()
def project() -> RustProject:
    # project() warms a shared dependency cache (keyed on Cargo.lock + sources)
    # so test/clippy/fmt reuse one compile.
    return hm.rust.project(path=".")


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    triggers=[hm.push(branch="main")],
)
def ci(project: hm.Target[RustProject]) -> tuple[hm.Step, ...]:
    # ci() is the zero-config DAG: test + clippy + fmt sharing one warmup.
    # To cross-compile, add e.g. project.build(target="wasm32-unknown-unknown") —
    # the rustup target is installed automatically.
    return project.ci()
