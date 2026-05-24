"""OCaml toolchain tests."""

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


def test_ocaml_object_form_full_chain():
    o = hm.ocaml(path="svc")
    p = hm.pipeline(o.build(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("opam" in c for c in cmds)
    assert any("opam switch create" in c for c in cmds)
    assert any("cd svc && opam exec -- dune build" in c for c in cmds)


def test_ocaml_actions_share_install():
    o = hm.ocaml(path="svc")
    p = hm.pipeline(o.build(), o.test(), o.fmt(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "opam switch create" in c]) == 1
    assert any("dune build" in c for c in cmds)
    assert any("dune runtest" in c for c in cmds)
    assert any("dune build @fmt" in c for c in cmds)


def test_ocaml_compiler_version_in_install():
    o = hm.ocaml(path=".", compiler="5.1.1")
    p = hm.pipeline(o.build())
    install = _step_by_substring(p, "opam switch create")
    assert "5.1.1" in install["cmd"]


def test_ocaml_invalid_compiler_rejected():
    with pytest.raises(ValueError, match="compiler"):
        hm.ocaml(compiler="oops!")


def test_ocaml_action_labels_auto_generated():
    o = hm.ocaml(path=".")
    assert o.build().label == ":ocaml: build"
    assert o.test().label == ":ocaml: test"
    assert o.fmt().label == ":ocaml: fmt"


def test_ocaml_bare_form_actions():
    p = hm.pipeline(hm.ocaml.build(), hm.ocaml.test(), hm.ocaml.fmt())
    cmds = _cmds(p)
    assert any("dune build" in c for c in cmds)
    assert any("dune runtest" in c for c in cmds)


def test_ocaml_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    o = hm.ocaml(path="svc", base=base)
    p = hm.pipeline(o.build(), default_image="ubuntu:24.04")
    assert not any("apt-get install" in c for c in _cmds(p))
