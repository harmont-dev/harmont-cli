//! End-to-end: `-k/--keep-going` continues independent DAG branches
//! even when one step fails, and without it the build fails fast.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;

const FORK_PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("two", default_image="alpine:3.20")
def two():
    a = hm.sh("exit 1", label="fail-step", image="alpine:3.20")
    b = hm.sh("echo ok", label="pass-step", image="alpine:3.20")
    return a.fork(b)
"#;

fn write_fork_pipeline(temp: &tempfile::TempDir) {
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), FORK_PIPELINE_PY).unwrap();
}

#[test]
#[ignore = "requires Docker daemon"]
fn keep_going_runs_independent_branches() {
    let temp = tempfile::tempdir().unwrap();
    write_fork_pipeline(&temp);

    let out = Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "-k", "--format", "json", "two"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "expected non-zero exit; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).unwrap();

    let mut step_starts: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line did not parse as JSON: {line}\nerror: {e}"));
        let kind = v
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or_else(|| panic!("missing 'kind' on line: {line}"));
        if kind == "step_start" {
            if let Some(key) = v.get("step_key").and_then(|k| k.as_str()) {
                step_starts.push(key.to_string());
            }
        }
    }

    assert!(
        step_starts.iter().any(|k| k == "fail-step"),
        "expected step_start for 'fail-step'; saw: {step_starts:?}\nstdout:\n{stdout}"
    );
    assert!(
        step_starts.iter().any(|k| k == "pass-step"),
        "expected step_start for 'pass-step'; saw: {step_starts:?}\nstdout:\n{stdout}"
    );
}

#[test]
#[ignore = "requires Docker daemon"]
fn without_keep_going_fails_fast() {
    let temp = tempfile::tempdir().unwrap();
    write_fork_pipeline(&temp);

    let out = Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "--format", "json", "two"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "expected non-zero exit; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).unwrap();

    let mut saw_chain_failed = false;
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line did not parse as JSON: {line}\nerror: {e}"));
        let kind = v
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or_else(|| panic!("missing 'kind' on line: {line}"));
        if kind == "chain_failed" {
            saw_chain_failed = true;
        }
    }

    assert!(
        saw_chain_failed,
        "expected chain_failed event in stdout:\n{stdout}"
    );
}
