"""Tests for the small surviving validator set."""

from __future__ import annotations

import pytest

from harmont._validation import validate_positive_int


def test_positive_int_accepts_none():
    validate_positive_int(None, "f", "C")


def test_positive_int_accepts_one():
    validate_positive_int(1, "f", "C")


def test_positive_int_rejects_zero():
    with pytest.raises(ValueError, match="positive integer"):
        validate_positive_int(0, "f", "C")


def test_positive_int_rejects_negative():
    with pytest.raises(ValueError, match="positive integer"):
        validate_positive_int(-3, "f", "C")


def test_positive_int_rejects_non_int():
    with pytest.raises(ValueError, match="positive integer"):
        validate_positive_int("12", "f", "C")  # type: ignore[arg-type]
