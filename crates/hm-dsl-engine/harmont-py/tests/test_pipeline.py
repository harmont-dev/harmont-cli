"""High-level pipeline-factory tests. Lowering details live in
test_pipeline_lowering.py; this file only covers the public factory."""

from __future__ import annotations

import pytest

from harmont import pipeline, scratch


def test_pipeline_returns_v2_dict():
    p = pipeline([scratch().sh("echo", label="echo")])
    assert p["version"] == "0"
    assert isinstance(p["graph"], dict)
    assert len(p["graph"]["nodes"]) == 1


def test_pipeline_factory_rejects_no_leaves():
    # `harmont.pipeline` (re-exported) is a polymorphic facade: no-arg
    # call routes to the @hm.pipeline decorator path. The factory's
    # "at least one leaf" guard is tested via the submodule directly.
    from harmont._pipeline import pipeline as _factory

    with pytest.raises(ValueError, match="at least one leaf"):
        _factory([])


def test_pipeline_rejects_legacy_single_step_form():
    # Pre-CLI-9 `pipeline(step)` must fail fast with the migration hint, not
    # silently route to the @hm.pipeline decorator and blow up downstream.
    step = scratch().sh("echo", label="echo")
    with pytest.raises(TypeError, match="single list of leaves") as exc:
        pipeline(step)
    assert "hm.pipeline([step])" in str(exc.value)


def test_pipeline_rejects_legacy_variadic_step_form():
    a = scratch().sh("a", label="a")
    b = scratch().sh("b", label="b")
    with pytest.raises(TypeError, match="single list of leaves") as exc:
        pipeline(a, b)
    assert "hm.pipeline([a, b])" in str(exc.value)


def test_pipeline_default_image_lowers_to_dict():
    p = pipeline(
        [scratch().sh("echo", label="a", image="ubuntu:24.04")],
        default_image="alpine:3.20",
    )
    assert p["default_image"] == "alpine:3.20"
    step = p["graph"]["nodes"][0]["step"]
    assert step["image"] == "ubuntu:24.04"
    assert step["label"] == "a"
