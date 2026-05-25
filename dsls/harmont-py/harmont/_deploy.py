"""Driver-agnostic deployment registry, decorator, and Dep marker.

This module is intentionally driver-free. Concrete deployment types
(``LocalDeployment``, future ``AwsDeployment``, …) live in their own
driver subpackages (``harmont.dev``, future ``harmont.aws``).
The registry stores deployments polymorphically; CLI subcommands filter
by ``isinstance`` or by the ``driver`` discriminator.
"""

from __future__ import annotations

import dataclasses
import re
from dataclasses import dataclass
from functools import wraps
from typing import TYPE_CHECKING, Any

from ._deps import call_with_deps, validate_target_signature

if TYPE_CHECKING:
    from collections.abc import Callable


@dataclass(frozen=True)
class Deployment:
    """Abstract deployment record. Subclassed per driver.

    ``name`` is the slug the user passed to ``@hm.deploy``.
    ``driver`` is the discriminator string ("local" for ``hm.dev``).
    """

    name: str
    driver: str


# Registry: slug -> zero-arg callable that re-invokes the user-defined
# function with deps resolved. Same shape as REGISTRATIONS for pipelines.
DEPLOYMENTS: dict[str, Callable[[], Deployment]] = {}


_SLUG_RE = re.compile(r"^[a-z][a-z0-9-]{0,30}$")


def _validate_slug(slug: str) -> None:
    """Raise ValueError if slug does not satisfy Docker container-name rules."""
    if not _SLUG_RE.match(slug):
        msg = (
            f"hm: invalid deployment slug {slug!r}\n"
            "  → use lowercase letters, digits, and '-', "
            "start with a letter, max 31 chars (Docker container name rules)"
        )
        raise ValueError(msg)


def deploy(
    slug: str | None = None,
    *,
    name: str | None = None,
) -> Callable[[Callable[..., Any]], Callable[[], Deployment]]:
    """Register a function as a deployment.

    The wrapped function returns a :class:`Deployment` (typically the
    output of :func:`harmont.dev.deploy` or any future driver's factory).
    Parameters are resolved via the shared marker machinery: ``Target[T]``,
    ``BaseImage[...]``, and ``Dep[T]`` (deployment-to-deployment refs).

    Usage::

        @hm.deploy("db")
        def db():
            return hm.dev.deploy(image="postgres:16", port_mapping={5432: hm.dev.port()})

        @hm.deploy("api")
        def api(db: hm.Dep[hm.Deployment]):
            return hm.dev.deploy(
                image="myapp:latest",
                port_mapping={8000: hm.dev.port()},
                env={"DB_HOST": db.name},
            )

    Args:
        slug: Registry key. Must match ``^[a-z][a-z0-9-]{0,30}$``
              (Docker container-name rules). Defaults to ``fn.__name__``.
        name: Reserved for future use as a human-readable display name.
              Has no effect in v1; the slug is the public identity.

    Raises:
        ValueError: On invalid or duplicate slug.
        TypeError: On unmarkered parameters without defaults (raised by
                   the shared :func:`validate_target_signature`), or if
                   the wrapped function returns a non-Deployment value.
    """
    del name  # reserved-for-future-use; explicitly drop the unused binding

    def decorator(fn: Callable[..., Any]) -> Callable[[], Deployment]:
        validate_target_signature(fn)
        resolved_slug = slug if slug is not None else fn.__name__
        _validate_slug(resolved_slug)
        if resolved_slug in DEPLOYMENTS:
            msg = (
                f"hm: duplicate deployment slug {resolved_slug!r}\n"
                "  → each @hm.deploy must have a unique slug; "
                "pass an explicit slug= to disambiguate"
            )
            raise ValueError(msg)

        @wraps(fn)
        def wrapper() -> Deployment:
            value = call_with_deps(fn)
            if not isinstance(value, Deployment):
                msg = (
                    f"hm.deploy({resolved_slug!r}) must return a Deployment, "
                    f"got {type(value).__name__}\n"
                    "  → return the output of hm.dev.deploy(...) or another "
                    "driver's factory"
                )
                raise TypeError(msg)
            # Stamp the resolved slug into the returned dataclass so callers
            # see name=<slug> regardless of what the factory left in `name`.
            return dataclasses.replace(value, name=resolved_slug)

        DEPLOYMENTS[resolved_slug] = wrapper
        return wrapper

    return decorator


def dep_graph() -> dict[str, tuple[str, ...]]:
    """Return slug -> tuple of upstream slugs, in parameter order.

    Walks DEPLOYMENTS; for each registered slug, introspects the wrapped
    function's signature for ``Dep[T]`` parameters. Plain defaults and
    Target/BaseImage markers do not produce edges in the deploy graph.
    """
    import inspect
    import typing as _typing

    from ._typing import _DepMarker

    out: dict[str, tuple[str, ...]] = {}
    for slug, wrapper in DEPLOYMENTS.items():
        fn = wrapper.__wrapped__  # type: ignore[attr-defined]
        sig = inspect.signature(fn)
        hints = _typing.get_type_hints(fn, include_extras=True)
        deps: list[str] = []
        for name in sig.parameters:
            ann = hints.get(name)
            if ann is None:
                continue
            if _typing.get_origin(ann) is None:
                continue
            metadata = _typing.get_args(ann)[1:]
            if any(isinstance(m, _DepMarker) for m in metadata):
                deps.append(name)
        out[slug] = tuple(deps)
    return out


def topo_order() -> list[str]:
    """Topological ordering of DEPLOYMENTS by dep_graph; deps first.

    Raises RuntimeError on cycles. Stable under insertion order for
    independent slugs (preserves decoration order within a level).
    """
    g = dep_graph()
    # Kahn's algorithm w/ stable level ordering (insertion-order of g).
    indeg: dict[str, int] = {}
    for slug, upstreams in g.items():
        indeg[slug] = sum(1 for u in upstreams if u in g)
    order: list[str] = []
    while True:
        progressed = False
        for slug in list(g.keys()):
            if slug in order:
                continue
            if indeg[slug] == 0:
                order.append(slug)
                for downstream, upstreams in g.items():
                    if slug in upstreams and downstream not in order:
                        indeg[downstream] -= 1
                progressed = True
        if not progressed:
            break
    if len(order) != len(g):
        unresolved = [s for s in g if s not in order]
        msg = (
            f"hm: dep cycle among deployments: {', '.join(unresolved)}\n"
            "  → break the cycle, or factor shared state into a target"
        )
        raise RuntimeError(msg)
    return order
