//! `hm plugin list` smoke test.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup and assertions"
)]

use assert_cmd::Command;
use predicates::str::contains;
use rstest::rstest;

#[rstest]
fn plugin_list_shows_registered_runners() {
    Command::cargo_bin("hm")
        .unwrap()
        .arg("plugin")
        .arg("list")
        .assert()
        .success()
        // `plugin list` reports through `tracing` (stderr), per the
        // CLI-wide "no raw println/eprintln" convention (#14).
        .stderr(contains("docker"));
}
