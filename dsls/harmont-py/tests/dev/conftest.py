"""Per-test reset of every registry the deploy DSL touches."""

from __future__ import annotations

import pytest

from harmont._deploy import DEPLOYMENTS
from harmont._registry import clear_registry
from harmont._target import clear_target_cache


@pytest.fixture(autouse=True)
def _reset_registries():
    """Clear every module-level registry before each test so order is irrelevant."""
    DEPLOYMENTS.clear()
    clear_registry()
    clear_target_cache()
    yield
    DEPLOYMENTS.clear()
    clear_registry()
    clear_target_cache()
