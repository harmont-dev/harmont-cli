//! `hm pipelines` emits the discovery envelope JSON to stdout.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr
)]

use assert_cmd::Command;

#[test]
fn pipelines_emits_discovery_envelope() {
    if which::which("python3").is_err() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir_all(&harmont).unwrap();
    std::fs::write(
        harmont.join("ci.py"),
        r"import harmont as hm

@hm.pipeline('ci', name='CI', triggers=[hm.push(branch='main')])
def ci() -> hm.Step:
    return hm.scratch().sh('echo test', label='test')
",
    )
    .unwrap();

    let out = Command::cargo_bin("hm")
        .unwrap()
        .arg("pipelines")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["pipelines"][0]["slug"], "ci");
    assert_eq!(v["pipelines"][0]["triggers"][0]["event"], "push");
}

#[test]
fn pipelines_emits_empty_envelope_when_no_harmont_dir() {
    // A repo that declares no pipelines must yield an empty envelope, not an
    // error (the backend fans discovery across every repo in an installation,
    // most of which carry no `.hm/`). No python3 needed — this short-
    // circuits before the DSL engine, so the test always runs.
    let dir = tempfile::tempdir().unwrap();

    let out = Command::cargo_bin("hm")
        .unwrap()
        .arg("pipelines")
        .arg("--dir")
        .arg(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["pipelines"], serde_json::json!([]));
}
