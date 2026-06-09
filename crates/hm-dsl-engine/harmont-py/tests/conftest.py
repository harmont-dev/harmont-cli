"""Shared pytest fixtures for cidsl/py tests.

The :func:`_chdir_to_repo_root` autouse fixture anchors every test's
working directory at the repo root so that toolchain abstractions
which glob the filesystem at construction time resolve real files.
"""

from __future__ import annotations

from pathlib import Path

import pytest

_REPO_ROOT = Path(__file__).resolve().parents[3]


@pytest.fixture(autouse=True)
def _chdir_to_repo_root(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(_REPO_ROOT)
