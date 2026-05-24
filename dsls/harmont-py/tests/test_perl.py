"""Perl toolchain tests."""

from __future__ import annotations

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    raise AssertionError(needle)


def test_perl_object_form_full_chain():
    pl = hm.perl(path="svc")
    p = hm.pipeline(pl.test(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("perl" in c and "cpanminus" in c for c in cmds)
    assert any("cd svc && cpanm --installdeps" in c for c in cmds)
    assert any("cd svc && prove -lv t/" in c for c in cmds)


def test_perl_actions_share_install():
    pl = hm.perl(path="svc")
    p = hm.pipeline(pl.test(), pl.lint(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "cpanminus" in c]) == 1
    assert len([c for c in cmds if "cpanm --installdeps" in c]) == 1
    assert any("prove -lv t/" in c for c in cmds)
    assert any("perlcritic" in c for c in cmds)


def test_perl_cpanm_cached_on_cpanfile():
    pl = hm.perl(path="svc")
    p = hm.pipeline(pl.test())
    deps = _step_by_substring(p, "cpanm --installdeps")
    assert deps["cache"]["policy"] == "on_change"
    assert "svc/cpanfile" in deps["cache"]["paths"]


def test_perl_action_labels_auto_generated():
    pl = hm.perl(path=".")
    assert pl.test().label == ":perl: test"
    assert pl.lint().label == ":perl: lint"


def test_perl_bare_form_actions():
    p = hm.pipeline(hm.perl.test(), hm.perl.lint())
    cmds = _cmds(p)
    assert any("prove" in c for c in cmds)
    assert any("perlcritic" in c for c in cmds)


def test_perl_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    pl = hm.perl(path="svc", base=base)
    p = hm.pipeline(pl.test(), default_image="ubuntu:24.04")
    assert not any("apt-get install" in c for c in _cmds(p))
