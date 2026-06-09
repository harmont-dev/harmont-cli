"""CMake (C/C++) toolchain tests — comprehensive suite for the redesigned module."""

from __future__ import annotations

import pytest

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


# ---------------------------------------------------------------------------
# TestCMakeToolchain
# ---------------------------------------------------------------------------


class TestCMakeToolchain:
    def test_default_toolchain_installs_cmake_ninja_ccache(self):
        tc = hm.cmake()
        p = hm.pipeline([tc.installed], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        apt_cmd = next(c for c in cmds if "apt-get install" in c)
        assert "cmake" in apt_cmd
        assert "ninja-build" in apt_cmd
        assert "ccache" in apt_cmd

    def test_clang_18_compiler_installs_clang_18(self):
        tc = hm.cmake(compiler="clang-18")
        p = hm.pipeline([tc.installed], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        apt_cmd = next(c for c in cmds if "apt-get install" in c)
        assert "clang-18" in apt_cmd

    def test_gcc_14_compiler_installs_gcc_14_and_gpp_14(self):
        tc = hm.cmake(compiler="gcc-14")
        p = hm.pipeline([tc.installed], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        apt_cmd = next(c for c in cmds if "apt-get install" in c)
        assert "gcc-14" in apt_cmd
        assert "g++-14" in apt_cmd

    def test_invalid_compiler_raises_valueerror(self):
        with pytest.raises(ValueError, match="compiler"):
            hm.cmake(compiler="msvc-19")

    def test_ccache_false_omits_ccache_from_apt_and_flags(self):
        tc = hm.cmake(ccache=False)
        proj = tc.project(path=".")
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        apt_cmd = next(c for c in cmds if "apt-get install" in c)
        assert "ccache" not in apt_cmd
        configure_cmd = next(c for c in cmds if "cmake -S" in c)
        assert "CMAKE_C_COMPILER_LAUNCHER=ccache" not in configure_cmd
        assert "CMAKE_CXX_COMPILER_LAUNCHER=ccache" not in configure_cmd

    def test_toolchain_shared_across_projects_single_apt_install(self):
        tc = hm.cmake()
        proj1 = tc.project(path="svc1")
        proj2 = tc.project(path="svc2")
        p = hm.pipeline([proj1.built, proj2.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        apt_installs = [c for c in cmds if "apt-get install" in c]
        assert len(apt_installs) == 1


# ---------------------------------------------------------------------------
# TestCMakeProject
# ---------------------------------------------------------------------------


class TestCMakeProject:
    def test_build_produces_configure_and_build_commands(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cmake -S . -B build" in c for c in cmds)
        assert any("cmake --build" in c for c in cmds)

    def test_warmup_uses_relative_build_dir_after_cd(self):
        proj = hm.cmake(path="infra/agent")
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        warmup = next(c for c in cmds if "cmake -S . -B build" in c)
        assert "cd infra/agent" in warmup
        assert "cmake --build build " in warmup
        assert "infra/agent/build" not in warmup

    def test_uses_ninja_generator_by_default(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        configure_cmd = next(c for c in cmds if "cmake -S" in c)
        assert "-G Ninja" in configure_cmd

    def test_no_build_type_by_default(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        configure_cmd = next(c for c in cmds if "cmake -S" in c)
        assert "CMAKE_BUILD_TYPE" not in configure_cmd

    def test_defines_cmake_build_type(self):
        proj = hm.cmake(path="svc", defines={"CMAKE_BUILD_TYPE": "Debug"})
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        configure_cmd = next(c for c in cmds if "cmake -S" in c)
        assert "CMAKE_BUILD_TYPE=Debug" in configure_cmd

    def test_defines_produces_d_flags(self):
        proj = hm.cmake(path="svc", defines={"BUILD_TESTING": "ON"})
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        configure_cmd = next(c for c in cmds if "cmake -S" in c)
        assert "-DBUILD_TESTING=ON" in configure_cmd

    def test_defines_build_shared_libs(self):
        proj = hm.cmake(path="svc", defines={"BUILD_SHARED_LIBS": "ON"})
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        configure_cmd = next(c for c in cmds if "cmake -S" in c)
        assert "-DBUILD_SHARED_LIBS=ON" in configure_cmd

    def test_defines_cmake_cxx_standard(self):
        proj = hm.cmake(path="svc", defines={"CMAKE_CXX_STANDARD": "20"})
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        configure_cmd = next(c for c in cmds if "cmake -S" in c)
        assert "-DCMAKE_CXX_STANDARD=20" in configure_cmd

    def test_preset_produces_preset_flag_and_no_build_type(self):
        proj = hm.cmake(path="svc", preset="ci-linux")
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        configure_cmd = next(c for c in cmds if "--preset" in c)
        assert "--preset ci-linux" in configure_cmd
        assert "CMAKE_BUILD_TYPE" not in configure_cmd

    def test_ccache_true_adds_compiler_launcher_flags(self):
        proj = hm.cmake(path="svc", ccache=True)
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        configure_cmd = next(c for c in cmds if "cmake -S" in c)
        assert "-DCMAKE_C_COMPILER_LAUNCHER=ccache" in configure_cmd
        assert "-DCMAKE_CXX_COMPILER_LAUNCHER=ccache" in configure_cmd

    def test_test_produces_ctest_with_output_on_failure_and_parallel(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline([proj.test()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        test_cmd = next(c for c in cmds if "ctest" in c)
        assert "--output-on-failure" in test_cmd
        assert "--parallel" in test_cmd

    def test_test_includes_incremental_build(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline([proj.test()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        test_cmd = next(c for c in cmds if "ctest" in c)
        assert "cmake --build" in test_cmd

    def test_test_uses_absolute_path_for_standalone_step(self):
        proj = hm.cmake(path="infra/agent")
        p = hm.pipeline([proj.test()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        test_cmd = next(c for c in cmds if "ctest" in c)
        assert "cmake --build infra/agent/build" in test_cmd

    def test_install_with_prefix(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline([proj.install(prefix="/usr/local")], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        install_cmd = next(c for c in cmds if "cmake --install" in c)
        assert "--prefix /usr/local" in install_cmd

    def test_fmt_runs_clang_format_dry_run(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline([proj.fmt()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        fmt_cmd = next(c for c in cmds if "xargs clang-format" in c)
        assert "--dry-run --Werror" in fmt_cmd
        assert "-not -path './build/*'" in fmt_cmd

    def test_fmt_parent_is_toolchain_installed(self):
        proj = hm.cmake(path="svc")
        fmt_step = proj.fmt()
        assert fmt_step.parent is proj.toolchain.installed

    def test_lint_runs_run_clang_tidy(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline([proj.lint()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("run-clang-tidy" in c for c in cmds)

    def test_lint_parent_is_built(self):
        proj = hm.cmake(path="svc")
        lint_step = proj.lint()
        assert lint_step.parent is proj.built

    def test_package_runs_cpack(self):
        proj = hm.cmake(path="svc")
        p = hm.pipeline([proj.package()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cpack" in c for c in cmds)


# ---------------------------------------------------------------------------
# TestCMakeVcpkg
# ---------------------------------------------------------------------------


class TestCMakeVcpkg:
    def test_deps_vcpkg_produces_bootstrap_command(self):
        proj = hm.cmake(path="svc", deps="vcpkg")
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("bootstrap-vcpkg" in c for c in cmds)

    def test_invalid_deps_raises_valueerror(self):
        with pytest.raises(ValueError, match="deps"):
            hm.cmake(path="svc", deps="conan")

    def test_vcpkg_step_has_on_change_cache_policy(self):
        proj = hm.cmake(path="svc", deps="vcpkg")
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        nodes = p["graph"]["nodes"]
        vcpkg_node = next(n for n in nodes if "bootstrap-vcpkg" in n["step"]["cmd"])
        assert vcpkg_node["step"]["cache"]["policy"] == "on_change"


# ---------------------------------------------------------------------------
# TestCMakeBareForm
# ---------------------------------------------------------------------------


class TestCMakeBareForm:
    def test_bare_build_produces_cmake_build(self):
        p = hm.pipeline([hm.cmake.build()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cmake --build" in c for c in cmds)

    def test_bare_test_produces_ctest(self):
        p = hm.pipeline([hm.cmake.test()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("ctest" in c for c in cmds)

    def test_bare_fmt_produces_clang_format(self):
        p = hm.pipeline([hm.cmake.fmt()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("clang-format" in c for c in cmds)


# ---------------------------------------------------------------------------
# TestCMakeLabels
# ---------------------------------------------------------------------------


class TestCMakeLabels:
    def test_built_label(self):
        proj = hm.cmake(path="svc")
        assert proj.built.label == ":cmake: build"

    def test_test_label(self):
        proj = hm.cmake(path="svc")
        assert proj.test().label == ":cmake: test"

    def test_fmt_label(self):
        proj = hm.cmake(path="svc")
        assert proj.fmt().label == ":cmake: fmt"

    def test_lint_label(self):
        proj = hm.cmake(path="svc")
        assert proj.lint().label == ":cmake: lint"


# ---------------------------------------------------------------------------
# TestCMakeWithBase
# ---------------------------------------------------------------------------


class TestCMakeWithBase:
    def test_providing_base_skips_apt_install(self):
        base = hm.scratch().sh("custom base", label="base")
        proj = hm.cmake(path="svc", base=base)
        p = hm.pipeline([proj.built], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert not any("apt-get install" in c for c in cmds)
