//! `hm version` should exit 0 and print the version.

#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn version_prints_version() {
    Command::cargo_bin("hm")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        // The `version` subcommand reports through `tracing` (stderr), per
        // the CLI-wide "no raw println/eprintln" convention (#14).
        .stderr(contains("hm "));
}
