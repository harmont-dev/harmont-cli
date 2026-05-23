"""resolve_deps signature introspection + cycle detection (HAR-28 follow-up).

Most behavioral coverage of the marker-driven resolver lives in
``test_strict_signature.py``. This file is now the residual set:
signature-kind rejection (``*args`` / ``**kwargs`` / positional-only),
default-value handling on plain params, and cycle detection.
"""
from __future__ import annotations

import pytest

import harmont as hm  # noqa: TC001  used in annotations + tests subscript at runtime
from harmont._deps import (
    call_with_deps,
    clear_target_names,
    register_named_target,
    resolve_deps,
)
from harmont._step import Step  # noqa: TC001  used in annotations + isinstance checks


@pytest.fixture(autouse=True)
def _reset_named_targets():
    clear_target_names()
    yield
    clear_target_names()


def test_zero_param_fn_resolves_to_empty_kwargs():
    def fn() -> None: ...

    assert resolve_deps(fn) == {}


def test_default_used_when_param_has_no_marker():
    def fn(missing: str = "default") -> None: ...

    assert resolve_deps(fn) == {"missing": "default"}


def test_var_args_rejected():
    def fn(*args) -> None: ...

    with pytest.raises(TypeError, match="hm: target functions cannot take \\*args"):
        resolve_deps(fn)


def test_var_kwargs_rejected():
    def fn(**kwargs) -> None: ...

    with pytest.raises(TypeError, match="hm: target functions cannot take \\*\\*kwargs"):
        resolve_deps(fn)


def test_positional_only_param_rejected():
    def fn(x, /) -> None: ...

    with pytest.raises(TypeError, match="hm: target functions cannot have positional-only"):
        resolve_deps(fn)


def test_cycle_detection_two_targets():
    # a depends on b, b depends on a. Resolving either must raise.
    a_calls = b_calls = 0

    def a(b: hm.Target[Step]) -> str:
        nonlocal a_calls
        a_calls += 1
        return f"a({b})"

    def b(a: hm.Target[Step]) -> str:
        nonlocal b_calls
        b_calls += 1
        return f"b({a})"

    register_named_target("a", lambda: call_with_deps(a))
    register_named_target("b", lambda: call_with_deps(b))

    with pytest.raises(RuntimeError, match="hm: dependency cycle"):
        call_with_deps(a)
