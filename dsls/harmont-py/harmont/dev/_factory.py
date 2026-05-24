"""hm.dev.deploy(...) — the public factory for LocalDeployment.

Validation is deliberately strict and fix-directed. The @hm.deploy
decorator only learns the slug at decoration time, so this factory
emits LocalDeployment with name="" — the decorator stamps the slug
in afterwards via dataclasses.replace.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from ._deployment import LocalDeployment
from ._port import _PortSentinel

if TYPE_CHECKING:
    from collections.abc import Iterable, Mapping

    from harmont._step import Step


def deploy(
    *,
    image: str | None = None,
    from_: Step | None = None,
    cmd: Iterable[str] | None = None,
    port_mapping: Mapping[int, _PortSentinel] | None = None,
    env: Mapping[str, str] | None = None,
    volumes: Mapping[str, str] | None = None,
    workdir: str | None = None,
) -> LocalDeployment:
    """Construct a LocalDeployment.

    Exactly one of ``image`` or ``from_`` is required. ``port_mapping``
    keys are container ports (1..65535); values must be the
    ``hm.dev.port()`` sentinel in v1. See the design spec § 1 for the
    full validation table.
    """
    if (image is None) == (from_ is None):
        msg = (
            "hm.dev.deploy requires exactly one of `image=` or `from_=`, "
            f"got image={image!r}, from_={from_!r}\n"
            '  → pick one. Use `image="..."` for a published image, '
            "`from_=<Step>` to build from a Step chain."
        )
        raise ValueError(msg)

    pm = _validate_port_mapping(port_mapping)
    env_resolved = _validate_env(env)
    volumes_resolved = _validate_volumes(volumes)
    cmd_resolved = _validate_cmd(cmd)
    workdir_resolved = _validate_workdir(workdir)

    return LocalDeployment(
        name="",  # decorator stamps the slug in
        driver="local",
        image=image,
        from_step=from_,
        cmd=cmd_resolved,
        port_mapping=pm,
        env=env_resolved,
        volumes=volumes_resolved,
        workdir=workdir_resolved,
    )


def _validate_port_mapping(
    pm: Mapping[int, _PortSentinel] | None,
) -> Mapping[int, _PortSentinel]:
    if pm is None:
        return {}
    result: dict[int, _PortSentinel] = {}
    for k, v in pm.items():
        if not isinstance(k, int) or k < 1 or k > 65535:
            msg = (
                f"hm.dev.deploy port_mapping key must be int in 1..65535, "
                f"got {k!r}\n"
                "  → keys are container ports the service listens on"
            )
            raise ValueError(msg)
        if not isinstance(v, _PortSentinel):
            msg = (
                f"hm.dev.deploy port_mapping value must be hm.dev.port(), "
                f"got {type(v).__name__}\n"
                "  → use hm.dev.port() to ask the OS for a free host port"
            )
            raise TypeError(msg)
        result[k] = v
    return result


def _validate_env(env: Mapping[str, str] | None) -> Mapping[str, str]:
    if env is None:
        return {}
    for k, v in env.items():
        if not isinstance(k, str):
            msg = f"hm.dev.deploy env key must be str, got {type(k).__name__}"
            raise TypeError(msg)
        if not isinstance(v, str):
            msg = (
                f"hm.dev.deploy env value for {k!r} must be str, "
                f"got {type(v).__name__}\n"
                "  → call str(...) at the call site so the conversion is explicit"
            )
            raise TypeError(msg)
    return dict(env)


def _validate_volumes(
    volumes: Mapping[str, str] | None,
) -> Mapping[str, str]:
    if volumes is None:
        return {}
    for hp, cp in volumes.items():
        if not isinstance(hp, str) or not hp:
            msg = (
                f"hm.dev.deploy volumes host path must be a non-empty str, "
                f"got {hp!r} ({type(hp).__name__})"
            )
            raise ValueError(msg)
        if not isinstance(cp, str) or not cp.startswith("/"):
            msg = (
                f"hm.dev.deploy volumes container path {cp!r} must start with "
                "'/'; append ':ro' for read-only mounts (e.g. '/workspace:ro')"
            )
            raise ValueError(msg)
    return dict(volumes)


def _validate_cmd(cmd: Iterable[str] | None) -> tuple[str, ...] | None:
    if cmd is None:
        return None
    items = tuple(cmd)
    for x in items:
        if not isinstance(x, str):
            msg = (
                f"hm.dev.deploy cmd elements must be str, got {type(x).__name__}\n"
                "  → call str(...) at the call site so the conversion is explicit"
            )
            raise TypeError(msg)
    return items


def _validate_workdir(workdir: str | None) -> str | None:
    if workdir is None:
        return None
    if not workdir.startswith("/"):
        msg = (
            f"hm.dev.deploy workdir must be an absolute path, got {workdir!r}\n"
            "  → workdir is interpreted inside the container; "
            "use a path that starts with '/'"
        )
        raise ValueError(msg)
    return workdir
