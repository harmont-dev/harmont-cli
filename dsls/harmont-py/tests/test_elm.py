"""Elm project abstraction tests."""

from __future__ import annotations

import pytest

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


def test_elm_full_chain():
    elm = hm.elm(path="app")
    p = hm.pipeline(elm.make("src/Main.elm"), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("apt-get install" in c for c in cmds)
    assert any("deb.nodesource.com" in c for c in cmds)
    assert any("elm/compiler/releases" in c for c in cmds)
    assert any("cd app && elm make src/Main.elm" in c for c in cmds)


def test_elm_make_with_output():
    elm = hm.elm(path="app")
    s = elm.make("src/Main.elm", output="/tmp/elm.js")  # noqa: S108
    assert s.cmd is not None
    assert "elm make src/Main.elm --output=/tmp/elm.js" in s.cmd


def test_elm_make_without_output():
    elm = hm.elm(path="app")
    s = elm.make("src/Main.elm")
    assert s.cmd is not None
    assert "elm make src/Main.elm" in s.cmd
    assert "--output" not in s.cmd


def test_elm_test_uses_npx():
    elm = hm.elm(path="app")
    s = elm.test()
    assert s.cmd is not None
    assert "cd app && npx --yes elm-test" in s.cmd


def test_elm_review_uses_npx():
    elm = hm.elm(path="app")
    s = elm.review()
    assert s.cmd is not None
    assert "cd app && npx --yes elm-review" in s.cmd


def test_elm_fmt_uses_npx():
    elm = hm.elm(path="app")
    s = elm.fmt()
    assert s.cmd is not None
    assert "cd app && npx --yes elm-format --validate ." in s.cmd


def test_elm_version_in_install_cmd():
    elm = hm.elm(path=".", elm_version="0.19.1")
    p = hm.pipeline(elm.make("src/Main.elm"))
    install = _step_by_substring(p, "elm/compiler/releases")
    assert "0.19.1" in install["cmd"]


def test_elm_invalid_version():
    with pytest.raises(ValueError, match="elm_version"):
        hm.elm(elm_version="bad")


def test_elm_node_version_in_install_cmd():
    elm = hm.elm(path=".", node_version="22")
    p = hm.pipeline(elm.make("src/Main.elm"))
    node = _step_by_substring(p, "deb.nodesource.com")
    assert "setup_22.x" in node["cmd"]


def test_elm_install_cache_forever():
    elm = hm.elm(path="app")
    p = hm.pipeline(elm.make("src/Main.elm"))
    elm_install = _step_by_substring(p, "elm/compiler/releases")
    node_install = _step_by_substring(p, "deb.nodesource.com")
    assert elm_install["cache"]["policy"] == "forever"
    assert node_install["cache"]["policy"] == "forever"


def test_elm_action_labels():
    elm = hm.elm(path="app")
    assert elm.make("src/Main.elm").label == ":elm: make src/Main.elm"
    assert elm.test().label == ":elm: test"
    assert elm.review().label == ":elm: review"
    assert elm.fmt().label == ":elm: fmt"


def test_elm_actions_share_install():
    elm = hm.elm(path="app")
    p = hm.pipeline(
        elm.make("src/Main.elm"),
        elm.test(),
        elm.review(),
        elm.fmt(),
        default_image="ubuntu:24.04",
    )
    cmds = _cmds(p)
    assert len([c for c in cmds if "elm/compiler/releases" in c]) == 1


def test_elm_with_base_skips_apt():
    base = hm.scratch().sh("base", label="base")
    elm = hm.elm(path="app", base=base)
    p = hm.pipeline(elm.make("src/Main.elm"))
    cmds = _cmds(p)
    # apt-base (curl + ca-certificates) is skipped. nodesource installer
    # itself runs `apt-get install -y nodejs` so don't assert on
    # apt-get; check the apt-base packages instead.
    assert not any("ca-certificates" in c for c in cmds)
    assert any("deb.nodesource.com" in c for c in cmds)


def test_elm_bare_form_make():
    p = hm.pipeline(hm.elm.make("src/Main.elm", path="app"))
    cmds = _cmds(p)
    assert any("cd app && elm make src/Main.elm" in c for c in cmds)


def test_elm_bare_form_forwards_action_kwargs():
    s = hm.elm.make("src/Main.elm", path=".", label=":elm: custom")
    assert s.label == ":elm: custom"
