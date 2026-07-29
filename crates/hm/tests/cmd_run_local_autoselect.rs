//! `hm run --local` with no pipeline slug should auto-pick the sole
//! declared pipeline.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup and assertions"
)]

use assert_cmd::Command;
use predicates::str::contains;
use rstest::rstest;

const PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("only-one")
def only_one() -> hm.Step:
    return hm.sh("echo autoselected", label="hi", image="alpine:3.20")
"#;

#[rstest]
#[ignore = "requires Docker daemon; opt-in with `cargo test -- --ignored`"]
fn auto_selects_sole_pipeline() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), PIPELINE_PY).unwrap();

    Command::cargo_bin("hm")
        .unwrap()
        .args(["run"])
        .current_dir(temp.path())
        .assert()
        .success()
        .stderr(contains("autoselected"));
}

#[rstest]
#[case::many(
    r#"
import harmont as hm

@hm.pipeline("a")
def a() -> hm.Step:
    return hm.sh("echo a", image="alpine:3.20")

@hm.pipeline("b")
def b() -> hm.Step:
    return hm.sh("echo b", image="alpine:3.20")
"#,
    "this repo declares pipelines"
)]
#[case::zero("import harmont as hm\n", "no pipelines declared")]
fn run_without_slug_reports_selection_error(
    #[case] pipeline_src: &str,
    #[case] expected_stderr: &str,
) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), pipeline_src).unwrap();

    Command::cargo_bin("hm")
        .unwrap()
        .args(["run"])
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(contains(expected_stderr));
}
