//! `hm init` scaffolds a `.hm/` pipeline from a project template.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup and assertions"
)]

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use rstest::rstest;

fn hm() -> Command {
    Command::cargo_bin("hm").unwrap()
}

// ── non-interactive (--template) ──────────────────────────────

#[rstest]
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
        content.contains("@hm.pipeline"),
        "expected pipeline decorator"
    );
    assert!(
        content.contains("hm.rust.project("),
        "expected rust.project() entrypoint, got:\n{content}"
    );
    assert!(
        content.contains(".ci()"),
        "expected the one-call .ci() DAG, got:\n{content}"
    );
    assert!(
        !content.contains("rust.toolchain("),
        "template should not use the legacy toolchain() API, got:\n{content}"
    );
}

#[rstest]
fn init_existing_hm_dir_no_pipeline_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".hm")).unwrap();

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success();
}

#[rstest]
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

#[rstest]
fn init_force_preserves_coresident_files() {
    // `--force` must overwrite ONLY the target template file. It must never
    // wipe the whole `.hm/` directory: config.toml and any co-resident
    // pipeline (e.g. a hand-written deploy.py) must survive.
    let dir = tempfile::tempdir().unwrap();
    let harmont = dir.path().join(".hm");
    std::fs::create_dir(&harmont).unwrap();
    std::fs::write(harmont.join("config.toml"), "backend = \"cloud\"\n").unwrap();
    std::fs::write(harmont.join("deploy.py"), "# co-resident pipeline").unwrap();

    hm().args(["init", "--template", "rust", "--force", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    assert!(dir.path().join(".hm/pipeline.py").exists());
    assert_eq!(
        std::fs::read_to_string(harmont.join("config.toml")).unwrap(),
        "backend = \"cloud\"\n",
        "config.toml must survive --force"
    );
    assert_eq!(
        std::fs::read_to_string(harmont.join("deploy.py")).unwrap(),
        "# co-resident pipeline",
        "co-resident pipeline must survive --force"
    );
}

#[rstest]
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

#[rstest]
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

#[rstest]
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

#[rstest]
fn init_unknown_template_rejected_by_clap() {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", "cobol", "--dir"])
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(contains("invalid value"));
}

#[rstest]
#[case::cmake("cmake")]
#[case::elixir("elixir")]
#[case::nextjs("nextjs")]
#[case::js("js")]
#[case::rust("rust")]
#[case::zig("zig")]
#[case::python("python")]
fn init_all_templates_create_files(#[case] slug: &str) {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", slug, "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    let has_py = dir.path().join(".hm/pipeline.py").exists();
    assert!(has_py, "template {slug}: no pipeline file created");
}

// ── roundtrip: init → render ──────────────────────────────────

fn has_python() -> bool {
    which::which("python3").is_ok()
}

#[rstest]
#[case::cmake("cmake")]
#[case::elixir("elixir")]
#[case::rust("rust")]
#[case::python("python")]
fn init_python_templates_roundtrip_render(#[case] slug: &str) {
    if !has_python() {
        return;
    }

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

    let v: serde_json::Value =
        serde_json::from_slice(&out).unwrap_or_else(|e| panic!("template {slug}: invalid JSON: {e}"));
    assert_eq!(v["version"], "0", "template {slug}: expected v0 IR");
    assert!(
        v["graph"].is_object(),
        "template {slug}: expected graph object"
    );
}

// ── skills ───────────────────────────────────────────────────────

#[rstest]
#[case::validate_ci("validate-ci")]
#[case::write_pipeline("write-pipeline")]
#[case::convert_gha("convert-gha")]
fn init_noninteractive_skips_skills(#[case] skill: &str) {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    let skill_md = dir.path().join(format!(".claude/skills/{skill}/SKILL.md"));
    assert!(
        !skill_md.exists(),
        "non-interactive init should not create the {skill} skill"
    );
}

#[rstest]
#[case::validate_ci(
    include_str!("../src/commands/init_templates/skill_validate_ci.md"),
    &["hm run"]
)]
#[case::write_pipeline(
    include_str!("../src/commands/init_templates/skill_write_pipeline.md"),
    &["docs.harmont.dev", "hm run", "gh issue create"]
)]
#[case::convert_gha(
    include_str!("../src/commands/init_templates/skill_convert_gha.md"),
    &["write-pipeline", "actions/cache", "actions/checkout"]
)]
fn skill_content_is_well_formed(#[case] content: &str, #[case] extras: &[&str]) {
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
    for needle in extras {
        assert!(
            content.contains(needle),
            "skill must reference `{needle}`"
        );
    }
}

#[rstest]
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

#[rstest]
#[case::no_dir(false)]
#[case::empty_dir(true)]
fn init_no_gha_hint_without_real_workflows(#[case] create_empty_workflows_dir: bool) {
    let dir = tempfile::tempdir().unwrap();
    if create_empty_workflows_dir {
        std::fs::create_dir_all(dir.path().join(".github/workflows")).unwrap();
    }

    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicates::str::contains("convert-gha").not());
}

#[rstest]
fn init_skips_template_prompt_when_pipeline_exists() {
    // A project that already has a pipeline. Running `hm init` with no
    // --template (interactive intent) in a non-TTY context must NOT try to
    // prompt for a template; it should skip template selection, leave the
    // pipeline untouched, and exit successfully.
    let dir = tempfile::tempdir().unwrap();
    let hm_dir = dir.path().join(".hm");
    std::fs::create_dir(&hm_dir).unwrap();
    std::fs::write(hm_dir.join("pipeline.py"), "# existing").unwrap();

    hm().args(["init", "--dir"])
        .arg(dir.path())
        .assert()
        .success()
        .stderr(contains("skipping template selection"))
        .stderr(contains("template selection cancelled").not());

    // Existing pipeline must be left exactly as-is.
    let content = std::fs::read_to_string(hm_dir.join("pipeline.py")).unwrap();
    assert_eq!(content, "# existing", "pipeline.py must be untouched");

    // Non-TTY: skills are not installed (no prompt possible).
    assert!(
        !dir.path()
            .join(".claude/skills/validate-ci/SKILL.md")
            .exists(),
        "skills should not install without a TTY"
    );
}

#[rstest]
fn init_without_template_in_non_tty_errors_clearly() {
    // No pipeline, no --template, no TTY: cannot prompt, so fail with a
    // helpful hint rather than a raw dialoguer IO error.
    let dir = tempfile::tempdir().unwrap();

    hm().args(["init", "--dir"])
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(contains("no template specified"));

    assert!(
        !dir.path().join(".hm/pipeline.py").exists(),
        "no pipeline should be written when none could be chosen"
    );
}

// ── cloud registration ──────────────────────────────────────

#[rstest]
fn init_noninteractive_skips_cloud_registration() {
    let dir = tempfile::tempdir().unwrap();
    hm().args(["init", "--template", "rust", "--dir"])
        .arg(dir.path())
        .assert()
        .success();

    let config = dir.path().join(".hm/config.toml");
    assert!(
        !config.exists(),
        "non-interactive init should not create .hm/config.toml"
    );
}

#[rstest]
fn cloud_project_config_layers_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let hm_dir = dir.path().join(".hm");
    std::fs::create_dir(&hm_dir).unwrap();

    let config_path = hm_dir.join("config.toml");
    let content = "backend = \"cloud\"\n\n[cloud]\norg = \"test-org\"\n";
    std::fs::write(&config_path, content).unwrap();

    let cfg = hm_config::Config::load_from_paths(None, Some(&config_path)).unwrap();
    assert_eq!(cfg.backend, hm_config::Backend::Cloud);
    assert_eq!(cfg.cloud.org.as_deref(), Some("test-org"));
    // Unrelated defaults survive layering.
    assert_eq!(cfg.preferences.format, "human");
}
