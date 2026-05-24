"""Render a chain-DSL pipeline dict to the v0 IR JSON string.

The wire format uses petgraph-serde graph encoding: nodes carry
CommandStep dicts and edges encode ``builds_in`` / ``depends_on``
relationships.

Cache keys are resolved in keygen.resolve_pipeline_keys before
serialization, so the emitted JSON includes `cache.key` for every
node whose policy is not 'none'.
"""

from __future__ import annotations

import copy
import json
import os
import time
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from collections.abc import Mapping

from .keygen import resolve_pipeline_keys


def pipeline_to_json(
    p: dict[str, Any],
    *,
    pipeline_org: str | None = None,
    pipeline_slug: str | None = None,
    now: int | None = None,
    base_path: Path | None = None,
    env: Mapping[str, str] | None = None,
) -> str:
    """Render the pipeline dict (as returned by `pipeline(...)`) to JSON.

    Resolves cache keys before serialization. Defaults mirror the
    environment hooks of the old Scheme renderer:
      pipeline_org  <- env["HARMONT_PIPELINE_ORG"] or "default"
      pipeline_slug <- env["HARMONT_PIPELINE_SLUG"] or "default"
      now           <- int(time.time())
      base_path     <- Path.cwd()
      env           <- os.environ
    """
    env_map: Mapping[str, str] = env if env is not None else os.environ
    org = (
        pipeline_org
        if pipeline_org is not None
        else env_map.get("HARMONT_PIPELINE_ORG", "default")
    )
    slug = (
        pipeline_slug
        if pipeline_slug is not None
        else env_map.get("HARMONT_PIPELINE_SLUG", "default")
    )
    render_now = now if now is not None else int(time.time())
    bp = base_path if base_path is not None else Path.cwd()

    body = copy.deepcopy(p)
    resolve_pipeline_keys(
        body.get("graph", {}),
        pipeline_org=org,
        pipeline_slug=slug,
        now=render_now,
        base_path=bp,
        env=env_map,
    )
    return json.dumps(body, ensure_ascii=False, separators=(", ", ": "))
