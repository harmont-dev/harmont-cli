"""Step.sh chain method + cwd= kwarg (HAR-28)."""
from __future__ import annotations

from harmont._step import scratch
from harmont.cache import CacheNone


def test_sh_links_parent_and_sets_cmd():
    parent = scratch()
    child = parent.sh("echo hi")
    assert child.parent is parent
    assert child.cmd == "echo hi"
    assert child.is_wait is False


def test_sh_carries_all_kwargs():
    s = scratch().sh(
        "make",
        label="build",
        cache=CacheNone(),
        env={"CI": "true"},
        timeout_seconds=600,
        key="explicit-key",
    )
    assert s.label == "build"
    assert s.cache == CacheNone()
    assert s.env == {"CI": "true"}
    assert s.timeout_seconds == 600
    assert s.key_override == "explicit-key"


def test_sh_cwd_prepends_cd():
    s = scratch().sh("pytest -v", cwd="cidsl/py")
    assert s.cmd == "cd cidsl/py && pytest -v"


def test_sh_cwd_none_leaves_cmd_unchanged():
    s = scratch().sh("echo hi", cwd=None)
    assert s.cmd == "echo hi"


def test_sh_cwd_empty_string_is_rejected():
    import pytest

    with pytest.raises(ValueError, match="hm: cwd must be a non-empty path"):
        scratch().sh("echo", cwd="")


def test_sh_inherits_image_from_scratch_parent():
    """A scratch root with image= set propagates to its first .sh() child."""
    from harmont._step import Step

    root = Step(image="ubuntu-24.04")  # scratch with image
    child = root.sh("apt-get update")
    assert child.image == "ubuntu-24.04"


def test_sh_image_inheritance_does_not_apply_to_grandchildren():
    """The inheritance is narrow: only scratch → first child. Subsequent
    .sh() calls don't inherit from a non-scratch parent."""
    from harmont._step import Step

    root = Step(image="ubuntu-24.04")
    first = root.sh("a")
    second = first.sh("b")
    assert first.image == "ubuntu-24.04"
    assert second.image is None  # parent has cmd, doesn't propagate


def test_sh_explicit_image_overrides_scratch_inheritance():
    """If the caller passes image= explicitly, it wins over inheritance."""
    from harmont._step import Step

    root = Step(image="ubuntu-24.04")
    child = root.sh("a", image="alpine:3.20")
    assert child.image == "alpine:3.20"


def test_sh_scratch_without_image_remains_none():
    """The existing scratch().sh() pattern is unchanged."""
    from harmont._step import scratch

    s = scratch().sh("echo")
    assert s.image is None


