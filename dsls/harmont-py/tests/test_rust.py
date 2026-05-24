"""Rust toolchain abstraction tests."""
from __future__ import annotations

import pytest

import harmont as hm
from harmont.cache import CacheOnChange


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


def test_rust_object_form_full_chain():
    rust = hm.rust(path="cli")
    p = hm.pipeline(rust.build(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("apt-get install" in c for c in cmds)
    assert any("sh.rustup.rs" in c for c in cmds)
    assert any("cd cli && cargo build" in c for c in cmds)


def test_rust_actions_share_install_step():
    rust = hm.rust(path="cli")
    p = hm.pipeline(rust.build(), rust.test(), rust.clippy(), rust.fmt(), rust.doc(),
                    default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "sh.rustup.rs" in c]) == 1
    assert len([c for c in cmds if "apt-get install" in c]) == 1
    assert any("cargo build" in c for c in cmds)
    assert any("cargo test" in c for c in cmds)
    assert any("cargo clippy --all-targets -- -D warnings" in c for c in cmds)
    assert any("cargo fmt --check" in c for c in cmds)
    assert any("cargo doc --no-deps" in c for c in cmds)


def test_rust_build_release_flag():
    rust = hm.rust(path=".")
    s = rust.build(release=True)
    assert s.cmd is not None
    assert "cargo build --release" in s.cmd


def test_rust_test_release_flag():
    rust = hm.rust(path=".")
    s = rust.test(release=True)
    assert s.cmd is not None
    assert "cargo test --release" in s.cmd


def test_rust_rustup_cache_forever():
    rust = hm.rust(path="cli")
    p = hm.pipeline(rust.build())
    rustup = _step_by_substring(p, "sh.rustup.rs")
    assert rustup["cache"]["policy"] == "forever"


def test_rust_default_components():
    rust = hm.rust(path=".")
    p = hm.pipeline(rust.build())
    rustup = _step_by_substring(p, "sh.rustup.rs")
    assert "--component clippy,rustfmt" in rustup["cmd"]


def test_rust_components_override():
    rust = hm.rust(path=".", components=("clippy",))
    p = hm.pipeline(rust.build())
    rustup = _step_by_substring(p, "sh.rustup.rs")
    assert "--component clippy" in rustup["cmd"]
    assert "rustfmt" not in rustup["cmd"]


def test_rust_version_in_rustup_cmd():
    rust = hm.rust(path=".", version="1.81.0")
    p = hm.pipeline(rust.build())
    rustup = _step_by_substring(p, "sh.rustup.rs")
    assert "--default-toolchain 1.81.0" in rustup["cmd"]


def test_rust_invalid_version_rejected():
    with pytest.raises(ValueError, match="version"):
        hm.rust(version="not a valid; version")


def test_rust_installed_escape_hatch_chains():
    rust = hm.rust(path="cli")
    custom = rust.installed.sh(
        "cd cli && cargo build --release --features foo",
        label=":rust: custom",
    )
    p = hm.pipeline(custom)
    cmds = _cmds(p)
    assert any("--features foo" in c for c in cmds)


def test_rust_action_labels_auto_generated():
    rust = hm.rust(path=".")
    assert rust.build().label == ":rust: build"
    assert rust.test().label == ":rust: test"
    assert rust.clippy().label == ":rust: clippy"
    assert rust.fmt().label == ":rust: fmt"
    assert rust.doc().label == ":rust: doc"


def test_rust_action_label_override():
    rust = hm.rust(path=".")
    s = rust.build(label=":rust: dev build")
    assert s.label == ":rust: dev build"


def test_rust_action_cache_forwarded():
    rust = hm.rust(path=".")
    s = rust.build(cache=CacheOnChange(paths=("Cargo.lock",)))
    assert s.cache == CacheOnChange(paths=("Cargo.lock",))


def test_rust_image_emitted_on_apt_step():
    rust = hm.rust(path=".", image="alpine:3.20")
    p = hm.pipeline(rust.build())
    apt = _step_by_substring(p, "apt-get install")
    assert apt.get("image") == "alpine:3.20"


def test_rust_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    rust = hm.rust(path="cli", base=base)
    p = hm.pipeline(rust.build(), default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert not any("apt-get install" in c for c in cmds)
    assert any("custom base" in c for c in cmds)
    assert any("sh.rustup.rs" in c for c in cmds)
    assert any("cd cli && cargo build" in c for c in cmds)


def test_rust_bare_form_build():
    p = hm.pipeline(hm.rust.build())
    cmds = _cmds(p)
    assert any("cd . && cargo build" in c for c in cmds)


def test_rust_bare_form_all_actions():
    p = hm.pipeline(hm.rust.build(), hm.rust.test(), hm.rust.clippy(),
                    hm.rust.fmt(), hm.rust.doc())
    cmds = _cmds(p)
    assert any("cargo build" in c for c in cmds)
    assert any("cargo test" in c for c in cmds)
    assert any("cargo clippy" in c for c in cmds)
    assert any("cargo fmt --check" in c for c in cmds)
    assert any("cargo doc --no-deps" in c for c in cmds)


def test_rust_bare_form_accepts_path_kwarg():
    p = hm.pipeline(hm.rust.test(path="cli"))
    cmds = _cmds(p)
    assert any("cd cli && cargo test" in c for c in cmds)


def test_rust_bare_form_forwards_action_kwargs():
    s = hm.rust.build(path="cli", label=":rust: custom")
    assert s.label == ":rust: custom"
