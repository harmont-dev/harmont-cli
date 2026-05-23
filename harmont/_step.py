"""Internal Step dataclass — the chain primitive.

Public callers go through `scratch`, `wait`, `Step.sh`, `Step.fork`
re-exported from `harmont/__init__.py`. This module is private; nothing
outside `harmont` should import from it.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from .cache import CachePolicy


@dataclass(frozen=True)
class Step:
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
    the step has a ``builds_in`` parent (the parent's snapshot wins);
    falls back to the pipeline's ``default_image`` when unset."""

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
        timeout_seconds: int | None = None,
        image: str | None = None,
        runner: str | None = None,
        runner_args: dict[str, Any] | None = None,
        key: str | None = None,
    ) -> Step:
        if cwd == "":
            msg = (
                "hm: cwd must be a non-empty path\n"
                '  → omit cwd= to run in the workspace root, '
                'or pass cwd="some/dir"'
            )
            raise ValueError(msg)
        effective_cmd = f"cd {cwd} && {cmd}" if cwd is not None else cmd
        # Image inheritance: a scratch root (cmd is None) with image set
        # passes it down to the first emitted command step. Once the
        # chain has a real cmd, inheritance stops — keeps wire format
        # identical for normal chains.
        effective_image = image if image is not None else (
            self.image if self.cmd is None else None
        )
        return Step(
            cmd=effective_cmd,
            parent=self,
            label=label,
            cache=cache,
            env=env,
            timeout_seconds=timeout_seconds,
            image=effective_image,
            runner=runner,
            runner_args=runner_args,
            key_override=key,
        )

    def fork(self, label: str | None = None) -> Step:
        return Step(cmd=None, parent=self, label=label)


def scratch() -> Step:
    return Step()


def wait(*, continue_on_failure: bool = False) -> Step:
    return Step(is_wait=True, continue_on_failure=continue_on_failure)
