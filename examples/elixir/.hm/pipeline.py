"""Elixir example pipeline."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true", "MIX_ENV": "test"},
    triggers=[hm.push(branch="main")],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.elixir(path=".")
    return (
        project.compile(),
        project.test(),
        project.format(),
        project.credo(),
        project.dialyzer(),
        project.deps_audit(),
        project.hex_audit(),
    )
