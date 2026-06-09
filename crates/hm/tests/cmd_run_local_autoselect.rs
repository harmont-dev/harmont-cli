//! `hm run --local` with no pipeline slug should auto-pick the sole
//! declared pipeline.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use predicates::str::contains;

const PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("only-one", default_image="alpine:3.20")
def only_one() -> hm.Step:
    return hm.sh("echo autoselected", label="hi", image="alpine:3.20")
"#;

#[test]
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

#[test]
fn many_pipelines_still_requires_arg() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(
        temp.path().join(".hm/pipeline.py"),
        r#"
import harmont as hm

@hm.pipeline("a", default_image="alpine:3.20")
def a() -> hm.Step:
    return hm.sh("echo a")

@hm.pipeline("b", default_image="alpine:3.20")
def b() -> hm.Step:
    return hm.sh("echo b")
"#,
    )
    .unwrap();

    Command::cargo_bin("hm")
        .unwrap()
        .args(["run"])
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(contains("this repo declares pipelines"));
}

#[test]
fn zero_pipelines_returns_a_helpful_error() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(
        temp.path().join(".hm/pipeline.py"),
        "import harmont as hm\n",
    )
    .unwrap();

    Command::cargo_bin("hm")
        .unwrap()
        .args(["run"])
        .current_dir(temp.path())
        .assert()
        .failure()
        .stderr(contains("no pipelines declared"));
}
