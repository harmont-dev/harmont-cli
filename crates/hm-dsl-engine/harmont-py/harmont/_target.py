"""@hm.target — memoized, composable building blocks (HAR-28).

A target is a function that returns a ``Step`` (or a toolchain wrapper
that unwraps to one). The decorator:

  1. Registers the wrapped function by name in the global registry
     (``harmont._deps._TARGETS_BY_NAME``), so other targets can
     declare it as a parameter.
  2. Memoizes the return value per envelope render so targets calling
     other targets dedup correctly.
  3. Resolves any parameters declared by the wrapped function via
     ``call_with_deps`` (cycle-aware).

Pytest-style fixture form:

    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    @hm.target()
    def venv(apt_base) -> hm.Step:
        return apt_base.sh("python3 -m venv .venv")

Explicit-call form is still supported:

    @hm.target()
    def venv() -> hm.Step:
        return apt_base().sh("python3 -m venv .venv")

The cache lives in a module-level dict keyed by the wrapped function
object. ``dump_registry_json()`` clears it before each render; tests
clear it via the fixture pattern documented in ``cidsl/py/CLAUDE.md``.
"""

from __future__ import annotations

from functools import wraps
from typing import TYPE_CHECKING, Any

from ._deps import (
    call_with_deps,
    clear_target_names,
    register_named_target,
    validate_target_signature,
)

if TYPE_CHECKING:
    from collections.abc import Callable


_TARGET_CACHE: dict[Callable[..., Any], Any] = {}
_DYNAMIC_TARGETS_BY_NAME: dict[str, Callable[..., Any]] = {}


def clear_target_memo() -> None:
    """Reset only the per-render memoization cache.

    Called at the start of every envelope render so two consecutive renders
    don't share cached ``Step`` values. The named-target registry is NOT
    touched — it is populated once at decoration time and must remain in
    place so pipeline fixture-style params can resolve their dependencies
    during the same render.
    """
    _TARGET_CACHE.clear()


def clear_target_cache() -> None:
    """Reset target memoization AND the named-target registry.

    Test-only helper: between tests we want a clean slate. During an
    envelope render the named registry stays put — only the memo cache
    is wiped via ``clear_target_memo()``.
    """
    _TARGET_CACHE.clear()
    _DYNAMIC_TARGETS_BY_NAME.clear()
    clear_target_names()


def evaluate_dynamic_target(name: str) -> Any:
    """Evaluate a registered dynamic target for runtime graph expansion."""
    try:
        fn = _DYNAMIC_TARGETS_BY_NAME[name]
    except KeyError as e:
        raise KeyError(f"hm: dynamic target {name!r} not found") from e

    from ._unwrap import as_leaves

    return as_leaves(call_with_deps(fn))


def render_dynamic_target_json(
    name: str,
    *,
    env: dict[str, str] | None = None,
) -> str:
    """Evaluate one dynamic target and serialize its concrete graph fragment."""
    from ._pipeline import pipeline, pipeline_to_json

    clear_target_memo()
    runtime_env = env if env is not None else {}
    leaves = evaluate_dynamic_target(name)
    fragment = pipeline(leaves, env=runtime_env)
    return pipeline_to_json(
        fragment,
        pipeline_slug=name,
        env=runtime_env,
    )


def target(
    *,
    name: str | None = None,
    dynamic: bool = False,
) -> Callable[[Callable[..., Any]], Callable[[], Any]]:
    """Mark a function as a reusable, memoized pipeline building block.

    The wrapped function may declare dependencies as parameters; each
    parameter name is resolved against the global target registry
    (pytest-fixture style). The return value is memoized per render so
    targets calling other targets dedup correctly.

    Args:
        name: Registry key for this target. Defaults to the decorated
            function's name. Override when the name collides with another
            target or a more human-readable key is preferred.
        dynamic: Defer this target's evaluation until pipeline execution.
            The initial bake emits a dynamic placeholder referencing its name.

    Returns:
        A decorator that registers and memoizes the wrapped function.

    Examples:
        >>> import harmont as hm
        >>> @hm.target()
        ... def apt_base() -> hm.Step:
        ...     return hm.sh("apt-get update")
    """

    def decorator(fn: Callable[..., Any]) -> Callable[[], Any]:
        validate_target_signature(fn)
        target_name = name if name is not None else fn.__name__  # ty: ignore[unresolved-attribute]

        @wraps(fn)
        def wrapper() -> Any:
            if dynamic:
                from ._step import Step

                return Step(
                    dynamic_target_name=target_name,
                    key_override=target_name,
                )
            if fn not in _TARGET_CACHE:
                _TARGET_CACHE[fn] = call_with_deps(fn)
            return _TARGET_CACHE[fn]

        register_named_target(target_name, wrapper)
        if dynamic:
            _DYNAMIC_TARGETS_BY_NAME[target_name] = fn
        return wrapper

    return decorator
