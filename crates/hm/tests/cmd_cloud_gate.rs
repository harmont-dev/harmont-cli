//! All `hm cloud` subcommands should print the waitlist gate message.

#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn cloud_login_prints_waitlist() {
    Command::cargo_bin("hm")
        .unwrap()
        .args(["cloud", "login"])
        .assert()
        .success()
        .stderr(contains("https://harmont.dev"));
}

#[test]
fn cloud_billing_prints_waitlist() {
    Command::cargo_bin("hm")
        .unwrap()
        .args(["cloud", "billing", "balance"])
        .assert()
        .success()
        .stderr(contains("not yet available"));
}
