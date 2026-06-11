"""zig monorepo + js parallelism demo.

Three patterns demonstrated together:

  1. apt-base forks into separate language chains (zig vs node) that
     run in parallel containers.
  2. ONE zig install is shared by TWO zig sub-projects. The two
     project chains fork off the single :zig: install snapshot.
  3. Independent chains run concurrently — everything that can run
     in parallel does.
"""
from __future__ import annotations

from datetime import timedelta
from typing import Annotated

import harmont as hm
from harmont._js import JsProject
from harmont._zig import ZigProject, ZigToolchain


@hm.target()
def apt_base(base: Annotated[hm.Step, hm.BaseImage("ubuntu:24.04")]) -> hm.Step:
    return base.sh(
        "apt-get update && "
        "apt-get install -y --no-install-recommends "
        "curl ca-certificates xz-utils",
        label=":apt: base",
        cache=hm.ttl(timedelta(days=1)),
    )


@hm.target()
def zig(apt_base: hm.Target[hm.Step]) -> ZigToolchain:
    return hm.zig(base=apt_base)


@hm.target()
def zig_lib_a(zig: hm.Target[ZigToolchain]) -> ZigProject:
    return zig.project(path="zig-a")


@hm.target()
def zig_lib_b(zig: hm.Target[ZigToolchain]) -> ZigProject:
    return zig.project(path="zig-b")


@hm.target()
def web_project(apt_base: hm.Target[hm.Step]) -> JsProject:
    return hm.js.project(path="web", base=apt_base)


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    triggers=[hm.push(branch="main")],
)
def ci(
    zig_lib_a: hm.Target[ZigProject],
    zig_lib_b: hm.Target[ZigProject],
    web_project: hm.Target[JsProject],
) -> tuple[hm.Step, ...]:
    return (
        zig_lib_a.build(),
        zig_lib_a.test(),
        zig_lib_b.build(),
        zig_lib_b.test(),
        web_project.run("build"),
        web_project.run("test"),
        web_project.run("lint"),
    )
