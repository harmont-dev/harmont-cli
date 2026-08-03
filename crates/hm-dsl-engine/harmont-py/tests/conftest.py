"""Shared pytest fixtures for cidsl/py tests.

The :func:`_chdir_to_repo_root` autouse fixture anchors every test's
working directory at the repo root so that toolchain abstractions
which glob the filesystem at construction time resolve real files.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import pytest

_REPO_ROOT = Path(__file__).resolve().parents[3]


@pytest.fixture(autouse=True)
def _chdir_to_repo_root(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(_REPO_ROOT)


def cmd_from_step(step: dict[str, Any]) -> str | None:
    """Extract the command string from a step dict (v0 IR format)."""
    action = step.get("action")
    if isinstance(action, dict):
        return action.get("cmd")
    return None


def cmds_from_graph(pipeline: dict[str, Any]) -> list[str | None]:
    """Extract all command strings from a pipeline graph's nodes."""
    return [cmd_from_step(n["step"]) for n in pipeline["graph"]["nodes"]]
