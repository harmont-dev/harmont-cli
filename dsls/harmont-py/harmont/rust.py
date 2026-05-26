"""Rust toolchain abstraction (HAR-15).

Public surface lives on the module-level singleton :data:`rust`:

    hm.rust.toolchain(...)  -> RustToolchain  (install-only)
    hm.rust.project(...)    -> RustProject    (full CI DAG)
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step
    from .cache import CachePolicy

APT_PACKAGES = (
    "curl",
    "ca-certificates",
    "build-essential",
    "pkg-config",
    "libssl-dev",
)

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
            "cargo clippy --all-targets -- -D warnings",
            ":rust: clippy",
            **kw,
        )

    def fmt(self, **kw: Any) -> Step:
        return self._emit("cargo fmt --check", ":rust: fmt", **kw)

    def warmup(self, **kw: Any) -> Step:
        return self._emit(
            "cargo build --workspace --tests --locked",
            ":rust: warmup",
            **kw,
        )

    def doc(self, **kw: Any) -> Step:
        return self._emit("cargo doc --no-deps", ":rust: doc", **kw)


@dataclass(frozen=True)
class RustProject:
    """High-level Rust CI DAG — constructed via ``hm.rust.project()``."""

    toolchain: RustToolchain
    warmup: Step

    def test(self, *, flags: tuple[str, ...] = (), **kw: Any) -> Step:
        extra = (" " + " ".join(flags)) if flags else ""
        return self.warmup.sh(
            self.toolchain._wrap(f"cargo test --workspace --locked{extra}"),  # noqa: SLF001
            label=kw.pop("label", ":rust: test"),
            **kw,
        )

    def clippy(self, *, flags: tuple[str, ...] = (), **kw: Any) -> Step:
        extra = (" " + " ".join(flags)) if flags else ""
        return self.warmup.sh(
            self.toolchain._wrap(  # noqa: SLF001
                f"cargo clippy --workspace --tests --locked{extra} -- -D warnings"
            ),
            label=kw.pop("label", ":rust: clippy"),
            **kw,
        )

    def fmt(self, *, flags: tuple[str, ...] = (), **kw: Any) -> Step:
        extra = (" " + " ".join(flags)) if flags else ""
        return self.toolchain._emit(  # noqa: SLF001
            f"cargo fmt --check{extra}", ":rust: fmt", **kw
        )


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


def _make_rust_project(
    *,
    path: str = ".",
    version: str = "stable",
    image: str | None = None,
    components: tuple[str, ...] = ("clippy", "rustfmt"),
    base: Step | None = None,
    cache: CachePolicy | None = None,
) -> RustProject:
    tc = _make_rust(
        path=path,
        version=version,
        image=image,
        components=components,
        base=base,
    )

    lock_path = f"{path}/Cargo.lock" if path != "." else "Cargo.lock"
    warmup_cache = cache if cache is not None else CacheOnChange(paths=(lock_path,))

    warm = tc._emit(  # noqa: SLF001
        "cargo build --workspace --tests --locked",
        ":rust: warmup",
        cache=warmup_cache,
    )

    return RustProject(toolchain=tc, warmup=warm)


class _RustEntry:
    """Namespace for ``hm.rust.toolchain()`` and ``hm.rust.project()``."""

    @staticmethod
    def toolchain(
        *,
        path: str = ".",
        version: str = "stable",
        image: str | None = None,
        components: tuple[str, ...] = ("clippy", "rustfmt"),
        base: Step | None = None,
    ) -> RustToolchain:
        return _make_rust(
            path=path,
            version=version,
            image=image,
            components=components,
            base=base,
        )

    @staticmethod
    def project(
        *,
        path: str = ".",
        version: str = "stable",
        image: str | None = None,
        components: tuple[str, ...] = ("clippy", "rustfmt"),
        base: Step | None = None,
        cache: CachePolicy | None = None,
    ) -> RustProject:
        return _make_rust_project(
            path=path,
            version=version,
            image=image,
            components=components,
            base=base,
            cache=cache,
        )


rust = _RustEntry()
