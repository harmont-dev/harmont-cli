"""harmont — chain-style Python DSL for Harmont CI pipelines.

The whole public surface:

    scratch()                -> Step (root)
    sh(cmd, **kw)            -> Step  (== scratch().sh(cmd, **kw))
    Step.sh(cmd, **kw)       -> Step
    Step.fork(label=None)    -> Step
    wait(*, continue_on_failure=False) -> Step

    pipeline(leaves, *, env=None) -> dict (v0 IR)
    pipeline_to_json(p, **kw) -> str

    @pipeline(slug, ..., triggers=[...], allow_manual=True)  -> decorator
    push(branch=..., tag=...)         -> PushTrigger
    pull_request(branches=..., types=...) -> PullRequestTrigger
    dump_registry_json()              -> str  (HAR-9 envelope)

Cache helpers: `ttl`, `on_change`, `forever`, `compose`.

``hm.pipeline`` is polymorphic. When called with a list of ``Step``
objects it builds a v0 IR dict (the factory). When called with no
positionals or a string slug it returns a decorator that registers a
function as a CI pipeline (HAR-9).
"""

from __future__ import annotations

from dataclasses import replace as _replace
from typing import TYPE_CHECKING, Any

from . import _decorator, py
from ._cmake import CMakeProject, CMakeToolchain, cmake
from ._duration import parse_duration as _parse_duration
from ._elixir import elixir
from ._envelope import dump_registry_json
from ._go import go
from ._js import JsProject, js, ts
from ._pipeline import pipeline as _pipeline_factory
from ._pipeline import pipeline_to_json
from ._python import python
from ._rust import RustProject, rust
from ._step import Step, scratch, wait
from ._target import clear_target_cache, target  # noqa: F401  clear_target_cache used by tests
from ._toolchain import apt_base
from ._typing import BaseImage, Target
from ._zig import zig
from .cache import (
    CacheCompose,
    CacheForever,
    CacheNone,
    CacheOnChange,
    CachePolicy,
    CacheTTL,
)
from .triggers import pull_request, push
from .triggers import pull_request as pr
from .types import Pipeline

if TYPE_CHECKING:
    from datetime import timedelta


def pipeline(*args: Any, **kwargs: Any) -> Any:
    """Build a v0 IR dict or register a pipeline function.

    This function is polymorphic based on the type of its positional arguments.

    Factory form — first positional is a list/tuple of ``Step``s:

        pipeline([step1, step2, ...], env=None) -> dict

    Decorator form — no positionals or a string slug:

        @pipeline(slug=None, *, name=None, triggers=(), allow_manual=True,
                  env=None)
        def my_pipeline() -> Step: ...

    The discriminant is the type of the first positional argument:
    a list or tuple routes to the factory path; anything else
    (including no positionals) routes to the decorator path.

    Returns:
        A v0 IR ``dict`` in factory form, or a decorator in decorator form.

    Raises:
        TypeError: When called with the legacy variadic ``Step`` form
            (``pipeline(step)`` / ``pipeline(a, b)``). The factory now takes
            a single list of leaves.
    """
    if args and isinstance(args[0], (list, tuple)):
        return _pipeline_factory(args[0], **kwargs)
    # Legacy form: leaves passed as positional Step args (pre-CLI-9
    # `pipeline(step)` / `pipeline(a, b)`). Without this guard the call would
    # fall through to the decorator and fail far downstream with a cryptic
    # AttributeError. Fail fast with the migration hint instead.
    if args and all(isinstance(a, Step) for a in args):
        msg = (
            "hm.pipeline() takes a single list of leaves, not variadic Step "
            "arguments\n"
            f"  observed: {len(args)} positional Step "
            f"argument{'s' if len(args) != 1 else ''}\n"
            "  → wrap the leaves in a list, e.g. "
            "hm.pipeline([step]) or hm.pipeline([a, b])"
        )
        raise TypeError(msg)
    return _decorator.pipeline(*args, **kwargs)


def ttl(duration: timedelta) -> CacheTTL:
    """Create a time-to-live cache policy.

    The step's snapshot is reused until ``duration`` has elapsed since the
    last successful run, floored to UTC midnight. Two builds within the same
    UTC day share a cache key; a build the following day rebuilds.

    Args:
        duration: How long the cached result remains valid.

    Returns:
        A ``CacheTTL`` policy for use in ``.sh(cache=...)``.

    Examples:
        >>> from datetime import timedelta
        >>> import harmont as hm
        >>> step = hm.sh("apt-get update", cache=hm.ttl(timedelta(days=1)))
    """
    return CacheTTL(duration=duration)


def on_change(*paths: str) -> CacheOnChange:
    """Create a content-addressed cache policy keyed on file hashes.

    The step's snapshot is reused until any file under ``paths`` changes.
    Paths are relative to the source-archive root and resolved at render
    time by the key generator.

    Args:
        *paths: One or more paths (relative to workspace root) to watch for
            changes.

    Returns:
        A ``CacheOnChange`` policy for use in ``.sh(cache=...)``.

    Examples:
        >>> import harmont as hm
        >>> step = hm.sh("pip install -r requirements.txt",
        ...               cache=hm.on_change("requirements.txt"))
    """
    return CacheOnChange(paths=tuple(paths))


def forever(env_keys: tuple[str, ...] = ()) -> CacheForever:
    """Create a permanent cache policy.

    The step's snapshot is reused indefinitely, keyed on (command, parent,
    env_keys). Suitable for deterministic installs where the command string
    itself encodes the version (e.g. downloading a pinned binary). Do not
    use for steps that fetch mutable remote resources.

    Args:
        env_keys: Environment variable names whose values are folded into
            the cache key. Use this when the command's behavior varies by
            environment (e.g. ``GOARCH``).

    Returns:
        A ``CacheForever`` policy for use in ``.sh(cache=...)``.

    Examples:
        >>> import harmont as hm
        >>> step = hm.sh("curl .../go1.23.tar.gz | tar ...", cache=hm.forever())
    """
    return CacheForever(env_keys=env_keys)


def compose(*policies: CachePolicy) -> CacheCompose:
    """Combine multiple cache policies: hit only when every sub-policy hits.

    Use to express compound invalidation conditions such as "rebuild daily
    OR when these files change".

    Args:
        *policies: Two or more ``CachePolicy`` instances to combine.

    Returns:
        A ``CacheCompose`` policy for use in ``.sh(cache=...)``.

    Examples:
        >>> from datetime import timedelta
        >>> import harmont as hm
        >>> policy = hm.compose(
        ...     hm.ttl(timedelta(days=1)),
        ...     hm.on_change("api/cabal.project"),
        ... )
    """
    return CacheCompose(policies=tuple(policies))


def timeout(duration: str | int | timedelta, step: Step) -> Step:
    """Apply a wall-clock timeout to a single step.

    The executor (and ``hm run`` locally) kills the step's process once
    ``duration`` elapses; the step then fails as *timed out*. Wrapping a
    step that already has a timeout replaces it.

    Args:
        duration: ``"30s"`` / ``"5m"`` / ``"1h30m"`` (units ``h``, ``m``,
            ``s``), an ``int`` number of seconds, or a ``timedelta``.
        step: The command step to bound. Must be a real command step,
            not a ``wait`` barrier.

    Returns:
        A new ``Step`` identical to ``step`` but with the timeout set.

    Raises:
        ValueError: If ``step`` is a ``wait`` barrier, or ``duration`` is
            malformed / non-positive.
        TypeError: If ``duration`` is not a str, int, or timedelta.

    Examples:
        >>> import harmont as hm
        >>> step = hm.timeout("30s", hm.sh("echo foobar"))
    """
    if step.is_wait:
        msg = (
            "hm: timeout() cannot wrap a wait() barrier\n"
            "  → apply timeout() to a command step, e.g. "
            'hm.timeout("30s", hm.sh("make test"))'
        )
        raise ValueError(msg)
    return _replace(step, timeout_seconds=_parse_duration(duration))


def sh(
    cmd: str,
    *,
    cwd: str | None = None,
    label: str | None = None,
    cache: CachePolicy | None = None,
    env: dict[str, str] | None = None,
    image: str | None = None,
    key: str | None = None,
) -> Step:
    """Start a chain with a single shell command.

    Shorthand for ``scratch().sh(cmd, ...)``. All keyword arguments are
    forwarded to ``Step.sh``.

    To set a timeout, wrap the result with ``hm.timeout(duration, step)``.

    Args:
        cmd: Shell command to run.
        cwd: Directory to run in, relative to the workspace root. Omit to
            run in the root.
        label: Human-facing label shown in the UI. Defaults to the command.
        cache: Cache policy controlling result reuse across builds.
        env: Per-step environment variables merged on top of pipeline-level env.
        image: Local-mode Docker base image override for this step.
        key: Manual key override for this step in the v0 IR.

    Returns:
        A new root ``Step`` with the command set.

    Examples:
        >>> import harmont as hm
        >>> step = hm.sh("cargo build")
    """
    return scratch().sh(
        cmd,
        cwd=cwd,
        label=label,
        cache=cache,
        env=env,
        image=image,
        key=key,
    )


def group(steps: list[Step] | tuple[Step, ...]) -> tuple[Step, ...]:
    """Collect a list of steps into a tuple for use as a target return value.

    ``pipeline()`` and ``@pipeline`` both accept a tuple of leaves.
    ``group()`` converts a list to that tuple for convenience.

    Args:
        steps: The leaf steps to collect.

    Returns:
        A tuple of the input steps.

    Examples:
        >>> import harmont as hm
        >>> proj = hm.rust.project()
        >>> leaves = hm.group([proj.test(), proj.clippy(), proj.fmt()])
    """
    return tuple(steps)


__all__ = [
    "BaseImage",
    "CMakeProject",
    "CMakeToolchain",
    "CacheCompose",
    "CacheForever",
    "CacheNone",
    "CacheOnChange",
    "CachePolicy",
    "CacheTTL",
    "JsProject",
    "Pipeline",
    "RustProject",
    "Step",
    "Target",
    "apt_base",
    "cmake",
    "compose",
    "dump_registry_json",
    "elixir",
    "forever",
    "go",
    "group",
    "js",
    "on_change",
    "pipeline",
    "pipeline_to_json",
    "pr",
    "pull_request",
    "push",
    "py",
    "python",
    "rust",
    "scratch",
    "sh",
    "target",
    "timeout",
    "ts",
    "ttl",
    "wait",
    "zig",
]
