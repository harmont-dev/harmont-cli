"""@hm.pipeline decorator — see docs/superpowers/specs/2026-05-10-har-9-imperfect-dsl-design.md."""
from __future__ import annotations

import re
from functools import wraps
from typing import TYPE_CHECKING, Any

from ._deps import call_with_deps, validate_target_signature
from ._registry import PipelineRegistration, register

if TYPE_CHECKING:
    from collections.abc import Callable

    from .triggers import Trigger

_SLUG_RE = re.compile(r"^[a-z][a-z0-9-]{0,63}$")


def _validate_slug(slug: str) -> None:
    if not _SLUG_RE.match(slug):
        msg = (
            f"invalid pipeline slug {slug!r}\n"
            f"  → use lowercase letters, digits, and '-', "
            f"start with a letter, max 64 chars"
        )
        raise ValueError(msg)


def pipeline(
    slug: str | None = None,
    *,
    name: str | None = None,
    triggers: tuple[Trigger, ...] | list[Trigger] = (),
    allow_manual: bool = True,
    env: dict[str, str] | None = None,
    default_image: str | None = None,
) -> Callable[[Callable[..., Any]], Callable[[], Any]]:
    """Register a function as a CI pipeline.

    The wrapped function returns a :class:`Step`, a tuple of leaves
    (:data:`Pipeline`), or any toolchain wrapper that
    :func:`harmont._unwrap.as_leaves` can coerce. The function may
    declare dependencies as parameters (pytest-style); each parameter
    name is resolved against the global target registry.
    """
    def decorator(fn: Callable[..., Any]) -> Callable[[], Any]:
        validate_target_signature(fn)
        resolved = slug if slug is not None else fn.__name__
        _validate_slug(resolved)

        @wraps(fn)
        def wrapper() -> Any:
            return call_with_deps(fn)

        register(
            PipelineRegistration(
                slug=resolved,
                name=name if name is not None else resolved,
                triggers=tuple(triggers),
                allow_manual=allow_manual,
                env=env,
                default_image=default_image,
                fn=wrapper,
            )
        )
        return wrapper

    return decorator
