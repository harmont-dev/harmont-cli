"""Validators used by the chain DSL. Kept tiny on purpose."""

from __future__ import annotations


def validate_positive_int(value: int | None, field_name: str, container_name: str) -> None:
    if value is None:
        return
    if not isinstance(value, int) or value < 1:
        msg = f"{container_name}.{field_name} must be a positive integer; got {value!r}"
        raise ValueError(msg)
