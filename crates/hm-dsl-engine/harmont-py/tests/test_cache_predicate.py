"""Tests for the ``predicate`` cache policy."""

from __future__ import annotations

import hashlib
from pathlib import Path

import harmont as hm
from harmont._pipeline import _cache_to_dict
from harmont.cache import CachePredicate
from harmont.keygen import resolve_pipeline_keys

NUL = "\x00"


def _sha256_hex(s: str) -> str:
    return hashlib.sha256(s.encode("utf-8")).hexdigest()


def _make_graph(nodes, edges=None):
    return {
        "nodes": nodes,
        "node_holes": [],
        "edge_property": "directed",
        "edges": edges or [],
    }


# -- factory -----------------------------------------------------------------


def test_predicate_factory_returns_cache_predicate():
    policy = hm.predicate(lambda: "v1")
    assert isinstance(policy, CachePredicate)


# -- lowering ----------------------------------------------------------------


def test_predicate_lowering_calls_fn_and_produces_dict():
    policy = hm.predicate(lambda: "v1")
    d = _cache_to_dict(policy)
    assert d == {"policy": "predicate", "value": "v1"}


def test_predicate_lowering_stringifies_return_value():
    policy = hm.predicate(lambda: 42)
    d = _cache_to_dict(policy)
    assert d == {"policy": "predicate", "value": "42"}


def test_predicate_fn_is_called_at_lowering_time():
    calls: list[int] = []

    def counter():
        calls.append(1)
        return "x"

    policy = hm.predicate(counter)
    assert len(calls) == 0
    _cache_to_dict(policy)
    assert len(calls) == 1


# -- keygen ------------------------------------------------------------------


def test_predicate_keygen_produces_deterministic_key():
    graph = _make_graph(
        [
            {
                "step": {
                    "key": "a",
                    "cmd": "echo",
                    "cache": {"policy": "predicate", "value": "v1"},
                },
                "env": {},
            },
        ]
    )
    out = resolve_pipeline_keys(
        graph,
        pipeline_org="default",
        pipeline_slug="default",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    policy_res = "predicate-" + _sha256_hex("v1")
    expected = _sha256_hex(
        "default" + NUL + "default" + NUL + "a" + NUL + "scratch" + NUL + policy_res
    )
    assert out["nodes"][0]["step"]["cache"]["key"] == expected


def test_different_predicate_values_produce_different_keys():
    def make_graph(value):
        return _make_graph(
            [
                {
                    "step": {
                        "key": "a",
                        "cmd": "echo",
                        "cache": {"policy": "predicate", "value": value},
                    },
                    "env": {},
                },
            ]
        )

    g1 = make_graph("v1")
    g2 = make_graph("v2")
    resolve_pipeline_keys(
        g1,
        pipeline_org="o",
        pipeline_slug="s",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    resolve_pipeline_keys(
        g2,
        pipeline_org="o",
        pipeline_slug="s",
        now=0,
        base_path=Path("/tmp"),  # noqa: S108
        env={},
    )
    assert g1["nodes"][0]["step"]["cache"]["key"] != g2["nodes"][0]["step"]["cache"]["key"]
