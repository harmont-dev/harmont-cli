"""Scala CI pipeline."""

from __future__ import annotations

import harmont as hm
from harmont._scala import ScalaProject


@hm.target()
def project() -> ScalaProject:
    # project() warms a shared dependency cache (keyed on build.sbt + sources)
    # so compile/test reuse one `sbt update`.
    return hm.scala.project(path=".")


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    triggers=[hm.push(branch="main")],
)
def ci(project: hm.Target[ScalaProject]) -> tuple[hm.Step, ...]:
    # ci() is the zero-config DAG: compile + test sharing one warmup, plus a
    # scalafmt check running in parallel off the toolchain install.
    return project.ci()
