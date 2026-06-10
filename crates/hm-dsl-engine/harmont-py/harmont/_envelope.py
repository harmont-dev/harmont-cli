"""Envelope renderer — produces the schema_version=1 JSON document.

See docs/superpowers/specs/2026-05-10-har-9-imperfect-dsl-design.md
§ "The envelope" for the wire format.

Each registered pipeline carries its resolved v0 IR as a nested
``definition`` object. Consumers (api, cli) read that directly — no
intermediate Scheme stage exists since HAR-16.
"""

from __future__ import annotations

import json
import os
import time
from pathlib import Path
from typing import TYPE_CHECKING, Any

from ._pipeline import pipeline as _assemble
from ._registry import REGISTRATIONS, PipelineRegistration
from ._target import clear_target_memo
from ._unwrap import as_leaves
from .keygen import resolve_pipeline_keys

if TYPE_CHECKING:
    from collections.abc import Mapping


def _render_one(
    reg: PipelineRegistration,
    *,
    pipeline_org: str,
    now: int,
    base_path: Path,
    env: Mapping[str, str],
) -> dict[str, Any]:
    raw = reg.fn()
    try:
        leaves = as_leaves(raw)
    except TypeError as e:
        msg = f"pipeline {reg.slug!r}: invalid return value\n  → {e}"
        raise TypeError(msg) from e
    ir = _assemble(leaves, env=reg.env, default_image=reg.default_image, timeout=reg.timeout)
    resolve_pipeline_keys(
        ir.get("graph", {}),
        pipeline_org=pipeline_org,
        pipeline_slug=reg.slug,
        now=now,
        base_path=base_path,
        env=env,
    )
    return {
        "slug": reg.slug,
        "name": reg.name,
        "allow_manual": reg.allow_manual,
        "triggers": [t.to_dict() for t in reg.triggers],
        "definition": ir,
    }


def dump_registry_json(
    *,
    pipeline_org: str | None = None,
    now: int | None = None,
    base_path: Path | None = None,
    env: Mapping[str, str] | None = None,
) -> str:
    """Emit the schema_version=1 envelope JSON.

    Defaults mirror ``pipeline_to_json``:
      ``pipeline_org`` <- ``env["HARMONT_PIPELINE_ORG"]`` or ``"default"``
      ``now``          <- ``int(time.time())``
      ``base_path``    <- ``Path.cwd()`` (resolves ``on_change`` cache paths)
      ``env``          <- ``os.environ``
    Per-pipeline slug is read from each registration.

    The target memoization cache is cleared at the start of each render
    so per-pipeline target invocations dedup within a single render but
    don't leak across renders. The named-target registry is left intact
    so pipeline fixture-style params can resolve their dependencies.
    """
    clear_target_memo()
    env_map: Mapping[str, str] = env if env is not None else os.environ
    org = (
        pipeline_org
        if pipeline_org is not None
        else env_map.get("HARMONT_PIPELINE_ORG", "default")
    )
    render_now = now if now is not None else int(time.time())
    bp = base_path if base_path is not None else Path.cwd()
    return json.dumps(
        {
            "schema_version": "1",
            "pipelines": [
                _render_one(reg, pipeline_org=org, now=render_now, base_path=bp, env=env_map)
                for reg in REGISTRATIONS
            ],
        }
    )
