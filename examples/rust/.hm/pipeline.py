"""Rust example pipeline."""
from __future__ import annotations

import harmont as hm
from harmont._rust import RustToolchain


@hm.target()
def project() -> RustToolchain:
    return hm.rust.toolchain(path=".")


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def ci(project: hm.Target[RustToolchain]) -> tuple[hm.Step, ...]:
    return (project.build(), project.test(), project.clippy(), project.fmt())
