"""Haskell toolchain + package abstraction (HAR-15).

Public surface lives on the module-level singleton ``haskell``. Call
it to construct a ``HaskellToolchain`` (which then spawns one
``HaskellPackage`` per cabal package via ``.package(path)``), or
use the bare-form action methods (``haskell.build(path=..., ghc=...)``,
etc.) for a one-shot leaf.

The chain is:

    scratch -> apt-base -> ghcup-install -> <pkg>-deps -> <pkg>-action

``ghcup-install`` is cached forever (keyed on the GHC version baked
into the command). Each package's ``deps`` step is cached
``CacheOnChange`` against the package's cabal files.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, overload

from ._toolchain import make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = (
    "curl",
    "ca-certificates",
    "build-essential",
    "libgmp-dev",
    "libffi-dev",
    "libncurses-dev",
    "zlib1g-dev",
)

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_VERSION_RE = re.compile(r"^[a-zA-Z0-9.-]+$")


def _ghcup_cmd(ghc: str, cabal: str) -> str:
    # `fourmolu` backs `pkg.fmt()`. We pull a pre-built binary from
    # the fourmolu GitHub releases rather than `cabal install fourmolu`
    # because the latter compiles from source on every cold cache,
    # adding ~10 minutes per pipeline first-run. `hlint` (for the
    # rarely-used `pkg.hlint()`) and HLS are intentionally NOT
    # installed here — pipelines that need them should layer their
    # own step.
    fourmolu_url = (
        "https://github.com/fourmolu/fourmolu/releases/download/"
        "v0.18.0.0/fourmolu-0.18.0.0-linux-x86_64"
    )
    return (
        "curl -fsSL https://downloads.haskell.org/~ghcup/x86_64-linux-ghcup "
        "-o /usr/local/bin/ghcup && chmod +x /usr/local/bin/ghcup && "
        f"ghcup install ghc {ghc} && ghcup install cabal {cabal} && "
        f"ghcup set ghc {ghc} && ghcup set cabal {cabal} && "
        "ln -sf /root/.ghcup/bin/* /usr/local/bin/ && "
        f"curl -fsSL {fourmolu_url} -o /usr/local/bin/fourmolu && "
        "chmod +x /usr/local/bin/fourmolu"
    )


@dataclass(frozen=True)
class HaskellPackage:
    """One cabal package — returned by ``HaskellToolchain.package()``.

    ``installed`` is the package's ``deps`` step (the chain ancestor every
    action leaf attaches to). Exposed so callers can chain custom commands
    onto the deps-installed snapshot via ``pkg.installed.sh(...)``.
    """

    path: str
    installed: Step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def build(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && cabal build all",
            f":haskell: {self.path} build",
            **kw,
        )

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && cabal test all",
            f":haskell: {self.path} test",
            **kw,
        )

    def lint(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && cabal build all --flag werror",
            f":haskell: {self.path} lint",
            **kw,
        )

    def hlint(self, **kw: Any) -> Step:
        return self._emit(
            f"hlint {self.path}",
            f":haskell: {self.path} hlint",
            **kw,
        )

    def fmt(self, **kw: Any) -> Step:
        return self._emit(
            f"fourmolu --mode check {self.path}",
            f":haskell: {self.path} fmt",
            **kw,
        )


@dataclass(frozen=True)
class HaskellToolchain:
    """Haskell toolchain install chain — constructed via ``hm.haskell()``.

    Holds the shared ``ghcup`` install step. Spawn one ``HaskellPackage``
    per cabal package via ``.package(path)``. All packages from one
    toolchain share the same ghcup-install step.
    """

    ghc: str
    cabal_version: str
    installed: Step

    def package(
        self,
        path: str,
        *,
        cache_paths: tuple[str, ...] | None = None,
    ) -> HaskellPackage:
        """Create a ``HaskellPackage`` for the cabal package at ``path``.

        Emits a ``cabal build all --only-dependencies`` step cached on the
        package's cabal files. Action methods on the returned package attach
        to that deps step.

        Args:
            path: Path to the cabal package root. Must contain a ``*.cabal``
                file and optionally a ``cabal.project``.
            cache_paths: Override the set of paths used for cache invalidation.
                Defaults to ``(f"{path}/*.cabal", f"{path}/cabal.project")``.

        Returns:
            A ``HaskellPackage`` ready for action methods.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.haskell(ghc="9.6.7")
            >>> pkg = tc.package("api")
            >>> hm.pipeline(pkg.build(), pkg.test())
        """
        if cache_paths is not None:
            paths = cache_paths
        else:
            paths = (f"{path}/*.cabal", f"{path}/cabal.project")
        deps = self.installed.sh(
            f"cabal update && cd {path} && cabal build all --only-dependencies",
            label=f":haskell: {path} deps",
            cache=CacheOnChange(paths=paths),
        )
        return HaskellPackage(path=path, installed=deps)

    def cabal(
        self,
        path: str,
        *,
        cache_paths: tuple[str, ...] | None = None,
    ) -> HaskellPackage:
        """Create a ``HaskellPackage`` for the cabal package at ``path``.

        Alias for ``.package()``; reads more naturally for cabal projects.

        Args:
            path: Path to the cabal package root.
            cache_paths: Override the set of paths used for cache invalidation.

        Returns:
            A ``HaskellPackage`` ready for action methods.
        """
        return self.package(path, cache_paths=cache_paths)


def _make_toolchain(
    *,
    ghc: str,
    cabal: str,
    image: str | None,
    base: Step | None,
) -> HaskellToolchain:
    installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=_ghcup_cmd(ghc, cabal),
        install_cache=CacheForever(env_keys=()),
        lang_tag="haskell",
        install_tag="ghcup",
        image=image,
        base=base,
    )
    return HaskellToolchain(ghc=ghc, cabal_version=cabal, installed=installed)


def _validate_ghc(ghc: str | None) -> str:
    if ghc is None:
        msg = (
            "hm.haskell: ghc is required\n"
            '  → pass ghc="9.6.7" (or another GHC version your packages support)'
        )
        raise ValueError(msg)
    if not _VERSION_RE.match(ghc):
        msg = f'hm.haskell: invalid ghc {ghc!r}\n  → use a GHC version like "9.6.7"'
        raise ValueError(msg)
    return ghc


class HaskellEntry:
    """Callable singleton for the Haskell toolchain — access as ``hm.haskell``.

    Supports both object form (``hm.haskell(ghc="9.6.7")``) and bare form
    (``hm.haskell.build(ghc="9.6.7", path="api")``, etc.).
    """

    @overload
    def __call__(
        self,
        *,
        ghc: str,
        cabal: str = ...,
        image: str | None = ...,
        base: Step | None = ...,
    ) -> HaskellToolchain: ...

    @overload
    def __call__(
        self,
        *,
        ghc: str,
        path: str,
        cabal: str = ...,
        image: str | None = ...,
        base: Step | None = ...,
        cache_paths: tuple[str, ...] | None = ...,
    ) -> HaskellPackage: ...

    def __call__(
        self,
        *,
        ghc: str | None = None,
        cabal: str = "latest",
        image: str | None = None,
        base: Step | None = None,
        path: str | None = None,
        cache_paths: tuple[str, ...] | None = None,
    ) -> HaskellToolchain | HaskellPackage:
        """Install GHC via ghcup and return a toolchain or package.

        Returns a ``HaskellToolchain`` when ``path`` is omitted, or a
        ``HaskellPackage`` when ``path`` is provided.

        Args:
            ghc: GHC version to install (e.g. ``"9.6.7"``). Required.
            cabal: cabal-install version. Defaults to ``"latest"``.
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach to instead of emitting a fresh
                apt-base step.
            path: Cabal package root. When provided, the call returns a
                ``HaskellPackage`` directly rather than a toolchain.
            cache_paths: Override the cabal-file paths for deps-step cache
                invalidation. Only meaningful when ``path`` is also set.

        Returns:
            A ``HaskellToolchain`` when ``path`` is omitted, or a
            ``HaskellPackage`` when ``path`` is provided.

        Raises:
            ValueError: If ``ghc`` is ``None`` or not a valid version string.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.haskell(ghc="9.6.7")
            >>> pkg = tc.package("api")
            >>> hm.pipeline(pkg.build(), pkg.test())
        """
        ghc_v = _validate_ghc(ghc)
        toolchain = _make_toolchain(ghc=ghc_v, cabal=cabal, image=image, base=base)
        if path is None:
            return toolchain
        return toolchain.package(path, cache_paths=cache_paths)

    def _pkg(self, **kw: Any) -> HaskellPackage:
        path = kw.pop("path", ".")
        pkg = self(path=path, **kw)
        assert isinstance(pkg, HaskellPackage)  # noqa: S101 — narrow overload result
        return pkg

    def build(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._pkg(**kw).build(**action_kw)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._pkg(**kw).test(**action_kw)

    def lint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._pkg(**kw).lint(**action_kw)

    def hlint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._pkg(**kw).hlint(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._pkg(**kw).fmt(**action_kw)


haskell: HaskellEntry = HaskellEntry()
