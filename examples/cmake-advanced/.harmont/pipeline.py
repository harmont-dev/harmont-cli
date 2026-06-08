"""Advanced CMake pipeline — compiler matrix, sanitizers, coverage."""
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
        build_type="Release",
        std=20,
        defines={"BUILD_TESTING": "ON"},
    )
    return (
        project.test(),
        project.lint(),
        project.fmt(),
    )


@hm.pipeline(
    "sanitizers",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def sanitizers() -> tuple[hm.Step, ...]:
    project = hm.cmake(path=".", compiler="clang-18")
    return (
        project.sanitize("asan"),
        project.sanitize("tsan"),
    )


@hm.pipeline(
    "coverage",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def coverage() -> tuple[hm.Step, ...]:
    project = hm.cmake(path=".")
    return (project.coverage(),)
