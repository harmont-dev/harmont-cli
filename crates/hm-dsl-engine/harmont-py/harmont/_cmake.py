"""CMake (C/C++) toolchain — three-tier abstraction.

Public surface lives on the module-level singleton ``cmake``:

    hm.cmake()              -> CMakeToolchain  (install-only, multi-project)
    hm.cmake(path=".")      -> CMakeProject    (full CI DAG)
    hm.cmake.build()        -> Step            (bare-form one-shot)

The chain is:

    scratch -> apt-base (build-essential, cmake, ninja-build, pkg-config,
               ccache, clang-format, clang-tidy, [compiler pkgs])
            -> cmake-verify (cmake --version && ninja --version ...)
            -> warmup (configure + build, cached)
            -> action leaves

``CMakeToolchain`` holds the verified install step.  ``CMakeProject``
holds a pre-built warmup step and exposes action methods (test, install,
fmt, lint, package).
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, overload

from ._toolchain import make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step
    from .cache import CachePolicy

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_COMPILER_RE = re.compile(r"^(gcc|clang)(-\d+)?$")


def _apt_packages(
    compiler: str | None,
    *,
    ccache: bool,
    generator: str,
) -> tuple[str, ...]:
    """Compute the dynamic list of apt packages to install."""
    pkgs: list[str] = ["cmake", "build-essential", "pkg-config"]
    if generator == "ninja":
        pkgs.append("ninja-build")
    if ccache:
        pkgs.append("ccache")
    pkgs.append("clang-format")
    pkgs.append("clang-tidy")
    if compiler is not None:
        m = _COMPILER_RE.match(compiler)
        if m is None:
            msg = (
                f"hm.cmake: invalid compiler {compiler!r}\n"
                '  → use "gcc", "gcc-14", "clang", or "clang-18"'
            )
            raise ValueError(msg)
        family = m.group(1)
        suffix = m.group(2) or ""
        if family == "gcc":
            pkgs.append(f"gcc{suffix}")
            pkgs.append(f"g++{suffix}")
        else:
            pkgs.append(f"clang{suffix}")
            pkgs.append(f"lld{suffix}")
    return tuple(pkgs)


def _verify_cmd(compiler: str | None, *, ccache: bool, generator: str) -> str:
    """Build the cmake-verify shell command."""
    parts = ["cmake --version"]
    if generator == "ninja":
        parts.append("ninja --version")
    if ccache:
        parts.append("ccache --version")
    if compiler is not None:
        m = _COMPILER_RE.match(compiler)
        if m:
            family = m.group(1)
            suffix = m.group(2) or ""
            if family == "gcc":
                parts.append(f"gcc{suffix} --version")
            else:
                parts.append(f"clang{suffix} --version")
    return " && ".join(parts)


def _configure_cmd(
    *,
    path: str,
    preset: str | None,
    defines: dict[str, str] | None,
    compiler: str | None,
    ccache: bool,
    generator: str,
    build_dir: str = "build",
) -> str:
    """Build the cmake configure command."""
    if preset is not None:
        return f"cd {path} && cmake --preset {preset}"

    gen_flag = "Ninja" if generator == "ninja" else "Unix Makefiles"
    parts = [
        f"cd {path} && cmake -S . -B {build_dir}",
        f"-G {gen_flag}",
        "-DCMAKE_EXPORT_COMPILE_COMMANDS=ON",
    ]

    if ccache:
        parts.append("-DCMAKE_C_COMPILER_LAUNCHER=ccache")
        parts.append("-DCMAKE_CXX_COMPILER_LAUNCHER=ccache")

    if compiler is not None:
        m = _COMPILER_RE.match(compiler)
        if m:
            family = m.group(1)
            suffix = m.group(2) or ""
            if family == "gcc":
                parts.append(f"-DCMAKE_C_COMPILER=gcc{suffix}")
                parts.append(f"-DCMAKE_CXX_COMPILER=g++{suffix}")
            else:
                parts.append(f"-DCMAKE_C_COMPILER=clang{suffix}")
                parts.append(f"-DCMAKE_CXX_COMPILER=clang++{suffix}")

    if defines:
        for k, v in defines.items():
            parts.append(f"-D{k}={v}")

    return " ".join(parts)


def _build_cmd(
    path: str,
    target: str | None,
    build_dir: str = "build",
    *,
    relative: bool = False,
) -> str:
    """Build the cmake --build command.

    When ``relative=True``, omit the ``path/`` prefix — for use inside a shell
    that already ``cd``-ed into ``path``.
    """
    prefix = build_dir if relative else f"{path}/{build_dir}"
    cmd = f"cmake --build {prefix} --parallel $(nproc)"
    if target is not None:
        cmd += f" --target {target}"
    return cmd


@dataclass(frozen=True)
class CMakeProject:
    """CMake project CI DAG — constructed via ``hm.cmake(path=...)`` or
    ``CMakeToolchain.project(...)``.

    Holds the toolchain reference and a pre-built warmup step. Action
    methods branch off the appropriate ancestor for DAG parallelism.
    """

    toolchain: CMakeToolchain
    built: Step
    path: str
    _build_cmd: str

    def build(self) -> Step:
        """Return the warmup step (configure+build, cached)."""
        return self.built

    def test(self, *, parallel: bool = True, **kw: Any) -> Step:
        """Incremental build + ctest. Branches off ``built``."""
        par_flag = " --parallel $(nproc)" if parallel else ""
        cmd = (
            f"{self._build_cmd} && "
            f"ctest --test-dir {self.path}/build --output-on-failure{par_flag}"
        )
        if kw.get("label") is None:
            kw["label"] = ":cmake: test"
        return self.built.sh(cmd, **kw)

    def install(self, *, prefix: str | None = None, **kw: Any) -> Step:
        """cmake --install. Branches off ``built``."""
        prefix_flag = f" --prefix {prefix}" if prefix else ""
        cmd = f"cmake --install {self.path}/build{prefix_flag}"
        if kw.get("label") is None:
            kw["label"] = ":cmake: install"
        return self.built.sh(cmd, **kw)

    def fmt(self, *, fix: bool = False, **kw: Any) -> Step:
        """clang-format check. Branches off ``toolchain.installed`` (NOT built)."""
        mode = "-i" if fix else "--dry-run --Werror"
        cmd = (
            f"cd {self.path} && find . -not -path './build/*'"
            f" -name '*.c' -o -name '*.h'"
            f" -o -name '*.cpp' -o -name '*.hpp' -o -name '*.cc' -o -name '*.cxx' | "
            f"xargs clang-format {mode}"
        )
        if kw.get("label") is None:
            kw["label"] = ":cmake: fmt"
        return self.toolchain.installed.sh(cmd, **kw)

    def lint(self, **kw: Any) -> Step:
        """run-clang-tidy. Branches off ``built``."""
        cmd = f"cd {self.path} && run-clang-tidy -p build"
        if kw.get("label") is None:
            kw["label"] = ":cmake: lint"
        return self.built.sh(cmd, **kw)

    def package(self, *, generator: str | None = None, **kw: Any) -> Step:
        """cpack. Branches off ``built``."""
        gen_flag = f" -G {generator}" if generator else ""
        cmd = f"cd {self.path}/build && cpack{gen_flag}"
        if kw.get("label") is None:
            kw["label"] = ":cmake: package"
        return self.built.sh(cmd, **kw)


@dataclass(frozen=True)
class CMakeToolchain:
    """CMake toolchain install chain — constructed via ``hm.cmake()`` with no path.

    Holds the verified cmake-install step. Spawn ``CMakeProject`` instances
    via ``.project(...)``; all projects from one toolchain share the same
    install step.
    """

    installed: Step
    compiler: str | None
    ccache: bool
    generator: str

    def project(
        self,
        *,
        path: str = ".",
        preset: str | None = None,
        defines: dict[str, str] | None = None,
        deps: str | None = None,
        target: str | None = None,
        cache: CachePolicy | None = None,
    ) -> CMakeProject:
        """Create a ``CMakeProject`` from this toolchain.

        Args:
            path: Path to the project root (where ``CMakeLists.txt`` lives).
            preset: CMake preset name. When set, configure uses ``--preset``.
            defines: ``-D`` key=value pairs passed to cmake configure.
            deps: Dependency manager. ``"vcpkg"`` inserts a vcpkg-install step.
            target: Build only this target (``--target``).
            cache: Override the warmup step's cache policy.

        Returns:
            A ``CMakeProject`` ready for action methods.
        """
        if deps is not None and deps != "vcpkg":
            msg = f'hm.cmake: invalid deps {deps!r}\n  → use "vcpkg" or None'
            raise ValueError(msg)
        configure = _configure_cmd(
            path=path,
            preset=preset,
            defines=defines,
            compiler=self.compiler,
            ccache=self.ccache,
            generator=self.generator,
        )
        build_abs = _build_cmd(path, target=target)
        build_rel = _build_cmd(path, target=target, relative=True)
        warmup_cmd = f"{configure} && {build_rel}"

        # Determine warmup cache policy
        if cache is not None:
            warmup_cache: CachePolicy = cache
        elif deps == "vcpkg":
            warmup_cache = CacheOnChange(paths=("vcpkg.json",))
        else:
            cmakelists = f"{path}/CMakeLists.txt" if path != "." else "CMakeLists.txt"
            warmup_cache = CacheOnChange(paths=(cmakelists,))

        # Determine the parent for the warmup step
        if deps == "vcpkg":
            # Insert vcpkg-install step between toolchain.installed and warmup
            vcpkg_cmd = (
                "git clone https://github.com/microsoft/vcpkg.git /opt/vcpkg && "
                "/opt/vcpkg/bootstrap-vcpkg.sh && "
                f"cd {path} && /opt/vcpkg/vcpkg install"
            )
            vcpkg_step = self.installed.sh(
                vcpkg_cmd,
                label=":cmake: vcpkg",
                cache=CacheOnChange(paths=("vcpkg.json",)),
            )
            warmup_parent = vcpkg_step
        else:
            warmup_parent = self.installed

        built = warmup_parent.sh(
            warmup_cmd,
            label=":cmake: build",
            cache=warmup_cache,
        )

        return CMakeProject(
            toolchain=self,
            built=built,
            path=path,
            _build_cmd=build_abs,
        )


def _make_toolchain(
    *,
    compiler: str | None = None,
    ccache: bool = True,
    generator: str = "ninja",
    image: str | None = None,
    base: Step | None = None,
) -> CMakeToolchain:
    """Build the apt-base + cmake-verify chain and return a CMakeToolchain."""
    if generator not in ("ninja", "make"):
        msg = f'hm.cmake: invalid generator {generator!r}\n  → use "ninja" or "make"'
        raise ValueError(msg)
    if compiler is not None and not _COMPILER_RE.match(compiler):
        msg = (
            f"hm.cmake: invalid compiler {compiler!r}\n"
            '  → use "gcc", "gcc-14", "clang", or "clang-18"'
        )
        raise ValueError(msg)

    apt_pkgs = _apt_packages(compiler, ccache=ccache, generator=generator)
    verify = _verify_cmd(compiler, ccache=ccache, generator=generator)

    installed = make_install_chain(
        apt_packages=apt_pkgs,
        install_cmd=verify,
        install_cache=CacheForever(env_keys=()),
        lang_tag="cmake",
        install_tag="verify",
        image=image,
        base=base,
    )
    return CMakeToolchain(
        installed=installed,
        compiler=compiler,
        ccache=ccache,
        generator=generator,
    )


class CMakeEntry:
    """Callable singleton for the CMake toolchain — access as ``hm.cmake``.

    Supports three usage forms:

    - Toolchain form: ``hm.cmake()`` returns a ``CMakeToolchain`` shared
      across multiple projects.
    - Project form: ``hm.cmake(path=".")`` returns a ``CMakeProject`` directly.
    - Bare form: ``hm.cmake.build()``, ``hm.cmake.test()``, etc. for one-shot leaves.
    """

    @overload
    def __call__(
        self,
        *,
        compiler: str | None = ...,
        ccache: bool = ...,
        generator: str = ...,
        image: str | None = ...,
        base: Step | None = ...,
    ) -> CMakeToolchain: ...

    @overload
    def __call__(
        self,
        *,
        path: str,
        compiler: str | None = ...,
        ccache: bool = ...,
        generator: str = ...,
        preset: str | None = ...,
        defines: dict[str, str] | None = ...,
        deps: str | None = ...,
        target: str | None = ...,
        cache: CachePolicy | None = ...,
        image: str | None = ...,
        base: Step | None = ...,
    ) -> CMakeProject: ...

    def __call__(
        self,
        *,
        path: str | None = None,
        compiler: str | None = None,
        ccache: bool = True,
        generator: str = "ninja",
        preset: str | None = None,
        defines: dict[str, str] | None = None,
        deps: str | None = None,
        target: str | None = None,
        cache: CachePolicy | None = None,
        image: str | None = None,
        base: Step | None = None,
    ) -> CMakeToolchain | CMakeProject:
        """Install CMake and return a toolchain or project.

        Returns a ``CMakeToolchain`` when ``path`` is omitted, or a
        ``CMakeProject`` when ``path`` is provided.

        Args:
            path: Project root (where ``CMakeLists.txt`` lives). Omit to
                get a reusable ``CMakeToolchain``.
            compiler: Compiler to install and use (e.g. ``"gcc-14"``,
                ``"clang-18"``). None = system default.
            ccache: Enable ccache (default True).
            generator: Build system generator: ``"ninja"`` (default) or
                ``"make"``.
            preset: CMake preset name (overrides manual configure flags).
            defines: ``-D`` key=value pairs (e.g.
                ``{"CMAKE_BUILD_TYPE": "Release"}``).
            deps: Dependency manager (``"vcpkg"`` supported).
            target: Build only this target.
            cache: Override warmup cache policy.
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach to instead of a fresh apt-base.

        Returns:
            A ``CMakeToolchain`` or ``CMakeProject``.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.cmake(path=".", compiler="gcc-14")
            >>> hm.pipeline([proj.build(), proj.test()])
        """
        tc = _make_toolchain(
            compiler=compiler,
            ccache=ccache,
            generator=generator,
            image=image,
            base=base,
        )
        if path is None:
            return tc
        return tc.project(
            path=path,
            preset=preset,
            defines=defines,
            deps=deps,
            target=target,
            cache=cache,
        )

    def _project(self, **kw: Any) -> CMakeProject:
        path = kw.pop("path", ".")
        proj = self(path=path, **kw)
        assert isinstance(proj, CMakeProject)  # noqa: S101 — narrow overload result
        return proj

    def build(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        proj = self._project(**kw)
        # For bare-form, build() returns the warmup step directly
        if action_kw.get("label") is None:
            action_kw["label"] = ":cmake: build"
        return proj.built

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).test(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).fmt(**action_kw)

    def lint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).lint(**action_kw)

    def install(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).install(**action_kw)

    def package(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).package(**action_kw)


cmake: CMakeEntry = CMakeEntry()
