"""Local-driver registry dump.

Walks ``harmont._deploy.DEPLOYMENTS`` in topo order, lowering each
``LocalDeployment`` to the JSON shape described in
``docs/superpowers/specs/2026-05-21-hm-dev-deploy-design.md`` § 1.
Non-local deployments are passed through as ``{"driver": X,
"_unhandled": true}`` so the CLI can render them in ``hm dev ls``.

Step-chain deployments emit their pipeline as the existing v0 IR via
``harmont.pipeline()``; cache-keys are resolved through the standard
keygen path so the Rust executor can use the terminal key as the
build-image tag without re-running the algorithm.
"""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from harmont._deploy import DEPLOYMENTS, Deployment, dep_graph, topo_order
from harmont._target import clear_target_memo
from harmont.keygen import resolve_pipeline_keys
from harmont.pipeline import pipeline as _assemble

from ._deployment import LocalDeployment
from ._port import _PortSentinel

_SENTINEL_WIRE = "__hm_dev_port__"


def _lower_local(d: LocalDeployment, deps: tuple[str, ...]) -> dict[str, Any]:
    return {
        "driver": "local",
        "image": d.image,
        "from": _lower_from_step(d.from_step) if d.from_step is not None else None,
        "cmd": list(d.cmd) if d.cmd is not None else None,
        "port_mapping": {
            str(cport): _SENTINEL_WIRE
            for cport, value in d.port_mapping.items()
            if isinstance(value, _PortSentinel)
        },
        "env": dict(d.env),
        "volumes": dict(d.volumes),
        "workdir": d.workdir,
        "deps": list(deps),
    }


def _lower_from_step(step: Any) -> dict[str, Any]:
    """Lower a single Step (the deployment's `from_=`) into the v0 IR shape.

    The Step is treated as the terminal leaf of a one-pipeline IR.
    Cache-keys are resolved via the existing keygen so the Rust side
    can use them as image tags without re-running the algorithm.
    """
    ir = _assemble(step)
    resolve_pipeline_keys(
        ir.get("graph", {}),
        pipeline_org="hm-dev",
        pipeline_slug="hm-dev-build",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    return {"type": "step_chain", "pipeline_v0": ir}


def dump_registry_json(
    *,
    worktree_root: Path | None = None,
) -> str:
    """Emit the v0 deployment-registry JSON.

    ``worktree_root`` is recorded so the CLI can resolve relative
    ``volumes`` paths and the worktree-hash label. Pass the value
    yourself in tests; production use comes through the CLI shim
    (``python -m harmont.dev --dump-registry --worktree-root <PATH>``).
    """
    clear_target_memo()
    wt = Path(worktree_root) if worktree_root is not None else Path.cwd()
    order = topo_order()
    graph = dep_graph()
    deployments: dict[str, dict[str, Any]] = {}
    for slug in order:
        value = DEPLOYMENTS[slug]()
        if isinstance(value, LocalDeployment):
            deployments[slug] = _lower_local(value, graph[slug])
        elif isinstance(value, Deployment):
            deployments[slug] = {"driver": value.driver, "_unhandled": True}
        else:
            msg = (
                f"hm: @hm.deploy({slug!r}) returned {type(value).__name__}; "
                "expected a Deployment subclass"
            )
            raise TypeError(msg)
    return json.dumps({
        "schema_version": "0",
        "worktree": str(wt),
        "deployments": deployments,
    })
