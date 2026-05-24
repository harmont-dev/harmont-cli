"""Cache-key resolver.

Direct port of cidsl/lisp/src/harmont_macros.scm (resolve-cache-key
and helpers). Output bytes MUST match the Scheme version so cached
snapshots persisted before the Scheme removal remain reachable.

Algorithm (pre-image of the outer sha256):

    pipeline_org NUL pipeline_slug NUL step_key NUL
    parent_resolved_key NUL policy_resolution

policy_resolution branches:
    none      -> "none"          (no key emitted)
    forever   -> "forever-"   + sha256(cmd NUL env_subset)
    ttl       -> "ttl-N-"     + sha256(cmd NUL env_subset)   N = now // duration
    on_change -> "sha-"       + sha256(concat(file_hash(p) NUL for p in sorted))
    compose   -> "compose-"   + sha256(concat(resolve(sub) or "none"))

The Scheme `cache-when` policy is removed (see HAR-16) — it required a
Scheme sandbox that no longer exists.
"""

from __future__ import annotations

import hashlib
from pathlib import Path  # noqa: TC003  used at runtime in _path_hash
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from collections.abc import Mapping

NUL = "\x00"


def resolve_pipeline_keys(
    graph: dict[str, Any],
    *,
    pipeline_org: str,
    pipeline_slug: str,
    now: int,
    base_path: Path,
    env: Mapping[str, str],
) -> dict[str, Any]:
    """Walk graph nodes in order. For every node whose cache policy is not
    'none', compute a deterministic sha256 cache key and inject it into
    that node's step ``cache`` dict as ``cache["key"]``. Returns the
    same graph dict (mutated in place -- callers may rely on identity)."""
    nodes = graph.get("nodes", [])
    edges = graph.get("edges", [])

    # Build parent key map from builds_in edges.
    key_by_idx: dict[int, str] = {i: n["step"]["key"] for i, n in enumerate(nodes)}
    parent_key_map: dict[str, str] = {}
    for src, dst, kind in edges:
        if kind == "builds_in":
            parent_key_map[key_by_idx[dst]] = key_by_idx[src]

    resolved: dict[str, str] = {}
    for node in nodes:
        step = node["step"]
        cache = step.get("cache")
        if not cache or cache["policy"] == "none":
            continue
        cmd = step.get("cmd", "")
        parent = parent_key_map.get(step["key"])
        parent_resolved = _lookup_parent(parent, resolved)
        policy_res = _resolve_policy(cache, cmd, now, base_path, env)
        key = _sha256_hex(
            pipeline_org
            + NUL
            + pipeline_slug
            + NUL
            + step["key"]
            + NUL
            + parent_resolved
            + NUL
            + policy_res
        )
        cache["key"] = key
        resolved[step["key"]] = key
    return graph


def _lookup_parent(parent: str | None, resolved: dict[str, str]) -> str:
    if parent is None:
        return "scratch"
    key = resolved.get(parent)
    if key is None:
        msg = (
            f"step references builds_in {parent!r} which has no cached "
            f"key (parent must be defined upstream and cached)"
        )
        raise ValueError(msg)
    return key


def _resolve_policy(
    policy: dict[str, Any],
    cmd: str,
    now: int,
    base_path: Path,
    env: Mapping[str, str],
) -> str:
    kind = policy["policy"]
    if kind == "none":
        return "none"
    if kind == "forever":
        env_keys = policy.get("env_keys", [])
        return "forever-" + _sha256_hex(cmd + NUL + _env_subset(env_keys, env))
    if kind == "ttl":
        duration = policy["duration_seconds"]
        bucket = now // duration
        env_keys = policy.get("env_keys", [])
        return "ttl-" + str(bucket) + "-" + _sha256_hex(cmd + NUL + _env_subset(env_keys, env))
    if kind == "on_change":
        resolved: list[Path] = []
        for p in sorted(policy["paths"]):
            if any(c in p for c in ("*", "?", "[")):
                resolved.extend(sorted(base_path.glob(p)))
            else:
                full = base_path / p
                if full.exists():
                    resolved.append(full)
        pre = "".join(_path_hash(r) + NUL for r in resolved)
        return "sha-" + _sha256_hex(pre)
    if kind == "compose":
        subs = policy["sub_policies"]
        parts = [
            _resolve_policy(sub, cmd, now, base_path, env) if sub["policy"] != "none" else "none"
            for sub in subs
        ]
        return "compose-" + _sha256_hex("".join(parts))
    msg = f"resolve-policy-key: unknown policy {kind!r}"
    raise ValueError(msg)


def _env_subset(env_keys: list[str], env: Mapping[str, str]) -> str:
    sorted_keys = sorted(env_keys)
    return "".join(k + "=" + env.get(k, "") + NUL for k in sorted_keys)


def _path_hash(path: Path) -> str:
    """Hash a path's content for an `on_change` cache key.

    Files: hash the bytes.

    Directories: walk recursively in sorted order and fold each file's
    POSIX-style relative path + content into one SHA-256 stream. Empty
    directories hash to the empty stream's digest, which is stable.

    Missing paths fail loudly: ``on_change`` is a build-time invariant
    and a typo should not silently weaken the cache key.
    """
    if path.is_file():
        with path.open("rb") as fp:
            return hashlib.sha256(fp.read()).hexdigest()
    if path.is_dir():
        h = hashlib.sha256()
        files = sorted(p for p in path.rglob("*") if p.is_file())
        for child in files:
            rel = child.relative_to(path).as_posix()
            h.update(rel.encode("utf-8"))
            h.update(b"\x00")
            h.update(child.read_bytes())
            h.update(b"\x00")
        return h.hexdigest()
    msg = f"on_change path does not exist: {path}"
    raise FileNotFoundError(msg)


def _sha256_hex(s: str) -> str:
    return hashlib.sha256(s.encode("utf-8")).hexdigest()
