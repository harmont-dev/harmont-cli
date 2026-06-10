"""Environment access shared by static and dynamic pipeline evaluation."""

from __future__ import annotations

import os


def env(name: str, default: str | None = None) -> str | None:
    """Return an environment variable, or ``default`` when it is unset."""
    return os.environ.get(name, default)
