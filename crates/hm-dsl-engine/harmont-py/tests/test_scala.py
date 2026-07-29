"""Scala toolchain and project abstraction tests."""

from __future__ import annotations

import tempfile
from pathlib import Path

import pytest

import harmont as hm
from harmont._scala import _test_cmd
from harmont.cache import CacheOnChange
from harmont.keygen import resolve_pipeline_keys


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


class TestScalaToolchain:
    def test_full_chain(self):
        toolchain = hm.scala.toolchain(path="svc")
        project = hm.pipeline([toolchain.compile()])
        cmds = _cmds(project)
        assert any("apt-get install" in cmd for cmd in cmds)
        assert any("cs install" in cmd for cmd in cmds)
        assert any("cd svc && sbt compile" in cmd for cmd in cmds)

    def test_scala_project_actions_share_install_step(self):
        s = hm.scala.toolchain(path="svc")
        pipeline = hm.pipeline([s.warmup(), s.compile(), s.test(), s.fmt()])
        cmds = _cmds(pipeline)
        assert len([c for c in cmds if "apt-get install" in c]) == 1
        assert len([c for c in cmds if "cs install" in c]) == 1

    def test_cache_forever(self):
        toolchain = hm.scala.toolchain(path="cli")
        pipeline = hm.pipeline([toolchain.compile()])
        coursier_install = _step_by_substring(pipeline, "cs install")
        assert coursier_install["cache"]["policy"] == "forever"

    def test_version_in_coursier_install_cmd(self):
        tc = hm.scala.toolchain(path=".", scala_version="3.8.0")
        pipeline = hm.pipeline([tc.compile()])
        scala_toolchain = _step_by_substring(pipeline, "cs install")
        assert "scala:3.8.0" in scala_toolchain["cmd"]

    def test_invalid_version_rejected(self):
        with pytest.raises(ValueError, match="version"):
            hm.scala.toolchain(scala_version="not a valid; version")

    def test_below_minimum_version_rejected(self):
        with pytest.raises(ValueError, match=r"2\.13"):
            hm.scala.toolchain(scala_version="2.12.18")

    def test_installed_escape_hatch(self):
        tc = hm.scala.toolchain(path="cli")
        custom = tc.installed.sh("cd cli && sbt compile", label=":scala: custom")
        p = hm.pipeline([custom])
        cmds = _cmds(p)
        assert any("sbt compile" in cmd for cmd in cmds)

    def test_action_labels(self):
        tc = hm.scala.toolchain(path=".")
        assert tc.compile().label == ":scala: . compile"
        assert tc.test().label == ":scala: . test"
        assert tc.fmt().label == ":scala: . format"

    def test_action_label_override(self):
        tc = hm.scala.toolchain(path=".")
        s = tc.compile(label=":scala: dev compile")
        assert s.label == ":scala: dev compile"

    def test_action_cache_forwarded(self):
        tc = hm.scala.toolchain(path=".")
        s = tc.compile(cache=CacheOnChange(paths=("build.sbt",)))
        assert s.cache == CacheOnChange(paths=("build.sbt",))

    def test_image_emitted_on_apt_step(self):
        tc = hm.scala.toolchain(path=".", image="alpine:3.20")
        p = hm.pipeline([tc.compile()])
        apt_step = _step_by_substring(p, "apt-get install")
        assert apt_step.get("image") == "alpine:3.20"

    def test_with_base_skips_apt(self):
        base = hm.scratch().sh("custom base", label="base")
        tc = hm.scala.toolchain(path="cli", base=base)
        p = hm.pipeline([tc.compile()])
        cmds = _cmds(p)
        assert not any("apt-get install" in c for c in cmds)
        assert any("custom base" in c for c in cmds)
        assert any("cs install" in c for c in cmds)
        assert any("cd cli && sbt compile" in c for c in cmds)

    def test_warmup_returns_step(self):
        tc = hm.scala.toolchain(path="cli")
        step = tc.warmup()
        assert step.cmd is not None
        assert "sbt update" in step.cmd

    def test_warmup_chains_from_installed(self):
        tc = hm.scala.toolchain(path="cli")
        w = tc.warmup()
        assert w.parent is tc.installed

    def test_warmup_default_label(self):
        tc = hm.scala.toolchain(path=".")
        assert tc.warmup().label == ":scala: warmup"

    def test_warmup_label_override(self):
        tc = hm.scala.toolchain(path=".")
        assert tc.warmup(label=":scala: pre-compile").label == ":scala: pre-compile"

    def test_fmt_check_is_default(self):
        tc = hm.scala.toolchain(path=".")
        assert tc.fmt().cmd.endswith("sbt scalafmtCheckAll")

    def test_fmt_check_false_writes_in_place(self):
        tc = hm.scala.toolchain(path=".")
        assert tc.fmt(check=False).cmd.endswith("sbt scalafmtAll")


class TestScalaProject:
    def test_project_has_methods(self):
        proj = hm.scala.project(path="cli")
        assert proj.warmup.cmd is not None
        assert proj.compile().cmd is not None
        assert proj.test().cmd is not None
        assert proj.fmt().cmd is not None

    def test_empty_path_throws_error_is_caught(self):
        proj = hm.scala.project(path="")
        graph = {
            "nodes": [
                {
                    "step": {
                        "key": "a",
                        "cmd": "sbt compile",
                        "cache": {"policy": "on_change", "paths": list(proj.warmup.cache.paths)},
                    }
                }
            ],
            "node_holes": [],
            "edge_property": "directed",
            "edges": [],
        }

        with tempfile.TemporaryDirectory() as d:
            res = resolve_pipeline_keys(
                graph,
                pipeline_org="default",
                pipeline_slug="default",
                now=0,
                base_path=Path(d),
                env={},
            )
            assert res["nodes"][0]["step"]["cache"]["key"] is not None

    def test_warmup_implicit_cache_on_change(self):
        proj = hm.scala.project(path="cli")
        assert proj.warmup.cache == CacheOnChange(
            paths=(
                "cli/build.sbt",
                "cli/**/src/main/*.scala",
                "cli/**/src/test/*.scala",
                "cli/project/build.properties",
            )
        )

    def test_warmup_implicit_cache_dot_path(self):
        proj = hm.scala.project(path=".")
        assert proj.warmup.cache == CacheOnChange(
            paths=(
                "build.sbt",
                "**/src/main/*.scala",
                "**/src/test/*.scala",
                "project/build.properties",
            )
        )

    def test_warmup_cache_override(self):
        custom = CacheOnChange(paths=("build.sbt",))
        proj = hm.scala.project(path=".", cache=custom)
        assert proj.warmup.cache == custom

    def test_test_command(self):
        proj = hm.scala.project(path="cli")
        assert "sbt test" in proj.test().cmd

    def test_fmt_command(self):
        proj = hm.scala.project(path="cli")
        assert proj.fmt().cmd.endswith("sbt scalafmtCheckAll")

    def test_test_chains_off_warmup(self):
        proj = hm.scala.project(path=".")
        assert proj.test().parent is proj.warmup

    def test_fmt_chains_off_install(self):
        proj = hm.scala.project(path=".")
        assert proj.fmt().parent is proj.toolchain.installed

    def test_ci_returns_compile_test_fmt(self):
        proj = hm.scala.project(path=".")
        steps = proj.ci()
        cmds = [s.cmd for s in steps]
        assert any(c.endswith("sbt compile") for c in cmds)
        assert any("sbt test" in c for c in cmds)
        # CI verifies formatting rather than rewriting it in place.
        assert any(c.endswith("sbt scalafmtCheckAll") for c in cmds)

    def test_toolchain_escape_hatch(self):
        proj = hm.scala.project(path="cli")
        custom = proj.toolchain.installed.sh("custom", label="custom")
        assert custom.parent is proj.toolchain.installed

    def test_with_base_skips_apt(self):
        base = hm.scratch().sh("custom base", label="base")
        proj = hm.scala.project(path="cli", base=base)
        p = hm.pipeline([proj.test(), proj.compile(), proj.fmt()])
        cmds = _cmds(p)
        assert not any("apt-get install" in c for c in cmds)
        assert any("custom base" in c for c in cmds)

    def test_labels(self):
        proj = hm.scala.project(path="cli")
        assert proj.warmup.label == ":scala: sbt warmup"
        assert proj.test().label == ":scala: cli sbt test"
        assert proj.fmt().label == ":scala: cli format"
        assert proj.compile().label == ":scala: cli sbt compile"

    def test_pipeline_ir(self):
        proj = hm.scala.project(path="cli")
        p = hm.pipeline([proj.test(), proj.fmt(), proj.compile()])
        cmds = _cmds(p)
        assert any("sbt update" in c for c in cmds)
        assert any("sbt test" in c for c in cmds)
        assert any("sbt scalafmtCheckAll" in c for c in cmds)
        assert any("sbt compile" in c for c in cmds)
        assert len([c for c in cmds if "cs install" in c]) == 1
        assert len([c for c in cmds if "apt-get install" in c]) == 1

    def test_version_forwarded(self):
        proj = hm.scala.project(path=".", scala_version="3.3.4")
        p = hm.pipeline([proj.test()])
        cs_install = _step_by_substring(p, "cs install")
        assert "cs install scala:3.3.4" in cs_install["cmd"]


class TestTestCmd:
    @pytest.mark.parametrize(
        ("kwargs", "expected"),
        [
            ({}, "sbt test"),
            ({"query": "core"}, "sbt core / test"),
            ({"testnames": ("FooSpec", "BarSpec")}, "sbt test FooSpec BarSpec"),
            ({"query": "core", "testnames": ("FooSpec",)}, "sbt core / test FooSpec"),
            ({"options": {"-only": "FooSpec"}}, "sbt test -- -only FooSpec"),
            ({"options": {"-o": "a b"}}, "sbt test -- -o 'a b'"),
        ],
    )
    def test_renders(self, kwargs: dict, expected: str):
        assert _test_cmd(**kwargs) == expected
