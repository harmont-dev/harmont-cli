"""Key derivation: slug from label, hash fallback, collision resolution."""

from __future__ import annotations

from harmont._keys import hash_key, resolve_keys, slugify_label
from harmont._step import scratch


def test_slugify_strips_emoji_shortcodes():
    assert slugify_label(":rust: api build") == "api-build"


def test_slugify_lowercases_and_dashes_non_alnum():
    assert slugify_label("API Build (Test)") == "api-build-test"


def test_slugify_collapses_runs_of_dashes():
    assert slugify_label("foo  --  bar") == "foo-bar"


def test_slugify_trims_leading_trailing_dashes():
    assert slugify_label(":fire: !!! foo !!!") == "foo"


def test_slugify_empty_returns_empty_string():
    assert slugify_label(":fire:") == ""
    assert slugify_label("") == ""


def test_slugify_drops_non_ascii_letters():
    assert slugify_label("Café Build") == "caf-build"


def test_slugify_all_non_ascii_returns_empty_string():
    assert slugify_label("构建") == ""


def test_resolve_keys_falls_back_to_hash_for_non_ascii_only_label():
    s = scratch().sh("make", label="构建")
    keys = resolve_keys([s])
    assert len(keys[id(s)]) == 12  # hash, since slug is empty


def test_hash_key_is_deterministic_12_hex_chars():
    h1 = hash_key("parent-key", "make build", 0)
    h2 = hash_key("parent-key", "make build", 0)
    assert h1 == h2
    assert len(h1) == 12
    assert all(c in "0123456789abcdef" for c in h1)


def test_hash_key_changes_with_inputs():
    a = hash_key("p", "make", 0)
    b = hash_key("p", "make", 1)
    c = hash_key("p", "test", 0)
    d = hash_key("q", "make", 0)
    assert len({a, b, c, d}) == 4


def test_resolve_keys_uses_explicit_override():
    s = scratch().sh("make", key="my-key")
    keys = resolve_keys([s])
    assert keys[id(s)] == "my-key"


def test_resolve_keys_uses_label_slug_when_unique():
    s = scratch().sh("make", label=":rust: build")
    keys = resolve_keys([s])
    assert keys[id(s)] == "build"


def test_resolve_keys_falls_back_to_hash_when_label_collides():
    a = scratch().sh("make a", label=":rust: build")
    b = scratch().sh("make b", label=":rust: build")
    keys = resolve_keys([a, b])
    # Both colliding labels fall through to hash-derived keys.
    assert keys[id(a)] != "build"
    assert keys[id(b)] != "build"
    assert len(keys[id(a)]) == 12
    assert keys[id(a)] != keys[id(b)]


def test_resolve_keys_falls_back_to_hash_when_no_label():
    s = scratch().sh("make")
    keys = resolve_keys([s])
    assert len(keys[id(s)]) == 12


def test_resolve_keys_explicit_override_wins_even_under_collision():
    a = scratch().sh("make a", label=":rust: build", key="explicit-a")
    b = scratch().sh("make b", label=":rust: build")
    keys = resolve_keys([a, b])
    assert keys[id(a)] == "explicit-a"
    # `b` had a label that would have been "build", but `a` claimed
    # "build" via override, so `b` falls to hash.
    assert keys[id(b)] != "build"
    assert len(keys[id(b)]) == 12
