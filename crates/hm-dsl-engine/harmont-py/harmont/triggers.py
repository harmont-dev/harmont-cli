"""Trigger DSL constructors and types.

Three triggers in v1: push, pull_request, schedule. Each constructor
returns a frozen dataclass with a ``to_dict()`` method that produces the
wire-format JSON object documented in
docs/superpowers/specs/2026-05-10-har-9-imperfect-dsl-design.md.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


def _normalise_globs(value: str | list[str] | tuple[str, ...] | None) -> tuple[str, ...] | None:
    if value is None:
        return None
    if isinstance(value, str):
        return (value,)
    return tuple(value)


@dataclass(frozen=True)
class Trigger:
    """Base class. Concrete subclasses override ``to_dict``."""

    def to_dict(self) -> dict[str, Any]:
        raise NotImplementedError


@dataclass(frozen=True)
class PushTrigger(Trigger):
    branches: tuple[str, ...] | None
    tags: tuple[str, ...] | None

    def to_dict(self) -> dict[str, Any]:
        out: dict[str, Any] = {"event": "push"}
        if self.branches is not None:
            out["branches"] = list(self.branches)
        if self.tags is not None:
            out["tags"] = list(self.tags)
        return out


def push(
    branch: str | list[str] | tuple[str, ...] | None = None,
    tag: str | list[str] | tuple[str, ...] | None = None,
) -> PushTrigger:
    """Trigger on a git push.

    Pass exactly one of ``branch`` or ``tag``. Each is a glob or list
    of globs (``*`` matches any chars including ``/``; ``?`` matches one
    char).
    """
    if (branch is None) == (tag is None):
        msg = (
            "hm.push: pass exactly one of branch or tag\n"
            '  → e.g. hm.push(branch="main") or hm.push(tag="v*")'
        )
        raise ValueError(msg)
    return PushTrigger(
        branches=_normalise_globs(branch),
        tags=_normalise_globs(tag),
    )


_PR_TYPES = frozenset({"opened", "synchronize", "reopened", "closed", "ready_for_review"})
_DEFAULT_PR_TYPES = ("opened", "synchronize", "reopened")


@dataclass(frozen=True)
class PullRequestTrigger(Trigger):
    branches: tuple[str, ...] | None
    types: tuple[str, ...]

    def to_dict(self) -> dict[str, Any]:
        out: dict[str, Any] = {"event": "pull_request"}
        if self.branches is not None:
            out["branches"] = list(self.branches)
        out["types"] = list(self.types)
        return out


def pull_request(
    branches: str | list[str] | tuple[str, ...] | None = None,
    types: list[str] | tuple[str, ...] | None = None,
) -> PullRequestTrigger:
    """Trigger on a GitHub pull_request event.

    ``branches`` filters by the PR's *target* branch. ``types`` selects
    PR-action keywords; defaults to opened/synchronize/reopened (mirrors
    GHA).
    """
    resolved_types = tuple(types) if types is not None else _DEFAULT_PR_TYPES
    if not resolved_types:
        msg = "hm.pull_request: types must be non-empty"
        raise ValueError(msg)
    bad = [t for t in resolved_types if t not in _PR_TYPES]
    if bad:
        valid = ", ".join(sorted(_PR_TYPES))
        msg = f"unknown pull_request type {bad[0]!r}\n  → valid: {valid}"
        raise ValueError(msg)
    return PullRequestTrigger(
        branches=_normalise_globs(branches),
        types=resolved_types,
    )


@dataclass(frozen=True)
class ScheduleTrigger(Trigger):
    cron: str

    def to_dict(self) -> dict[str, Any]:
        return {"event": "schedule", "cron": self.cron}


def schedule(cron: str) -> ScheduleTrigger:
    """Trigger on a UTC cron schedule.

    ``cron`` is a five-field crontab expression (minute hour day month
    dow). Always interpreted as UTC.
    """
    try:
        from croniter import croniter as _cron_cls

        is_valid = _cron_cls.is_valid(cron)
    except ModuleNotFoundError:
        is_valid = True

    if not is_valid:
        msg = (
            f"hm.schedule: invalid cron expression {cron!r}\n"
            f"  → five-field crontab, UTC, e.g. '0 4 * * *'"
        )
        raise ValueError(msg)
    return ScheduleTrigger(cron=cron)
