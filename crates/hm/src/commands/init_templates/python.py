"""Python CI pipeline."""
from __future__ import annotations

import harmont as hm
from harmont._python import PythonToolchain


@hm.target()
def project() -> PythonToolchain:
    return hm.python(path=".")


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    triggers=[hm.push(branch="main")],
)
def ci(project: hm.Target[PythonToolchain]) -> tuple[hm.Step, ...]:
    return (
        project.test(),
        project.lint(),
        project.fmt(),
        project.typecheck(),
    )
