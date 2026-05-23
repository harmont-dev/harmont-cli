"""hm.dev.port() sentinel: equality, repr, and structural use."""
from __future__ import annotations

from harmont.dev import port


def test_port_returns_sentinel_singleton():
    a = port()
    b = port()
    assert a is b               # singleton — equality-by-identity is fine
    assert a == b


def test_port_repr_is_stable_and_introspectable():
    assert repr(port()) == "<hm.dev.port>"


def test_port_is_hashable():
    # frozen LocalDeployment uses port_mapping values inside a Mapping;
    # being hashable means user code can put it in sets / tuple keys
    # without surprise.
    assert {port(): 1}[port()] == 1
