//! End-to-end: a failing pipeline step routes through `BuildEvent::ChainFailed`
//! and both output plugins render it. See plan task B5 in
//! docs/superpowers/plans/2026-05-19-pr22-followups.md.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;

const FAILING_PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("failing")
def failing() -> hm.Step:
    return hm.sh("exit 7", label="oops", image="alpine:3.20")
"#;

fn write_failing_pipeline(temp: &tempfile::TempDir) {
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), FAILING_PIPELINE_PY).unwrap();
}

#[test]
#[ignore = "requires Docker daemon"]
fn human_format_renders_chain_failure_to_stderr() {
    let temp = tempfile::tempdir().unwrap();
    write_failing_pipeline(&temp);
    let assert = Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "--format", "human", "failing"])
        .current_dir(temp.path())
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    // The human plugin renders ChainFailed via:
    //   "chain {chain_idx}: FAILED at step '{failed_step_key}' (exit={exit_code}): {message}\n"
    // The step's `key` is the slugified label, so `label="oops"` => key="oops".
    assert!(
        stderr.contains("FAILED at step 'oops'"),
        "stderr missing \"FAILED at step 'oops'\":\n{stderr}"
    );
    assert!(
        stderr.contains("(exit=7)"),
        "stderr missing \"(exit=7)\":\n{stderr}"
    );
}

#[test]
#[ignore = "requires Docker daemon"]
fn json_format_emits_chain_failed_event() {
    let temp = tempfile::tempdir().unwrap();
    write_failing_pipeline(&temp);
    let out = Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "--format", "json", "failing"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "expected non-zero exit; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    // Every emitted line must parse as a JSON object carrying "kind".
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
            assert_eq!(
                v.get("failed_step_key").and_then(|k| k.as_str()),
                Some("oops"),
                "chain_failed event missing/incorrect failed_step_key=\"oops\" on line: {line}"
            );
            assert_eq!(
                v.get("exit_code").and_then(serde_json::Value::as_i64),
                Some(7),
                "chain_failed event missing/incorrect exit_code=7 on line: {line}"
            );
        }
    }
    assert!(
        saw_chain_failed,
        "stdout missing chain_failed event:\n{stdout}"
    );
}
