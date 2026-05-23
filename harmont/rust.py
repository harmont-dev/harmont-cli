"""Rust toolchain abstraction (HAR-15).

Public surface lives on the module-level singleton :data:`rust`. Call it
to construct a :class:`RustToolchain`, or use the bare-form action
methods (``rust.build()``, ``rust.test()``, etc.) for a one-shot leaf.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = (
    "curl", "ca-certificates", "build-essential", "pkg-config", "libssl-dev",
)

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_VERSION_RE = re.compile(r"^[a-z0-9.-]+$")


def _rustup_cmd(version: str, components: tuple[str, ...]) -> str:
    comps = ",".join(components)
    return (
        "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | "
        f"sh -s -- -y --default-toolchain {version} --profile minimal "
        f"--component {comps} && . $HOME/.cargo/env && "
        "rustc --version && cargo --version"
    )


@dataclass(frozen=True)
class RustToolchain:
    """Constructed via :func:`rust` (the ``hm.rust`` singleton)."""

    path: str
    installed: Step

    def _wrap(self, cargo: str) -> str:
        return f". $HOME/.cargo/env && cd {self.path} && {cargo}"

    def _emit(self, cargo: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(self._wrap(cargo), **kw)

    def build(self, *, release: bool = False, **kw: Any) -> Step:
        flag = " --release" if release else ""
        return self._emit(f"cargo build{flag}", ":rust: build", **kw)

    def test(self, *, release: bool = False, **kw: Any) -> Step:
        flag = " --release" if release else ""
        return self._emit(f"cargo test{flag}", ":rust: test", **kw)

    def clippy(self, **kw: Any) -> Step:
        return self._emit(
            "cargo clippy --all-targets -- -D warnings", ":rust: clippy", **kw,
        )

    def fmt(self, **kw: Any) -> Step:
        return self._emit("cargo fmt --check", ":rust: fmt", **kw)

    def doc(self, **kw: Any) -> Step:
        return self._emit("cargo doc --no-deps", ":rust: doc", **kw)


def _make_rust(
    *,
    path: str = ".",
    version: str = "stable",
    image: str | None = None,
    components: tuple[str, ...] = ("clippy", "rustfmt"),
    base: Step | None = None,
) -> RustToolchain:
    if not _VERSION_RE.match(version):
        msg = (
            f"hm.rust: invalid version {version!r}\n"
            '  → use a rustup channel name (e.g. "stable") or a '
            'pinned version (e.g. "1.81.0")'
        )
        raise ValueError(msg)
    installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=_rustup_cmd(version, components),
        install_cache=CacheForever(env_keys=()),
        lang_tag="rust",
        install_tag="rustup",
        image=image,
        base=base,
    )
    return RustToolchain(path=path, installed=installed)


class _RustEntry:
    """Callable singleton — supports both object form and bare form."""

    def __call__(
        self,
        *,
        path: str = ".",
        version: str = "stable",
        image: str | None = None,
        components: tuple[str, ...] = ("clippy", "rustfmt"),
        base: Step | None = None,
    ) -> RustToolchain:
        return _make_rust(
            path=path, version=version, image=image,
            components=components, base=base,
        )

    def build(self, *, release: bool = False, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).build(release=release, **action_kw)

    def test(self, *, release: bool = False, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(release=release, **action_kw)

    def clippy(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).clippy(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).fmt(**action_kw)

    def doc(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).doc(**action_kw)


rust = _RustEntry()
