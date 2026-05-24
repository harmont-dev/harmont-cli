"""Shared pytest fixtures for cidsl/py tests.

The :func:`_chdir_to_repo_root` autouse fixture anchors every test's
working directory at the repo root so that toolchain abstractions
which glob the filesystem at construction time
(e.g. :func:`harmont.haskell.HaskellToolchain.package`) resolve real
files in ``api/``, ``freestyle/``, ``app/``, etc.
"""

from __future__ import annotations

from pathlib import Path

import pytest

_REPO_ROOT = Path(__file__).resolve().parents[3]


@pytest.fixture(autouse=True)
def _chdir_to_repo_root(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(_REPO_ROOT)
