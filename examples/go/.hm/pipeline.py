"""Go example pipeline."""
from __future__ import annotations

import harmont as hm
from harmont._go import GoToolchain


@hm.target()
def project() -> GoToolchain:
    return hm.go(path=".")


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def ci(project: hm.Target[GoToolchain]) -> tuple[hm.Step, ...]:
    return (project.build(), project.test(), project.vet(), project.fmt())
