"""Cache-key resolver -- direct ports of the Scheme algorithm in
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


def _make_graph(nodes, edges=None):
    """Build a minimal graph dict for keygen tests."""
    return {
        "nodes": nodes,
        "node_holes": [],
        "edge_property": "directed",
        "edges": edges or [],
    }


def test_none_policy_emits_no_key():
    graph = _make_graph([
        {
            "step": {"key": "a", "cmd": "echo", "cache": {"policy": "none"}},
            "env": {},
        },
    ])
    out = resolve_pipeline_keys(
        graph,
        pipeline_org="default",
        pipeline_slug="default",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    assert "key" not in out["nodes"][0]["step"]["cache"]


def test_forever_policy_key_matches_scheme_formula():
    graph = _make_graph([
        {
            "step": {
                "key": "a",
                "cmd": "echo hi",
                "cache": {"policy": "forever", "env_keys": []},
            },
            "env": {},
        },
    ])
    out = resolve_pipeline_keys(
        graph,
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
    assert out["nodes"][0]["step"]["cache"]["key"] == expected


def test_ttl_policy_key_includes_bucket():
    graph = _make_graph([
        {
            "step": {
                "key": "a",
                "cmd": "x",
                "cache": {"policy": "ttl", "duration_seconds": 3600, "env_keys": []},
            },
            "env": {},
        },
    ])
    out = resolve_pipeline_keys(
        graph,
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
    assert out["nodes"][0]["step"]["cache"]["key"] == expected


def test_on_change_reads_file_contents():
    with tempfile.TemporaryDirectory() as d:
        f = Path(d) / "file.txt"
        f.write_bytes(b"hello")
        graph = _make_graph([
            {
                "step": {
                    "key": "a",
                    "cmd": "make",
                    "cache": {"policy": "on_change", "paths": ["file.txt"]},
                },
                "env": {},
            },
        ])
        out = resolve_pipeline_keys(
            graph,
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
        assert out["nodes"][0]["step"]["cache"]["key"] == expected


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

        graph = _make_graph([
            {
                "step": {
                    "key": "s",
                    "cmd": "make",
                    "cache": {"policy": "on_change", "paths": ["dir/"]},
                },
                "env": {},
            },
        ])
        out1 = resolve_pipeline_keys(
            graph,
            pipeline_org="default",
            pipeline_slug="default",
            now=0,
            base_path=root,
            env={},
        )
        key1 = out1["nodes"][0]["step"]["cache"]["key"]

        # Same tree -> same key.
        graph2 = _make_graph([
            {
                "step": {
                    "key": "s",
                    "cmd": "make",
                    "cache": {"policy": "on_change", "paths": ["dir/"]},
                },
                "env": {},
            },
        ])
        out_again = resolve_pipeline_keys(
            graph2,
            pipeline_org="default",
            pipeline_slug="default",
            now=0,
            base_path=root,
            env={},
        )
        assert out_again["nodes"][0]["step"]["cache"]["key"] == key1

        # Modify a file -> key changes.
        (sub / "a.txt").write_bytes(b"alpha2")
        graph3 = _make_graph([
            {
                "step": {
                    "key": "s",
                    "cmd": "make",
                    "cache": {"policy": "on_change", "paths": ["dir/"]},
                },
                "env": {},
            },
        ])
        out2 = resolve_pipeline_keys(
            graph3,
            pipeline_org="default",
            pipeline_slug="default",
            now=0,
            base_path=root,
            env={},
        )
        assert out2["nodes"][0]["step"]["cache"]["key"] != key1


def test_on_change_missing_path_raises():
    with tempfile.TemporaryDirectory() as d:
        graph = _make_graph([
            {
                "step": {
                    "key": "s",
                    "cmd": "make",
                    "cache": {"policy": "on_change", "paths": ["nope/"]},
                },
                "env": {},
            },
        ])
        with pytest.raises(FileNotFoundError, match="on_change path does not exist"):
            resolve_pipeline_keys(
                graph,
                pipeline_org="default",
                pipeline_slug="default",
                now=0,
                base_path=Path(d),
                env={},
            )


def test_env_keys_are_sorted_and_picked_up():
    graph = _make_graph([
        {
            "step": {
                "key": "a",
                "cmd": "echo",
                "cache": {"policy": "forever", "env_keys": ["BAR", "FOO"]},
            },
            "env": {},
        },
    ])
    out = resolve_pipeline_keys(
        graph,
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
    assert out["nodes"][0]["step"]["cache"]["key"] == expected


def test_parent_key_chains_through_resolved_cache_keys():
    graph = _make_graph(
        [
            {
                "step": {
                    "key": "a",
                    "cmd": "x",
                    "cache": {"policy": "forever", "env_keys": []},
                },
                "env": {},
            },
            {
                "step": {
                    "key": "b",
                    "cmd": "y",
                    "cache": {"policy": "forever", "env_keys": []},
                },
                "env": {},
            },
        ],
        edges=[[0, 1, "builds_in"]],
    )
    out = resolve_pipeline_keys(
        graph,
        pipeline_org="default",
        pipeline_slug="default",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    parent_key = out["nodes"][0]["step"]["cache"]["key"]
    inner_b = _sha256_hex("y" + NUL + "")
    policy_res = "forever-" + inner_b
    expected_b = _sha256_hex(
        "default" + NUL + "default" + NUL + "b" + NUL + parent_key + NUL + policy_res
    )
    assert out["nodes"][1]["step"]["cache"]["key"] == expected_b


def test_compose_concatenates_subpolicies():
    graph = _make_graph([
        {
            "step": {
                "key": "a",
                "cmd": "z",
                "cache": {
                    "policy": "compose",
                    "sub_policies": [
                        {"policy": "forever", "env_keys": []},
                        {"policy": "none"},
                    ],
                },
            },
            "env": {},
        },
    ])
    out = resolve_pipeline_keys(
        graph,
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
    assert out["nodes"][0]["step"]["cache"]["key"] == expected


def test_parent_without_cache_is_planerror():
    graph = _make_graph(
        [
            {
                "step": {"key": "a", "cmd": "x"},
                "env": {},
            },
            {
                "step": {
                    "key": "b",
                    "cmd": "y",
                    "cache": {"policy": "forever", "env_keys": []},
                },
                "env": {},
            },
        ],
        edges=[[0, 1, "builds_in"]],
    )
    with pytest.raises(ValueError, match="builds_in 'a' which has no cached key"):
        resolve_pipeline_keys(
            graph,
            pipeline_org="default",
            pipeline_slug="default",
            now=0,
            base_path=Path("/tmp"),  # noqa: S108
            env={},
        )
