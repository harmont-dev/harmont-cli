//! Docker-gated integration test for `hm cache save` / `hm cache restore`.
//!
//! Run: `cargo test -p harmont-cli --features docker-integration -- --ignored cache`

#![cfg(feature = "docker-integration")]
#![allow(
    clippy::unwrap_used,
    reason = "integration tests panic on unexpected failures"
)]
#![allow(
    clippy::expect_used,
    reason = "integration tests panic on unexpected failures"
)]
#![allow(
    clippy::ignore_without_reason,
    reason = "reason is in the test name and doc comment above"
)]

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

#[test]
#[ignore]
fn cache_save_creates_manifest() {
    let cache_dir = TempDir::new().unwrap();
    let cache_path = cache_dir.path();

    let out = Command::cargo_bin("hm")
        .unwrap()
        .args(["cache", "save", cache_path.to_str().unwrap()])
        .assert()
        .success();

    // manifest.json must exist
    let manifest_path = cache_path.join("manifest.json");
    assert!(manifest_path.exists(), "manifest.json should exist");

    let content = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(manifest["version"], 1);

    // stdout has the 16-char hex content hash
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let hash = stdout.trim();
    assert_eq!(hash.len(), 16, "content hash should be 16 hex chars");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "should be hex");
}

#[test]
#[ignore]
fn cache_save_is_deterministic() {
    let cache_dir = TempDir::new().unwrap();
    let path = cache_dir.path().to_str().unwrap();

    let out1 = Command::cargo_bin("hm")
        .unwrap()
        .args(["cache", "save", path])
        .assert()
        .success();
    let out2 = Command::cargo_bin("hm")
        .unwrap()
        .args(["cache", "save", path])
        .assert()
        .success();

    let h1 = String::from_utf8(out1.get_output().stdout.clone()).unwrap();
    let h2 = String::from_utf8(out2.get_output().stdout.clone()).unwrap();
    assert_eq!(h1.trim(), h2.trim(), "content hash should be deterministic");
}

#[test]
#[ignore]
fn cache_restore_after_save() {
    let cache_dir = TempDir::new().unwrap();
    let path = cache_dir.path().to_str().unwrap();

    // Save first
    Command::cargo_bin("hm")
        .unwrap()
        .args(["cache", "save", path])
        .assert()
        .success();

    // Restore — all images already present
    Command::cargo_bin("hm")
        .unwrap()
        .args(["cache", "restore", path])
        .assert()
        .success()
        .stderr(contains("already present"));
}

#[test]
#[ignore]
fn cache_restore_missing_dir() {
    Command::cargo_bin("hm")
        .unwrap()
        .args([
            "cache",
            "restore",
            "/tmp/harmont-nonexistent-cache-dir-test",
        ])
        .assert()
        .success()
        .stderr(contains("0/0"));
}
