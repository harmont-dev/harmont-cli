"""Annotation markers for fixture-style target parameters (HAR-28).

Two markers are public surface:

  Target[T]      — declares a dependency on a registered target by
                   parameter name. The resolved value is typed `T`
                   (whatever the target returns — `Step`,
                   `HaskellPackage`, `ElmProject`, etc.).

  BaseImage(X)   — used in ``Annotated[Step, BaseImage("X")]``. Declares
                   a scratch-rooted Step in image "X" as the parameter
                   value. The first ``.sh()`` call on the parameter
                   inherits ``image="X"``, so the first emitted IR step
                   carries it in the v0 wire format.

Both surface as PEP 593 ``Annotated[...]`` so static type-checkers see
the concrete type (``Step``, ``HaskellPackage``, etc.) while the runtime
decorator reads the marker from ``typing.get_type_hints(include_extras=True)``.

Examples:

    @hm.target()
    def venv(apt_base: hm.Target[hm.Step]) -> hm.Step:
        # mypy/pyright: apt_base is Step. assert_type passes.
        return apt_base.sh("python3 -m venv .venv")

    @hm.target()
    def apt_base(
        base: Annotated[hm.Step, hm.BaseImage("ubuntu-24.04")],
    ) -> hm.Step:
        # mypy/pyright: base is Step. assert_type passes.
        return base.sh("apt-get update")

The callable ``BaseImage(...)`` form is preferred over the older
``BaseImage["..."]`` subscript form because type checkers parse the
hyphenated image string as arithmetic in subscript position.
"""

from __future__ import annotations

from typing import Annotated, TypeVar

T = TypeVar("T")


class _TargetMarker:
    """Sentinel class for Annotated metadata. The module-level
    instance ``_TARGET_MARKER`` is the actual sentinel value."""

    __slots__ = ()

    def __repr__(self) -> str:
        return "<hm.Target marker>"


_TARGET_MARKER = _TargetMarker()


# Annotated with a TypeVar produces a generic alias; subscripting
# ``Target[Step]`` resolves to ``Annotated[Step, _TARGET_MARKER]``.
Target = Annotated[T, _TARGET_MARKER]


class _BaseImageMarker:
    """Metadata holder for the BaseImage("...") annotation."""

    __slots__ = ("image",)

    def __init__(self, image: str) -> None:
        self.image = image

    def __repr__(self) -> str:
        return f"<hm.BaseImage({self.image!r}) marker>"

    def __eq__(self, other: object) -> bool:
        return isinstance(other, _BaseImageMarker) and self.image == other.image

    def __hash__(self) -> int:
        return hash(("_BaseImageMarker", self.image))


def BaseImage(image: str) -> _BaseImageMarker:  # noqa: N802 — factory mimicking a type
    """Annotation metadata factory. Use as
    ``Annotated[Step, BaseImage("ubuntu-24.04")]``.

    The decorator injects a ``Step(image="ubuntu-24.04")`` (a scratch
    root with the image set) as the parameter value. The first
    ``.sh(...)`` call on it inherits the image so the first emitted
    IR step carries ``image="ubuntu-24.04"`` in the v0 wire format.
    """
    if not isinstance(image, str) or not image:
        msg = (
            'hm: BaseImage(...) takes a non-empty image string\n  → e.g. BaseImage("ubuntu-24.04")'
        )
        raise TypeError(msg)
    return _BaseImageMarker(image)


class _DepMarker:
    """Sentinel class for Annotated metadata. Marks a parameter as a
    dependency on another @hm.deploy by parameter name; the injected
    value is the resolved Deployment. The module-level instance
    ``_DEP_MARKER`` is the actual sentinel value embedded in
    ``Annotated[T, _DEP_MARKER]`` by the ``Dep`` alias.
    """

    __slots__ = ()

    def __repr__(self) -> str:
        return "<hm.Dep marker>"


_DEP_MARKER = _DepMarker()


# hm.Dep[Deployment] (or a concrete subclass) -> Annotated[T, _DEP_MARKER].
Dep = Annotated[T, _DEP_MARKER]
