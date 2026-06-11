"""Rust toolchain abstraction (HAR-15).

Public surface lives on the module-level singleton ``rust``:

    hm.rust.toolchain(...)  -> RustToolchain  (install-only)
    hm.rust.project(...)    -> RustProject    (full CI DAG)
"""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._cargo import CargoOpts, cargo_flags
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


def _build_cmd(**o: Any) -> str:
    return f"cargo build{cargo_flags(CargoOpts(**o))}"


def _test_cmd(*, nextest: bool = False, **o: Any) -> str:
    runner = "cargo nextest run" if nextest else "cargo test"
    return f"{runner}{cargo_flags(CargoOpts(**o))}"


def _doctest_cmd(**o: Any) -> str:
    return f"cargo test{cargo_flags(CargoOpts(**o))} --doc"


def _clippy_cmd(*, deny_warnings: bool = True, extra_lints: tuple[str, ...] = (), **o: Any) -> str:
    mid = cargo_flags(CargoOpts(**o))
    trail = (["-D warnings"] if deny_warnings else []) + list(extra_lints)
    suffix = (" -- " + " ".join(trail)) if trail else ""
    return f"cargo clippy{mid}{suffix}"


def _fmt_cmd(
    *,
    all: bool = True,  # noqa: A002
    check: bool = True,
    flags: tuple[str, ...] = (),
) -> str:
    toks = ["cargo fmt"]
    if all:
        toks.append("--all")
    if check:
        toks.append("--check")
    toks += list(flags)
    return " ".join(toks)


def _doc_cmd(*, no_deps: bool = True, document_private_items: bool = False, **o: Any) -> str:
    doc_toks: list[str] = []
    if no_deps:
        doc_toks.append("--no-deps")
    if document_private_items:
        doc_toks.append("--document-private-items")
    prefix = (" " + " ".join(doc_toks)) if doc_toks else ""
    return f"cargo doc{prefix}{cargo_flags(CargoOpts(**o))}"


def _doc_env(kw: dict[str, Any], *, deny_warnings: bool) -> None:
    if deny_warnings:
        user_env = kw.get("env")
        merged = dict(user_env) if user_env else {}
        merged.setdefault("RUSTDOCFLAGS", "-D warnings")
        kw["env"] = merged


def _hack_cmd(
    *,
    subcommand: str = "check",
    depth: int = 2,
    each_feature: bool = False,
    no_dev_deps: bool = True,
    skip: tuple[str, ...] = (),
    include_features: tuple[str, ...] = (),
    keep_going: bool = False,
    flags: tuple[str, ...] = (),
) -> str:
    toks = ["cargo hack", subcommand]
    if each_feature:
        toks.append("--each-feature")
    else:
        toks += ["--feature-powerset", "--depth", str(depth)]
    if no_dev_deps:
        toks.append("--no-dev-deps")
    if skip:
        toks.append("--skip " + ",".join(shlex.quote(s) for s in skip))
    if include_features:
        toks.append("--include-features " + ",".join(shlex.quote(s) for s in include_features))
    if keep_going:
        toks.append("--keep-going")
    toks += list(flags)
    return " ".join(toks)


@dataclass(frozen=True)
class RustToolchain:
    """Rust toolchain install chain — constructed via ``hm.rust.toolchain()``.

    Holds the rustup install step. Action methods (``build``, ``test``,
    ``doctest``, ``clippy``, ``fmt``, ``doc``, ``warmup``) attach leaves to
    ``installed``. Every action accepts the shared cargo options (``packages``,
    ``features``, ``all_features``, ``no_default_features``, ``target``,
    ``release``, ``profile``, ``locked``, ``workspace``) plus a ``flags``
    escape hatch, and forwards Step kwargs (``label``, ``cache``, ``env``,
    ``image`` …) unchanged.
    """

    path: str
    installed: Step

    def _wrap(self, cargo: str) -> str:
        return f". $HOME/.cargo/env && cd {self.path} && {cargo}"

    def _emit(self, cargo: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(self._wrap(cargo), **kw)

    def build(
        self,
        *,
        workspace: bool = False,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        all_targets: bool = False,
        release: bool = False,
        profile: str | None = None,
        locked: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        return self._emit(
            _build_cmd(
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                all_targets=all_targets,
                release=release,
                profile=profile,
                locked=locked,
                flags=flags,
            ),
            ":rust: build",
            **kw,
        )

    def test(
        self,
        *,
        nextest: bool = False,
        workspace: bool = False,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        all_targets: bool = False,
        release: bool = False,
        profile: str | None = None,
        locked: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        return self._emit(
            _test_cmd(
                nextest=nextest,
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                all_targets=all_targets,
                release=release,
                profile=profile,
                locked=locked,
                flags=flags,
            ),
            ":rust: test",
            **kw,
        )

    def doctest(
        self,
        *,
        workspace: bool = False,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        locked: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        return self._emit(
            _doctest_cmd(
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                locked=locked,
                flags=flags,
            ),
            ":rust: doctest",
            **kw,
        )

    def clippy(
        self,
        *,
        workspace: bool = False,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        all_targets: bool = True,
        locked: bool = True,
        deny_warnings: bool = True,
        extra_lints: tuple[str, ...] = (),
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        return self._emit(
            _clippy_cmd(
                deny_warnings=deny_warnings,
                extra_lints=extra_lints,
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                all_targets=all_targets,
                locked=locked,
                flags=flags,
            ),
            ":rust: clippy",
            **kw,
        )

    def fmt(
        self,
        *,
        all: bool = True,  # noqa: A002
        check: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        return self._emit(_fmt_cmd(all=all, check=check, flags=flags), ":rust: fmt", **kw)

    def doc(
        self,
        *,
        no_deps: bool = True,
        document_private_items: bool = False,
        workspace: bool = False,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        locked: bool = True,
        deny_warnings: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        _doc_env(kw, deny_warnings=deny_warnings)
        return self._emit(
            _doc_cmd(
                no_deps=no_deps,
                document_private_items=document_private_items,
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                locked=locked,
                flags=flags,
            ),
            ":rust: doc",
            **kw,
        )

    def warmup(self, **kw: Any) -> Step:
        return self._emit(
            "cargo build --workspace --tests --locked",
            ":rust: warmup",
            **kw,
        )

    def feature_powerset(
        self,
        *,
        subcommand: str = "check",
        depth: int = 2,
        each_feature: bool = False,
        no_dev_deps: bool = True,
        skip: tuple[str, ...] = (),
        include_features: tuple[str, ...] = (),
        keep_going: bool = False,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        """Run a cargo-hack feature sweep (powerset, or ``--each-feature``).

        Installs ``cargo-hack`` (cached forever) then runs the sweep. Mirrors
        the tokio/reqwest/tracing CI idiom: ``--feature-powerset --depth 2
        --no-dev-deps``.
        """
        # Global install — no crate dir needed, so skip _wrap's `cd <path>`
        # and source the cargo env directly. Keeps the CacheForever key
        # identical across toolchains regardless of path.
        installed_hack = self.installed.sh(
            ". $HOME/.cargo/env && cargo install cargo-hack --locked",
            label=":rust: install cargo-hack",
            cache=CacheForever(env_keys=()),
        )
        cmd = _hack_cmd(
            subcommand=subcommand,
            depth=depth,
            each_feature=each_feature,
            no_dev_deps=no_dev_deps,
            skip=skip,
            include_features=include_features,
            keep_going=keep_going,
            flags=flags,
        )
        if kw.get("label") is None:
            kw["label"] = ":rust: feature-powerset"
        return installed_hack.sh(self._wrap(cmd), **kw)


@dataclass(frozen=True)
class RustProject:
    """High-level Rust CI DAG — constructed via ``hm.rust.project()``.

    Action methods (``build``, ``test``, ``doctest``, ``clippy``, ``fmt``,
    ``doc``) attach leaves to the shared warmup step so dependency compilation
    is reused. ``ci()`` returns the standard DAG in one call. Methods default
    to ``workspace=True``.
    """

    toolchain: RustToolchain
    warmup: Step

    def _emit(self, cargo: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.warmup.sh(self.toolchain._wrap(cargo), **kw)  # noqa: SLF001

    def build(
        self,
        *,
        workspace: bool = True,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        all_targets: bool = False,
        release: bool = False,
        profile: str | None = None,
        locked: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        return self._emit(
            _build_cmd(
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                all_targets=all_targets,
                release=release,
                profile=profile,
                locked=locked,
                flags=flags,
            ),
            ":rust: build",
            **kw,
        )

    def test(
        self,
        *,
        nextest: bool = False,
        workspace: bool = True,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        all_targets: bool = False,
        release: bool = False,
        profile: str | None = None,
        locked: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        return self._emit(
            _test_cmd(
                nextest=nextest,
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                all_targets=all_targets,
                release=release,
                profile=profile,
                locked=locked,
                flags=flags,
            ),
            ":rust: test",
            **kw,
        )

    def doctest(
        self,
        *,
        workspace: bool = True,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        locked: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        return self._emit(
            _doctest_cmd(
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                locked=locked,
                flags=flags,
            ),
            ":rust: doctest",
            **kw,
        )

    def clippy(
        self,
        *,
        workspace: bool = True,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        all_targets: bool = True,
        locked: bool = True,
        deny_warnings: bool = True,
        extra_lints: tuple[str, ...] = (),
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        return self._emit(
            _clippy_cmd(
                deny_warnings=deny_warnings,
                extra_lints=extra_lints,
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                all_targets=all_targets,
                locked=locked,
                flags=flags,
            ),
            ":rust: clippy",
            **kw,
        )

    def fmt(
        self,
        *,
        all: bool = True,  # noqa: A002
        check: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        # fmt has no warmup dependency; chain off the install step (like the
        # toolchain) so it can run without waiting on the build warmup.
        return self.toolchain.fmt(all=all, check=check, flags=flags, **kw)

    def doc(
        self,
        *,
        no_deps: bool = True,
        document_private_items: bool = False,
        workspace: bool = True,
        packages: tuple[str, ...] = (),
        exclude: tuple[str, ...] = (),
        all_features: bool = False,
        no_default_features: bool = False,
        features: tuple[str, ...] = (),
        target: str | None = None,
        locked: bool = True,
        deny_warnings: bool = True,
        flags: tuple[str, ...] = (),
        **kw: Any,
    ) -> Step:
        _doc_env(kw, deny_warnings=deny_warnings)
        return self._emit(
            _doc_cmd(
                no_deps=no_deps,
                document_private_items=document_private_items,
                workspace=workspace,
                packages=packages,
                exclude=exclude,
                all_features=all_features,
                no_default_features=no_default_features,
                features=features,
                target=target,
                locked=locked,
                flags=flags,
            ),
            ":rust: doc",
            **kw,
        )

    def ci(self, *, nextest: bool = False, doc: bool = False) -> tuple[Step, ...]:
        """The zero-config Rust CI DAG. test and clippy chain off the shared
        warmup; fmt runs off the toolchain install step, in parallel.

        With ``nextest=True`` the test step uses ``cargo nextest run`` and a
        companion ``doctest()`` step is appended (nextest cannot run doctests).
        With ``doc=True`` a ``doc()`` step is appended.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.rust.project()
            >>> hm.pipeline(list(proj.ci(nextest=True)))
        """
        steps: list[Step] = [self.test(nextest=nextest)]
        if nextest:
            steps.append(self.doctest())
        steps.append(self.clippy())
        steps.append(self.fmt())
        if doc:
            steps.append(self.doc())
        return tuple(steps)

    def feature_powerset(self, **kw: Any) -> Step:
        return self.toolchain.feature_powerset(**kw)


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
    toml_glob = f"{path}/**/Cargo.toml" if path != "." else "**/Cargo.toml"
    rs_glob = f"{path}/**/*.rs" if path != "." else "**/*.rs"
    warmup_cache = (
        cache if cache is not None else CacheOnChange(paths=(lock_path, toml_glob, rs_glob))
    )

    warm = tc._emit(  # noqa: SLF001
        "cargo build --workspace --tests --locked",
        ":rust: warmup",
        cache=warmup_cache,
    )

    return RustProject(toolchain=tc, warmup=warm)


class RustEntry:
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
        """Install the Rust toolchain via rustup.

        Produces a ``RustToolchain`` whose ``installed`` step is the
        rustup-install step. Action methods on the toolchain attach leaves
        to ``installed``. Use ``project()`` instead when you want a
        pre-built warmup step shared across test/clippy/fmt.

        Args:
            path: Path to the crate or workspace root.
            version: rustup channel name (``"stable"``) or a pinned version
                (``"1.81.0"``).
            image: Local-mode Docker base image override.
            components: rustup components to install alongside the toolchain.
                Defaults to ``("clippy", "rustfmt")``.
            base: Existing ``Step`` to attach the toolchain install to instead
                of emitting a fresh apt-base step. Use to share one apt-base
                across multiple toolchains.

        Returns:
            A ``RustToolchain`` ready for action methods.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.rust.toolchain(version="1.81.0")
            >>> hm.pipeline([tc.test(), tc.clippy()])
        """
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
        """Build a high-level Rust CI DAG.

        Installs the toolchain via rustup, warms a dependency cache keyed on
        ``Cargo.lock``, and returns a ``RustProject`` whose ``.test()``,
        ``.clippy()``, and ``.fmt()`` methods build on that warmup step so
        dependency compilation is shared.

        Args:
            path: Path to the crate or workspace root.
            version: rustup channel name (``"stable"``) or a pinned version
                (``"1.81.0"``).
            image: Local-mode Docker base image override.
            components: rustup components to install alongside the toolchain.
                Defaults to ``("clippy", "rustfmt")``.
            base: Existing ``Step`` to attach to instead of emitting a fresh
                apt-base step.
            cache: Override the warmup step's cache policy. Defaults to
                ``CacheOnChange`` keyed on ``Cargo.lock``, ``**/Cargo.toml``,
                and ``**/*.rs``.

        Returns:
            A ``RustProject`` exposing the common CI steps.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.rust.project()
            >>> hm.group([proj.test(), proj.clippy(), proj.fmt()])
        """
        return _make_rust_project(
            path=path,
            version=version,
            image=image,
            components=components,
            base=base,
            cache=cache,
        )


rust: RustEntry = RustEntry()
