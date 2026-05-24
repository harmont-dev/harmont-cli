"""Ruby toolchain tests."""
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


def test_ruby_object_form_full_chain():
    rb = hm.ruby(path="svc")
    p = hm.pipeline(rb.test(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("ruby-full" in c for c in cmds)
    assert any("gem install bundler" in c for c in cmds)
    assert any("cd svc && bundle install" in c for c in cmds)
    assert any("cd svc && bundle exec rspec" in c for c in cmds)


def test_ruby_actions_share_install():
    rb = hm.ruby(path="svc")
    p = hm.pipeline(rb.test(), rb.lint(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "ruby-full" in c]) == 1
    assert any("bundle exec rspec" in c for c in cmds)
    assert any("bundle exec rubocop" in c for c in cmds)


def test_ruby_bundle_install_cached_on_lockfile():
    rb = hm.ruby(path="svc")
    p = hm.pipeline(rb.test())
    bundle = _step_by_substring(p, "bundle install")
    assert bundle["cache"]["policy"] == "on_change"
    assert "svc/Gemfile.lock" in bundle["cache"]["paths"]


def test_ruby_action_labels_auto_generated():
    rb = hm.ruby(path=".")
    assert rb.test().label == ":ruby: test"
    assert rb.lint().label == ":ruby: lint"


def test_ruby_bare_form_actions():
    p = hm.pipeline(hm.ruby.test(), hm.ruby.lint())
    cmds = _cmds(p)
    assert any("rspec" in c for c in cmds)
    assert any("rubocop" in c for c in cmds)


def test_ruby_invalid_version_rejected():
    with pytest.raises(ValueError, match="version"):
        hm.ruby(version="bogus; oops")


def test_ruby_pinned_version_not_yet_supported():
    with pytest.raises(NotImplementedError, match="not yet wired in"):
        hm.ruby(version="3.2.2")


def test_ruby_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    rb = hm.ruby(path="svc", base=base)
    p = hm.pipeline(rb.test(), default_image="ubuntu:24.04")
    assert not any("ruby-full" in c for c in _cmds(p))
