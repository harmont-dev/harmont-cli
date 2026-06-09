"""harmont — chain-style Python DSL for Harmont CI pipelines.

The whole public surface:

    scratch()                -> Step (root)
    sh(cmd, **kw)            -> Step  (== scratch().sh(cmd, **kw))
    Step.sh(cmd, **kw)       -> Step
    Step.fork(label=None)    -> Step
    wait(*, continue_on_failure=False) -> Step

    pipeline(leaves, *, env=None, default_image=None) -> dict (v0 IR)
    pipeline_to_json(p, **kw) -> str

    @pipeline(slug, ..., triggers=[...], allow_manual=True)  -> decorator
    push(branch=..., tag=...)         -> PushTrigger
    pull_request(branches=..., types=...) -> PullRequestTrigger
    dump_registry_json()              -> str  (HAR-9 envelope)

Cache helpers: `ttl`, `on_change`, `forever`, `compose`.

``hm.pipeline`` is polymorphic. When called with positional ``Step``
arguments it builds a v0 IR dict (the factory). When called with no
positionals or a string slug it returns a decorator that registers a
function as a CI pipeline (HAR-9).
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from . import _decorator, py
from ._cmake import CMakeProject, CMakeToolchain, cmake
from ._elixir import elixir
from ._envelope import dump_registry_json
from ._go import go
from ._js import JsProject, js, ts
from ._pipeline import pipeline as _pipeline_factory
from ._pipeline import pipeline_to_json
from ._python import python
from ._ruby import ruby
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

        pipeline([step1, step2, ...], env=None, default_image=None) -> dict

    Decorator form — no positionals or a string slug:

        @pipeline(slug=None, *, name=None, triggers=(), allow_manual=True,
                  env=None, default_image=None)
        def my_pipeline() -> Step: ...

    The discriminant is the type of the first positional argument:
    a list or tuple routes to the factory path; anything else
    (including no positionals) routes to the decorator path.

    Returns:
        A v0 IR ``dict`` in factory form, or a decorator in decorator form.
    """
    if args and isinstance(args[0], (list, tuple)):
        return _pipeline_factory(args[0], **kwargs)
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
    """Start a chain with a single shell command.

    Shorthand for ``scratch().sh(cmd, ...)``. All keyword arguments are
    forwarded to ``Step.sh``.

    Args:
        cmd: Shell command to run.
        cwd: Directory to run in, relative to the workspace root. Omit to
            run in the root.
        label: Human-facing label shown in the UI. Defaults to the command.
        cache: Cache policy controlling result reuse across builds.
        env: Per-step environment variables merged on top of pipeline-level env.
        timeout_seconds: Hard wall-clock timeout in seconds.
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
        timeout_seconds=timeout_seconds,
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
    "ruby",
    "rust",
    "scratch",
    "sh",
    "target",
    "ts",
    "ttl",
    "wait",
    "zig",
]
