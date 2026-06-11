"""Bun example pipeline."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    triggers=[hm.push(branch="main")],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.js.project(path=".", runtime="bun")
    return (
        project.run("build"),
        project.run("test"),
        project.run("lint"),
    )
