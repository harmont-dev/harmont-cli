"""Elixir CI pipeline."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true", "MIX_ENV": "test"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.elixir(path=".")
    return (
        project.compile(),
        project.test(),
        project.format(),
    )
