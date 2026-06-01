"""Shared dependency resolution for @hm.target and @hm.pipeline (HAR-28).

Strict-marker model:
- ``Target[T]``            — resolve by parameter name from the global
                             target registry; raise if not found.
- ``BaseImage["X"]``       — inject a scratch-rooted ``Step(image=X)``.
- plain param with default — bind the default value.
- anything else            — raise at decoration time via
                             ``validate_target_signature``.

Cycle detection uses a module-level "currently resolving" stack keyed
by function name; the dump_registry_json render clears it at the
start of every render along with the target memoization cache.
"""

from __future__ import annotations

import inspect
import typing
from typing import TYPE_CHECKING, Any

from ._step import Step
from ._typing import _TARGET_MARKER, _BaseImageMarker

if TYPE_CHECKING:
    from collections.abc import Callable


_TARGETS_BY_NAME: dict[str, Callable[[], Any]] = {}
_RESOLVING: list[str] = []


def register_named_target(name: str, fn: Callable[[], Any]) -> None:
    """Register a named target. Raises on duplicate name."""
    if name in _TARGETS_BY_NAME:
        msg = (
            f"hm: duplicate target name {name!r}\n"
            "  → each @hm.target must have a unique name; pass "
            'name="..." to disambiguate'
        )
        raise ValueError(msg)
    _TARGETS_BY_NAME[name] = fn


def clear_target_names() -> None:
    """Reset the name registry and cycle-detection stack. Used by tests
    and `clear_target_cache()` (the full reset used at test boundaries)."""
    _TARGETS_BY_NAME.clear()
    _RESOLVING.clear()


def _param_kind_error(param: inspect.Parameter) -> str | None:
    """Return a fix-directed error message if `param` has a forbidden kind."""
    kind = param.kind
    if kind == inspect.Parameter.VAR_POSITIONAL:
        return (
            "hm: target functions cannot take *args\n"
            "  → declare each dependency as an explicit named parameter"
        )
    if kind == inspect.Parameter.VAR_KEYWORD:
        return (
            "hm: target functions cannot take **kwargs\n"
            "  → declare each dependency as an explicit named parameter"
        )
    if kind == inspect.Parameter.POSITIONAL_ONLY:
        return (
            f"hm: target functions cannot have positional-only "
            f"parameters (got {param.name!r})\n"
            "  → remove the '/' marker; parameters must be name-resolvable"
        )
    return None


def _marker_for(annotation: Any) -> object | None:
    """Inspect an `Annotated[T, ...]` annotation and return the
    hm-specific marker (a `_TargetMarker` or `_BaseImageMarker`)
    if present, else None."""
    if typing.get_origin(annotation) is None:
        return None
    metadata = typing.get_args(annotation)[1:]
    for meta in metadata:
        if meta is _TARGET_MARKER:
            return _TARGET_MARKER  # type: ignore[no-any-return]
        if isinstance(meta, _BaseImageMarker):
            return meta
    return None


def _safe_get_type_hints(fn: Callable[..., Any]) -> dict[str, Any]:
    """`typing.get_type_hints(fn, include_extras=True)` but tolerant of
    forward references that fail to resolve — fall back to the raw
    `__annotations__` dict so markers still surface."""
    try:
        return typing.get_type_hints(fn, include_extras=True)
    except Exception:  # intentionally broad; fallback path
        return dict(getattr(fn, "__annotations__", {}))


def validate_target_signature(fn: Callable[..., Any]) -> None:
    """Decoration-time validation. Raise TypeError on any of:

      - `*args` / `**kwargs` / positional-only parameter.
      - Parameter with no marker and no default value.

    A parameter with an `hm.Target[T]` or `hm.BaseImage["X"]` marker
    in its annotation is always valid. A parameter with neither
    marker but a default value is allowed (the default is used).
    """
    sig = inspect.signature(fn)
    hints = _safe_get_type_hints(fn)
    for param in sig.parameters.values():
        kind_err = _param_kind_error(param)
        if kind_err is not None:
            raise TypeError(kind_err)
        annotation = hints.get(param.name)
        if _marker_for(annotation) is not None:
            continue
        if param.default is not inspect.Parameter.empty:
            continue
        msg = (
            f"hm: parameter {param.name!r} has no marker and no default\n"
            "  → annotate with Target[T] (target dep) or "
            'BaseImage["..."] (scratch image), or give it a default'
        )
        raise TypeError(msg)


def resolve_deps(fn: Callable[..., Any]) -> dict[str, Any]:
    """Walk ``fn``'s signature and produce the kwargs to invoke it.

    Marker dispatch per parameter:
      - `Target[T]`        → look up param name in `_TARGETS_BY_NAME`;
                             raise if not found.
      - `BaseImage["X"]`   → inject `Step(image="X")` (a scratch root).
      - no marker, default → bind the default value.
      - no marker, no default → raise (caught earlier by
        `validate_target_signature` for well-formed targets).
    """
    sig = inspect.signature(fn)
    hints = _safe_get_type_hints(fn)
    kwargs: dict[str, Any] = {}
    for param in sig.parameters.values():
        kind_err = _param_kind_error(param)
        if kind_err is not None:
            raise TypeError(kind_err)
        annotation = hints.get(param.name)
        marker = _marker_for(annotation)
        if marker is _TARGET_MARKER:
            if param.name not in _TARGETS_BY_NAME:
                msg = (
                    f"hm: target {param.name!r} not found\n"
                    "  → declare it with @hm.target() or rename the "
                    "parameter to match an existing target"
                )
                raise TypeError(msg)
            kwargs[param.name] = _TARGETS_BY_NAME[param.name]()
            continue
        if isinstance(marker, _BaseImageMarker):
            kwargs[param.name] = Step(image=marker.image)
            continue
        if param.default is not inspect.Parameter.empty:
            kwargs[param.name] = param.default
            continue
        msg = (
            f"hm: parameter {param.name!r} has no marker and no default\n"
            '  → annotate with Target[T] or BaseImage["..."], or '
            "give it a default"
        )
        raise TypeError(msg)
    return kwargs


def call_with_deps(fn: Callable[..., Any]) -> Any:
    """Resolve ``fn``'s parameters and call it. Detects cycles."""
    name = fn.__name__  # ty: ignore[unresolved-attribute]
    if name in _RESOLVING:
        cycle = " → ".join([*_RESOLVING, name])
        msg = (
            f"hm: dependency cycle detected\n"
            f"  → {cycle}\n"
            "  fix: break the cycle, or extract a shared root target"
        )
        raise RuntimeError(msg)
    _RESOLVING.append(name)
    try:
        return fn(**resolve_deps(fn))
    finally:
        _RESOLVING.pop()
