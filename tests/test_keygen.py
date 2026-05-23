"""Cache-key resolver — direct ports of the Scheme algorithm in
harmont_macros.scm. Keys must be byte-identical to what harmont-eval
produced pre-removal, so existing cached snapshots remain reachable."""

from __future__ import annotations

import hashlib
import tempfile
from pathlib import Path

import pytest

from harmont.keygen import resolve_pipeline_keys


def _sha256_hex(s: str) -> str:
    return hashlib.sha256(s.encode("utf-8")).hexdigest()


NUL = "\x00"


def test_none_policy_emits_no_key():
    steps = [
        {
            "type": "command",
            "key": "a",
            "cmd": "echo",
            "builds_in": None,
            "cache": {"policy": "none"},
        },
    ]
    out = resolve_pipeline_keys(
        steps,
        pipeline_org="default",
        pipeline_slug="default",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    assert "key" not in out[0]["cache"]


def test_forever_policy_key_matches_scheme_formula():
    steps = [
        {
            "type": "command",
            "key": "a",
            "cmd": "echo hi",
            "builds_in": None,
            "cache": {"policy": "forever", "env_keys": []},
        },
    ]
    out = resolve_pipeline_keys(
        steps,
        pipeline_org="default",
        pipeline_slug="default",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    inner = _sha256_hex("echo hi" + NUL + "")
    policy_res = "forever-" + inner
    expected = _sha256_hex(
        "default" + NUL + "default" + NUL + "a" + NUL + "scratch" + NUL + policy_res
    )
    assert out[0]["cache"]["key"] == expected


def test_ttl_policy_key_includes_bucket():
    steps = [
        {
            "type": "command",
            "key": "a",
            "cmd": "x",
            "builds_in": None,
            "cache": {"policy": "ttl", "duration_seconds": 3600, "env_keys": []},
        },
    ]
    out = resolve_pipeline_keys(
        steps,
        pipeline_org="default",
        pipeline_slug="default",
        now=7200,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    inner = _sha256_hex("x" + NUL + "")
    policy_res = "ttl-2-" + inner
    expected = _sha256_hex(
        "default" + NUL + "default" + NUL + "a" + NUL + "scratch" + NUL + policy_res
    )
    assert out[0]["cache"]["key"] == expected


def test_on_change_reads_file_contents():
    with tempfile.TemporaryDirectory() as d:
        f = Path(d) / "file.txt"
        f.write_bytes(b"hello")
        steps = [
            {
                "type": "command",
                "key": "a",
                "cmd": "make",
                "builds_in": None,
                "cache": {"policy": "on_change", "paths": ["file.txt"]},
            },
        ]
        out = resolve_pipeline_keys(
            steps,
            pipeline_org="default",
            pipeline_slug="default",
            now=0,
            base_path=Path(d),
            env={},
        )
        file_hash = hashlib.sha256(b"hello").hexdigest()
        inner = _sha256_hex(file_hash + NUL)
        policy_res = "sha-" + inner
        expected = _sha256_hex(
            "default" + NUL + "default" + NUL + "a" + NUL + "scratch" + NUL + policy_res
        )
        assert out[0]["cache"]["key"] == expected


def test_on_change_handles_directory_paths():
    """A directory path in ``on_change`` hashes every file inside,
    sorted, with its relative path included in the stream. Two builds
    of the same tree produce the same key; touching a file under the
    directory flips the key."""
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        sub = root / "dir"
        sub.mkdir()
        (sub / "a.txt").write_bytes(b"alpha")
        (sub / "b.txt").write_bytes(b"beta")

        steps = [
            {
                "type": "command",
                "key": "s",
                "cmd": "make",
                "builds_in": None,
                "cache": {"policy": "on_change", "paths": ["dir/"]},
            },
        ]
        out1 = resolve_pipeline_keys(
            list(steps),
            pipeline_org="default",
            pipeline_slug="default",
            now=0,
            base_path=root,
            env={},
        )
        key1 = out1[0]["cache"]["key"]

        # Same tree → same key.
        out_again = resolve_pipeline_keys(
            [dict(s, cache=dict(s["cache"])) for s in steps],
            pipeline_org="default",
            pipeline_slug="default",
            now=0,
            base_path=root,
            env={},
        )
        assert out_again[0]["cache"]["key"] == key1

        # Modify a file → key changes.
        (sub / "a.txt").write_bytes(b"alpha2")
        out2 = resolve_pipeline_keys(
            [dict(s, cache=dict(s["cache"])) for s in steps],
            pipeline_org="default",
            pipeline_slug="default",
            now=0,
            base_path=root,
            env={},
        )
        assert out2[0]["cache"]["key"] != key1


def test_on_change_missing_path_raises():
    with tempfile.TemporaryDirectory() as d:
        steps = [
            {
                "type": "command",
                "key": "s",
                "cmd": "make",
                "builds_in": None,
                "cache": {"policy": "on_change", "paths": ["nope/"]},
            },
        ]
        with pytest.raises(FileNotFoundError, match="on_change path does not exist"):
            resolve_pipeline_keys(
                steps,
                pipeline_org="default",
                pipeline_slug="default",
                now=0,
                base_path=Path(d),
                env={},
            )


def test_env_keys_are_sorted_and_picked_up():
    steps = [
        {
            "type": "command",
            "key": "a",
            "cmd": "echo",
            "builds_in": None,
            "cache": {"policy": "forever", "env_keys": ["BAR", "FOO"]},
        },
    ]
    out = resolve_pipeline_keys(
        steps,
        pipeline_org="default",
        pipeline_slug="default",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={"FOO": "1", "BAR": "2"},
    )
    env_str = "BAR=2" + NUL + "FOO=1" + NUL
    inner = _sha256_hex("echo" + NUL + env_str)
    policy_res = "forever-" + inner
    expected = _sha256_hex(
        "default" + NUL + "default" + NUL + "a" + NUL + "scratch" + NUL + policy_res
    )
    assert out[0]["cache"]["key"] == expected


def test_parent_key_chains_through_resolved_cache_keys():
    steps = [
        {
            "type": "command",
            "key": "a",
            "cmd": "x",
            "builds_in": None,
            "cache": {"policy": "forever", "env_keys": []},
        },
        {
            "type": "command",
            "key": "b",
            "cmd": "y",
            "builds_in": "a",
            "cache": {"policy": "forever", "env_keys": []},
        },
    ]
    out = resolve_pipeline_keys(
        steps,
        pipeline_org="default",
        pipeline_slug="default",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    parent_key = out[0]["cache"]["key"]
    inner_b = _sha256_hex("y" + NUL + "")
    policy_res = "forever-" + inner_b
    expected_b = _sha256_hex(
        "default" + NUL + "default" + NUL + "b" + NUL + parent_key + NUL + policy_res
    )
    assert out[1]["cache"]["key"] == expected_b


def test_compose_concatenates_subpolicies():
    steps = [
        {
            "type": "command",
            "key": "a",
            "cmd": "z",
            "builds_in": None,
            "cache": {
                "policy": "compose",
                "sub_policies": [
                    {"policy": "forever", "env_keys": []},
                    {"policy": "none"},
                ],
            },
        },
    ]
    out = resolve_pipeline_keys(
        steps,
        pipeline_org="default",
        pipeline_slug="default",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    forever_inner = _sha256_hex("z" + NUL + "")
    sub1 = "forever-" + forever_inner
    sub2 = "none"
    inner = _sha256_hex(sub1 + sub2)
    policy_res = "compose-" + inner
    expected = _sha256_hex(
        "default" + NUL + "default" + NUL + "a" + NUL + "scratch" + NUL + policy_res
    )
    assert out[0]["cache"]["key"] == expected


def test_parent_without_cache_is_planerror():
    steps = [
        {"type": "command", "key": "a", "cmd": "x", "builds_in": None},
        {
            "type": "command",
            "key": "b",
            "cmd": "y",
            "builds_in": "a",
            "cache": {"policy": "forever", "env_keys": []},
        },
    ]
    with pytest.raises(ValueError, match="builds_in 'a' which has no cached key"):
        resolve_pipeline_keys(
            steps,
            pipeline_org="default",
            pipeline_slug="default",
            now=0,
            base_path=Path("/tmp"),  # noqa: S108
            env={},
        )
