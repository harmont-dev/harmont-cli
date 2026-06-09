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


_NEW_URL_FORMAT_VERSION = (0, 14, 1)


def _parse_version(version: str) -> tuple[int, ...]:
    return tuple(int(p) for p in version.split("."))


def _zig_install_cmd(version: str) -> str:
    if _parse_version(version) >= _NEW_URL_FORMAT_VERSION:
        tarball = f"zig-x86_64-linux-{version}.tar.xz"
    else:
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
    """Zig project rooted on a specific path — constructed via ``hm.zig(path=...)``.

    ``installed`` is the zig-install step. Action methods (``build``,
    ``test``, ``fmt``) attach leaves to ``installed``.
    """

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
    """Zig toolchain install chain — constructed via ``hm.zig()`` with no ``path``.

    Holds the shared zig-install step. Spawn one ``ZigProject`` per
    subdirectory via ``.project(path)``; all projects from one toolchain
    share the same install step, so the emitted IR contains a single
    ``:zig: install`` node fanned out to N project chains.
    """

    version: str
    installed: Step

    def project(self, path: str = ".") -> ZigProject:
        """Create a ``ZigProject`` rooted at ``path`` from this toolchain.

        Args:
            path: Path to the Zig project root relative to the workspace.

        Returns:
            A ``ZigProject`` whose ``installed`` step is shared with this
            toolchain.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.zig(version="0.14.1")
            >>> lib = tc.project("lib-a")
            >>> app = tc.project("app")
            >>> hm.pipeline([lib.test(), app.test()])
        """
        return ZigProject(path=path, installed=self.installed)


def _make_toolchain(
    *,
    version: str,
    image: str | None,
    base: Step | None,
) -> ZigToolchain:
    if not _VERSION_RE.match(version):
        msg = f'hm.zig: invalid version {version!r}\n  → use a Zig version like "0.14.1"'
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


class ZigEntry:
    """Callable singleton for the Zig toolchain — access as ``hm.zig``.

    Supports three usage forms:

    - Toolchain form: ``hm.zig(version="0.14.1")`` returns a ``ZigToolchain``
      shared across multiple projects.
    - Project form: ``hm.zig(path=".")`` returns a ``ZigProject`` directly.
    - Bare form: ``hm.zig.build()``, ``hm.zig.test()``, etc. for one-shot leaves.
    """

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
        version: str = "0.14.1",
        image: str | None = None,
        base: Step | None = None,
    ) -> ZigToolchain | ZigProject:
        """Install Zig and return a toolchain or project.

        Returns a ``ZigToolchain`` when ``path`` is omitted, or a ``ZigProject``
        when ``path`` is provided.

        Args:
            path: Zig project root. Omit to get a reusable ``ZigToolchain``
                from which multiple projects can be spawned.
            version: Zig release version (e.g. ``"0.14.1"``). Must be a
                full ``MAJOR.MINOR.PATCH`` string.
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach to instead of emitting a fresh
                apt-base step.

        Returns:
            A ``ZigToolchain`` when ``path`` is omitted, or a ``ZigProject``
            when ``path`` is provided.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.zig(path=".", version="0.14.1")
            >>> hm.pipeline([proj.build(), proj.test()])
        """
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


zig: ZigEntry = ZigEntry()
