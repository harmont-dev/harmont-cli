"""harmont — chain-style Python DSL for Harmont CI pipelines.

The whole public surface:

    scratch()                -> Step (root)
    sh(cmd, **kw)            -> Step  (== scratch().sh(cmd, **kw))
    Step.sh(cmd, **kw)       -> Step
    Step.fork(label=None)    -> Step
    wait(*, continue_on_failure=False) -> Step

    pipeline(*leaves, env=None, default_image=None) -> dict (v0 IR)
    pipeline_to_json(p, **kw) -> str

    @pipeline(slug, ..., triggers=[...], allow_manual=True)  -> decorator
    push(branch=..., tag=...)         -> PushTrigger
    pull_request(branches=..., types=...) -> PullRequestTrigger
    schedule(cron=...)                 -> ScheduleTrigger
    dump_registry_json()              -> str  (HAR-9 envelope)

Cache helpers: ttl, on_change, forever, compose.

``hm.pipeline`` is polymorphic. When called with positional ``Step``
arguments it builds a v0 IR dict (the factory). When called with no
positionals or a string slug it returns a decorator that registers a
function as a CI pipeline (HAR-9).
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from . import _decorator, dev
from ._deploy import Deployment, deploy
from ._envelope import dump_registry_json
from ._step import Step, scratch, wait
from ._target import clear_target_cache, target  # noqa: F401  clear_target_cache used by tests
from ._typing import BaseImage, Dep, Target
from .cache import (
    CacheCompose,
    CacheForever,
    CacheNone,
    CacheOnChange,
    CachePolicy,
    CacheTTL,
)
from .cmake import cmake
from .composer import composer
from .dotnet import dotnet
from .elm import elm
from .go import go
from .gradle import gradle
from .haskell import haskell
from .npm import npm
from .ocaml import ocaml
from .perl import perl
from .pipeline import pipeline as _pipeline_factory
from .pipeline import pipeline_to_json
from .python import python
from .ruby import ruby
from .rust import rust
from .triggers import pull_request, push, schedule
from .types import Pipeline
from .zig import zig

if TYPE_CHECKING:
    from datetime import timedelta


def pipeline(*args: Any, **kwargs: Any) -> Any:
    """Polymorphic entry point.

    - ``pipeline(*leaves, env=..., default_image=...)`` — every
      positional arg is a :class:`Step`; returns the v0 IR dict (the
      factory).
    - ``pipeline(slug=None, *, name=..., triggers=..., allow_manual=...,
      env=..., default_image=...)`` — no positionals or a string slug;
      returns a decorator that registers the wrapped function in the
      module-level :data:`~harmont._registry.REGISTRATIONS` table
      (HAR-9).

    The discriminant is the *type* of the positional arguments: any
    non-Step positional (including a string slug, or no positional at
    all) routes to the decorator path.
    """
    if args and all(isinstance(a, Step) for a in args):
        return _pipeline_factory(*args, **kwargs)
    return _decorator.pipeline(*args, **kwargs)


def ttl(duration: timedelta) -> CacheTTL:
    return CacheTTL(duration=duration)


def on_change(*paths: str) -> CacheOnChange:
    return CacheOnChange(paths=tuple(paths))


def forever(env_keys: tuple[str, ...] = ()) -> CacheForever:
    return CacheForever(env_keys=env_keys)


def compose(*policies: CachePolicy) -> CacheCompose:
    return CacheCompose(policies=tuple(policies))


def sh(
    cmd: str,
    *,
    cwd: str | None = None,
    label: str | None = None,
    cache: CachePolicy | None = None,
    env: dict[str, str] | None = None,
    timeout_seconds: int | None = None,
    image: str | None = None,
    key: str | None = None,
) -> Step:
    """Shorthand for ``scratch().sh(cmd, ...)`` — start a chain in one call."""
    return scratch().sh(
        cmd,
        cwd=cwd,
        label=label,
        cache=cache,
        env=env,
        timeout_seconds=timeout_seconds,
        image=image,
        key=key,
    )


__all__ = [
    "BaseImage",
    "CacheCompose",
    "CacheForever",
    "CacheNone",
    "CacheOnChange",
    "CachePolicy",
    "CacheTTL",
    "Dep",
    "Deployment",
    "Pipeline",
    "Step",
    "Target",
    "cmake",
    "compose",
    "composer",
    "deploy",
    "dev",
    "dotnet",
    "dump_registry_json",
    "elm",
    "forever",
    "go",
    "gradle",
    "haskell",
    "npm",
    "ocaml",
    "on_change",
    "perl",
    "pipeline",
    "pipeline_to_json",
    "pull_request",
    "push",
    "python",
    "ruby",
    "rust",
    "schedule",
    "scratch",
    "sh",
    "target",
    "ttl",
    "wait",
    "zig",
]
