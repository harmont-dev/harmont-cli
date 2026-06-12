"""Internal Step dataclass — the chain primitive.

Public callers go through `scratch`, `wait`, `Step.sh`, `Step.fork`
re-exported from ``harmont/__init__.py``. This module is private; nothing
outside ``harmont`` should import from it.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from .cache import CachePolicy


@dataclass(frozen=True)
class Step:
    """Immutable chain node — the primitive the DSL is built on.

    Steps are constructed via `scratch()` or `wait()` and extended by
    calling ``.sh()`` or ``.fork()`` on the result. Every mutating method
    returns a new ``Step``; the receiver is unchanged.
    """

    cmd: str | None = None
    parent: Step | None = None
    """In-tree pointer used by the lowering pass to walk back to the
    nearest emitted ancestor. Distinct from the wire-format
    ``builds_in`` field, which carries the resolved key string."""

    is_wait: bool = False
    continue_on_failure: bool = False
    label: str | None = None
    cache: CachePolicy | None = None
    env: dict[str, str] | None = None
    timeout_seconds: int | None = None
    image: str | None = None
    """Local-mode Docker base image override for this step. Ignored when
    the step has a ``builds_in`` parent (the parent's snapshot wins).
    When unset, root steps fall back to ``ubuntu:24.04``; child steps
    inherit the parent's snapshot."""

    runner: str | None = None
    """Step-executor plugin runner name. ``None`` = default (Docker)."""

    runner_args: dict[str, Any] | None = None
    """Plugin-specific runner arguments. Validated by the executor
    plugin's ``step_schema`` if it set one."""

    key_override: str | None = None
    """Manual key override; surfaces as the `key=` kwarg on `.sh()`.
    The field is renamed so it doesn't shadow the runtime-derived key
    the lowering pass produces in pipeline.py."""

    def sh(
        self,
        cmd: str,
        *,
        cwd: str | None = None,
        label: str | None = None,
        cache: CachePolicy | None = None,
        env: dict[str, str] | None = None,
        image: str | None = None,
        runner: str | None = None,
        runner_args: dict[str, Any] | None = None,
        key: str | None = None,
    ) -> Step:
        """Append a shell command to this chain.

        Returns a new ``Step``; the receiver is unchanged (steps are immutable).

        To set a timeout, wrap the result with ``hm.timeout(duration, step)``.

        Args:
            cmd: Shell command to run.
            cwd: Directory to run in, relative to the workspace root. Omit to
                run in the root; pass a non-empty path to change directory first.
            label: Human-facing label shown in the UI. Defaults to the command.
            cache: Cache policy controlling result reuse across builds.
            env: Per-step environment variables, merged on top of pipeline-level
                env at render time.
            image: Local-mode Docker base image for this step. Ignored when the
                step has a ``builds_in`` parent (the parent's snapshot wins).
            runner: Executor plugin runner name. ``None`` selects the default
                Docker runner.
            runner_args: Plugin-specific arguments validated by the runner's
                schema.
            key: Manual key override for this step in the v0 IR. Auto-derived
                from the command when omitted.

        Returns:
            A new ``Step`` with this command appended to the chain.

        Raises:
            ValueError: If ``cwd`` is an empty string.
        """
        if cwd == "":
            msg = (
                "hm: cwd must be a non-empty path\n"
                "  → omit cwd= to run in the workspace root, "
                'or pass cwd="some/dir"'
            )
            raise ValueError(msg)
        effective_cmd = f"cd {cwd} && {cmd}" if cwd is not None else cmd
        # Image inheritance: a scratch root (cmd is None) with image set
        # passes it down to the first emitted command step. Once the
        # chain has a real cmd, inheritance stops — keeps wire format
        # identical for normal chains.
        effective_image = (
            image if image is not None else (self.image if self.cmd is None else None)
        )
        return Step(
            cmd=effective_cmd,
            parent=self,
            label=label,
            cache=cache,
            env=env,
            image=effective_image,
            runner=runner,
            runner_args=runner_args,
            key_override=key,
        )

    def fork(self, label: str | None = None) -> Step:
        """Create a branch point from this step.

        Returns a new scratch-rooted ``Step`` whose parent is ``self``.
        Downstream ``.sh()`` calls on the fork produce independent leaves
        that all share ``self`` as their nearest emitted ancestor.

        Args:
            label: Optional label for the fork node in the UI.

        Returns:
            A new ``Step`` branching from this one.
        """
        return Step(cmd=None, parent=self, label=label)


def scratch() -> Step:
    """Create a new root step with no command.

    Use as the starting point for a chain, or call `sh()` at the module
    level to combine ``scratch()`` and ``.sh()`` in one call.

    Returns:
        A new root ``Step`` with no command or parent.

    Examples:
        >>> import harmont as hm
        >>> step = hm.scratch().sh("echo hello")
    """
    return Step()


def wait(*, continue_on_failure: bool = False) -> Step:
    """Insert a synchronization barrier between pipeline stages.

    All steps emitted before the barrier must finish before any step
    emitted after it starts. Equivalent to Buildkite's ``wait`` step.

    Args:
        continue_on_failure: When ``True``, the barrier passes even if
            upstream steps have failed, allowing cleanup or notification
            steps to run.

    Returns:
        A ``Step`` that lowers to a wait barrier in the v0 IR.

    Examples:
        >>> import harmont as hm
        >>> p = hm.pipeline([hm.sh("make build"), hm.wait(), hm.sh("make deploy")])
    """
    return Step(is_wait=True, continue_on_failure=continue_on_failure)
