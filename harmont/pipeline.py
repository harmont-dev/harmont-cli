"""Pipeline factory + lowering pass.

The factory walks back from each leaf via `Step.parent`, collects every
unique step (keyed by `id`, since structurally-equal forks must keep
distinct keys), topo-sorts by parent edges with a stable
leaf-then-DFS-pre tiebreaker, and lowers each step to a JSON-shaped
dict matching the v0 IR schema.

Use `pipeline_to_json` from `json_emit` to emit the wire-format string.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from ._keys import resolve_keys
from .cache import (
    CacheCompose,
    CacheForever,
    CacheNone,
    CacheOnChange,
    CachePolicy,
    CacheTTL,
)

if TYPE_CHECKING:
    from ._step import Step


def pipeline(
    *leaves: Step,
    env: dict[str, str] | None = None,
    default_image: str | None = None,
) -> dict[str, Any]:
    """Top-level factory. Returns a JSON-shaped dict (version "0").

    ``default_image`` is the local-mode fallback Docker image: it
    applies to every command step that lacks both a ``builds_in``
    parent and a per-step ``image`` override.
    """
    if not leaves:
        msg = (
            "pipeline must have at least one leaf — "
            "pass the terminal step(s) of each branch as positional args"
        )
        raise ValueError(msg)
    out: dict[str, Any] = {"version": "0"}
    if env is not None:
        out["env"] = env
    if default_image is not None:
        out["default_image"] = default_image
    out["steps"] = _lower_to_dicts(list(leaves))
    return out


def _lower_to_dicts(leaves: list[Step]) -> list[dict[str, Any]]:
    """Walk back via `parent`, topo-sort, emit one dict per emitted step.

    `scratch` and `fork` nodes carry no command and are not emitted as
    JSON steps; they exist only to set the `parent` of their children.
    """
    ordered = _topo_collect(leaves)
    keys = resolve_keys([s for s in ordered if s.cmd is not None and not s.is_wait])
    out: list[dict[str, Any]] = []
    for s in ordered:
        if s.is_wait:
            d: dict[str, Any] = {"type": "wait"}
            if s.continue_on_failure:
                d["continue_on_failure"] = True
            out.append(d)
            continue
        if s.cmd is None:
            # scratch or fork — passthrough, not emitted
            continue
        parent_key = _resolved_parent_key(s, keys)
        d = {
            "type": "command",
            "key": keys[id(s)],
            "cmd": s.cmd,
            "builds_in": parent_key,
        }
        if s.label is not None:
            d["label"] = s.label
        if s.cache is not None:
            d["cache"] = _cache_to_dict(s.cache)
        if s.env is not None:
            d["env"] = s.env
        if s.timeout_seconds is not None:
            d["timeout_seconds"] = s.timeout_seconds
        if s.image is not None:
            d["image"] = s.image
        if s.runner is not None:
            d["runner"] = s.runner
        if s.runner_args is not None:
            d["runner_args"] = s.runner_args
        out.append(d)
    return out


def _topo_collect(leaves: list[Step]) -> list[Step]:
    """Collect every Step reachable from `leaves` via `parent`, return them
    in parent-before-child order. Tiebreak by leaf order, then DFS-pre on
    each leaf chain (deterministic). Wait steps are inserted in their
    leaf-tuple position."""
    seen: set[int] = set()
    ordered: list[Step] = []

    for leaf in leaves:
        if leaf.is_wait:
            ordered.append(leaf)
            continue
        chain: list[Step] = []
        node: Step | None = leaf
        while node is not None:
            if id(node) in seen:
                break
            chain.append(node)
            node = node.parent
        # chain is leaf -> root order; reverse for parent-first.
        for s in reversed(chain):
            if id(s) in seen:
                continue
            seen.add(id(s))
            ordered.append(s)
    return ordered


def _resolved_parent_key(s: Step, keys: dict[int, str]) -> str | None:
    """Walk back through scratch/fork nodes to the nearest emitted ancestor."""
    node = s.parent
    while node is not None:
        if node.cmd is not None and not node.is_wait:
            return keys[id(node)]
        node = node.parent
    return None


def _cache_to_dict(policy: CachePolicy) -> dict[str, Any]:
    """Render a CachePolicy to its JSON-shape dict.

    Cache key resolution happens in keygen.resolve_pipeline_keys after
    the pipeline structure is built.
    """
    if isinstance(policy, CacheNone):
        return {"policy": "none"}
    if isinstance(policy, CacheForever):
        return {"policy": "forever", "env_keys": list(policy.env_keys)}
    if isinstance(policy, CacheTTL):
        return {
            "policy": "ttl",
            "duration_seconds": int(policy.duration.total_seconds()),
            "env_keys": list(policy.env_keys),
        }
    if isinstance(policy, CacheOnChange):
        return {"policy": "on_change", "paths": list(policy.paths)}
    if isinstance(policy, CacheCompose):
        return {
            "policy": "compose",
            "sub_policies": [_cache_to_dict(p) for p in policy.policies],
        }
    msg = f"unknown CachePolicy: {type(policy).__name__}"
    raise TypeError(msg)


from .json_emit import pipeline_to_json as _pipeline_to_json  # noqa: E402


def pipeline_to_json(p: dict[str, Any], **kw: Any) -> str:
    """Convenience re-export so callers can do
    ``harmont.pipeline_to_json(pipeline(...))`` without importing
    `json_emit` directly. See `json_emit.pipeline_to_json` for kwargs."""
    return _pipeline_to_json(p, **kw)
