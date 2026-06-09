//! `hm init` scaffolds a `.harmont/` pipeline from a project template.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use predicates::str::contains;

fn hm() -> Command {
    Command::cargo_bin("hm").unwrap()
}

// ── non-interactive (--template) ──────────────────────────────

#[test]
fn init_rust_creates_pipeline_py() {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    let pipeline = dir.path().join(".harmont/pipeline.py");
    assert!(pipeline.exists(), "expected {}", pipeline.display());

    let content = std::fs::read_to_string(&pipeline).unwrap();
    assert!(content.contains("hm.rust"), "expected rust toolchain import");
    assert!(content.contains("@hm.pipeline"), "expected pipeline decorator");
}

#[test]
fn init_zig_creates_pipeline_ts() {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", "zig", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    let pipeline = dir.path().join(".harmont/pipeline.ts");
    assert!(pipeline.exists(), "expected {}", pipeline.display());

    let content = std::fs::read_to_string(&pipeline).unwrap();
    assert!(content.contains("zig"), "expected zig toolchain import");
    assert!(content.contains("export default"), "expected default export");
}

#[test]
fn init_fails_on_existing_harmont_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".harmont")).unwrap();

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(contains(".harmont"));
}

#[test]
fn init_force_overwrites_existing() {
    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".harmont");
    std::fs::create_dir(&harmont).unwrap();
    std::fs::write(harmont.join("old.py"), "# old").unwrap();

    hm().args(["init", "--template", "rust", "--force", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    assert!(dir.path().join(".harmont/pipeline.py").exists());
    assert!(
        !harmont.join("old.py").exists(),
        "stale file should be removed on --force"
    );
}

#[test]
fn init_unknown_template_rejected_by_clap() {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", "cobol", "--dir"])
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(contains("invalid value"));
}

#[test]
fn init_all_templates_create_files() {
    for slug in ["cmake", "elixir", "nextjs", "js", "rust", "zig", "python"] {
        let dir = tempfile::tempdir().unwrap();
        hm().args(["init", "--template", slug, "--dir"])
            .arg(dir.path())
            .assert()
            .success();

        let has_py = dir.path().join(".harmont/pipeline.py").exists();
        let has_ts = dir.path().join(".harmont/pipeline.ts").exists();
        assert!(
            has_py || has_ts,
            "template {slug}: no pipeline file created"
        );
    }
}

// ── roundtrip: init → render ──────────────────────────────────

fn has_python() -> bool {
    which::which("python3").is_ok()
}

fn has_js_runtime() -> bool {
    which::which("bun").is_ok() || which::which("node").is_ok()
}

#[test]
fn init_python_templates_roundtrip_render() {
    if !has_python() {
        return;
    }

    for slug in ["cmake", "elixir", "rust", "python"] {
        let dir = tempfile::tempdir().unwrap();
        hm().args(["init", "--template", slug, "--dir"])
            .arg(dir.path())
            .assert()
            .success();

        let out = hm()
            .args(["render", "ci", "--dir"])
            .arg(dir.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();

        let v: serde_json::Value = serde_json::from_slice(&out)
            .unwrap_or_else(|e| panic!("template {slug}: invalid JSON: {e}"));
        assert_eq!(v["version"], "0", "template {slug}: expected v0 IR");
        assert!(v["graph"].is_object(), "template {slug}: expected graph object");
    }
}

#[test]
fn init_ts_templates_roundtrip_render() {
    if !has_js_runtime() {
        return;
    }

    for slug in ["nextjs", "js", "zig"] {
        let dir = tempfile::tempdir().unwrap();
        hm().args(["init", "--template", slug, "--dir"])
            .arg(dir.path())
            .assert()
            .success();

        let out = hm()
            .args(["render", "ci", "--dir"])
            .arg(dir.path())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();

        let v: serde_json::Value = serde_json::from_slice(&out)
            .unwrap_or_else(|e| panic!("template {slug}: invalid JSON: {e}"));
        assert_eq!(v["version"], "0", "template {slug}: expected v0 IR");
        assert!(v["graph"].is_object(), "template {slug}: expected graph object");
    }
}
