"""Elixir Phoenix example pipeline."""
from __future__ import annotations

import harmont as hm


@hm.target()
def project():
    return hm.elixir(path=".", elixir_version="1.18.3", otp_version="27.3.3")


@hm.pipeline(
    "ci",
    env={"CI": "true", "MIX_ENV": "test"},
    triggers=[hm.push(branch="main"), hm.pr()],
)
def ci(project: hm.Target) -> tuple[hm.Step, ...]:
    return (
        project.compile(),
        project.test(cover=True),
        project.format(),
        project.credo(),
        project.dialyzer(),
        project.sobelow(),
        project.deps_audit(),
        project.hex_audit(),
    )


@hm.pipeline(
    "deploy",
    env={"MIX_ENV": "prod"},
    triggers=[hm.push(branch="main")],
)
def deploy(project: hm.Target) -> tuple[hm.Step, ...]:
    return (
        project.compile(),
        project.mix("assets.deploy"),
        project.release(),
    )
