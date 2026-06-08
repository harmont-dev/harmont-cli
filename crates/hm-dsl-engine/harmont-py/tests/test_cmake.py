"""CMake (C/C++) toolchain tests."""

from __future__ import annotations

import pytest

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def test_cmake_object_form_full_chain():
    cm = hm.cmake(path="svc")
    p = hm.pipeline([cm.build()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("build-essential" in c for c in cmds)
    assert any("cmake --version" in c for c in cmds)
    assert any("cmake --build build" in c for c in cmds)
    assert any("cmake -S . -B build" in c for c in cmds)


def test_cmake_actions_share_install():
    cm = hm.cmake(path="svc")
    p = hm.pipeline([cm.configure(), cm.build(), cm.test(), cm.fmt()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "cmake --version" in c]) == 1
    assert len([c for c in cmds if "apt-get install" in c]) == 1
    assert any("cmake -S . -B build" in c for c in cmds)
    assert any("cmake --build build" in c for c in cmds)
    assert any("ctest --test-dir build" in c for c in cmds)
    assert any("clang-format --dry-run --Werror" in c for c in cmds)


def test_cmake_cpp_label_prefix():
    cm = hm.cmake(path=".", lang="cpp")
    assert cm.build().label == ":cpp: build"
    assert cm.test().label == ":cpp: test"
    assert cm.fmt().label == ":cpp: fmt"


def test_cmake_c_label_prefix_default():
    cm = hm.cmake(path=".")
    assert cm.build().label == ":c: build"


def test_cmake_invalid_lang_rejected():
    with pytest.raises(ValueError, match="lang"):
        hm.cmake(lang="rust")


def test_cmake_bare_form_actions():
    p = hm.pipeline([hm.cmake.configure(), hm.cmake.build(), hm.cmake.test(), hm.cmake.fmt()])
    cmds = _cmds(p)
    assert any("cmake -S . -B build" in c for c in cmds)
    assert any("cmake --build build" in c for c in cmds)
    assert any("ctest" in c for c in cmds)
    assert any("clang-format" in c for c in cmds)


def test_cmake_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    cm = hm.cmake(path="svc", base=base)
    p = hm.pipeline([cm.build()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert not any("build-essential" in c for c in cmds)
