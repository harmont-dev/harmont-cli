"""Unit tests for harmont.cache policy types."""

from __future__ import annotations

import dataclasses
from datetime import timedelta

import pytest

from harmont.cache import (
    CacheCompose,
    CacheForever,
    CacheNone,
    CacheOnChange,
    CachePolicy,
    CacheTTL,
)


def test_cache_none_is_a_cache_policy():
    p = CacheNone()
    assert isinstance(p, CachePolicy)


def test_cache_none_is_frozen():
    p = CacheNone()
    with pytest.raises(dataclasses.FrozenInstanceError):
        p.foo = "bar"  # type: ignore[attr-defined]


def test_cache_forever_default_env_keys_is_empty_tuple():
    p = CacheForever()
    assert p.env_keys == ()


def test_cache_forever_accepts_env_keys():
    p = CacheForever(env_keys=("ARCH", "VARIANT"))
    assert p.env_keys == ("ARCH", "VARIANT")


def test_cache_ttl_requires_duration():
    p = CacheTTL(duration=timedelta(days=1))
    assert p.duration == timedelta(days=1)
    assert p.env_keys == ()


def test_cache_on_change_requires_paths():
    p = CacheOnChange(paths=("api/cabal.project",))
    assert p.paths == ("api/cabal.project",)


def test_cache_on_change_has_no_env_keys_field():
    """Per design spec — CacheOnChange's key already covers env-driven invalidation \
by hashing files."""
    with pytest.raises(TypeError):
        CacheOnChange(paths=("a",), env_keys=("X",))  # type: ignore[call-arg]


def test_cache_compose_takes_tuple_of_policies():
    p = CacheCompose(
        policies=(
            CacheTTL(duration=timedelta(days=1)),
            CacheOnChange(paths=("a",)),
        )
    )
    assert len(p.policies) == 2


def test_cache_when_is_removed():
    import harmont

    assert not hasattr(harmont, "when")
    assert not hasattr(harmont, "CacheWhen")
