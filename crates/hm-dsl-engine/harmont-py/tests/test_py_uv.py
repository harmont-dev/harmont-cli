"""py.uv toolchain namespace tests."""

from __future__ import annotations

import pytest

import harmont as hm
from harmont._serialize import serialize_step_chain
from harmont.cache import CacheOnChange


def _cmds(leaves: list) -> list[str]:
    chain = serialize_step_chain(list(leaves))
    return [s["cmd"] for s in chain["steps"] if s.get("cmd") is not None]


def _step_by_substring(leaves: list, needle: str) -> dict:
    chain = serialize_step_chain(list(leaves))
    for s in chain["steps"]:
        if needle in (s.get("cmd") or ""):
            return s
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


# ── TestUvObjectForm ─────────────────────────────────────────────


class TestUvObjectForm:
    def test_full_chain(self):
        proj = hm.py.uv(path="svc")
        cmds = _cmds([proj.test()])
        assert any("apt-get install" in c for c in cmds)
        assert any("astral.sh/uv/install.sh" in c for c in cmds)
        assert any("cd svc && uv sync" in c for c in cmds)
        assert any("cd svc && uv run pytest" in c for c in cmds)

    def test_shared_install(self):
        proj = hm.py.uv(path="svc")
        cmds = _cmds([proj.test(), proj.lint(), proj.fmt(), proj.typecheck()])
        assert len([c for c in cmds if "astral.sh/uv/install.sh" in c]) == 1
        assert len([c for c in cmds if "apt-get install" in c]) == 1
        assert any("uv run pytest" in c for c in cmds)
        assert any("uv run ruff check" in c for c in cmds)
        assert any("uv run ruff format --check" in c for c in cmds)
        assert any("uv run ty check" in c for c in cmds)

    def test_sync_cached_on_change(self):
        proj = hm.py.uv(path="svc")
        sync = _step_by_substring([proj.test()], "uv sync")
        assert sync["cache"]["policy"] == "on_change"
        assert "svc/uv.lock" in sync["cache"]["paths"]
        assert "svc/pyproject.toml" in sync["cache"]["paths"]

    def test_install_cache_forever(self):
        proj = hm.py.uv(path=".")
        install = _step_by_substring([proj.test()], "astral.sh/uv/install.sh")
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
        cmds = _cmds([proj.run("flask run --port 8080")])
        assert any("cd svc && uv run flask run --port 8080" in c for c in cmds)

    def test_run_auto_label_uses_first_word(self):
        proj = hm.py.uv(path=".")
        assert proj.run("flask run --port 8080").label == ":python: flask"

    def test_build_command(self):
        proj = hm.py.uv(path="svc")
        cmds = _cmds([proj.build()])
        assert any("cd svc && uv build" in c for c in cmds)

    def test_lock_check_command(self):
        proj = hm.py.uv(path="svc")
        cmds = _cmds([proj.lock_check()])
        assert any("cd svc && uv lock --check" in c for c in cmds)

    def test_publish_command(self):
        proj = hm.py.uv(path="svc")
        cmds = _cmds([proj.publish()])
        assert any("cd svc && uv publish" in c for c in cmds)


# ── TestUvChainSetup ────────────────────────────────────────────


class TestUvChainSetup:
    def test_image_emitted_on_apt_step(self):
        proj = hm.py.uv(path=".", image="ubuntu:24.04")
        apt = _step_by_substring([proj.test()], "apt-get install")
        assert apt.get("image") == "ubuntu:24.04"

    def test_base_skips_apt(self):
        base = hm.scratch().sh("custom base", label="base")
        proj = hm.py.uv(path="svc", base=base)
        cmds = _cmds([proj.test()])
        assert not any("apt-get install" in c for c in cmds)
        assert any("custom base" in c for c in cmds)
        assert any("astral.sh/uv/install.sh" in c for c in cmds)

    def test_installed_escape_hatch(self):
        proj = hm.py.uv(path="svc")
        custom = proj.installed.sh(
            "cd svc && uv run python -m mytool",
            label=":python: custom",
        )
        cmds = _cmds([custom])
        assert any("mytool" in c for c in cmds)


# ── TestUvVersionValidation ─────────────────────────────────────


class TestUvVersionValidation:
    def test_pinned_version(self):
        proj = hm.py.uv(path=".", version="0.4.18")
        install = _step_by_substring([proj.test()], "astral.sh/uv/install.sh")
        assert "UV_VERSION=0.4.18" in install["cmd"]

    def test_invalid_version_rejected(self):
        with pytest.raises(ValueError, match="invalid version"):
            hm.py.uv(version="not a valid; version")


# ── TestUvBareForm ───────────────────────────────────────────────


class TestUvBareForm:
    def test_bare_test(self):
        cmds = _cmds([hm.py.uv.test()])
        assert any("cd . && uv run pytest" in c for c in cmds)

    def test_bare_lint(self):
        cmds = _cmds([hm.py.uv.lint()])
        assert any("cd . && uv run ruff check" in c for c in cmds)

    def test_bare_fmt(self):
        cmds = _cmds([hm.py.uv.fmt()])
        assert any("cd . && uv run ruff format --check" in c for c in cmds)

    def test_bare_typecheck(self):
        cmds = _cmds([hm.py.uv.typecheck()])
        assert any("cd . && uv run ty check" in c for c in cmds)

    def test_bare_run(self):
        cmds = _cmds([hm.py.uv.run("serve")])
        assert any("cd . && uv run serve" in c for c in cmds)

    def test_bare_build(self):
        cmds = _cmds([hm.py.uv.build()])
        assert any("cd . && uv build" in c for c in cmds)
