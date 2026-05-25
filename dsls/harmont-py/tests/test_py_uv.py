"""py.uv toolchain namespace tests."""

from __future__ import annotations

import pytest

import harmont as hm
from harmont.cache import CacheOnChange


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


# ── TestUvObjectForm ─────────────────────────────────────────────


class TestUvObjectForm:
    def test_full_chain(self):
        proj = hm.py.uv(path="svc")
        p = hm.pipeline(proj.test(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("apt-get install" in c for c in cmds)
        assert any("astral.sh/uv/install.sh" in c for c in cmds)
        assert any("cd svc && uv sync" in c for c in cmds)
        assert any("cd svc && uv run pytest" in c for c in cmds)

    def test_shared_install(self):
        proj = hm.py.uv(path="svc")
        p = hm.pipeline(
            proj.test(),
            proj.lint(),
            proj.fmt(),
            proj.typecheck(),
            default_image="ubuntu:24.04",
        )
        cmds = _cmds(p)
        assert len([c for c in cmds if "astral.sh/uv/install.sh" in c]) == 1
        assert len([c for c in cmds if "apt-get install" in c]) == 1
        assert any("uv run pytest" in c for c in cmds)
        assert any("uv run ruff check" in c for c in cmds)
        assert any("uv run ruff format --check" in c for c in cmds)
        assert any("uv run ty check" in c for c in cmds)

    def test_sync_cached_on_change(self):
        proj = hm.py.uv(path="svc")
        p = hm.pipeline(proj.test())
        sync = _step_by_substring(p, "uv sync")
        assert sync["cache"]["policy"] == "on_change"
        assert "svc/uv.lock" in sync["cache"]["paths"]
        assert "svc/pyproject.toml" in sync["cache"]["paths"]

    def test_install_cache_forever(self):
        proj = hm.py.uv(path=".")
        p = hm.pipeline(proj.test())
        install = _step_by_substring(p, "astral.sh/uv/install.sh")
        assert install["cache"]["policy"] == "forever"


# ── TestUvActions ────────────────────────────────────────────────


class TestUvActions:
    def test_labels_auto_generated(self):
        proj = hm.py.uv(path=".")
        assert proj.test().label == ":python: test"
        assert proj.lint().label == ":python: lint"
        assert proj.fmt().label == ":python: fmt"
        assert proj.typecheck().label == ":python: typecheck"
        assert proj.build().label == ":python: build"
        assert proj.lock_check().label == ":python: lock-check"
        assert proj.publish().label == ":python: publish"

    def test_label_override(self):
        proj = hm.py.uv(path=".")
        assert proj.test(label=":python: smoke").label == ":python: smoke"

    def test_typecheck_paths_string(self):
        proj = hm.py.uv(path="myapp")
        s = proj.typecheck(paths="src")
        assert "uv run ty check src" in s.cmd

    def test_typecheck_paths_list(self):
        proj = hm.py.uv(path="myapp")
        s = proj.typecheck(paths=["src", "tests"])
        assert "uv run ty check src tests" in s.cmd

    def test_typecheck_paths_default(self):
        proj = hm.py.uv(path="myapp")
        s = proj.typecheck()
        assert "uv run ty check ." in s.cmd

    def test_cache_forwarded(self):
        proj = hm.py.uv(path=".")
        s = proj.test(cache=CacheOnChange(paths=("pyproject.toml",)))
        assert s.cache == CacheOnChange(paths=("pyproject.toml",))

    def test_run_command(self):
        proj = hm.py.uv(path="svc")
        p = hm.pipeline(proj.run("flask run --port 8080"))
        cmds = _cmds(p)
        assert any("cd svc && uv run flask run --port 8080" in c for c in cmds)

    def test_run_auto_label_uses_first_word(self):
        proj = hm.py.uv(path=".")
        assert proj.run("flask run --port 8080").label == ":python: flask"

    def test_build_command(self):
        proj = hm.py.uv(path="svc")
        p = hm.pipeline(proj.build())
        cmds = _cmds(p)
        assert any("cd svc && uv build" in c for c in cmds)

    def test_lock_check_command(self):
        proj = hm.py.uv(path="svc")
        p = hm.pipeline(proj.lock_check())
        cmds = _cmds(p)
        assert any("cd svc && uv lock --check" in c for c in cmds)

    def test_publish_command(self):
        proj = hm.py.uv(path="svc")
        p = hm.pipeline(proj.publish())
        cmds = _cmds(p)
        assert any("cd svc && uv publish" in c for c in cmds)


# ── TestUvChainSetup ────────────────────────────────────────────


class TestUvChainSetup:
    def test_image_emitted_on_apt_step(self):
        proj = hm.py.uv(path=".", image="ubuntu:24.04")
        p = hm.pipeline(proj.test())
        apt = _step_by_substring(p, "apt-get install")
        assert apt.get("image") == "ubuntu:24.04"

    def test_base_skips_apt(self):
        base = hm.scratch().sh("custom base", label="base")
        proj = hm.py.uv(path="svc", base=base)
        p = hm.pipeline(proj.test(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert not any("apt-get install" in c for c in cmds)
        assert any("custom base" in c for c in cmds)
        assert any("astral.sh/uv/install.sh" in c for c in cmds)

    def test_installed_escape_hatch(self):
        proj = hm.py.uv(path="svc")
        custom = proj.installed.sh(
            "cd svc && uv run python -m mytool",
            label=":python: custom",
        )
        p = hm.pipeline(custom)
        cmds = _cmds(p)
        assert any("mytool" in c for c in cmds)


# ── TestUvVersionValidation ─────────────────────────────────────


class TestUvVersionValidation:
    def test_pinned_version(self):
        proj = hm.py.uv(path=".", version="0.4.18")
        p = hm.pipeline(proj.test())
        install = _step_by_substring(p, "astral.sh/uv/install.sh")
        assert "UV_VERSION=0.4.18" in install["cmd"]

    def test_invalid_version_rejected(self):
        with pytest.raises(ValueError, match="invalid version"):
            hm.py.uv(version="not a valid; version")


# ── TestUvBareForm ───────────────────────────────────────────────


class TestUvBareForm:
    def test_bare_test(self):
        p = hm.pipeline(hm.py.uv.test())
        cmds = _cmds(p)
        assert any("cd . && uv run pytest" in c for c in cmds)

    def test_bare_lint(self):
        p = hm.pipeline(hm.py.uv.lint())
        cmds = _cmds(p)
        assert any("cd . && uv run ruff check" in c for c in cmds)

    def test_bare_fmt(self):
        p = hm.pipeline(hm.py.uv.fmt())
        cmds = _cmds(p)
        assert any("cd . && uv run ruff format --check" in c for c in cmds)

    def test_bare_typecheck(self):
        p = hm.pipeline(hm.py.uv.typecheck())
        cmds = _cmds(p)
        assert any("cd . && uv run ty check" in c for c in cmds)

    def test_bare_run(self):
        p = hm.pipeline(hm.py.uv.run("serve"))
        cmds = _cmds(p)
        assert any("cd . && uv run serve" in c for c in cmds)

    def test_bare_build(self):
        p = hm.pipeline(hm.py.uv.build())
        cmds = _cmds(p)
        assert any("cd . && uv build" in c for c in cmds)
