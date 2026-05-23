"""LocalDeployment — the concrete dataclass for the local Docker driver.

Construction is mediated by ``harmont.dev._factory.deploy(...)``; the
factory does input validation and coerces fields. ``__post_init__`` is
the last-line invariant check (driver must be 'local').
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from harmont._deploy import Deployment

if TYPE_CHECKING:
    from collections.abc import Mapping

    from harmont._step import Step

    from ._port import _PortSentinel


@dataclass(frozen=True)
class LocalDeployment(Deployment):
    """Local Docker deployment record.

    Exactly one of ``image`` or ``from_step`` is non-None — enforced by
    ``deploy(...)``. ``port_mapping`` keys are container ports (1..65535);
    values are ``_PortSentinel`` (the ``hm.dev.port()`` singleton).
    ``volumes`` maps host paths (relative or absolute) to container
    paths (with optional ``:ro`` suffix).
    """
    image: str | None
    from_step: Step | None
    cmd: tuple[str, ...] | None
    port_mapping: Mapping[int, _PortSentinel]
    env: Mapping[str, str]
    volumes: Mapping[str, str]
    workdir: str | None

    def __post_init__(self) -> None:
        if self.driver != "local":
            msg = (
                f"LocalDeployment.driver must be 'local', got {self.driver!r}\n"
                "  → use the harmont.dev._factory.deploy() function "
                "instead of constructing LocalDeployment directly"
            )
            raise ValueError(msg)
