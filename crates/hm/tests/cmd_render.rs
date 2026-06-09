//! `hm render <slug>` emits one pipeline's v0 IR JSON to stdout.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr
)]

use assert_cmd::Command;

fn write_fixture(dir: &std::path::Path) {
    let harmont = dir.join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(
        harmont.join("ci.py"),
        r"import harmont as hm

@hm.pipeline('ci')
def ci() -> hm.Step:
    return hm.scratch().sh('echo test', label='test')
",
    )
    .unwrap();
}

#[test]
fn render_emits_v0_ir_for_slug() {
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path());

    let out = Command::cargo_bin("hm")
        .unwrap()
        .arg("render")
        .arg("ci")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["version"], "0");
    assert!(
        v["graph"].is_object(),
        "expected a graph object in the v0 IR, got: {v}"
    );
}

#[test]
fn render_unknown_slug_fails_with_available_on_stderr() {
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path());

    Command::cargo_bin("hm")
        .unwrap()
        .arg("render")
        .arg("nope")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .failure()
        // Both the bad slug and the list of available slugs must reach stderr.
        .stderr(predicates::str::contains("nope"))
        .stderr(predicates::str::contains("available: ci"));
}
