"""C++ example pipeline."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.cmake(path=".", build_type="Release", std=17)
    return (
        project.test(),
        project.lint(),
        project.fmt(),
    )
