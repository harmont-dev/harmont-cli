"""Elm project abstraction (HAR-15).

Public surface lives on the module-level singleton :data:`elm`. Call it
to construct an :class:`ElmProject`, or use the bare-form action
methods (``elm.make(...)``, ``elm.test()``, etc.) for a one-shot leaf.

Chain shape: scratch -> apt-base -> nodesource node install -> elm
binary download -> action leaves. Node is required because elm-test,
elm-review, and elm-format all run under npx.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain, node_install_cmd
from .cache import CacheForever

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("curl", "ca-certificates")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_VERSION_RE = re.compile(r"^[0-9]+(\.[0-9]+)+$")


def _elm_install_cmd(elm_version: str) -> str:
    return (
        f"curl -fsSL https://github.com/elm/compiler/releases/download/"
        f"{elm_version}/binary-for-linux-64-bit.gz -o /tmp/elm.gz && "
        "gunzip /tmp/elm.gz && chmod +x /tmp/elm && "
        "mv /tmp/elm /usr/local/bin/elm"
    )


@dataclass(frozen=True)
class ElmProject:
    """Constructed via :func:`elm` (the ``hm.elm`` singleton)."""

    path: str
    installed: Step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def make(self, target: str, *, output: str | None = None, **kw: Any) -> Step:
        suffix = f" --output={output}" if output is not None else ""
        return self._emit(
            f"cd {self.path} && elm make {target}{suffix}",
            f":elm: make {target}",
            **kw,
        )

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && npx --yes elm-test",
            ":elm: test",
            **kw,
        )

    def review(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && npx --yes elm-review",
            ":elm: review",
            **kw,
        )

    def fmt(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && npx --yes elm-format --validate .",
            ":elm: fmt",
            **kw,
        )


def _make_elm(
    *,
    path: str = ".",
    elm_version: str = "0.19.1",
    node_version: str = "20",
    image: str | None = None,
    base: Step | None = None,
) -> ElmProject:
    if not _VERSION_RE.match(elm_version):
        msg = f'hm.elm: invalid elm_version {elm_version!r}\n  → e.g. elm_version="0.19.1"'
        raise ValueError(msg)
    node_installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=node_install_cmd(node_version),
        install_cache=CacheForever(env_keys=()),
        lang_tag="elm",
        install_tag="node",
        image=image,
        base=base,
    )
    elm_installed = node_installed.sh(
        _elm_install_cmd(elm_version),
        label=":elm: install",
        cache=CacheForever(env_keys=()),
    )
    return ElmProject(path=path, installed=elm_installed)


class _ElmEntry:
    """Callable singleton — supports both object form and bare form."""

    def __call__(
        self,
        *,
        path: str = ".",
        elm_version: str = "0.19.1",
        node_version: str = "20",
        image: str | None = None,
        base: Step | None = None,
    ) -> ElmProject:
        return _make_elm(
            path=path,
            elm_version=elm_version,
            node_version=node_version,
            image=image,
            base=base,
        )

    def make(self, target: str, *, output: str | None = None, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).make(target, output=output, **action_kw)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def review(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).review(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).fmt(**action_kw)


elm = _ElmEntry()
