"""Serialize a pipeline's Step chain to the raw step-chain wire format.

Replaces the Python lowering pass: instead of lowering Step chains all the
way to the petgraph-serde IR, this emits the flat ``RawStepChain`` format —
a list of steps that reference their parents by index — which Rust
(`crates/hm-dsl-engine/src/step_chain.rs`) deserializes and lowers.

Every reachable node is serialized, including scratch/fork passthroughs and
`wait` barriers; Rust drops the passthroughs and translates barriers into
`depends_on` edges. Env layering, key resolution, and the default-image
stamp also live on the Rust side, so this module emits only the raw
per-step fields.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from ._duration import parse_duration
from ._pipeline import _cache_to_dict, _topo_collect

if TYPE_CHECKING:
    from ._step import Step


def serialize_step_chain(
    leaves: list[Step] | tuple[Step, ...],
    *,
    env: dict[str, str] | None = None,
    timeout: str | int | None = None,
) -> dict[str, Any]:
    """Serialize the Step chains behind ``leaves`` into ``RawStepChain`` shape.

    Walks back from each leaf via ``Step.parent``, assigns every unique step
    (by ``id``) a dense index in parent-before-child order, and emits each
    step with its ``parent_idx`` pointing at the parent's index. Steps are
    keyed by identity, so structurally-equal forks keep distinct indices.

    Args:
        leaves: Terminal step(s) of each branch. Must be non-empty.
        env: Pipeline-level environment variables. Layered under per-step
            env by the Rust lowering pass.
        timeout: Whole-build wall-clock budget (``"30m"``, an int number of
            seconds, or a ``timedelta``).

    Returns:
        A JSON-shaped dict with ``steps``, ``leaf_indices``, and optional
        ``pipeline_env`` / ``pipeline_timeout_seconds``.
    """
    ordered = _topo_collect(list(leaves))
    idx_by_id: dict[int, int] = {id(s): i for i, s in enumerate(ordered)}

    steps = [_serialize_step(s, idx_by_id) for s in ordered]
    leaf_indices = [idx_by_id[id(leaf)] for leaf in leaves]

    out: dict[str, Any] = {"steps": steps, "leaf_indices": leaf_indices}
    if env is not None:
        out["pipeline_env"] = dict(env)
    if timeout is not None:
        out["pipeline_timeout_seconds"] = parse_duration(timeout)
    return out


def _serialize_step(step: Step, idx_by_id: dict[int, int]) -> dict[str, Any]:
    """Serialize a single step. ``cmd`` and ``parent_idx`` are always present
    (Rust requires them); every other field is emitted only when set, and the
    two bool flags only when true, matching Rust's serde defaults."""
    d: dict[str, Any] = {
        "cmd": step.cmd,
        "parent_idx": idx_by_id[id(step.parent)] if step.parent is not None else None,
    }
    if step.is_wait:
        d["is_wait"] = True
    if step.continue_on_failure:
        d["continue_on_failure"] = True
    if step.label is not None:
        d["label"] = step.label
    if step.cache is not None:
        d["cache"] = _cache_to_dict(step.cache)
    if step.env is not None:
        d["env"] = dict(step.env)
    if step.timeout_seconds is not None:
        d["timeout_seconds"] = step.timeout_seconds
    if step.image is not None:
        d["image"] = step.image
    if step.runner is not None:
        d["runner"] = step.runner
    if step.runner_args is not None:
        d["runner_args"] = step.runner_args
    if step.key_override is not None:
        d["key_override"] = step.key_override
    return d
