"""C example pipeline."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.cmake(path=".", defines={"CMAKE_BUILD_TYPE": "Release"})
    return (
        project.test(),
        project.fmt(),
    )
