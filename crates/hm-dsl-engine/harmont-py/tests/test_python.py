"""Python (uv) toolchain abstraction tests."""

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


def test_python_object_form_full_chain():
    py = hm.python(path="svc")
    cmds = _cmds([py.test()])
    assert any("apt-get install" in c for c in cmds)
    assert any("astral.sh/uv/install.sh" in c for c in cmds)
    assert any("cd svc && uv sync" in c for c in cmds)
    assert any("cd svc && uv run pytest" in c for c in cmds)


def test_python_actions_share_install_step():
    py = hm.python(path="svc")
    cmds = _cmds([py.test(), py.lint(), py.fmt(), py.typecheck()])
    assert len([c for c in cmds if "astral.sh/uv/install.sh" in c]) == 1
    assert len([c for c in cmds if "apt-get install" in c]) == 1
    assert any("uv run pytest" in c for c in cmds)
    assert any("uv run ruff check" in c for c in cmds)
    assert any("uv run ruff format --check" in c for c in cmds)
    assert any("uv run ty check" in c for c in cmds)


def test_python_sync_cached_on_change_of_lockfile():
    py = hm.python(path="svc")
    sync = _step_by_substring([py.test()], "uv sync")
    assert sync["cache"]["policy"] == "on_change"
    assert "svc/uv.lock" in sync["cache"]["paths"]
    assert "svc/pyproject.toml" in sync["cache"]["paths"]


def test_python_install_cache_forever():
    py = hm.python(path=".")
    install = _step_by_substring([py.test()], "astral.sh/uv/install.sh")
    assert install["cache"]["policy"] == "forever"


def test_python_bare_form_test():
    cmds = _cmds([hm.python.test()])
    assert any("cd . && uv run pytest" in c for c in cmds)


def test_python_bare_form_all_actions():
    cmds = _cmds([hm.python.test(), hm.python.lint(), hm.python.fmt(), hm.python.typecheck()])
    assert any("pytest" in c for c in cmds)
    assert any("ruff check" in c for c in cmds)
    assert any("ruff format --check" in c for c in cmds)
    assert any("ty check" in c for c in cmds)


def test_python_action_labels_auto_generated():
    py = hm.python(path=".")
    assert py.test().label == ":python: test"
    assert py.lint().label == ":python: lint"
    assert py.fmt().label == ":python: fmt"
    assert py.typecheck().label == ":python: typecheck"


def test_python_typecheck_paths_string():
    py = hm.python(path="myapp")
    s = py.typecheck(paths="src")
    assert "uv run ty check src" in s.cmd


def test_python_typecheck_paths_list():
    py = hm.python(path="myapp")
    s = py.typecheck(paths=["src", "tests"])
    assert "uv run ty check src tests" in s.cmd


def test_python_typecheck_paths_default():
    py = hm.python(path="myapp")
    s = py.typecheck()
    assert "uv run ty check ." in s.cmd


def test_python_action_label_override():
    py = hm.python(path=".")
    assert py.test(label=":python: smoke").label == ":python: smoke"


def test_python_action_cache_forwarded():
    py = hm.python(path=".")
    s = py.test(cache=CacheOnChange(paths=("pyproject.toml",)))
    assert s.cache == CacheOnChange(paths=("pyproject.toml",))


def test_python_image_emitted_on_apt_step():
    py = hm.python(path=".", image="ubuntu:24.04")
    apt = _step_by_substring([py.test()], "apt-get install")
    assert apt.get("image") == "ubuntu:24.04"


def test_python_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    py = hm.python(path="svc", base=base)
    cmds = _cmds([py.test()])
    assert not any("apt-get install" in c for c in cmds)
    assert any("custom base" in c for c in cmds)
    assert any("astral.sh/uv/install.sh" in c for c in cmds)


def test_python_installed_escape_hatch_chains():
    py = hm.python(path="svc")
    custom = py.installed.sh(
        "cd svc && uv run python -m mytool",
        label=":python: custom",
    )
    cmds = _cmds([custom])
    assert any("mytool" in c for c in cmds)


def test_python_uv_version_in_install_cmd():
    py = hm.python(path=".", uv_version="0.4.18")
    install = _step_by_substring([py.test()], "astral.sh/uv/install.sh")
    assert "UV_VERSION=0.4.18" in install["cmd"]


def test_python_invalid_uv_version_rejected():
    with pytest.raises(ValueError, match="uv_version"):
        hm.python(uv_version="not a valid; version")
