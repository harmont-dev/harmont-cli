"""Coerce toolchain return values to ``tuple[Step, ...]`` (HAR-28).

Used by ``@hm.target`` and by the envelope renderer when a pipeline's
return value carries language-toolchain objects instead of bare Steps.
Each toolchain has one unambiguous default action:

  HaskellPackage  -> .build()
  RustToolchain   -> .build()
  NpmProject      -> .install()   (the npm-ci leaf - verifies deps)
  ElmProject      -> .make("src/Main.elm")

Authors who want a different default call the explicit action method.
"""

from __future__ import annotations

from ._step import Step
from .elm import ElmProject
from .haskell import HaskellPackage
from .npm import NpmProject
from .py.uv import UvProject
from .rust import RustProject, RustToolchain


def _one(obj: object) -> tuple[Step, ...]:
    if isinstance(obj, Step):
        return (obj,)
    if isinstance(obj, RustProject):
        return (obj.test(), obj.clippy(), obj.fmt())
    if isinstance(obj, HaskellPackage):
        return (obj.build(),)
    if isinstance(obj, RustToolchain):
        return (obj.build(),)
    if isinstance(obj, NpmProject):
        return (obj.install(),)
    if isinstance(obj, ElmProject):
        return (obj.make("src/Main.elm"),)
    if isinstance(obj, UvProject):
        return (obj.test(),)
    if isinstance(obj, (tuple, list)):
        return as_leaves(obj)
    msg = (
        f"hm.target: cannot use {type(obj).__name__} as a pipeline leaf\n"
        "  → return one of: Step, tuple[Step, ...], RustProject, RustToolchain, "
        "HaskellPackage, NpmProject, ElmProject, UvProject"
    )
    raise TypeError(msg)


def as_leaves(obj: object) -> tuple[Step, ...]:
    """Flatten ``obj`` into a tuple of leaf Steps.

    Recursive on tuples/lists. See module docstring for default-leaf
    rules per toolchain wrapper.
    """
    if isinstance(obj, (tuple, list)):
        out: list[Step] = []
        for item in obj:
            out.extend(_one(item))
        return tuple(out)
    return _one(obj)
