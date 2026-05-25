"""Composer (PHP / Laravel) toolchain tests."""

from __future__ import annotations

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    raise AssertionError(needle)


def test_composer_object_form_full_chain():
    c = hm.composer(path="svc")
    p = hm.pipeline(c.test(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("php-cli" in c_ for c_ in cmds)
    assert any("composer" in c_ for c_ in cmds)
    assert any("cd svc && composer install" in c_ for c_ in cmds)
    assert any("cd svc && vendor/bin/phpunit" in c_ for c_ in cmds)


def test_composer_actions_share_install():
    c = hm.composer(path="svc")
    p = hm.pipeline(c.test(), c.lint(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c_ for c_ in cmds if "php-cli" in c_]) == 1
    assert len([c_ for c_ in cmds if "composer install" in c_]) == 1
    assert any("vendor/bin/phpunit" in c_ for c_ in cmds)
    assert any("vendor/bin/phpstan analyse" in c_ for c_ in cmds)


def test_composer_install_cached_on_lockfile():
    c = hm.composer(path="svc")
    p = hm.pipeline(c.test())
    install = _step_by_substring(p, "composer install")
    assert install["cache"]["policy"] == "on_change"
    assert "svc/composer.lock" in install["cache"]["paths"]


def test_composer_laravel_swaps_test_action():
    c = hm.composer(path="svc", laravel=True)
    p = hm.pipeline(c.test(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("php artisan test" in c_ for c_ in cmds)
    assert not any("vendor/bin/phpunit" in c_ for c_ in cmds)


def test_composer_action_labels_auto_generated():
    c = hm.composer(path=".")
    assert c.test().label == ":php: test"
    assert c.lint().label == ":php: lint"


def test_composer_laravel_label_prefix():
    c = hm.composer(path=".", laravel=True)
    assert c.test().label == ":laravel: test"


def test_composer_bare_form_actions():
    p = hm.pipeline(hm.composer.test(), hm.composer.lint())
    cmds = _cmds(p)
    assert any("phpunit" in c for c in cmds)
    assert any("phpstan" in c for c in cmds)


def test_composer_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    c = hm.composer(path="svc", base=base)
    p = hm.pipeline(c.test(), default_image="ubuntu:24.04")
    assert not any("apt-get install" in c_ for c_ in _cmds(p))
