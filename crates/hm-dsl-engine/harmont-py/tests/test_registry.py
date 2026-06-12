"""Module-level pipeline registry."""

import pytest

from harmont._registry import (
    REGISTRATIONS,
    PipelineRegistration,
    clear_registry,
    register,
)


@pytest.fixture(autouse=True)
def _reset_registry():
    clear_registry()
    yield
    clear_registry()


def test_empty_registry():
    assert REGISTRATIONS == []


def test_register_appends():
    reg = PipelineRegistration(
        slug="ci",
        name="CI",
        triggers=(),
        allow_manual=True,
        env=None,
        fn=lambda: None,
    )
    register(reg)
    assert [reg] == REGISTRATIONS


def test_register_duplicate_slug_raises():
    fn = lambda: None  # noqa: E731 — intentional inline stub per HAR-9 Task 1.2 plan
    register(
        PipelineRegistration(
            slug="ci",
            name="CI",
            triggers=(),
            allow_manual=True,
            env=None,
            fn=fn,
        )
    )
    with pytest.raises(ValueError, match="duplicate pipeline slug") as excinfo:
        register(
            PipelineRegistration(
                slug="ci",
                name="CI",
                triggers=(),
                allow_manual=True,
                env=None,
                fn=fn,
            )
        )
    assert "duplicate pipeline slug" in str(excinfo.value)
    assert "ci" in str(excinfo.value)


def test_clear_resets():
    fn = lambda: None  # noqa: E731 — intentional inline stub per HAR-9 Task 1.2 plan
    register(
        PipelineRegistration(
            slug="ci",
            name="CI",
            triggers=(),
            allow_manual=True,
            env=None,
            fn=fn,
        )
    )
    clear_registry()
    assert REGISTRATIONS == []
