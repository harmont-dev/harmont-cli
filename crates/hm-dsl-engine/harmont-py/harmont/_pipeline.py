"""Pipeline factory + lowering pass.

The factory walks back from each leaf via `Step.parent`, collects every
unique step (keyed by `id`, since structurally-equal forks must keep
distinct keys), topo-sorts by parent edges with a stable
leaf-then-DFS-pre tiebreaker, and lowers each step to the petgraph-serde
graph format matching the v0 IR schema.

Use `pipeline_to_json` from `json_emit` to emit the wire-format string.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from ._duration import parse_duration
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
    leaves: list[Step] | tuple[Step, ...],
    *,
    env: dict[str, str] | None = None,
    default_image: str | None = None,
    timeout: str | int | None = None,
) -> dict[str, Any]:
    """Top-level factory. Returns a JSON-shaped dict (version "0").

    ``default_image`` is the local-mode fallback Docker image: it
    applies to every command step that lacks both a ``builds_in``
    parent edge and a per-step ``image`` override.

    ``timeout`` is a whole-build wall-clock budget (``"30m"``, ``"1h"``,
    or an int number of seconds). When it elapses the build is killed and
    fails as *timed out*, regardless of how far the step graph got.
    """
    if not leaves:
        msg = (
            "pipeline must have at least one leaf — "
            "pass the terminal step(s) of each branch in the first argument"
        )
        raise ValueError(msg)
    out: dict[str, Any] = {"version": "0"}
    if default_image is not None:
        out["default_image"] = default_image
    if timeout is not None:
        out["timeout_seconds"] = parse_duration(timeout)
    out["graph"] = _lower_to_graph(
        list(leaves),
        env=env,
        default_image=default_image,
    )
    return out


def _lower_to_graph(
    leaves: list[Step],
    *,
    env: dict[str, str] | None = None,
    default_image: str | None = None,
) -> dict[str, Any]:
    """Walk back via `parent`, topo-sort, emit petgraph-serde graph dict.

    `scratch` and `fork` nodes carry no command and are not emitted as
    graph nodes; they exist only to set the `parent` of their children.
    Wait steps are not emitted as nodes — they are translated into
    explicit ``depends_on`` edges.
    """
    ordered = _topo_collect(leaves)
    command_steps = [s for s in ordered if s.cmd is not None and not s.is_wait]
    keys = resolve_keys(command_steps)

    # Assign integer node indices (dense, in emission order).
    idx_by_id: dict[int, int] = {}
    for i, s in enumerate(command_steps):
        idx_by_id[id(s)] = i

    # Track which node indices have a builds_in parent (for default_image).
    has_builds_in_parent: set[int] = set()

    nodes: list[dict[str, Any]] = []
    edges: list[list[Any]] = []

    # Collect all command-step indices emitted before each wait barrier.
    # When we encounter a wait, every step after the wait gets a
    # depends_on edge from every step before the wait.
    pre_wait_indices: list[int] = []
    # Pending depends_on sources (from the most recent wait barrier).
    pending_depends_on: list[int] = []

    for s in ordered:
        if s.is_wait:
            # All command-step indices emitted so far (after the last wait)
            # become sources for depends_on edges to subsequent steps.
            pending_depends_on = list(pre_wait_indices)
            pre_wait_indices = []
            continue

        if s.cmd is None:
            # scratch or fork — passthrough, not emitted.
            continue

        node_idx = idx_by_id[id(s)]
        step_key = keys[id(s)]

        # Build the CommandStep dict (no "type" or "builds_in" fields).
        step_dict: dict[str, Any] = {
            "key": step_key,
            "cmd": s.cmd,
        }
        if s.label is not None:
            step_dict["label"] = s.label
        if s.cache is not None:
            step_dict["cache"] = _cache_to_dict(s.cache)
        if s.timeout_seconds is not None:
            step_dict["timeout_seconds"] = s.timeout_seconds
        if s.image is not None:
            step_dict["image"] = s.image
        if s.runner is not None:
            step_dict["runner"] = s.runner
        if s.runner_args is not None:
            step_dict["runner_args"] = s.runner_args

        # Baseline env for non-interactive operation inside VMs/containers.
        merged_env: dict[str, str] = {
            "DEBIAN_FRONTEND": "noninteractive",
            "TERM": "dumb",
        }
        if env:
            merged_env.update(env)
        if s.env:
            merged_env.update(s.env)

        nodes.append({"step": step_dict, "env": merged_env})

        # builds_in edge from parent.
        parent_key = _resolved_parent_key(s, keys)
        if parent_key is not None:
            parent_idx = _find_idx_by_key(parent_key, command_steps, keys, idx_by_id)
            edges.append([parent_idx, node_idx, "builds_in"])
            has_builds_in_parent.add(node_idx)

        # depends_on edges from pre-wait steps.
        edges.extend([dep_idx, node_idx, "depends_on"] for dep_idx in pending_depends_on)

        pre_wait_indices.append(node_idx)

    # Apply default_image to root nodes (those without a builds_in parent).
    if default_image is not None:
        for i, node in enumerate(nodes):
            if i not in has_builds_in_parent and "image" not in node["step"]:
                node["step"]["image"] = default_image

    return {
        "nodes": nodes,
        "node_holes": [],
        "edge_property": "directed",
        "edges": edges,
    }


def _find_idx_by_key(
    key: str,
    command_steps: list[Step],
    keys: dict[int, str],
    idx_by_id: dict[int, int],
) -> int:
    """Return the node index for the step with the given resolved key."""
    for s in command_steps:
        if keys[id(s)] == key:
            return idx_by_id[id(s)]
    msg = f"BUG: no step with key {key!r}"
    raise KeyError(msg)


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
