# tests/test_timeout.py
from datetime import timedelta

import pytest

import harmont as hm


def test_timeout_sets_seconds_on_step():
    step = hm.timeout("30s", hm.sh("echo foo"))
    assert step.timeout_seconds == 30
    assert step.cmd == "echo foo"


def test_timeout_accepts_int_and_timedelta():
    assert hm.timeout(45, hm.sh("x")).timeout_seconds == 45
    assert hm.timeout(timedelta(minutes=2), hm.sh("x")).timeout_seconds == 120


def test_timeout_is_immutable_and_overrides():
    base = hm.sh("x")
    wrapped = hm.timeout("5m", base)
    assert base.timeout_seconds is None  # original untouched
    assert hm.timeout("1m", wrapped).timeout_seconds == 60  # last wins


def test_timeout_rejects_wait_barrier():
    with pytest.raises(ValueError, match="wait"):
        hm.timeout("30s", hm.wait())
