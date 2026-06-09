"""Target[T] and BaseImage(X) annotation markers (HAR-28 follow-up)."""

from __future__ import annotations

import typing
from typing import Annotated, get_args, get_type_hints

import pytest

import harmont as hm
from harmont._step import Step
from harmont._typing import _TARGET_MARKER, BaseImage, Target, _BaseImageMarker


def test_target_subscript_returns_annotated_with_marker():
    annot = Target[Step]
    assert typing.get_origin(annot) is not None  # Annotated[Step, ...]
    args = get_args(annot)
    assert args[0] is Step
    assert _TARGET_MARKER in args[1:]


def test_target_with_different_types():
    from harmont._rust import RustProject

    annot = Target[RustProject]
    args = get_args(annot)
    assert args[0] is RustProject


def test_target_used_as_param_annotation_resolves_via_get_type_hints():
    def fn(api: Target[Step]) -> Step:  # type: ignore[empty-body]
        ...

    hints = get_type_hints(fn, include_extras=True)
    annot = hints["api"]
    args = get_args(annot)
    assert args[0] is Step
    assert _TARGET_MARKER in args[1:]


def test_base_image_returns_marker_instance():
    marker = BaseImage("ubuntu-24.04")
    assert isinstance(marker, _BaseImageMarker)
    assert marker.image == "ubuntu-24.04"


def test_base_image_in_annotated_metadata():
    def fn(base: Annotated[Step, BaseImage("ubuntu-24.04")]) -> Step:  # type: ignore[empty-body]
        ...

    hints = get_type_hints(fn, include_extras=True)
    annot = hints["base"]
    args = get_args(annot)
    assert args[0] is Step
    markers = [a for a in args[1:] if isinstance(a, _BaseImageMarker)]
    assert len(markers) == 1
    assert markers[0].image == "ubuntu-24.04"


def test_base_image_rejects_empty_string():
    with pytest.raises(TypeError, match="hm: BaseImage\\(\\.\\.\\.\\) takes a non-empty image"):
        BaseImage("")


def test_base_image_rejects_non_string():
    with pytest.raises(TypeError, match="hm: BaseImage\\(\\.\\.\\.\\) takes a non-empty image"):
        BaseImage(42)  # type: ignore[arg-type]


def test_target_and_base_image_are_exported_from_harmont():
    assert hm.Target is Target
    assert hm.BaseImage is BaseImage
    assert "Target" in hm.__all__
    assert "BaseImage" in hm.__all__
