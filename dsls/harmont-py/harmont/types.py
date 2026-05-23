"""Type aliases for the chain DSL."""

from __future__ import annotations

from typing import TYPE_CHECKING, Union

if TYPE_CHECKING:
    from ._step import Step

EnvVars = dict[str, str]

Pipeline = Union["Step", "tuple[Step, ...]"]
