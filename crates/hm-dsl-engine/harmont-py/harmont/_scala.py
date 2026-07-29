"""Scala toolchain and CI-pipeline DSL.

Public surface is the module-level ``scala`` namespace:

    hm.scala.toolchain(...)  -> ScalaToolchain  (install-only)
    hm.scala.project(...)    -> ScalaProject    (full CI DAG)
"""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Self

from ._toolchain import advance_install, make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step
    from .cache import CachePolicy

APT_PACKAGES = ("curl", "ca-certificates", "openjdk-17-jdk-headless")
_VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")  # e.g. 3.3.8
_MIN_VERSION = (2, 13, 0)


def _parse_version(version: str) -> tuple[int, ...]:
    return tuple(int(p) for p in version.split("."))


def coursier_install_cmd(version: str = "3.3.8") -> str:
    return (
        "curl -fL https://github.com/coursier/launchers/raw/master/cs-x86_64-pc-linux.gz "
        "| gzip -d > cs && "
        "chmod +x cs && "
        "./cs setup --yes && "
        f"./cs install scala:{version} && "
        "ln -sf /root/.local/share/coursier/bin/* /usr/local/bin/"
    )


def _compile_cmd(*, query: str | None = None, test: bool = False) -> str:
    scope = ""
    if query:
        scope += f"{query} / "
    if test:
        scope += "Test / "
    return f"sbt {scope}compile"


# sbt [<query> / ] test [<testname> …] [ -- <options> ]
def _test_cmd(
    *,
    query: str | None = None,
    testnames: tuple[str, ...] = (),
    options: dict[str, Any] | None = None,
) -> str:
    scope = f"{query} / " if query else ""
    names = (" " + " ".join(testnames)) if testnames else ""
    opts = ""
    if options:
        rendered = " ".join(f"{key} {shlex.quote(str(value))}" for key, value in options.items())
        opts = f" -- {rendered}"
    return f"sbt {scope}test{names}{opts}"


def _fmt_cmd(*, check: bool = True) -> str:
    return "sbt scalafmtCheckAll" if check else "sbt scalafmtAll"


@dataclass(frozen=True)
class ScalaToolchain:
    """Scala install chain built by ``hm.scala.toolchain()``.

    Holds the coursier install step. Action methods (``compile``, ``test``,
    ``fmt``, ``warmup``) attach leaves to ``installed`` and forward Step kwargs
    (``label``, ``cache``, ``env``, ``image`` …) unchanged.
    """

    path: str
    installed: Step

    def setup(
        self,
        cmd: str,
        *,
        cwd: str | None = None,
        label: str | None = None,
        cache: CachePolicy | None = None,
        env: dict[str, str] | None = None,
    ) -> Self:
        """Append a post-install command and return an advanced toolchain; chainable.

        Use for prep steps the toolchain's actions must depend on but that the SDK
        does not model natively — code generation, fixtures, extra tooling. The
        returned object's action methods fork from this step.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.scala.toolchain(path=".").setup("cs install scalafix")
        """
        return advance_install(self, cmd, cwd=cwd, label=label, cache=cache, env=env)

    def _wrap(self, sbt_cmd: str) -> str:
        return (
            f'export PATH="$PATH:$HOME/.local/share/coursier/bin" && cd {self.path} && {sbt_cmd}'
        )

    def _emit(self, sbt_cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(self._wrap(sbt_cmd), **kw)

    def compile(self, *, query: str | None = None, test: bool = False, **kw: Any) -> Step:
        """Compile the project (``sbt compile``); ``test=True`` compiles the test
        sources (``sbt Test / compile``)."""
        cmd = _compile_cmd(query=query, test=test)
        return self._emit(cmd, f":scala: {self.path} compile", **kw)

    def test(
        self,
        *,
        query: str | None = None,
        testnames: tuple[str, ...] = (),
        options: dict[str, Any] | None = None,
        **kw: Any,
    ) -> Step:
        """Run the test suite (``sbt test``). ``query`` scopes to a subproject,
        ``testnames`` selects specific suites, ``options`` pass through after ``--``."""
        cmd = _test_cmd(query=query, testnames=testnames, options=options)
        return self._emit(cmd, f":scala: {self.path} test", **kw)

    def fmt(self, *, check: bool = True, **kw: Any) -> Step:
        """Format Scala sources. ``check=True`` (default) verifies formatting via
        ``sbt scalafmtCheckAll`` and fails on drift; ``check=False`` rewrites in
        place via ``sbt scalafmtAll``."""
        cmd = _fmt_cmd(check=check)
        return self._emit(cmd, f":scala: {self.path} format", **kw)

    def warmup(self, **kw: Any) -> Step:
        """Pre-resolve dependencies (``sbt update``) so later steps reuse the cache.
        Used internally by ``hm.scala.project()``."""
        return self._emit("sbt update", ":scala: warmup", **kw)


@dataclass(frozen=True)
class ScalaProject:
    """High-level Scala CI DAG built by ``hm.scala.project()``.

    ``compile`` and ``test`` attach to a shared ``warmup`` step so dependency
    resolution is reused; ``fmt`` forks off the toolchain install to run in
    parallel. ``ci()`` returns the standard DAG in one call.
    """

    path: str
    toolchain: ScalaToolchain
    warmup: Step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.warmup.sh(self.toolchain._wrap(cmd), **kw)  # noqa: SLF001

    def compile(self, *, query: str | None = None, test: bool = False, **kw: Any) -> Step:
        """Compile the project (``sbt compile``); ``test=True`` compiles the test
        sources (``sbt Test / compile``)."""
        cmd = _compile_cmd(query=query, test=test)
        return self._emit(cmd, f":scala: {self.path} sbt compile", **kw)

    def test(
        self,
        *,
        query: str | None = None,
        testnames: tuple[str, ...] = (),
        options: dict[str, Any] | None = None,
        **kw: Any,
    ) -> Step:
        """Run the test suite (``sbt test``). ``query`` scopes to a subproject,
        ``testnames`` selects specific suites, ``options`` pass through after ``--``."""
        cmd = _test_cmd(query=query, testnames=testnames, options=options)
        return self._emit(cmd, f":scala: {self.path} sbt test", **kw)

    def fmt(self, *, check: bool = True, **kw: Any) -> Step:
        """Format Scala sources (``sbt scalafmtCheckAll`` when ``check=True``,
        default). Forks off the toolchain install so it runs in parallel with the
        warmup rather than waiting on dependency resolution."""
        return self.toolchain.fmt(check=check, **kw)

    def ci(self) -> tuple[Step, ...]:
        """Zero-config Scala CI DAG: ``compile`` and ``test`` off the shared warmup,
        ``fmt`` (verify) in parallel off the install.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.scala.project()
            >>> hm.pipeline(list(proj.ci()))
        """
        return self.compile(), self.test(), self.fmt()


def _make_scala(
    *,
    path: str = ".",
    version: str = "3.3.8",
    image: str | None = None,
    base: Step | None = None,
) -> ScalaToolchain:
    if not _VERSION_RE.match(version):
        msg = f"hm.scala: invalid version {version!r}\n -> use a valid X.Y.Z Scala version"
        raise ValueError(msg)

    if _parse_version(version) < _MIN_VERSION:
        msg = f"hm.scala: unsupported version {version!r}\n -> use Scala 2.13.0 or newer"
        raise ValueError(msg)

    installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=coursier_install_cmd(version),
        install_cache=CacheForever(env_keys=()),
        lang_tag="scala",
        install_tag="install",
        image=image,
        base=base,
    )
    return ScalaToolchain(path=path, installed=installed)


def _make_scala_project(
    *,
    path: str = ".",
    version: str = "3.3.8",
    image: str | None = None,
    base: Step | None = None,
    cache: CachePolicy | None = None,
) -> ScalaProject:
    tc = _make_scala(path=path, version=version, image=image, base=base)

    if not path:
        path = "."

    prefix = "" if path == "." else f"{path}/"
    build_file = f"{prefix}build.sbt"
    properties_file = f"{prefix}project/build.properties"
    main_glob = f"{prefix}**/src/main/*.scala"
    test_glob = f"{prefix}**/src/test/*.scala"

    warmup_cache = cache or CacheOnChange(
        paths=(build_file, main_glob, test_glob, properties_file)
    )

    warm = tc._emit("sbt update", ":scala: sbt warmup", cache=warmup_cache)  # noqa: SLF001
    return ScalaProject(path=path, toolchain=tc, warmup=warm)


class ScalaEntry:
    """Namespace for ``hm.scala.toolchain()`` and ``hm.scala.project()``."""

    @staticmethod
    def toolchain(
        *,
        path: str = ".",
        scala_version: str = "3.3.8",
        image: str | None = None,
        base: Step | None = None,
    ) -> ScalaToolchain:
        """Install the Scala toolchain via coursier.

        Produces a ``ScalaToolchain`` whose ``installed`` step runs coursier;
        action methods attach leaves to it. Use ``project()`` instead when you
        want a shared warmup step across compile/test/fmt.

        Args:
            path: Path to the workspace root.
            scala_version: Pinned Scala version, e.g. ``"3.3.8"`` (2.13.0 or newer).
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach the install to instead of emitting
                a fresh apt-base step. Use to share one apt-base across toolchains.

        Returns:
            A ``ScalaToolchain`` ready for action methods.

        Raises:
            ValueError: If ``scala_version`` is malformed or older than 2.13.0.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.scala.toolchain(scala_version="3.3.8")
            >>> hm.pipeline([tc.compile(), tc.test()])
        """
        return _make_scala(path=path, version=scala_version, image=image, base=base)

    @staticmethod
    def project(
        *,
        path: str = ".",
        scala_version: str = "3.3.8",
        image: str | None = None,
        base: Step | None = None,
        cache: CachePolicy | None = None,
    ) -> ScalaProject:
        """Build a high-level Scala CI DAG.

        Installs the toolchain via coursier, warms a dependency cache keyed on
        ``build.sbt``/``project/build.properties``/``**/*.scala``, and returns a
        ``ScalaProject`` whose ``compile``/``test``/``fmt`` build on that warmup.

        Args:
            path: Path to the workspace root.
            scala_version: Pinned Scala version, e.g. ``"3.3.8"`` (2.13.0 or newer).
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach the install to instead of emitting
                a fresh apt-base step.
            cache: Override the warmup cache policy. Defaults to ``CacheOnChange``
                keyed on the build files and ``**/*.scala`` sources.

        Returns:
            A ``ScalaProject`` exposing the common CI steps.

        Raises:
            ValueError: If ``scala_version`` is malformed or older than 2.13.0.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.scala.project()
            >>> hm.pipeline(list(proj.ci()))
        """
        return _make_scala_project(
            path=path,
            version=scala_version,
            image=image,
            base=base,
            cache=cache,
        )


scala: ScalaEntry = ScalaEntry()
