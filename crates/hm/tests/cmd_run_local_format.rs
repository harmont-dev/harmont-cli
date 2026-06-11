//! End-to-end: `hm run --local --format <name>` exercises both
//! output plugins against a real Docker daemon.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use predicates::str::contains;

const PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("formatted")
def formatted() -> hm.Step:
    return hm.sh("echo formatted-hello", label="hi", image="alpine:3.20")
"#;

fn write_pipeline(temp: &tempfile::TempDir) {
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), PIPELINE_PY).unwrap();
}

#[test]
#[ignore = "requires Docker daemon"]
fn format_human_writes_prefixed_lines_to_stderr() {
    let temp = tempfile::tempdir().unwrap();
    write_pipeline(&temp);
    Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "--format", "human", "formatted"])
        .current_dir(temp.path())
        .assert()
        .success()
        .stderr(contains("[hi] formatted-hello"));
}

#[test]
#[ignore = "requires Docker daemon"]
fn format_json_writes_one_event_per_line_to_stdout() {
    let temp = tempfile::tempdir().unwrap();
    write_pipeline(&temp);
    let out = Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "--format", "json", "formatted"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    // Every line should parse as JSON.
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("line did not parse as JSON: {line}\nerror: {e}");
        });
        assert!(v.get("kind").is_some(), "missing kind on line: {line}");
    }
    // The build_start event must be present.
    assert!(stdout.contains(r#""kind":"build_start""#));
    // The step_log line containing "formatted-hello" must be present.
    assert!(stdout.contains("formatted-hello"));
}

#[test]
fn unknown_format_fails_fast_with_listing() {
    let temp = tempfile::tempdir().unwrap();
    write_pipeline(&temp);
    Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "--format", "nope", "formatted"])
        .current_dir(temp.path())
        .assert()
        .failure()
        // The doctrine error is reported through `tracing` (stderr).
        .stderr(contains("unknown --format 'nope'"));
}
