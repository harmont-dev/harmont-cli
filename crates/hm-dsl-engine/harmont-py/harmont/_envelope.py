"""Envelope renderer — produces the schema_version=1 JSON document.

See docs/superpowers/specs/2026-05-10-har-9-imperfect-dsl-design.md
§ "The envelope" for the wire format.

Each registered pipeline carries its raw step chain as a nested
``step_chain`` object. Rust (`crates/hm-dsl-engine/src/step_chain.rs`)
lowers that into the v0 IR — env layering, key resolution, and the
default-image stamp all live on the Rust side.
"""

from __future__ import annotations

import json
from typing import Any

from ._registry import REGISTRATIONS, PipelineRegistration
from ._serialize import serialize_step_chain
from ._target import clear_target_memo
from ._unwrap import as_leaves


def _render_one(reg: PipelineRegistration) -> dict[str, Any]:
    raw = reg.fn()
    try:
        leaves = as_leaves(raw)
    except TypeError as e:
        msg = f"pipeline {reg.slug!r}: invalid return value\n  → {e}"
        raise TypeError(msg) from e
    step_chain = serialize_step_chain(leaves, env=reg.env, timeout=reg.timeout)
    return {
        "slug": reg.slug,
        "name": reg.name,
        "allow_manual": reg.allow_manual,
        "triggers": [t.to_dict() for t in reg.triggers],
        "step_chain": step_chain,
    }


def dump_registry_json() -> str:
    """Emit the schema_version=1 envelope JSON.

    Each pipeline's raw step chain is serialized; Rust performs the
    lowering (env layering, cache-key resolution, default-image stamp).

    The target memoization cache is cleared at the start of each render
    so per-pipeline target invocations dedup within a single render but
    don't leak across renders. The named-target registry is left intact
    so pipeline fixture-style params can resolve their dependencies.
    """
    clear_target_memo()
    return json.dumps(
        {
            "schema_version": "1",
            "pipelines": [_render_one(reg) for reg in REGISTRATIONS],
        }
    )
