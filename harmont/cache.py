"""Cache policies for layered VM snapshots.

See docs/design/snapshots/2026-05-01-python-surface.md for the surface
and docs/design/snapshots/2026-05-01-data-model.md for the key formulas.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from datetime import timedelta


@dataclass(frozen=True)
class CachePolicy:
    """Base — never instantiate directly. Use the helpers below."""


@dataclass(frozen=True)
class CacheNone(CachePolicy):
    """Always run the step; never cache its snapshot.

    Equivalent to today's behavior. Default for command steps.
    """


@dataclass(frozen=True)
class CacheForever(CachePolicy):
    """Cache forever, keyed only on (command, parent, env_keys).

    Use for pure computations whose only inputs are visible to the planner.
    DO NOT use for installs that fetch the public internet — package repos
    drift; manual cache busts will be needed.
    """

    env_keys: tuple[str, ...] = ()


@dataclass(frozen=True)
class CacheTTL(CachePolicy):
    """Cache for `duration`; refresh once per window (UTC-midnight floored).

    Two builds within the same UTC day share a key; a build at 00:30 UTC
    the next day rebuilds.
    """

    duration: timedelta
    env_keys: tuple[str, ...] = ()


@dataclass(frozen=True)
class CacheOnChange(CachePolicy):
    """Rebuild whenever any file under `paths` changes.

    Paths are relative to the source-archive root. File hashes are
    computed at render time by `harmont.keygen` (paths are read from
    the source archive's checkout root).

    No `env_keys` field — file content already covers the invalidation
    surface.
    """

    paths: tuple[str, ...]


@dataclass(frozen=True)
class CacheCompose(CachePolicy):
    """Combine multiple policies. Cache hits ONLY when every sub-policy hits.

    Useful for "rebuild daily OR when these files change":

        CacheCompose(policies=(
            CacheTTL(duration=timedelta(days=1)),
            CacheOnChange(paths=("api/cabal.project",)),
        ))
    """

    policies: tuple[CachePolicy, ...]
