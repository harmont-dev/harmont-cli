"""Module-level registry of @pipeline-decorated functions.

Stage 1 (`dump_registry_json` in `_envelope`) walks REGISTRATIONS to
emit the envelope JSON the api/cli consume.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable

    from .triggers import Trigger


@dataclass(frozen=True)
class PipelineRegistration:
    slug: str
    name: str
    triggers: tuple[Trigger, ...]
    allow_manual: bool
    env: dict[str, str] | None
    fn: Callable[[], object]
    timeout: str | int | None = None


REGISTRATIONS: list[PipelineRegistration] = []


def register(reg: PipelineRegistration) -> None:
    """Append a registration; raise on duplicate slug."""
    if any(r.slug == reg.slug for r in REGISTRATIONS):
        msg = (
            f"duplicate pipeline slug {reg.slug!r}\n  → each @hm.pipeline must have a unique slug"
        )
        raise ValueError(msg)
    REGISTRATIONS.append(reg)


def clear_registry() -> None:
    """Wipe REGISTRATIONS. Test-fixture helper; not part of the public surface."""
    REGISTRATIONS.clear()
