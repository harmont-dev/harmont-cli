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


def test_imageless_root_gets_ubuntu_default():
    p = pipeline([scratch().sh("echo hi", label="a")])
    nodes = p["graph"]["nodes"]
    assert nodes[0]["step"]["image"] == "ubuntu:24.04"
    # No top-level default_image key is emitted anymore.
    assert "default_image" not in p


def test_explicit_root_image_is_preserved():
    p = pipeline([scratch().sh("echo hi", label="a", image="alpine:3.20")])
    assert p["graph"]["nodes"][0]["step"]["image"] == "alpine:3.20"


def test_child_step_stays_imageless():
    root = scratch().sh("echo p", label="p")
    child = root.sh("echo c", label="c")
    p = pipeline([child])
    nodes = {n["step"]["key"]: n["step"] for n in p["graph"]["nodes"]}
    # parent (root) gets the default; child boots from parent snapshot.
    assert "image" not in nodes["c"]
