"""Zig example pipeline."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    triggers=[hm.push(branch="main")],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.zig(path=".")
    return (
        project.build(),
        project.test(),
        project.fmt(),
    )
