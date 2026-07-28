"""Scala example pipeline"""
from __future__ import annotations

import harmont as hm
from harmont._scala import ScalaProject


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    triggers=[hm.push(branch="main")]
)
def ci(project: hm.Target[ScalaProject]) -> tuple[hm.Step, ...]:
    project = hm.scala.project(path=".")
    return project.ci()
