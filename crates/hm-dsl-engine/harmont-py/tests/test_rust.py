"""Rust toolchain and project abstraction tests."""

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


# --- RustToolchain (hm.rust.toolchain) ---


class TestRustToolchain:
    def test_full_chain(self):
        tc = hm.rust.toolchain(path="cli")
        p = hm.pipeline([tc.build()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("apt-get install" in c for c in cmds)
        assert any("sh.rustup.rs" in c for c in cmds)
        assert any("cd cli && cargo build" in c for c in cmds)

    def test_actions_share_install_step(self):
        tc = hm.rust.toolchain(path="cli")
        p = hm.pipeline(
            [tc.build(), tc.test(), tc.clippy(), tc.fmt(), tc.doc()],
            default_image="ubuntu:24.04",
        )
        cmds = _cmds(p)
        assert len([c for c in cmds if "sh.rustup.rs" in c]) == 1
        assert len([c for c in cmds if "apt-get install" in c]) == 1

    def test_build_release(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.build(release=True)
        assert "cargo build --release" in s.cmd

    def test_test_release(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.test(release=True)
        assert "cargo test --release" in s.cmd

    def test_rustup_cache_forever(self):
        tc = hm.rust.toolchain(path="cli")
        p = hm.pipeline([tc.build()])
        rustup = _step_by_substring(p, "sh.rustup.rs")
        assert rustup["cache"]["policy"] == "forever"

    def test_default_components(self):
        tc = hm.rust.toolchain(path=".")
        p = hm.pipeline([tc.build()])
        rustup = _step_by_substring(p, "sh.rustup.rs")
        assert "--component clippy,rustfmt" in rustup["cmd"]

    def test_components_override(self):
        tc = hm.rust.toolchain(path=".", components=("clippy",))
        p = hm.pipeline([tc.build()])
        rustup = _step_by_substring(p, "sh.rustup.rs")
        assert "--component clippy" in rustup["cmd"]
        assert "rustfmt" not in rustup["cmd"]

    def test_version_in_rustup_cmd(self):
        tc = hm.rust.toolchain(path=".", version="1.81.0")
        p = hm.pipeline([tc.build()])
        rustup = _step_by_substring(p, "sh.rustup.rs")
        assert "--default-toolchain 1.81.0" in rustup["cmd"]

    def test_invalid_version_rejected(self):
        with pytest.raises(ValueError, match="version"):
            hm.rust.toolchain(version="not a valid; version")

    def test_installed_escape_hatch(self):
        tc = hm.rust.toolchain(path="cli")
        custom = tc.installed.sh(
            "cd cli && cargo build --release --features foo",
            label=":rust: custom",
        )
        p = hm.pipeline([custom])
        cmds = _cmds(p)
        assert any("--features foo" in c for c in cmds)

    def test_action_labels(self):
        tc = hm.rust.toolchain(path=".")
        assert tc.build().label == ":rust: build"
        assert tc.test().label == ":rust: test"
        assert tc.clippy().label == ":rust: clippy"
        assert tc.fmt().label == ":rust: fmt"
        assert tc.doc().label == ":rust: doc"

    def test_action_label_override(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.build(label=":rust: dev build")
        assert s.label == ":rust: dev build"

    def test_action_cache_forwarded(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.build(cache=CacheOnChange(paths=("Cargo.lock",)))
        assert s.cache == CacheOnChange(paths=("Cargo.lock",))

    def test_image_emitted_on_apt_step(self):
        tc = hm.rust.toolchain(path=".", image="alpine:3.20")
        p = hm.pipeline([tc.build()])
        apt = _step_by_substring(p, "apt-get install")
        assert apt.get("image") == "alpine:3.20"

    def test_with_base_skips_apt(self):
        base = hm.scratch().sh("custom base", label="base")
        tc = hm.rust.toolchain(path="cli", base=base)
        p = hm.pipeline([tc.build()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert not any("apt-get install" in c for c in cmds)
        assert any("custom base" in c for c in cmds)
        assert any("sh.rustup.rs" in c for c in cmds)
        assert any("cd cli && cargo build" in c for c in cmds)

    def test_warmup_returns_step(self):
        tc = hm.rust.toolchain(path="cli")
        w = tc.warmup()
        assert w.cmd is not None
        assert "cargo build --workspace --tests --locked" in w.cmd

    def test_warmup_chains_from_installed(self):
        tc = hm.rust.toolchain(path="cli")
        w = tc.warmup()
        assert w.parent is tc.installed

    def test_warmup_default_label(self):
        tc = hm.rust.toolchain(path=".")
        assert tc.warmup().label == ":rust: warmup"

    def test_warmup_label_override(self):
        tc = hm.rust.toolchain(path=".")
        assert tc.warmup(label=":rust: pre-build").label == ":rust: pre-build"

    def test_warmup_in_pipeline(self):
        tc = hm.rust.toolchain(path="cli")
        w = tc.warmup()
        t = w.sh(
            ". $HOME/.cargo/env && cd cli && cargo test --workspace --locked",
            label=":rust: test",
        )
        p = hm.pipeline([t, tc.fmt()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cargo build --workspace --tests --locked" in c for c in cmds)
        assert any("cargo test --workspace --locked" in c for c in cmds)
        assert any("cargo fmt" in c for c in cmds)
        assert len([c for c in cmds if "sh.rustup.rs" in c]) == 1
        assert len([c for c in cmds if "apt-get install" in c]) == 1

    def test_build_locked_by_default(self):
        tc = hm.rust.toolchain(path=".")
        assert tc.build().cmd.endswith("cargo build --locked")

    def test_build_unlocked(self):
        tc = hm.rust.toolchain(path=".")
        assert tc.build(locked=False).cmd.endswith("cargo build")

    def test_build_features(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.build(features=("a", "b"), release=True)
        assert "cargo build --features a,b --release --locked" in s.cmd

    def test_build_packages(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.build(packages=("core", "cli"))
        assert "cargo build -p core -p cli --locked" in s.cmd

    def test_test_all_features(self):
        tc = hm.rust.toolchain(path=".")
        assert "cargo test --all-features --locked" in tc.test(all_features=True).cmd

    def test_test_nextest_switches_runner(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.test(nextest=True, workspace=True)
        assert "cargo nextest run --workspace --locked" in s.cmd

    def test_doctest_appends_doc(self):
        tc = hm.rust.toolchain(path=".")
        assert tc.doctest(workspace=True).cmd.endswith("cargo test --workspace --locked --doc")

    def test_doctest_default_label(self):
        tc = hm.rust.toolchain(path=".")
        assert tc.doctest().label == ":rust: doctest"

    def test_clippy_all_targets_locked_deny(self):
        tc = hm.rust.toolchain(path=".")
        assert "cargo clippy --all-targets --locked -- -D warnings" in tc.clippy().cmd

    def test_clippy_extra_lints(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.clippy(extra_lints=("-W clippy::pedantic",))
        assert "-- -D warnings -W clippy::pedantic" in s.cmd

    def test_clippy_no_deny(self):
        tc = hm.rust.toolchain(path=".")
        assert " -- " not in tc.clippy(deny_warnings=False).cmd

    def test_fmt_all_check_default(self):
        tc = hm.rust.toolchain(path=".")
        assert tc.fmt().cmd.endswith("cargo fmt --all --check")

    def test_doc_sets_rustdocflags_env(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.doc()
        assert "cargo doc --no-deps --locked" in s.cmd
        assert s.env == {"RUSTDOCFLAGS": "-D warnings"}

    def test_doc_respects_user_rustdocflags(self):
        tc = hm.rust.toolchain(path=".")
        s = tc.doc(env={"RUSTDOCFLAGS": "-D rustdoc::all"})
        assert s.env == {"RUSTDOCFLAGS": "-D rustdoc::all"}

    def test_doc_no_deny(self):
        tc = hm.rust.toolchain(path=".")
        assert tc.doc(deny_warnings=False).env is None


# --- RustProject (hm.rust.project) ---


class TestRustProject:
    def test_project_has_all_methods(self):
        proj = hm.rust.project(path="cli")
        assert proj.warmup.cmd is not None
        assert proj.test().cmd is not None
        assert proj.clippy().cmd is not None
        assert proj.fmt().cmd is not None

    def test_warmup_implicit_cache_on_change(self):
        proj = hm.rust.project(path="cli")
        assert proj.warmup.cache == CacheOnChange(
            paths=("cli/Cargo.lock", "cli/**/Cargo.toml", "cli/**/*.rs")
        )

    def test_warmup_implicit_cache_dot_path(self):
        proj = hm.rust.project(path=".")
        assert proj.warmup.cache == CacheOnChange(paths=("Cargo.lock", "**/Cargo.toml", "**/*.rs"))

    def test_warmup_cache_override(self):
        custom = CacheOnChange(paths=("Cargo.toml",))
        proj = hm.rust.project(path=".", cache=custom)
        assert proj.warmup.cache == custom

    def test_test_command(self):
        proj = hm.rust.project(path="cli")
        assert "cargo test --workspace --locked" in proj.test().cmd

    def test_test_flags(self):
        proj = hm.rust.project(path=".")
        step = proj.test(flags=("--lib", "--no-fail-fast"))
        assert "cargo test --workspace --locked --lib --no-fail-fast" in step.cmd

    def test_clippy_command(self):
        proj = hm.rust.project(path="cli")
        assert "cargo clippy --workspace --tests --locked -- -D warnings" in proj.clippy().cmd

    def test_clippy_flags(self):
        proj = hm.rust.project(path=".")
        step = proj.clippy(flags=("--fix",))
        assert "cargo clippy --workspace --tests --locked --fix -- -D warnings" in step.cmd

    def test_fmt_command(self):
        proj = hm.rust.project(path="cli")
        assert "cargo fmt --check" in proj.fmt().cmd

    def test_fmt_flags(self):
        proj = hm.rust.project(path=".")
        assert "cargo fmt --check --all" in proj.fmt(flags=("--all",)).cmd

    def test_test_chains_off_warmup(self):
        proj = hm.rust.project(path=".")
        assert proj.test().parent is proj.warmup

    def test_clippy_chains_off_warmup(self):
        proj = hm.rust.project(path=".")
        assert proj.clippy().parent is proj.warmup

    def test_fmt_chains_off_install(self):
        proj = hm.rust.project(path=".")
        assert proj.fmt().parent is proj.toolchain.installed

    def test_toolchain_escape_hatch(self):
        proj = hm.rust.project(path="cli")
        custom = proj.toolchain.installed.sh("custom", label="custom")
        assert custom.parent is proj.toolchain.installed

    def test_with_base_skips_apt(self):
        base = hm.scratch().sh("custom base", label="base")
        proj = hm.rust.project(path="cli", base=base)
        p = hm.pipeline([proj.test(), proj.clippy(), proj.fmt()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert not any("apt-get install" in c for c in cmds)
        assert any("custom base" in c for c in cmds)

    def test_labels(self):
        proj = hm.rust.project(path=".")
        assert proj.warmup.label == ":rust: warmup"
        assert proj.test().label == ":rust: test"
        assert proj.clippy().label == ":rust: clippy"
        assert proj.fmt().label == ":rust: fmt"

    def test_pipeline_ir(self):
        proj = hm.rust.project(path="cli")
        p = hm.pipeline([proj.test(), proj.clippy(), proj.fmt()], default_image="ubuntu:24.04")
        cmds = _cmds(p)
        assert any("cargo build --workspace --tests --locked" in c for c in cmds)
        assert any("cargo test --workspace --locked" in c for c in cmds)
        assert any("cargo clippy" in c for c in cmds)
        assert any("cargo fmt --check" in c for c in cmds)
        assert len([c for c in cmds if "sh.rustup.rs" in c]) == 1
        assert len([c for c in cmds if "apt-get install" in c]) == 1

    def test_version_forwarded(self):
        proj = hm.rust.project(path=".", version="1.81.0")
        p = hm.pipeline([proj.test()])
        rustup = _step_by_substring(p, "sh.rustup.rs")
        assert "--default-toolchain 1.81.0" in rustup["cmd"]
