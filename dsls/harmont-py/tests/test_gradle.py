"""Gradle (Java/Kotlin) toolchain tests."""
from __future__ import annotations

import pytest

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    raise AssertionError(needle)


def test_gradle_object_form_full_chain():
    g = hm.gradle(path="svc")
    p = hm.pipeline(g.build(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("openjdk-21-jdk-headless" in c for c in cmds)
    assert any("cd svc && ./gradlew build" in c for c in cmds)


def test_gradle_actions_share_install():
    g = hm.gradle(path="svc")
    p = hm.pipeline(g.build(), g.test(), g.lint(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "openjdk-21" in c]) == 1
    assert any("./gradlew build" in c for c in cmds)
    assert any("./gradlew test" in c for c in cmds)
    assert any("./gradlew check" in c for c in cmds)


def test_gradle_jdk_version_pinned():
    g = hm.gradle(path=".", jdk="17")
    p = hm.pipeline(g.build())
    apt = _step_by_substring(p, "openjdk-17")
    assert "openjdk-17-jdk-headless" in apt["cmd"]


def test_gradle_invalid_jdk_rejected():
    with pytest.raises(ValueError, match="jdk"):
        hm.gradle(jdk="bogus")


def test_gradle_kotlin_switch_changes_label():
    g = hm.gradle(path="svc", kotlin=True)
    assert g.build().label == ":kotlin: build"
    assert g.test().label == ":kotlin: test"
    assert g.lint().label == ":kotlin: lint"


def test_gradle_java_labels_default():
    g = hm.gradle(path="svc")
    assert g.build().label == ":java: build"
    assert g.test().label == ":java: test"
    assert g.lint().label == ":java: lint"


def test_gradle_bare_form_actions():
    p = hm.pipeline(hm.gradle.build(), hm.gradle.test(), hm.gradle.lint())
    cmds = _cmds(p)
    assert any("./gradlew build" in c for c in cmds)
    assert any("./gradlew test" in c for c in cmds)
    assert any("./gradlew check" in c for c in cmds)


def test_gradle_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    g = hm.gradle(path="svc", base=base)
    p = hm.pipeline(g.build(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert not any("openjdk" in c for c in cmds)
    assert any("custom base" in c for c in cmds)
