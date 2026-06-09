# tests/test_duration.py
from datetime import timedelta

import pytest

from harmont._duration import parse_duration


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("30s", 30),
        ("5m", 300),
        ("1h", 3600),
        ("1h30m", 5400),
        ("2h15m30s", 8130),
        (45, 45),
        (timedelta(minutes=2), 120),
    ],
)
def test_parse_duration_ok(value, expected):
    assert parse_duration(value) == expected


@pytest.mark.parametrize("bad", ["", "30", "30 s", "1d", "m", "-5s", "0s", 0, -3, True])
def test_parse_duration_rejects(bad):
    with pytest.raises((ValueError, TypeError)):
        parse_duration(bad)
