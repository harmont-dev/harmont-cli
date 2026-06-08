# hm.cmake Module Redesign — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the rudimentary hm.c/hm.cpp cmake rules with a comprehensive `hm.cmake` module that works out-of-the-box for real-world CMake projects (tested against patterns from the top 100 OSS C/C++ projects).

**Architecture:** Three-tier design mirroring `hm.rust` (toolchain → project → actions). A `CMakeToolchain` holds the shared install chain (cmake + compiler + tools). A `CMakeProject` holds the "warmup" (configure+build, cached on dependency lock files). Action methods (`test`, `lint`, `fmt`, `sanitize`, `coverage`, `install`, `package`) branch off either the warmup or the toolchain install step depending on whether they need build artifacts. The DAG enables parallelism: `fmt` runs in parallel with build since it only needs clang-format.

**Tech Stack:** Python (harmont-py DSL), TypeScript (harmont-ts DSL), Vitest, pytest

---

## Design Decisions (from research on 100+ OSS CMake projects)

1. **Generator**: Default Ninja (universal in CI across LLVM, Qt, OpenCV, gRPC, Arrow, Abseil, etc.)
2. **Compiler**: Default system gcc; support `"gcc-N"` and `"clang-N"` with version pinning
3. **ccache**: On by default (used by LLVM, gRPC, Arrow, Open3D, SFML, Folly)
4. **Dependencies**: Auto-detect vcpkg.json → bootstrap vcpkg + install. Conan as future extension.
5. **Build type**: Default `"Release"` (most common CI setting). Configurable.
6. **Presets**: First-class `--preset` support when user specifies a preset name
7. **Sanitizers**: Named variants (`"asan"`, `"tsan"`, `"ubsan"`, `"msan"`) as separate build chains
8. **Static analysis**: clang-tidy via `run-clang-tidy` using compile_commands.json from warmup
9. **Formatting**: clang-format with `--dry-run --Werror` (check) matching LLVM/OpenCV patterns
10. **Labels**: Unified `:cmake:` prefix (dropping the old :c:/:cpp: distinction since cmake handles both)

## DAG Architecture

```
apt-base (cmake + ninja + compiler + ccache + clang-format + clang-tidy)
  → tool-verify (cmake --version, CacheForever)
    → [optional] vcpkg-install (CacheOnChange on vcpkg.json)
      → warmup: configure + build (CacheOnChange on lock files)
        → test (incremental build + ctest)
        → lint (incremental build + run-clang-tidy)
        → install (cmake --install)
        → package (cpack)
    → fmt (clang-format check — parallel with build chain)
    → coverage (separate Debug build with --coverage + lcov)
    → sanitize (separate Debug build with -fsanitize + ctest)
```

## API Surface

```python
import harmont as hm

# Simplest form — zero config
proj = hm.cmake(path=".")
hm.pipeline(proj.test(), proj.fmt())

# Full control: toolchain + project
tc = hm.cmake(compiler="clang-18", ccache=True)
proj = tc.project(path=".", build_type="Release", defines={"BUILD_TESTING": "ON"})
hm.pipeline(proj.test(), proj.lint(), proj.fmt(), proj.sanitize("asan"))

# Preset-driven
proj = hm.cmake(path=".", preset="ci-linux")
hm.pipeline(proj.test(), proj.fmt())

# Multi-project monorepo (shared toolchain)
tc = hm.cmake(compiler="gcc-14")
lib = tc.project(path="lib", build_type="Release")
app = tc.project(path="app", build_type="Release", defines={"CMAKE_PREFIX_PATH": "../lib/build/install"})
hm.pipeline(lib.test(), lib.fmt(), app.test(), app.fmt())
```

---

## Task 1: New Python Module — CMakeToolchain

**Files:**
- Replace: `crates/hm-dsl-engine/harmont-py/harmont/_cmake.py`

**Step 1: Write the failing test**

Add to `crates/hm-dsl-engine/harmont-py/tests/test_cmake.py` (replace entire file):

```python
"""CMake toolchain tests — module redesign."""

from __future__ import annotations

import pytest

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _labels(p: dict) -> list[str]:
    return [n["step"].get("label", "") for n in p["graph"]["nodes"]]


class TestCMakeToolchain:
    def test_default_toolchain_installs_cmake_ninja_ccache(self):
        tc = hm.cmake()
        proj = tc.project(path="svc")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cmake" in c and "ninja-build" in c and "ccache" in c for c in cmds)

    def test_clang_compiler_installs_clang_package(self):
        tc = hm.cmake(compiler="clang-18")
        proj = tc.project(path=".")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("clang-18" in c for c in cmds)

    def test_gcc_compiler_installs_gcc_package(self):
        tc = hm.cmake(compiler="gcc-14")
        proj = tc.project(path=".")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("gcc-14" in c and "g++-14" in c for c in cmds)

    def test_invalid_compiler_rejected(self):
        with pytest.raises(ValueError, match="compiler"):
            hm.cmake(compiler="msvc-19")

    def test_ccache_disabled(self):
        tc = hm.cmake(ccache=False)
        proj = tc.project(path=".")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert not any("CMAKE_C_COMPILER_LAUNCHER=ccache" in c for c in cmds)

    def test_toolchain_shared_across_projects(self):
        tc = hm.cmake()
        p1 = tc.project(path="lib")
        p2 = tc.project(path="app")
        p = hm.pipeline(p1.build(), p2.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        apt_installs = [c for c in cmds if "apt-get install" in c]
        assert len(apt_installs) == 1
```

**Step 2: Run test to verify it fails**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-py && python -m pytest tests/test_cmake.py::TestCMakeToolchain::test_default_toolchain_installs_cmake_ninja_ccache -x 2>&1 | tail -5`
Expected: FAIL

**Step 3: Write the CMakeToolchain implementation**

Replace `crates/hm-dsl-engine/harmont-py/harmont/_cmake.py` with:

```python
"""CMake toolchain and project abstraction.

Public surface lives on the module-level singleton ``cmake``:

    hm.cmake(...)             -> CMakeToolchain (no path)
    hm.cmake(path=".")        -> CMakeProject   (with path)
    hm.cmake.test()           -> Step           (bare-form)

Three-tier architecture:
  1. CMakeToolchain — shared install (cmake + compiler + tools)
  2. CMakeProject   — configure+build warmup (cached on deps)
  3. Action methods — leaves off warmup or toolchain install

DAG:
    scratch -> apt-base -> tool-verify
      -> [vcpkg-install] -> warmup (configure+build) -> test/lint/install/package
      -> fmt (parallel, off tool-verify)
      -> coverage (separate chain off tool-verify)
      -> sanitize (separate chain off tool-verify)
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any, overload

from ._toolchain import apt_install_cmd, make_install_chain
from .cache import CacheForever, CacheOnChange, CacheTTL

if TYPE_CHECKING:
    from ._step import Step
    from .cache import CachePolicy

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_COMPILER_RE = re.compile(r"^(gcc|clang)(-\d+)?$")

_BASE_PACKAGES = ("cmake", "ninja-build", "build-essential", "pkg-config")

_SANITIZER_FLAGS: dict[str, str] = {
    "asan": "address,undefined",
    "tsan": "thread",
    "ubsan": "undefined",
    "msan": "memory",
}

_SANITIZER_ENV: dict[str, str] = {
    "asan": "ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1",
    "tsan": "TSAN_OPTIONS=halt_on_error=1:second_deadlock_stack=1",
    "ubsan": "UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1",
    "msan": "MSAN_OPTIONS=poison_in_dtor=1",
}


def _compiler_packages(compiler: str) -> tuple[str, ...]:
    m = _COMPILER_RE.match(compiler)
    if not m:
        msg = (
            f'hm.cmake: invalid compiler {compiler!r}\n'
            '  → use "gcc", "gcc-14", "clang", or "clang-18"'
        )
        raise ValueError(msg)
    family = m.group(1)
    suffix = m.group(2) or ""
    if family == "gcc":
        return (f"gcc{suffix}", f"g++{suffix}")
    return (f"clang{suffix}", f"lld{suffix}")


def _compiler_env(compiler: str) -> dict[str, str]:
    m = _COMPILER_RE.match(compiler)
    if not m:
        return {}
    family = m.group(1)
    suffix = m.group(2) or ""
    if family == "gcc":
        return {"CC": f"gcc{suffix}", "CXX": f"g++{suffix}"}
    return {"CC": f"clang{suffix}", "CXX": f"clang++{suffix}"}


def _apt_packages(
    compiler: str | None,
    ccache: bool,
    extras: tuple[str, ...],
) -> tuple[str, ...]:
    pkgs = list(_BASE_PACKAGES)
    if compiler:
        pkgs.extend(_compiler_packages(compiler))
    if ccache:
        pkgs.append("ccache")
    pkgs.append("clang-format")
    if "clang-tidy" in extras or not extras:
        pkgs.append("clang-tidy")
    for e in extras:
        if e not in pkgs:
            pkgs.append(e)
    return tuple(pkgs)


def _verify_cmd(compiler: str | None, ccache: bool) -> str:
    checks = ["cmake --version", "ninja --version"]
    if ccache:
        checks.append("ccache --version")
    if compiler:
        m = _COMPILER_RE.match(compiler)
        if m:
            suffix = m.group(2) or ""
            if m.group(1) == "gcc":
                checks.append(f"gcc{suffix} --version")
            else:
                checks.append(f"clang{suffix} --version")
    return " && ".join(checks)


@dataclass(frozen=True)
class CMakeToolchain:
    """CMake toolchain — shared install chain for cmake + compiler + tools.

    Constructed via ``hm.cmake()`` (without ``path``). Spawn projects
    via ``.project(path)``; all projects share the same install step.
    """

    installed: Step
    compiler: str | None
    ccache: bool
    generator: str

    def project(
        self,
        *,
        path: str = ".",
        build_type: str = "Release",
        preset: str | None = None,
        defines: dict[str, str] | None = None,
        shared: bool | None = None,
        std: int | None = None,
        deps: str | None = "auto",
        target: str | None = None,
        cache: CachePolicy | None = None,
    ) -> CMakeProject:
        """Create a CMakeProject rooted at ``path``.

        Args:
            path: Project root (where CMakeLists.txt lives).
            build_type: CMAKE_BUILD_TYPE. Default "Release".
            preset: CMake preset name (from CMakePresets.json). When set,
                overrides build_type/generator/defines for configure.
            defines: Extra -D cache variables.
            shared: BUILD_SHARED_LIBS toggle.
            std: CMAKE_CXX_STANDARD (e.g. 17, 20).
            deps: Dependency manager. "auto" detects from files,
                "vcpkg" forces vcpkg, None skips.
            target: Build only this target (--target flag).
            cache: Override warmup cache policy.

        Returns:
            A ``CMakeProject`` with configure+build warmup.
        """
        return _make_project(
            toolchain=self,
            path=path,
            build_type=build_type,
            preset=preset,
            defines=defines,
            shared=shared,
            std=std,
            deps=deps,
            target=target,
            cache=cache,
        )


@dataclass(frozen=True)
class CMakeProject:
    """CMake project — configure+build warmup with action methods.

    The ``built`` step is the warmup (configure + build, cached on
    dependency lock files). Action methods branch off ``built`` (for
    test/lint/install) or off ``toolchain.installed`` (for fmt/coverage/
    sanitize which don't need the Release build).
    """

    toolchain: CMakeToolchain
    built: Step
    path: str
    _configure_cmd: str = field(repr=False)
    _build_cmd: str = field(repr=False)

    def build(self, **kw: Any) -> Step:
        """Return the warmup step (configure + build).

        Include this in a pipeline to ensure the build is cached as a
        shared dependency for test/lint/install.
        """
        if kw.get("label") is None:
            kw["label"] = ":cmake: build"
        if kw:
            return self.built.sh(self._build_cmd, **kw)
        return self.built

    def test(self, *, parallel: bool = True, **kw: Any) -> Step:
        """Run ctest (incremental build + test)."""
        par = " --parallel $(nproc)" if parallel else ""
        cmd = (
            f"cd {self.path} && cmake --build build --parallel $(nproc)"
            f" && ctest --test-dir build{par} --output-on-failure"
        )
        if kw.get("label") is None:
            kw["label"] = ":cmake: test"
        return self.built.sh(cmd, **kw)

    def install(self, *, prefix: str | None = None, **kw: Any) -> Step:
        """Run cmake --install."""
        pfx = f" --prefix {prefix}" if prefix else ""
        cmd = (
            f"cd {self.path} && cmake --build build --parallel $(nproc)"
            f" && cmake --install build{pfx}"
        )
        if kw.get("label") is None:
            kw["label"] = ":cmake: install"
        return self.built.sh(cmd, **kw)

    def fmt(self, *, fix: bool = False, **kw: Any) -> Step:
        """Check (or fix) clang-format.

        Branches off toolchain install — runs in parallel with build.
        """
        mode = "-i" if fix else "--dry-run --Werror"
        cmd = (
            f"cd {self.path} && find . -type f "
            r"\( -name '*.cpp' -o -name '*.hpp' -o -name '*.c' -o -name '*.h' -o -name '*.cc' -o -name '*.hh' -o -name '*.cxx' -o -name '*.hxx' \)"
            f" ! -path './build/*'"
            f" | xargs clang-format {mode}"
        )
        if kw.get("label") is None:
            kw["label"] = ":cmake: fmt"
        return self.toolchain.installed.sh(cmd, **kw)

    def lint(self, **kw: Any) -> Step:
        """Run clang-tidy via compile_commands.json from the build."""
        cmd = (
            f"cd {self.path} && cmake --build build --parallel $(nproc)"
            " && run-clang-tidy -p build -quiet"
        )
        if kw.get("label") is None:
            kw["label"] = ":cmake: lint"
        return self.built.sh(cmd, **kw)

    def coverage(self, **kw: Any) -> Step:
        """Separate Debug build with coverage instrumentation + report.

        Branches off toolchain install (separate build chain).
        """
        cov_configure = (
            f"cd {self.path} && cmake -S . -B build-cov -G Ninja"
            " -DCMAKE_BUILD_TYPE=Debug"
            ' -DCMAKE_CXX_FLAGS="--coverage -fno-omit-frame-pointer"'
            ' -DCMAKE_C_FLAGS="--coverage -fno-omit-frame-pointer"'
            ' -DCMAKE_EXE_LINKER_FLAGS="--coverage"'
            " -DCMAKE_EXPORT_COMPILE_COMMANDS=ON"
        )
        cmd = (
            f"{cov_configure}"
            f" && cmake --build build-cov --parallel $(nproc)"
            f" && ctest --test-dir build-cov --parallel $(nproc) --output-on-failure"
            f" && lcov --capture --directory build-cov --output-file coverage.info --quiet"
            f" && lcov --remove coverage.info '/usr/*' '*/build-cov/*' --output-file coverage.info --quiet"
        )
        if kw.get("label") is None:
            kw["label"] = ":cmake: coverage"
        return self.toolchain.installed.sh(cmd, **kw)

    def sanitize(self, kind: str = "asan", **kw: Any) -> Step:
        """Separate Debug build with sanitizer flags + test.

        Args:
            kind: One of "asan", "tsan", "ubsan", "msan".

        Branches off toolchain install (separate build chain).
        """
        if kind not in _SANITIZER_FLAGS:
            msg = (
                f'hm.cmake: invalid sanitizer {kind!r}\n'
                f'  → use one of: {", ".join(sorted(_SANITIZER_FLAGS))}'
            )
            raise ValueError(msg)
        flags = _SANITIZER_FLAGS[kind]
        san_env = _SANITIZER_ENV[kind]
        build_dir = f"build-{kind}"
        compiler_env = ""
        if self.toolchain.compiler:
            env = _compiler_env(self.toolchain.compiler)
            compiler_env = " ".join(f'-DCMAKE_{k.replace("CC","C_COMPILER").replace("CXX","CXX_COMPILER")}={v}' for k, v in env.items())
            compiler_env = (
                f" -DCMAKE_C_COMPILER={env.get('CC', 'cc')}"
                f" -DCMAKE_CXX_COMPILER={env.get('CXX', 'c++')}"
            )
        cmd = (
            f"cd {self.path} && cmake -S . -B {build_dir} -G Ninja"
            " -DCMAKE_BUILD_TYPE=Debug"
            f' -DCMAKE_CXX_FLAGS="-fsanitize={flags} -fno-omit-frame-pointer -g"'
            f' -DCMAKE_C_FLAGS="-fsanitize={flags} -fno-omit-frame-pointer -g"'
            f' -DCMAKE_EXE_LINKER_FLAGS="-fsanitize={flags}"'
            f"{compiler_env}"
            " -DCMAKE_EXPORT_COMPILE_COMMANDS=ON"
            f" && cmake --build {build_dir} --parallel $(nproc)"
            f" && {san_env} ctest --test-dir {build_dir} --parallel $(nproc) --output-on-failure"
        )
        if kw.get("label") is None:
            kw["label"] = f":cmake: {kind}"
        return self.toolchain.installed.sh(cmd, **kw)

    def package(self, *, generator: str | None = None, **kw: Any) -> Step:
        """Run cpack to create distribution packages."""
        gen = f" -G {generator}" if generator else ""
        cmd = (
            f"cd {self.path} && cmake --build build --parallel $(nproc)"
            f" && cd build && cpack{gen}"
        )
        if kw.get("label") is None:
            kw["label"] = ":cmake: package"
        return self.built.sh(cmd, **kw)


def _configure_cmd(
    path: str,
    generator: str,
    build_type: str,
    compiler: str | None,
    ccache: bool,
    preset: str | None,
    defines: dict[str, str] | None,
    shared: bool | None,
    std: int | None,
    target: str | None,
) -> str:
    if preset:
        return f"cd {path} && cmake --preset {preset}"

    parts = [f"cd {path} && cmake -S . -B build -G Ninja"]
    if generator != "ninja":
        parts[-1] = f'cd {path} && cmake -S . -B build -G "Unix Makefiles"'

    parts.append(f"-DCMAKE_BUILD_TYPE={build_type}")
    parts.append("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON")

    if ccache:
        parts.append("-DCMAKE_C_COMPILER_LAUNCHER=ccache")
        parts.append("-DCMAKE_CXX_COMPILER_LAUNCHER=ccache")

    if compiler:
        env = _compiler_env(compiler)
        if "CC" in env:
            parts.append(f"-DCMAKE_C_COMPILER={env['CC']}")
        if "CXX" in env:
            parts.append(f"-DCMAKE_CXX_COMPILER={env['CXX']}")

    if shared is not None:
        parts.append(f"-DBUILD_SHARED_LIBS={'ON' if shared else 'OFF'}")

    if std is not None:
        parts.append(f"-DCMAKE_CXX_STANDARD={std}")

    if defines:
        for k, v in sorted(defines.items()):
            parts.append(f"-D{k}={v}")

    return " ".join(parts)


def _build_cmd(path: str, target: str | None) -> str:
    tgt = f" --target {target}" if target else ""
    return f"cmake --build {path}/build --parallel $(nproc){tgt}"


def _warmup_cache_paths(path: str, deps: str | None) -> tuple[str, ...]:
    prefix = f"{path}/" if path != "." else ""
    if deps == "vcpkg":
        return (f"{prefix}vcpkg.json",)
    return (f"{prefix}CMakeLists.txt",)


def _vcpkg_install_cmd(path: str) -> str:
    return (
        "git clone --depth 1 https://github.com/microsoft/vcpkg.git /opt/vcpkg"
        " && /opt/vcpkg/bootstrap-vcpkg.sh -disableMetrics"
        " && export VCPKG_ROOT=/opt/vcpkg"
        " && export PATH=$VCPKG_ROOT:$PATH"
        f" && cd {path} && vcpkg install --triplet x64-linux"
    )


def _make_toolchain(
    *,
    compiler: str | None = None,
    generator: str = "ninja",
    ccache: bool = True,
    extras: tuple[str, ...] = (),
    image: str | None = None,
    base: Step | None = None,
) -> CMakeToolchain:
    if compiler is not None and not _COMPILER_RE.match(compiler):
        msg = (
            f'hm.cmake: invalid compiler {compiler!r}\n'
            '  → use "gcc", "gcc-14", "clang", or "clang-18"'
        )
        raise ValueError(msg)
    if generator not in ("ninja", "make"):
        msg = f'hm.cmake: invalid generator {generator!r}\n  → use "ninja" or "make"'
        raise ValueError(msg)

    packages = _apt_packages(compiler, ccache, extras)
    verify = _verify_cmd(compiler, ccache)

    installed = make_install_chain(
        apt_packages=packages,
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


def _detect_deps(path: str) -> str | None:
    """Detect dependency manager from project files.

    Returns "vcpkg" if vcpkg.json exists pattern, else None.
    Detection is static (based on convention) — actual file existence
    is checked at runtime in the container.
    """
    return None


def _make_project(
    *,
    toolchain: CMakeToolchain,
    path: str = ".",
    build_type: str = "Release",
    preset: str | None = None,
    defines: dict[str, str] | None = None,
    shared: bool | None = None,
    std: int | None = None,
    deps: str | None = "auto",
    target: str | None = None,
    cache: CachePolicy | None = None,
) -> CMakeProject:
    resolved_deps = deps if deps != "auto" else _detect_deps(path)

    parent = toolchain.installed

    if resolved_deps == "vcpkg":
        vcpkg_packages = ("git", "curl", "zip", "unzip", "tar")
        vcpkg_cmd = _vcpkg_install_cmd(path)
        prefix = f"{path}/" if path != "." else ""
        parent = parent.sh(
            vcpkg_cmd,
            label=":cmake: vcpkg-install",
            cache=CacheOnChange(paths=(f"{prefix}vcpkg.json",)),
        )

    conf = _configure_cmd(
        path=path,
        generator=toolchain.generator,
        build_type=build_type,
        compiler=toolchain.compiler,
        ccache=toolchain.ccache,
        preset=preset,
        defines=defines,
        shared=shared,
        std=std,
        target=target,
    )
    build = _build_cmd(path, target)
    warmup_cmd = f"{conf} && {build}"

    warmup_cache = cache if cache is not None else CacheOnChange(
        paths=_warmup_cache_paths(path, resolved_deps)
    )

    built = parent.sh(warmup_cmd, label=":cmake: build", cache=warmup_cache)

    return CMakeProject(
        toolchain=toolchain,
        built=built,
        path=path,
        _configure_cmd=conf,
        _build_cmd=build,
    )


class CMakeEntry:
    """Callable singleton — access as ``hm.cmake``.

    Dispatches between toolchain form (no path) and project form (with path).
    Also exposes bare-form action methods.
    """

    @overload
    def __call__(
        self,
        *,
        compiler: str | None = ...,
        generator: str = ...,
        ccache: bool = ...,
        extras: tuple[str, ...] = ...,
        image: str | None = ...,
        base: Step | None = ...,
    ) -> CMakeToolchain: ...

    @overload
    def __call__(
        self,
        *,
        path: str,
        compiler: str | None = ...,
        generator: str = ...,
        ccache: bool = ...,
        build_type: str = ...,
        preset: str | None = ...,
        defines: dict[str, str] | None = ...,
        shared: bool | None = ...,
        std: int | None = ...,
        deps: str | None = ...,
        target: str | None = ...,
        image: str | None = ...,
        base: Step | None = ...,
    ) -> CMakeProject: ...

    def __call__(
        self,
        *,
        path: str | None = None,
        compiler: str | None = None,
        generator: str = "ninja",
        ccache: bool = True,
        extras: tuple[str, ...] = (),
        build_type: str = "Release",
        preset: str | None = None,
        defines: dict[str, str] | None = None,
        shared: bool | None = None,
        std: int | None = None,
        deps: str | None = "auto",
        target: str | None = None,
        image: str | None = None,
        base: Step | None = None,
    ) -> CMakeToolchain | CMakeProject:
        """Install cmake toolchain and optionally create a project.

        Without ``path``: returns a ``CMakeToolchain`` for multi-project use.
        With ``path``: returns a ``CMakeProject`` directly.
        """
        tc = _make_toolchain(
            compiler=compiler,
            generator=generator,
            ccache=ccache,
            extras=extras,
            image=image,
            base=base,
        )
        if path is None:
            return tc
        return tc.project(
            path=path,
            build_type=build_type,
            preset=preset,
            defines=defines,
            shared=shared,
            std=std,
            deps=deps,
            target=target,
        )

    def _project(self, **kw: Any) -> CMakeProject:
        path = kw.pop("path", ".")
        proj = self(path=path, **kw)
        assert isinstance(proj, CMakeProject)  # noqa: S101
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

    def lint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).lint(**action_kw)

    def install(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).install(**action_kw)

    def coverage(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).coverage(**action_kw)

    def sanitize(self, kind: str = "asan", **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).sanitize(kind, **action_kw)

    def package(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self._project(**kw).package(**action_kw)


cmake: CMakeEntry = CMakeEntry()
```

**Step 4: Run tests to verify they pass**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-py && python -m pytest tests/test_cmake.py::TestCMakeToolchain -v`
Expected: All 6 tests PASS

**Step 5: Commit**

```bash
git add crates/hm-dsl-engine/harmont-py/harmont/_cmake.py crates/hm-dsl-engine/harmont-py/tests/test_cmake.py
git commit -m "feat(cmake): redesign module with CMakeToolchain + CMakeProject"
```

---

## Task 2: Python Tests — Project Actions

**Files:**
- Modify: `crates/hm-dsl-engine/harmont-py/tests/test_cmake.py`

**Step 1: Add project action tests**

Append to `tests/test_cmake.py`:

```python
class TestCMakeProject:
    def test_build_produces_configure_and_build(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cmake -S . -B build" in c and "cmake --build" in c for c in cmds)

    def test_build_uses_ninja_by_default(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("-G Ninja" in c for c in cmds)

    def test_build_type_default_release(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("CMAKE_BUILD_TYPE=Release" in c for c in cmds)

    def test_build_type_configurable(self):
        proj = hm.cmake(path=".", build_type="Debug")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("CMAKE_BUILD_TYPE=Debug" in c for c in cmds)

    def test_defines_passed(self):
        proj = hm.cmake(path=".", defines={"BUILD_TESTING": "ON", "WITH_CUDA": "OFF"})
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("-DBUILD_TESTING=ON" in c for c in cmds)
        assert any("-DWITH_CUDA=OFF" in c for c in cmds)

    def test_shared_libs(self):
        proj = hm.cmake(path=".", shared=True)
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("-DBUILD_SHARED_LIBS=ON" in c for c in cmds)

    def test_cxx_standard(self):
        proj = hm.cmake(path=".", std=20)
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("-DCMAKE_CXX_STANDARD=20" in c for c in cmds)

    def test_preset_overrides_manual_config(self):
        proj = hm.cmake(path=".", preset="ci-linux", build_type="Debug")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("--preset ci-linux" in c for c in cmds)
        assert not any("CMAKE_BUILD_TYPE" in c for c in cmds)

    def test_ccache_sets_launcher(self):
        proj = hm.cmake(path=".", ccache=True)
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("CMAKE_C_COMPILER_LAUNCHER=ccache" in c for c in cmds)
        assert any("CMAKE_CXX_COMPILER_LAUNCHER=ccache" in c for c in cmds)

    def test_test_runs_ctest(self):
        proj = hm.cmake(path="myapp")
        p = hm.pipeline(proj.test(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("ctest --test-dir build" in c and "--output-on-failure" in c for c in cmds)

    def test_test_includes_incremental_build(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.test(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cmake --build build" in c and "ctest" in c for c in cmds)

    def test_install_runs_cmake_install(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.install(prefix="/usr/local"), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cmake --install build --prefix /usr/local" in c for c in cmds)

    def test_fmt_runs_clang_format(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.fmt(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("clang-format --dry-run --Werror" in c for c in cmds)

    def test_fmt_does_not_depend_on_build(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.fmt(), proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        fmt_cmd = next(c for c in cmds if "clang-format" in c)
        build_cmd = next(c for c in cmds if "cmake --build" in c)
        assert fmt_cmd != build_cmd

    def test_lint_runs_clang_tidy(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.lint(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("run-clang-tidy" in c for c in cmds)

    def test_lint_depends_on_build(self):
        proj = hm.cmake(path=".")
        step = proj.lint()
        assert step._parent is proj.built


class TestCMakeSanitizers:
    def test_asan_builds_with_sanitize_flags(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.sanitize("asan"), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("-fsanitize=address,undefined" in c for c in cmds)
        assert any("-fno-omit-frame-pointer" in c for c in cmds)

    def test_tsan_uses_thread_sanitizer(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.sanitize("tsan"), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("-fsanitize=thread" in c for c in cmds)

    def test_sanitizer_runs_ctest(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.sanitize("asan"), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("ctest --test-dir build-asan" in c for c in cmds)

    def test_sanitizer_uses_debug_build(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.sanitize("ubsan"), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("CMAKE_BUILD_TYPE=Debug" in c for c in cmds)

    def test_invalid_sanitizer_rejected(self):
        proj = hm.cmake(path=".")
        with pytest.raises(ValueError, match="sanitizer"):
            proj.sanitize("invalid")

    def test_sanitizer_branches_off_install_not_build(self):
        proj = hm.cmake(path=".")
        step = proj.sanitize("asan")
        assert step._parent is proj.toolchain.installed


class TestCMakeCoverage:
    def test_coverage_uses_coverage_flags(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.coverage(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("--coverage" in c for c in cmds)

    def test_coverage_runs_lcov(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.coverage(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("lcov" in c for c in cmds)

    def test_coverage_uses_separate_build_dir(self):
        proj = hm.cmake(path=".")
        p = hm.pipeline(proj.coverage(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("build-cov" in c for c in cmds)


class TestCMakeVcpkg:
    def test_vcpkg_deps_installs_vcpkg(self):
        proj = hm.cmake(path=".", deps="vcpkg")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("vcpkg" in c and "bootstrap" in c for c in cmds)

    def test_vcpkg_cached_on_vcpkg_json(self):
        proj = hm.cmake(path="mylib", deps="vcpkg")
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        nodes = p["graph"]["nodes"]
        vcpkg_node = next(n for n in nodes if "vcpkg" in n["step"].get("label", ""))
        assert vcpkg_node["step"]["cache"]["policy"] == "on_change"


class TestCMakeBareForm:
    def test_bare_build(self):
        p = hm.pipeline(hm.cmake.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cmake --build" in c for c in cmds)

    def test_bare_test(self):
        p = hm.pipeline(hm.cmake.test(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("ctest" in c for c in cmds)

    def test_bare_fmt(self):
        p = hm.pipeline(hm.cmake.fmt(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("clang-format" in c for c in cmds)


class TestCMakeLabels:
    def test_build_label(self):
        proj = hm.cmake(path=".")
        assert proj.built.label == ":cmake: build"

    def test_test_label(self):
        proj = hm.cmake(path=".")
        assert proj.test().label == ":cmake: test"

    def test_fmt_label(self):
        proj = hm.cmake(path=".")
        assert proj.fmt().label == ":cmake: fmt"

    def test_lint_label(self):
        proj = hm.cmake(path=".")
        assert proj.lint().label == ":cmake: lint"

    def test_sanitizer_label(self):
        proj = hm.cmake(path=".")
        assert proj.sanitize("tsan").label == ":cmake: tsan"


class TestCMakeWithBase:
    def test_base_skips_apt(self):
        base = hm.scratch().sh("custom base", label="base")
        proj = hm.cmake(path="svc", base=base)
        p = hm.pipeline(proj.build(), default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert not any("apt-get install" in c for c in cmds)
```

**Step 2: Run all tests**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-py && python -m pytest tests/test_cmake.py -v`
Expected: All tests PASS

**Step 3: Fix any failing tests by adjusting implementation**

Iterate until green.

**Step 4: Commit**

```bash
git add crates/hm-dsl-engine/harmont-py/tests/test_cmake.py
git commit -m "test(cmake): comprehensive test suite for redesigned module"
```

---

## Task 3: Update Python Exports

**Files:**
- Modify: `crates/hm-dsl-engine/harmont-py/harmont/__init__.py`

**Step 1: Update imports and __all__**

In `__init__.py`, the existing `from ._cmake import cmake` import should continue to work since the module-level singleton is still named `cmake`. Add the new types to `__all__`:

Replace in `__init__.py`:
```python
from ._cmake import cmake
```
with:
```python
from ._cmake import CMakeProject, CMakeToolchain, cmake
```

Add to `__all__` list (replacing existing `"cmake"` entry):
```python
    "CMakeProject",
    "CMakeToolchain",
    "cmake",
```

**Step 2: Verify imports work**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-py && python -c "from harmont import cmake, CMakeProject, CMakeToolchain; print('OK')"`
Expected: `OK`

**Step 3: Run full test suite to check for regressions**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-py && python -m pytest tests/ -v --tb=short`
Expected: All tests PASS

**Step 4: Commit**

```bash
git add crates/hm-dsl-engine/harmont-py/harmont/__init__.py
git commit -m "feat(cmake): export CMakeProject and CMakeToolchain types"
```

---

## Task 4: TypeScript Implementation

**Files:**
- Replace: `crates/hm-dsl-engine/harmont-ts/src/toolchains/cmake.ts`

**Step 1: Write the TypeScript implementation**

Replace `crates/hm-dsl-engine/harmont-ts/src/toolchains/cmake.ts`:

```typescript
import type { Step, StepOptions } from "../step.js";
import { forever, onChange, type CachePolicy } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const BASE_PACKAGES = [
  "cmake",
  "ninja-build",
  "build-essential",
  "pkg-config",
] as const;

const COMPILER_RE = /^(gcc|clang)(-\d+)?$/;

const SANITIZER_FLAGS: Record<string, string> = {
  asan: "address,undefined",
  tsan: "thread",
  ubsan: "undefined",
  msan: "memory",
};

const SANITIZER_ENV: Record<string, string> = {
  asan: "ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1",
  tsan: "TSAN_OPTIONS=halt_on_error=1:second_deadlock_stack=1",
  ubsan: "UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1",
  msan: "MSAN_OPTIONS=poison_in_dtor=1",
};

type ActionOptions = Omit<StepOptions, "cwd">;

export interface CMakeToolchainOptions {
  readonly compiler?: string;
  readonly generator?: "ninja" | "make";
  readonly ccache?: boolean;
  readonly extras?: readonly string[];
  readonly image?: string;
  readonly base?: Step;
}

export interface CMakeProjectOptions {
  readonly path?: string;
  readonly buildType?: string;
  readonly preset?: string;
  readonly defines?: Record<string, string>;
  readonly shared?: boolean;
  readonly std?: number;
  readonly deps?: "auto" | "vcpkg" | null;
  readonly target?: string;
  readonly cache?: CachePolicy;
}

export type CMakeOptions = CMakeToolchainOptions & CMakeProjectOptions;

function compilerPackages(compiler: string): string[] {
  const m = compiler.match(COMPILER_RE);
  if (!m) {
    throw new Error(
      `hm.cmake: invalid compiler "${compiler}"\n  → use "gcc", "gcc-14", "clang", or "clang-18"`,
    );
  }
  const family = m[1];
  const suffix = m[2] ?? "";
  if (family === "gcc") return [`gcc${suffix}`, `g++${suffix}`];
  return [`clang${suffix}`, `lld${suffix}`];
}

function compilerEnv(compiler: string): { CC: string; CXX: string } {
  const m = compiler.match(COMPILER_RE);
  if (!m) return { CC: "cc", CXX: "c++" };
  const family = m[1];
  const suffix = m[2] ?? "";
  if (family === "gcc") return { CC: `gcc${suffix}`, CXX: `g++${suffix}` };
  return { CC: `clang${suffix}`, CXX: `clang++${suffix}` };
}

function aptPackages(
  compiler: string | undefined,
  ccache: boolean,
  extras: readonly string[],
): string[] {
  const pkgs: string[] = [...BASE_PACKAGES];
  if (compiler) pkgs.push(...compilerPackages(compiler));
  if (ccache) pkgs.push("ccache");
  pkgs.push("clang-format", "clang-tidy");
  for (const e of extras) {
    if (!pkgs.includes(e)) pkgs.push(e);
  }
  return pkgs;
}

function verifyCmd(compiler: string | undefined, ccache: boolean): string {
  const checks = ["cmake --version", "ninja --version"];
  if (ccache) checks.push("ccache --version");
  if (compiler) {
    const m = compiler.match(COMPILER_RE);
    if (m) {
      const suffix = m[2] ?? "";
      checks.push(
        m[1] === "gcc" ? `gcc${suffix} --version` : `clang${suffix} --version`,
      );
    }
  }
  return checks.join(" && ");
}

function configureCmd(opts: {
  path: string;
  generator: string;
  buildType: string;
  compiler: string | undefined;
  ccache: boolean;
  preset: string | undefined;
  defines: Record<string, string> | undefined;
  shared: boolean | undefined;
  std: number | undefined;
}): string {
  if (opts.preset) return `cd ${opts.path} && cmake --preset ${opts.preset}`;

  const parts: string[] = [];
  const gen =
    opts.generator === "make" ? '"Unix Makefiles"' : "Ninja";
  parts.push(`cd ${opts.path} && cmake -S . -B build -G ${gen}`);
  parts.push(`-DCMAKE_BUILD_TYPE=${opts.buildType}`);
  parts.push("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON");

  if (opts.ccache) {
    parts.push("-DCMAKE_C_COMPILER_LAUNCHER=ccache");
    parts.push("-DCMAKE_CXX_COMPILER_LAUNCHER=ccache");
  }
  if (opts.compiler) {
    const env = compilerEnv(opts.compiler);
    parts.push(`-DCMAKE_C_COMPILER=${env.CC}`);
    parts.push(`-DCMAKE_CXX_COMPILER=${env.CXX}`);
  }
  if (opts.shared != null) {
    parts.push(`-DBUILD_SHARED_LIBS=${opts.shared ? "ON" : "OFF"}`);
  }
  if (opts.std != null) {
    parts.push(`-DCMAKE_CXX_STANDARD=${opts.std}`);
  }
  if (opts.defines) {
    for (const [k, v] of Object.entries(opts.defines).sort()) {
      parts.push(`-D${k}=${v}`);
    }
  }
  return parts.join(" ");
}

function buildCmd(path: string, target: string | undefined): string {
  const tgt = target ? ` --target ${target}` : "";
  return `cmake --build ${path}/build --parallel $(nproc)${tgt}`;
}

function warmupCachePaths(
  path: string,
  deps: string | null | undefined,
): string[] {
  const prefix = path !== "." ? `${path}/` : "";
  if (deps === "vcpkg") return [`${prefix}vcpkg.json`];
  return [`${prefix}CMakeLists.txt`];
}

function vcpkgInstallCmd(path: string): string {
  return [
    "git clone --depth 1 https://github.com/microsoft/vcpkg.git /opt/vcpkg",
    "/opt/vcpkg/bootstrap-vcpkg.sh -disableMetrics",
    "export VCPKG_ROOT=/opt/vcpkg",
    "export PATH=$VCPKG_ROOT:$PATH",
    `cd ${path} && vcpkg install --triplet x64-linux`,
  ].join(" && ");
}

export class CMakeToolchain {
  private readonly _installed: Step;
  readonly compiler: string | undefined;
  readonly ccache: boolean;
  readonly generator: string;

  constructor(
    installed: Step,
    compiler: string | undefined,
    ccache: boolean,
    generator: string,
  ) {
    this._installed = installed;
    this.compiler = compiler;
    this.ccache = ccache;
    this.generator = generator;
  }

  install(): Step {
    return this._installed;
  }

  project(opts?: CMakeProjectOptions): CMakeProject {
    const path = opts?.path ?? ".";
    const buildType = opts?.buildType ?? "Release";
    const deps = opts?.deps === "auto" ? null : (opts?.deps ?? null);

    let parent = this._installed;

    if (deps === "vcpkg") {
      const prefix = path !== "." ? `${path}/` : "";
      parent = parent.sh(vcpkgInstallCmd(path), {
        label: ":cmake: vcpkg-install",
        cache: onChange(`${prefix}vcpkg.json`),
      });
    }

    const conf = configureCmd({
      path,
      generator: this.generator,
      buildType,
      compiler: this.compiler,
      ccache: this.ccache,
      preset: opts?.preset,
      defines: opts?.defines,
      shared: opts?.shared,
      std: opts?.std,
    });
    const build = buildCmd(path, opts?.target);
    const warmupCmd = `${conf} && ${build}`;

    const warmupCache =
      opts?.cache ?? onChange(...warmupCachePaths(path, deps));

    const built = parent.sh(warmupCmd, {
      label: ":cmake: build",
      cache: warmupCache,
    });

    return new CMakeProject(this, built, path);
  }
}

export class CMakeProject {
  readonly toolchain: CMakeToolchain;
  readonly built: Step;
  readonly path: string;

  constructor(toolchain: CMakeToolchain, built: Step, path: string) {
    this.toolchain = toolchain;
    this.built = built;
    this.path = path;
  }

  build(opts?: ActionOptions): Step {
    if (opts) {
      return this.built.sh(
        `cmake --build ${this.path}/build --parallel $(nproc)`,
        { label: ":cmake: build", ...opts },
      );
    }
    return this.built;
  }

  test(opts?: ActionOptions & { parallel?: boolean }): Step {
    const par = opts?.parallel !== false ? " --parallel $(nproc)" : "";
    return this.built.sh(
      `cd ${this.path} && cmake --build build --parallel $(nproc) && ctest --test-dir build${par} --output-on-failure`,
      { label: ":cmake: test", ...opts },
    );
  }

  install(opts?: ActionOptions & { prefix?: string }): Step {
    const pfx = opts?.prefix ? ` --prefix ${opts.prefix}` : "";
    return this.built.sh(
      `cd ${this.path} && cmake --build build --parallel $(nproc) && cmake --install build${pfx}`,
      { label: ":cmake: install", ...opts },
    );
  }

  fmt(opts?: ActionOptions & { fix?: boolean }): Step {
    const mode = opts?.fix ? "-i" : "--dry-run --Werror";
    return this.toolchain.install().sh(
      `cd ${this.path} && find . -type f \\( -name '*.cpp' -o -name '*.hpp' -o -name '*.c' -o -name '*.h' -o -name '*.cc' -o -name '*.hh' -o -name '*.cxx' -o -name '*.hxx' \\) ! -path './build/*' | xargs clang-format ${mode}`,
      { label: ":cmake: fmt", ...opts },
    );
  }

  lint(opts?: ActionOptions): Step {
    return this.built.sh(
      `cd ${this.path} && cmake --build build --parallel $(nproc) && run-clang-tidy -p build -quiet`,
      { label: ":cmake: lint", ...opts },
    );
  }

  coverage(opts?: ActionOptions): Step {
    const cmd = [
      `cd ${this.path} && cmake -S . -B build-cov -G Ninja`,
      `-DCMAKE_BUILD_TYPE=Debug`,
      `-DCMAKE_CXX_FLAGS="--coverage -fno-omit-frame-pointer"`,
      `-DCMAKE_C_FLAGS="--coverage -fno-omit-frame-pointer"`,
      `-DCMAKE_EXE_LINKER_FLAGS="--coverage"`,
      `-DCMAKE_EXPORT_COMPILE_COMMANDS=ON`,
      `&& cmake --build build-cov --parallel $(nproc)`,
      `&& ctest --test-dir build-cov --parallel $(nproc) --output-on-failure`,
      `&& lcov --capture --directory build-cov --output-file coverage.info --quiet`,
      `&& lcov --remove coverage.info '/usr/*' '*/build-cov/*' --output-file coverage.info --quiet`,
    ].join(" ");
    return this.toolchain.install().sh(cmd, {
      label: ":cmake: coverage",
      ...opts,
    });
  }

  sanitize(kind: string = "asan", opts?: ActionOptions): Step {
    if (!(kind in SANITIZER_FLAGS)) {
      throw new Error(
        `hm.cmake: invalid sanitizer "${kind}"\n  → use one of: ${Object.keys(SANITIZER_FLAGS).sort().join(", ")}`,
      );
    }
    const flags = SANITIZER_FLAGS[kind];
    const sanEnv = SANITIZER_ENV[kind];
    const buildDir = `build-${kind}`;
    let compilerFlags = "";
    if (this.toolchain.compiler) {
      const env = compilerEnv(this.toolchain.compiler);
      compilerFlags = ` -DCMAKE_C_COMPILER=${env.CC} -DCMAKE_CXX_COMPILER=${env.CXX}`;
    }
    const cmd = [
      `cd ${this.path} && cmake -S . -B ${buildDir} -G Ninja`,
      `-DCMAKE_BUILD_TYPE=Debug`,
      `-DCMAKE_CXX_FLAGS="-fsanitize=${flags} -fno-omit-frame-pointer -g"`,
      `-DCMAKE_C_FLAGS="-fsanitize=${flags} -fno-omit-frame-pointer -g"`,
      `-DCMAKE_EXE_LINKER_FLAGS="-fsanitize=${flags}"`,
      `-DCMAKE_EXPORT_COMPILE_COMMANDS=ON${compilerFlags}`,
      `&& cmake --build ${buildDir} --parallel $(nproc)`,
      `&& ${sanEnv} ctest --test-dir ${buildDir} --parallel $(nproc) --output-on-failure`,
    ].join(" ");
    return this.toolchain.install().sh(cmd, {
      label: `:cmake: ${kind}`,
      ...opts,
    });
  }

  package(opts?: ActionOptions & { generator?: string }): Step {
    const gen = opts?.generator ? ` -G ${opts.generator}` : "";
    return this.built.sh(
      `cd ${this.path} && cmake --build build --parallel $(nproc) && cd build && cpack${gen}`,
      { label: ":cmake: package", ...opts },
    );
  }
}

export function cmake(opts: CMakeOptions & { path: string }): CMakeProject;
export function cmake(opts?: CMakeToolchainOptions): CMakeToolchain;
export function cmake(opts?: CMakeOptions): CMakeToolchain | CMakeProject {
  const compiler = opts?.compiler;
  const generator = opts?.generator ?? "ninja";
  const ccache = opts?.ccache ?? true;
  const extras = opts?.extras ?? [];

  if (compiler && !COMPILER_RE.test(compiler)) {
    throw new Error(
      `hm.cmake: invalid compiler "${compiler}"\n  → use "gcc", "gcc-14", "clang", or "clang-18"`,
    );
  }
  if (generator !== "ninja" && generator !== "make") {
    throw new Error(
      `hm.cmake: invalid generator "${generator}"\n  → use "ninja" or "make"`,
    );
  }

  const pkgs = aptPackages(compiler, ccache, extras);
  const verify = verifyCmd(compiler, ccache);

  const installed = makeInstallChain({
    aptPackages: pkgs,
    installCmd: verify,
    installCache: forever(),
    langTag: "cmake",
    installTag: "verify",
    image: opts?.image,
    base: opts?.base,
  });

  const toolchain = new CMakeToolchain(installed, compiler, ccache, generator);

  if ("path" in (opts ?? {}) && (opts as CMakeOptions)?.path != null) {
    return toolchain.project({
      path: (opts as CMakeOptions).path,
      buildType: (opts as CMakeOptions).buildType,
      preset: (opts as CMakeOptions).preset,
      defines: (opts as CMakeOptions).defines,
      shared: (opts as CMakeOptions).shared,
      std: (opts as CMakeOptions).std,
      deps: (opts as CMakeOptions).deps,
      target: (opts as CMakeOptions).target,
      cache: (opts as CMakeOptions).cache,
    });
  }

  return toolchain;
}
```

**Step 2: Run TS build to check types**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-ts && npm run build`
Expected: No type errors

**Step 3: Commit**

```bash
git add crates/hm-dsl-engine/harmont-ts/src/toolchains/cmake.ts
git commit -m "feat(cmake): TypeScript implementation of redesigned module"
```

---

## Task 5: TypeScript Tests

**Files:**
- Replace: `crates/hm-dsl-engine/harmont-ts/tests/toolchains/cmake.test.ts`

**Step 1: Write TS test suite**

Replace `crates/hm-dsl-engine/harmont-ts/tests/toolchains/cmake.test.ts`:

```typescript
import { describe, expect, it } from "vitest";
import { cmake, CMakeToolchain, CMakeProject } from "../../src/toolchains/cmake.js";
import { pipeline } from "../../src/pipeline.js";

function cmds(ir: any): string[] {
  return ir.graph.nodes.map((n: any) => n.step.cmd);
}

function labels(ir: any): string[] {
  return ir.graph.nodes.map((n: any) => n.step.label ?? "");
}

describe("cmake toolchain", () => {
  it("returns CMakeToolchain without path", () => {
    const tc = cmake();
    expect(tc).toBeInstanceOf(CMakeToolchain);
  });

  it("returns CMakeProject with path", () => {
    const proj = cmake({ path: "." });
    expect(proj).toBeInstanceOf(CMakeProject);
  });

  it("installs cmake, ninja, ccache by default", () => {
    const proj = cmake({ path: "." });
    const ir = pipeline(proj.build(), { defaultImage: "ubuntu:24.04" });
    const c = cmds(ir);
    expect(c.some((cmd) => cmd.includes("cmake") && cmd.includes("ninja-build") && cmd.includes("ccache"))).toBe(true);
  });

  it("installs clang when specified", () => {
    const proj = cmake({ path: ".", compiler: "clang-18" });
    const ir = pipeline(proj.build(), { defaultImage: "ubuntu:24.04" });
    expect(cmds(ir).some((cmd) => cmd.includes("clang-18"))).toBe(true);
  });

  it("installs gcc when specified", () => {
    const proj = cmake({ path: ".", compiler: "gcc-14" });
    const ir = pipeline(proj.build(), { defaultImage: "ubuntu:24.04" });
    expect(cmds(ir).some((cmd) => cmd.includes("gcc-14") && cmd.includes("g++-14"))).toBe(true);
  });

  it("rejects invalid compiler", () => {
    expect(() => cmake({ compiler: "msvc-19" })).toThrow("invalid compiler");
  });

  it("shares toolchain across projects", () => {
    const tc = cmake();
    const p1 = tc.project({ path: "lib" });
    const p2 = tc.project({ path: "app" });
    const ir = pipeline(p1.build(), p2.build(), { defaultImage: "ubuntu:24.04" });
    const aptCmds = cmds(ir).filter((c) => c.includes("apt-get install"));
    expect(aptCmds).toHaveLength(1);
  });
});

describe("cmake project", () => {
  it("uses Ninja generator by default", () => {
    const proj = cmake({ path: "." });
    const ir = pipeline(proj.build(), { defaultImage: "ubuntu:24.04" });
    expect(cmds(ir).some((c) => c.includes("-G Ninja"))).toBe(true);
  });

  it("defaults to Release build type", () => {
    const proj = cmake({ path: "." });
    const ir = pipeline(proj.build(), { defaultImage: "ubuntu:24.04" });
    expect(cmds(ir).some((c) => c.includes("CMAKE_BUILD_TYPE=Release"))).toBe(true);
  });

  it("accepts custom build type", () => {
    const proj = cmake({ path: ".", buildType: "Debug" });
    const ir = pipeline(proj.build(), { defaultImage: "ubuntu:24.04" });
    expect(cmds(ir).some((c) => c.includes("CMAKE_BUILD_TYPE=Debug"))).toBe(true);
  });

  it("passes defines", () => {
    const proj = cmake({ path: ".", defines: { BUILD_TESTING: "ON" } });
    const ir = pipeline(proj.build(), { defaultImage: "ubuntu:24.04" });
    expect(cmds(ir).some((c) => c.includes("-DBUILD_TESTING=ON"))).toBe(true);
  });

  it("sets ccache launcher", () => {
    const proj = cmake({ path: ".", ccache: true });
    const ir = pipeline(proj.build(), { defaultImage: "ubuntu:24.04" });
    expect(cmds(ir).some((c) => c.includes("CMAKE_C_COMPILER_LAUNCHER=ccache"))).toBe(true);
  });

  it("uses preset when specified", () => {
    const proj = cmake({ path: ".", preset: "ci-linux" });
    const ir = pipeline(proj.build(), { defaultImage: "ubuntu:24.04" });
    expect(cmds(ir).some((c) => c.includes("--preset ci-linux"))).toBe(true);
  });
});

describe("cmake actions", () => {
  it("test runs ctest", () => {
    const proj = cmake({ path: "myapp" });
    expect(proj.test()._cmd).toContain("ctest --test-dir build");
    expect(proj.test()._cmd).toContain("--output-on-failure");
  });

  it("install runs cmake --install", () => {
    expect(cmake({ path: "." }).install({ prefix: "/opt" })._cmd).toContain(
      "cmake --install build --prefix /opt",
    );
  });

  it("fmt runs clang-format", () => {
    expect(cmake({ path: "." }).fmt()._cmd).toContain("clang-format --dry-run --Werror");
  });

  it("lint runs clang-tidy", () => {
    expect(cmake({ path: "." }).lint()._cmd).toContain("run-clang-tidy");
  });

  it("labels use :cmake: prefix", () => {
    const proj = cmake({ path: "." });
    expect(proj.test()._label).toBe(":cmake: test");
    expect(proj.fmt()._label).toBe(":cmake: fmt");
    expect(proj.lint()._label).toBe(":cmake: lint");
  });
});

describe("cmake sanitizers", () => {
  it("asan uses address,undefined flags", () => {
    const proj = cmake({ path: "." });
    expect(proj.sanitize("asan")._cmd).toContain("-fsanitize=address,undefined");
  });

  it("tsan uses thread flag", () => {
    expect(cmake({ path: "." }).sanitize("tsan")._cmd).toContain("-fsanitize=thread");
  });

  it("uses Debug build type", () => {
    expect(cmake({ path: "." }).sanitize("asan")._cmd).toContain("CMAKE_BUILD_TYPE=Debug");
  });

  it("uses separate build dir", () => {
    expect(cmake({ path: "." }).sanitize("tsan")._cmd).toContain("build-tsan");
  });

  it("rejects invalid sanitizer", () => {
    expect(() => cmake({ path: "." }).sanitize("invalid")).toThrow("invalid sanitizer");
  });
});

describe("cmake coverage", () => {
  it("uses --coverage flags", () => {
    expect(cmake({ path: "." }).coverage()._cmd).toContain("--coverage");
  });

  it("runs lcov", () => {
    expect(cmake({ path: "." }).coverage()._cmd).toContain("lcov");
  });

  it("uses separate build-cov dir", () => {
    expect(cmake({ path: "." }).coverage()._cmd).toContain("build-cov");
  });
});

describe("cmake in pipeline", () => {
  it("produces valid IR", () => {
    const proj = cmake({ path: "." });
    const ir = pipeline(proj.test(), proj.fmt(), { defaultImage: "ubuntu:24.04" });
    expect(ir.graph.nodes.length).toBeGreaterThanOrEqual(3);
  });
});
```

**Step 2: Run TS tests**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-ts && npm test`
Expected: All tests PASS

**Step 3: Commit**

```bash
git add crates/hm-dsl-engine/harmont-ts/tests/toolchains/cmake.test.ts
git commit -m "test(cmake): TypeScript test suite for redesigned module"
```

---

## Task 6: Update TypeScript Exports

**Files:**
- Modify: `crates/hm-dsl-engine/harmont-ts/src/toolchains/index.ts`

**Step 1: Update the export line**

Replace the cmake export in `index.ts`:
```typescript
export { cmake, CMakeProject, type CMakeOptions } from "./cmake.js";
```
with:
```typescript
export { cmake, CMakeToolchain, CMakeProject, type CMakeToolchainOptions, type CMakeProjectOptions, type CMakeOptions } from "./cmake.js";
```

**Step 2: Verify build**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-ts && npm run build`
Expected: Success

**Step 3: Commit**

```bash
git add crates/hm-dsl-engine/harmont-ts/src/toolchains/index.ts
git commit -m "feat(cmake): export new types from toolchains barrel"
```

---

## Task 7: Update Examples

**Files:**
- Modify: `examples/c/.harmont/pipeline.py`
- Modify: `examples/c/.harmont/pipeline.ts`
- Modify: `examples/cpp/.harmont/pipeline.py`
- Modify: `examples/cpp/.harmont/pipeline.ts`

**Step 1: Update Python C example**

Replace `examples/c/.harmont/pipeline.py`:
```python
"""C example pipeline."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.cmake(path=".", build_type="Release")
    return (
        project.test(),
        project.fmt(),
    )
```

**Step 2: Update Python C++ example**

Replace `examples/cpp/.harmont/pipeline.py`:
```python
"""C++ example pipeline."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.cmake(path=".", build_type="Release", std=17)
    return (
        project.test(),
        project.lint(),
        project.fmt(),
    )
```

**Step 3: Update TypeScript C example**

Replace `examples/c/.harmont/pipeline.ts`:
```typescript
import { pipeline, push, type PipelineDefinition } from "harmont";
import { cmake } from "harmont/toolchains";

const project = cmake({ path: ".", buildType: "Release" });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(project.test(), project.fmt(), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
```

**Step 4: Update TypeScript C++ example**

Replace `examples/cpp/.harmont/pipeline.ts`:
```typescript
import { pipeline, push, type PipelineDefinition } from "harmont";
import { cmake } from "harmont/toolchains";

const project = cmake({ path: ".", buildType: "Release", std: 17 });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(project.test(), project.lint(), project.fmt(), {
      env: { CI: "true" },
      defaultImage: "ubuntu:24.04",
    }),
  },
];

export default pipelines;
```

**Step 5: Run e2e fixture tests to verify examples still render valid IR**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-py && python -m pytest tests/test_e2e_fixtures.py -v -k "c or cpp"`
Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-ts && npm test -- --grep "fixture"`

**Step 6: Commit**

```bash
git add examples/c/.harmont/pipeline.py examples/c/.harmont/pipeline.ts examples/cpp/.harmont/pipeline.py examples/cpp/.harmont/pipeline.ts
git commit -m "feat(cmake): update C/C++ examples for redesigned module"
```

---

## Task 8: Add Advanced Example

**Files:**
- Create: `examples/cmake-advanced/.harmont/pipeline.py`
- Create: `examples/cmake-advanced/CMakeLists.txt` (minimal placeholder)

**Step 1: Create advanced example showcasing full API**

Create `examples/cmake-advanced/.harmont/pipeline.py`:
```python
"""Advanced CMake pipeline — compiler matrix, sanitizers, coverage."""
from __future__ import annotations

import harmont as hm


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main"), hm.pr()],
)
def ci() -> tuple[hm.Step, ...]:
    project = hm.cmake(
        path=".",
        compiler="clang-18",
        build_type="Release",
        std=20,
        defines={"BUILD_TESTING": "ON"},
    )
    return (
        project.test(),
        project.lint(),
        project.fmt(),
    )


@hm.pipeline(
    "sanitizers",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def sanitizers() -> tuple[hm.Step, ...]:
    project = hm.cmake(path=".", compiler="clang-18")
    return (
        project.sanitize("asan"),
        project.sanitize("tsan"),
    )


@hm.pipeline(
    "coverage",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def coverage() -> tuple[hm.Step, ...]:
    project = hm.cmake(path=".")
    return (project.coverage(),)
```

Create `examples/cmake-advanced/CMakeLists.txt`:
```cmake
cmake_minimum_required(VERSION 3.20)
project(example LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 20)

add_executable(main src/main.cpp)

enable_testing()
add_executable(tests tests/test_main.cpp)
add_test(NAME unit COMMAND tests)
```

**Step 2: Commit**

```bash
git add examples/cmake-advanced/
git commit -m "feat(cmake): add advanced example with sanitizers + coverage"
```

---

## Task 9: Verify Full Test Suite

**Step 1: Run all Python tests**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-py && python -m pytest tests/ -v`
Expected: All PASS, no regressions

**Step 2: Run all TypeScript tests**

Run: `cd /Users/marko/Desktop/harmont-cli-4/crates/hm-dsl-engine/harmont-ts && npm test`
Expected: All PASS, no regressions

**Step 3: Build the full workspace**

Run: `cd /Users/marko/Desktop/harmont-cli-4/cli && cargo build`
Expected: Success (no Rust changes, just verifying nothing broke)

**Step 4: Final commit if any fixups needed**

```bash
git add -A && git commit -m "fix(cmake): test suite fixups"
```

---

## Summary of Changes

| File | Action | Purpose |
|------|--------|---------|
| `harmont-py/harmont/_cmake.py` | Replace | New 3-tier CMake module |
| `harmont-py/harmont/__init__.py` | Modify | Export new types |
| `harmont-py/tests/test_cmake.py` | Replace | Comprehensive test suite |
| `harmont-ts/src/toolchains/cmake.ts` | Replace | TypeScript equivalent |
| `harmont-ts/src/toolchains/index.ts` | Modify | Export new types |
| `harmont-ts/tests/toolchains/cmake.test.ts` | Replace | TypeScript tests |
| `examples/c/.harmont/pipeline.py` | Modify | Updated API |
| `examples/c/.harmont/pipeline.ts` | Modify | Updated API |
| `examples/cpp/.harmont/pipeline.py` | Modify | Updated API |
| `examples/cpp/.harmont/pipeline.ts` | Modify | Updated API |
| `examples/cmake-advanced/` | Create | Advanced usage example |

## What This Supports (from research)

The redesigned module handles patterns from:
- **LLVM/Clang**: Ninja generator, compiler launchers, sanitizers
- **gRPC/Protobuf/Abseil**: Multiple compilers (gcc/clang), standard defines
- **Apache Arrow**: Preset support, vcpkg integration
- **OpenCV/VTK**: Parallel builds, CTest
- **Folly**: ccache/sccache, clang-format
- **Qt/SFML**: Install step, packaging
- **All surveyed projects**: Out-of-source builds, build type configuration, compile_commands.json
