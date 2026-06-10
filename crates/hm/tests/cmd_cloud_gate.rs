//! Integration tests proving that `hm cloud` is wired to the real plugin
//! (the old waitlist gate has been removed).

#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

/// Any authenticated verb must fail fast with an auth-required error when
/// no token is present.  The command must NOT succeed (proving the gate is
/// gone and the plugin is reached), and the error message must tell the
/// user exactly how to fix it.
///
/// Hermetic: `HOME` is overridden to a fresh temp dir (no
/// `~/.config/hm/credentials.toml`) and `HM_API_TOKEN` is explicitly
/// unset, so no credentials can bleed in from the developer's machine.
#[test]
fn cloud_unauthed_verb_fails_with_login_hint() {
    let tmp = tempfile::tempdir().unwrap();
    Command::cargo_bin("hm")
        .unwrap()
        .args(["cloud", "billing", "balance"])
        .env("HOME", tmp.path())
        .env_remove("HM_API_TOKEN")
        .assert()
        .failure()
        .stderr(contains("not logged in"));
}

/// `hm cloud --help` must succeed and advertise the real subcommands.
/// It must not contain any trace of the removed waitlist copy.
///
/// This is the meaningful replacement for the old
/// `cloud_login_prints_waitlist` test: login now starts a real browser
/// OAuth flow that is unsuitable for a hermetic test, but `--help` proves
/// the subcommand is wired and the old gate text is gone.
#[test]
fn cloud_help_lists_real_subcommands_without_waitlist_text() {
    Command::cargo_bin("hm")
        .unwrap()
        .args(["cloud", "--help"])
        .assert()
        .success()
        .stdout(contains("login"))
        .stdout(contains("whoami"))
        .stdout(contains("build"))
        .stdout(predicates::str::contains("not yet available").not());
}
