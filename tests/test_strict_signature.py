"""Strict signature validation + Annotated-marker dispatch (HAR-28 follow-up)."""
from __future__ import annotations

from typing import Annotated

import pytest

import harmont as hm
from harmont._deps import (
    clear_target_names,
    register_named_target,
    resolve_deps,
    validate_target_signature,
)
from harmont._step import Step


@pytest.fixture(autouse=True)
def _reset():
    clear_target_names()
    yield
    clear_target_names()


def test_target_marker_resolves_via_registry():
    register_named_target("apt_base", lambda: Step(cmd="apt-get update"))

    def fn(apt_base: hm.Target[Step]) -> Step:  # type: ignore[empty-body]
        ...

    kwargs = resolve_deps(fn)
    assert isinstance(kwargs["apt_base"], Step)
    assert kwargs["apt_base"].cmd == "apt-get update"


def test_target_marker_missing_target_raises():
    def fn(missing: hm.Target[Step]) -> Step:  # type: ignore[empty-body]
        ...

    with pytest.raises(TypeError, match="hm: target 'missing' not found"):
        resolve_deps(fn)


def test_base_image_marker_injects_scratch_step_with_image():
    def fn(base: Annotated[Step, hm.BaseImage("ubuntu-24.04")]) -> Step:  # type: ignore[empty-body]
        ...

    kwargs = resolve_deps(fn)
    base = kwargs["base"]
    assert isinstance(base, Step)
    assert base.parent is None
    assert base.cmd is None
    assert base.image == "ubuntu-24.04"


def test_base_image_then_sh_emits_step_with_image():
    """End-to-end: BaseImage param → .sh() → first emitted cmd step carries image."""
    def fn(base: Annotated[Step, hm.BaseImage("ubuntu-24.04")]) -> Step:  # type: ignore[empty-body]
        ...

    base = resolve_deps(fn)["base"]
    first = base.sh("apt-get update")
    assert first.image == "ubuntu-24.04"


def test_unannotated_param_with_no_default_is_strict_error():
    def fn(x) -> Step:  # type: ignore[empty-body, no-untyped-def]
        ...

    with pytest.raises(TypeError, match="hm: parameter 'x' has no marker"):
        validate_target_signature(fn)


def test_plain_annotation_no_marker_no_default_is_strict_error():
    def fn(x: int) -> Step:  # type: ignore[empty-body]
        ...

    with pytest.raises(TypeError, match="hm: parameter 'x' has no marker"):
        validate_target_signature(fn)


def test_plain_param_with_default_is_allowed():
    def fn(image_tag: str = "ubuntu:24.04") -> Step:  # type: ignore[empty-body]
        ...

    validate_target_signature(fn)  # no raise
    assert resolve_deps(fn) == {"image_tag": "ubuntu:24.04"}


def test_validate_signature_rejects_var_args():
    def fn(*args) -> Step:  # type: ignore[empty-body, no-untyped-def]
        ...

    with pytest.raises(TypeError, match="hm: target functions cannot take \\*args"):
        validate_target_signature(fn)


def test_validate_signature_rejects_var_kwargs():
    def fn(**kwargs) -> Step:  # type: ignore[empty-body, no-untyped-def]
        ...

    with pytest.raises(TypeError, match="hm: target functions cannot take \\*\\*kwargs"):
        validate_target_signature(fn)


def test_validate_signature_rejects_positional_only():
    def fn(x: hm.Target[Step], /) -> Step:  # type: ignore[empty-body]
        ...

    with pytest.raises(TypeError, match="hm: target functions cannot have positional-only"):
        validate_target_signature(fn)


def test_zero_param_fn_is_valid():
    def fn() -> Step:  # type: ignore[empty-body]
        ...

    validate_target_signature(fn)  # no raise
    assert resolve_deps(fn) == {}


def test_target_marker_strict_no_default_fallback():
    """Even with a default, Target marker requires the target to exist."""
    def fn(api: hm.Target[Step] = None) -> Step:  # type: ignore[assignment,empty-body]
        ...

    # The default annotation is parsed but Target is strict — must resolve.
    with pytest.raises(TypeError, match="hm: target 'api' not found"):
        resolve_deps(fn)
