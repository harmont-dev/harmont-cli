"""Zig toolchain abstraction.

Chain: scratch -> apt-base (curl, xz-utils, ca-certificates) -> zig-install
(download tarball from ziglang.org, extract to /usr/local/zig) -> action
leaves.

Two entry shapes:

  hm.zig(path=".")                # one-shot: returns ZigProject directly
  hm.zig()                        # multi-project: returns ZigToolchain
  tc.project(path="lib-a")        # spawn one ZigProject per subdir

The toolchain form holds the shared zig-install Step. Two .project()
calls reuse it, so the emitted v0 IR contains a single :zig: install
node with N project chains fanning out from it.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, overload

from ._toolchain import make_install_chain
from .cache import CacheForever

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("curl", "ca-certificates", "xz-utils")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")


def _zig_install_cmd(version: str) -> str:
    tarball = f"zig-linux-x86_64-{version}.tar.xz"
    url = f"https://ziglang.org/download/{version}/{tarball}"
    return (
        f"curl -fsSL {url} -o /tmp/zig.tar.xz && "
        "rm -rf /usr/local/zig && mkdir -p /usr/local/zig && "
        "tar -xJf /tmp/zig.tar.xz -C /usr/local/zig --strip-components=1 && "
        "ln -sf /usr/local/zig/zig /usr/local/bin/zig && zig version"
    )


@dataclass(frozen=True)
class ZigProject:
    path: str
    installed: Step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def build(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && zig build",
            f":zig: {self.path} build",
            **kw,
        )

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && zig build test",
            f":zig: {self.path} test",
            **kw,
        )

    def fmt(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && zig fmt --check .",
            f":zig: {self.path} fmt",
            **kw,
        )


@dataclass(frozen=True)
class ZigToolchain:
    """Constructed via :func:`zig` when no ``path`` is supplied.

    Holds the shared zig-install Step. Spawn one :class:`ZigProject`
    per subdir via :meth:`project`; all projects from one toolchain
    share the same install Step, so the emitted IR contains a single
    :zig: install node fanned out to N project chains.
    """

    version: str
    installed: Step

    def project(self, path: str = ".") -> ZigProject:
        return ZigProject(path=path, installed=self.installed)


def _make_toolchain(
    *,
    version: str,
    image: str | None,
    base: Step | None,
) -> ZigToolchain:
    if not _VERSION_RE.match(version):
        msg = f'hm.zig: invalid version {version!r}\n  → use a Zig version like "0.13.0"'
        raise ValueError(msg)
    installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=_zig_install_cmd(version),
        install_cache=CacheForever(env_keys=()),
        lang_tag="zig",
        install_tag="install",
        image=image,
        base=base,
    )
    return ZigToolchain(version=version, installed=installed)


class _ZigEntry:
    """Callable singleton — supports object form, toolchain form, and bare form."""

    @overload
    def __call__(
        self,
        *,
        version: str = ...,
        image: str | None = ...,
        base: Step | None = ...,
    ) -> ZigToolchain: ...

    @overload
    def __call__(
        self,
        *,
        path: str,
        version: str = ...,
        image: str | None = ...,
        base: Step | None = ...,
    ) -> ZigProject: ...

    def __call__(
        self,
        *,
        path: str | None = None,
        version: str = "0.13.0",
        image: str | None = None,
        base: Step | None = None,
    ) -> ZigToolchain | ZigProject:
        toolchain = _make_toolchain(version=version, image=image, base=base)
        if path is None:
            return toolchain
        return toolchain.project(path)

    def _project(self, **kw: Any) -> ZigProject:
        path = kw.pop("path", ".")
        proj = self(path=path, **kw)
        assert isinstance(proj, ZigProject)  # noqa: S101 — narrow overload result
        return proj

    def build(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).build(**action_kw)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).test(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).fmt(**action_kw)


zig = _ZigEntry()
