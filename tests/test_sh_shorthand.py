"""hm.sh top-level shorthand (HAR-28)."""
from __future__ import annotations

import harmont as hm
from harmont.cache import CacheNone


def test_hm_sh_returns_step_rooted_at_scratch():
    s = hm.sh("apt-get update")
    assert isinstance(s, hm.Step)
    assert s.parent is not None
    assert s.parent.cmd is None
    assert s.parent.parent is None
    assert s.cmd == "apt-get update"


def test_hm_sh_chains_with_sh():
    s = hm.sh("apt-get update").sh("apt-get install -y python3")
    assert s.cmd == "apt-get install -y python3"
    assert s.parent is not None
    assert s.parent.cmd == "apt-get update"


def test_hm_sh_accepts_all_step_sh_kwargs():
    s = hm.sh(
        "make",
        label="build",
        cache=CacheNone(),
        env={"CI": "true"},
        timeout_seconds=600,
        image="alpine:3.20",
        key="explicit",
    )
    assert s.label == "build"
    assert s.cache == CacheNone()
    assert s.env == {"CI": "true"}
    assert s.timeout_seconds == 600
    assert s.image == "alpine:3.20"
    assert s.key_override == "explicit"


def test_hm_sh_cwd_kwarg():
    s = hm.sh("pytest -v", cwd="cidsl/py")
    assert s.cmd == "cd cidsl/py && pytest -v"
