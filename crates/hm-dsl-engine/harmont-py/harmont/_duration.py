# harmont/_duration.py
"""Parse a human duration to a positive integer number of seconds.

Used by ``hm.timeout`` and ``hm.pipeline(timeout=...)``. Accepts a
Go-style string (``"30s"``, ``"5m"``, ``"1h30m"``), an ``int`` number of
seconds, or a ``datetime.timedelta``.
"""

from __future__ import annotations

import re
from datetime import timedelta

# A non-empty run of <digits><unit> segments, units hours/minutes/seconds.
_DURATION_RE = re.compile(r"(?:\d+[hms])+")
_SEGMENT_RE = re.compile(r"(\d+)([hms])")
_UNIT_SECONDS = {"h": 3600, "m": 60, "s": 1}


def parse_duration(value: str | int | timedelta) -> int:
    """Normalize ``value`` to a positive integer number of seconds.

    Args:
        value: ``"30s"`` / ``"5m"`` / ``"1h30m"`` (units ``h``, ``m``, ``s``),
            an ``int`` count of seconds, or a ``timedelta``.

    Returns:
        The duration in whole seconds (always ``> 0``).

    Raises:
        TypeError: If ``value`` is not a str, int, or timedelta.
        ValueError: If the string is malformed or the duration is not positive.
    """
    # bool is an int subclass; reject it so timeout(True, ...) is an error.
    if isinstance(value, bool):
        msg = f"hm: timeout duration must be a str, int, or timedelta — got {value!r}"
        raise TypeError(msg)
    if isinstance(value, timedelta):
        seconds = int(value.total_seconds())
    elif isinstance(value, int):
        seconds = value
    elif isinstance(value, str):
        seconds = _parse_str(value)
    else:
        msg = (
            f"hm: timeout duration must be a str, int, or timedelta — "
            f"got {type(value).__name__}"
        )
        raise TypeError(msg)

    if seconds <= 0:
        msg = (
            f"hm: timeout duration must be positive — got {value!r}\n"
            f'  → use a value like "30s" or "5m"'
        )
        raise ValueError(msg)
    return seconds


def _parse_str(text: str) -> int:
    stripped = text.strip()
    if not _DURATION_RE.fullmatch(stripped):
        msg = (
            f"hm: invalid timeout duration {text!r}\n"
            f'  → use a Go-style duration like "30s", "5m", or "1h30m" '
            f"(units: h, m, s)"
        )
        raise ValueError(msg)
    return sum(
        int(n) * _UNIT_SECONDS[unit] for n, unit in _SEGMENT_RE.findall(stripped)
    )
