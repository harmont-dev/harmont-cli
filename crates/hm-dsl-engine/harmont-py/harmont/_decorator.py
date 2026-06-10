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
    timeout: str | int | None = None,
) -> Callable[[Callable[..., Any]], Callable[[], Any]]:
    """Register a function as a CI pipeline (decorator form).

    The wrapped function returns a ``Step``, a tuple of leaves
    (``Pipeline``), or any toolchain wrapper that ``as_leaves()`` can
    coerce. The function may declare dependencies as parameters
    (pytest-fixture style); each parameter name is resolved against the
    global target registry.

    Args:
        slug: Pipeline identifier used as the registry key and in the API.
            Must match ``[a-z][a-z0-9-]{0,63}``. Defaults to the decorated
            function's name.
        name: Human-readable pipeline name shown in the UI. Defaults to
            ``slug``.
        triggers: Trigger objects (``PushTrigger``, ``PullRequestTrigger``)
            that activate this pipeline automatically.
        allow_manual: When ``True``, the pipeline can be triggered manually
            via the UI or API in addition to its configured triggers.
        env: Pipeline-level environment variables applied to every step.
        default_image: Local-mode Docker base image applied to root steps
            that lack an explicit ``image`` or ``builds_in`` parent.
        timeout: Whole-build wall-clock budget ("30m", "1h", or int
            seconds). The build is killed and fails as timed out once it
            elapses.

    Returns:
        A decorator that registers the wrapped function and returns it
        unchanged (same call signature).

    Raises:
        ValueError: If ``slug`` does not match the allowed pattern.
    """

    def decorator(fn: Callable[..., Any]) -> Callable[[], Any]:
        validate_target_signature(fn)
        resolved = slug if slug is not None else fn.__name__  # ty: ignore[unresolved-attribute]
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
                timeout=timeout,
            )
        )
        return wrapper

    return decorator
