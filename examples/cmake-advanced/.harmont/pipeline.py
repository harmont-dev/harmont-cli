"""Advanced CMake pipeline — compiler selection, multiple actions."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main"), hm.pr()],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.cmake(
        path=".",
        compiler="clang-18",
        defines={
            "CMAKE_BUILD_TYPE": "Release",
            "CMAKE_CXX_STANDARD": "20",
            "BUILD_TESTING": "ON",
        },
    )
    return (
        project.test(),
        project.lint(),
        project.fmt(),
    )
