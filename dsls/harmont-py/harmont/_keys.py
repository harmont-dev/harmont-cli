"""Key derivation for chain-DSL steps.

Order of precedence per the design doc:
  1. explicit `key=` override on .sh()
  2. slugified label (when unique within the pipeline)
  3. stable 12-char hash of (parent_resolved_key, cmd, position)

Collision policy: when two steps' label-slugs collide and neither
claimed the slug via explicit `key=`, both fall back to hash. An
explicit override always wins, even if it would collide with another
step's natural slug.
"""

from __future__ import annotations

import hashlib
import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterable

    from ._step import Step

_EMOJI_SHORTCODE_RE = re.compile(r":[a-z0-9_+-]+:")
_NON_ALNUM_RE = re.compile(r"[^a-z0-9]+")


def slugify_label(label: str) -> str:
    """Lowercase, strip ``:emoji_codes:``, replace non-alnum runs with ``-``,
    trim leading/trailing dashes.

    Slugs are ASCII-only by policy (matches Buildkite). Non-ASCII
    letters are treated as separators: ``"Café Build"`` slugs to
    ``"caf-build"`` and ``"构建"`` slugs to ``""``. Labels that reduce
    to the empty string fall back to a hash key in ``resolve_keys``;
    the user's label is preserved on the step's ``label`` field for
    display, only the cross-reference key is hash-based.
    """
    s = label.lower()
    s = _EMOJI_SHORTCODE_RE.sub(" ", s)
    s = _NON_ALNUM_RE.sub("-", s)
    return s.strip("-")


def hash_key(parent_key: str, cmd: str, position: int) -> str:
    """Stable 12-char SHA-256 prefix over (parent_key, cmd, position).

    Used as the fallback key when no usable slug is available."""
    h = hashlib.sha256()
    h.update(parent_key.encode("utf-8"))
    h.update(b"\x00")
    h.update(cmd.encode("utf-8"))
    h.update(b"\x00")
    h.update(str(position).encode("utf-8"))
    return h.hexdigest()[:12]


def resolve_keys(steps: Iterable[Step]) -> dict[int, str]:
    """Resolve each Step's key. Returns ``{id(step): key}``.

    The ``id()`` indexing is deliberate: two structurally-equal Steps
    that arose from independent fork branches must keep distinct keys,
    and frozen-dataclass equality would conflate them.
    """
    steps_list = list(steps)

    overrides: dict[int, str] = {}
    # Natural slug per step (computed for every labeled step, even
    # those with explicit overrides — see slug_counts below).
    natural_slugs: dict[int, str] = {}
    for s in steps_list:
        if s.key_override is not None:
            overrides[id(s)] = s.key_override
        if s.label is not None:
            slug = slugify_label(s.label)
            if slug:
                natural_slugs[id(s)] = slug

    # Reserve every override; any natural slug that matches a reserved
    # override is a collision for the slug claimant.
    reserved = set(overrides.values())

    # Detect slug collisions across every labeled step — including those
    # with explicit overrides. An override-bearing step still "claims"
    # its natural slug for collision purposes, so a peer with the same
    # label can't quietly take it.
    slug_counts: dict[str, int] = {}
    for slug in natural_slugs.values():
        slug_counts[slug] = slug_counts.get(slug, 0) + 1

    # The slug pool that non-override steps may draw from: only steps
    # without a `key=` override are eligible to receive their slug.
    label_slugs: dict[int, str] = {
        sid: slug for sid, slug in natural_slugs.items() if sid not in overrides
    }

    keys: dict[int, str] = {}
    for position, s in enumerate(steps_list):
        sid = id(s)
        if sid in overrides:
            keys[sid] = overrides[sid]
            continue
        candidate_slug = label_slugs.get(sid)
        if (
            candidate_slug is not None
            and candidate_slug not in reserved
            and slug_counts[candidate_slug] == 1
        ):
            keys[sid] = candidate_slug
            reserved.add(candidate_slug)
            continue
        # Fall back to hash. Parent resolved key may not be in `keys`
        # yet; use the empty string as a sentinel — call sites that
        # need the resolved parent_key pass it explicitly via the
        # lowering pass (see pipeline.py).
        parent_key = ""
        if s.parent is not None and id(s.parent) in keys:
            parent_key = keys[id(s.parent)]
        keys[sid] = hash_key(parent_key, s.cmd or "", position)
    return keys
