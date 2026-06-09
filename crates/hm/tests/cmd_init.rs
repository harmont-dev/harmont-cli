//! `hm init` scaffolds a `.hm/` pipeline from a project template.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
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

    let pipeline = dir.path().join(".hm/pipeline.py");
    assert!(pipeline.exists(), "expected {}", pipeline.display());

    let content = std::fs::read_to_string(&pipeline).unwrap();
    assert!(
        content.contains("hm.rust"),
        "expected rust toolchain import"
    );
    assert!(
        content.contains("@hm.pipeline"),
        "expected pipeline decorator"
    );
}

#[test]
fn init_zig_creates_pipeline_ts() {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", "zig", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    let pipeline = dir.path().join(".hm/pipeline.ts");
    assert!(pipeline.exists(), "expected {}", pipeline.display());

    let content = std::fs::read_to_string(&pipeline).unwrap();
    assert!(content.contains("zig"), "expected zig toolchain import");
    assert!(
        content.contains("export default"),
        "expected default export"
    );
}

#[test]
fn init_existing_hm_dir_no_pipeline_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".hm")).unwrap();

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success();
}

#[test]
fn init_existing_pipeline_without_force_warns_and_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir(&harmont).unwrap();
    std::fs::write(harmont.join("pipeline.py"), "# old").unwrap();

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success()
        .stderr(contains("pipeline already exists"));
}

#[test]
fn init_force_overwrites_existing() {
    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir(&harmont).unwrap();
    std::fs::write(harmont.join("old.py"), "# old").unwrap();

    hm().args(["init", "--template", "rust", "--force", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    assert!(dir.path().join(".hm/pipeline.py").exists());
    assert!(
        !harmont.join("old.py").exists(),
        "stale file should be removed on --force"
    );
}

#[test]
fn init_force_replaces_existing_pipeline() {
    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir(&harmont).unwrap();
    std::fs::write(harmont.join("pipeline.py"), "# old pipeline").unwrap();

    hm().args(["init", "--template", "rust", "--force", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    let content = std::fs::read_to_string(harmont.join("pipeline.py")).unwrap();
    assert!(
        content.contains("hm.rust"),
        "force should overwrite with new template content"
    );
    assert!(
        !content.contains("# old pipeline"),
        "old content should be gone"
    );
}

#[test]
fn init_skips_pipeline_when_one_exists() {
    let dir = tempfile::tempdir().unwrap();
    let hm_dir = dir.path().join(".hm");
    std::fs::create_dir(&hm_dir).unwrap();
    std::fs::write(hm_dir.join("pipeline.py"), "# existing").unwrap();

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success()
        .stderr(contains("pipeline already exists"));

    let content = std::fs::read_to_string(hm_dir.join("pipeline.py")).unwrap();
    assert_eq!(
        content, "# existing",
        "pipeline.py should be left untouched"
    );
}

#[test]
fn init_writes_pipeline_when_hm_dir_exists_but_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".hm")).unwrap();

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    assert!(
        dir.path().join(".hm/pipeline.py").exists(),
        "pipeline should be created even though .hm/ existed"
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

        let has_py = dir.path().join(".hm/pipeline.py").exists();
        let has_ts = dir.path().join(".hm/pipeline.ts").exists();
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
        assert!(
            v["graph"].is_object(),
            "template {slug}: expected graph object"
        );
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
        assert!(
            v["graph"].is_object(),
            "template {slug}: expected graph object"
        );
    }
}

// ── skills ───────────────────────────────────────────────────────

#[test]
fn init_noninteractive_skips_skills() {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    let skill_validate = dir.path().join(".claude/skills/validate-ci/SKILL.md");
    assert!(
        !skill_validate.exists(),
        "non-interactive init should not create skills"
    );

    let skill_pipeline = dir.path().join(".claude/skills/write-pipeline/SKILL.md");
    assert!(
        !skill_pipeline.exists(),
        "non-interactive init should not create write-pipeline skill"
    );
}

#[test]
fn init_noninteractive_skips_convert_gha_skill() {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    let skill = dir.path().join(".claude/skills/convert-gha/SKILL.md");
    assert!(
        !skill.exists(),
        "non-interactive init should not create convert-gha skill"
    );
}

#[test]
fn skill_validate_ci_content_is_well_formed() {
    let content = include_str!(
        "../src/commands/init_templates/skill_validate_ci.md"
    );
    assert!(!content.is_empty(), "skill template must not be empty");
    assert!(
        content.contains("hm run"),
        "skill must reference `hm run`"
    );
    assert!(
        content.contains("## When to use"),
        "skill must have 'When to use' section"
    );
    assert!(
        content.contains("## When NOT to use"),
        "skill must have 'When NOT to use' section"
    );
    assert!(
        content.contains("## Procedure"),
        "skill must have 'Procedure' section"
    );
}

#[test]
fn skill_write_pipeline_content_is_well_formed() {
    let content = include_str!(
        "../src/commands/init_templates/skill_write_pipeline.md"
    );
    assert!(!content.is_empty(), "skill template must not be empty");
    assert!(
        content.contains("docs.harmont.dev"),
        "skill must reference documentation site"
    );
    assert!(
        content.contains("hm run"),
        "skill must reference `hm run` for validation"
    );
    assert!(
        content.contains("## When to use"),
        "skill must have 'When to use' section"
    );
    assert!(
        content.contains("## When NOT to use"),
        "skill must have 'When NOT to use' section"
    );
    assert!(
        content.contains("## Procedure"),
        "skill must have 'Procedure' section"
    );
    assert!(
        content.contains("gh issue create"),
        "skill must include gh issue filing instructions"
    );
}

#[test]
fn init_detects_github_workflows_in_noninteractive_mode() {
    let dir = tempfile::tempdir().unwrap();
    let workflows = dir.path().join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    std::fs::write(workflows.join("ci.yml"), "name: CI\non: push").unwrap();

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success()
        .stderr(contains("convert-gha"));
}

#[test]
fn skill_convert_gha_content_is_well_formed() {
    let content = include_str!(
        "../src/commands/init_templates/skill_convert_gha.md"
    );
    assert!(!content.is_empty(), "skill template must not be empty");
    assert!(
        content.contains("## When to use"),
        "skill must have 'When to use' section"
    );
    assert!(
        content.contains("## When NOT to use"),
        "skill must have 'When NOT to use' section"
    );
    assert!(
        content.contains("## Procedure"),
        "skill must have 'Procedure' section"
    );
    assert!(
        content.contains("write-pipeline"),
        "skill must reference write-pipeline skill"
    );
    assert!(
        content.contains("actions/cache"),
        "skill must mention actions/cache and implicit caching"
    );
    assert!(
        content.contains("actions/checkout"),
        "skill must mention actions/checkout is not needed"
    );
}

#[test]
fn init_no_gha_hint_without_workflows_dir() {
    let dir = tempfile::tempdir().unwrap();

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("convert-gha").not());
}

#[test]
fn init_no_gha_hint_with_empty_workflows_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".github/workflows")).unwrap();

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("convert-gha").not());
}
