//! End-to-end: `-k/--keep-going` continues independent DAG branches
//! even when one step fails, and without it the build fails fast.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;

const FORK_PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("two")
def two():
    root = hm.scratch().fork()
    a = root.sh("exit 1", label="fail-step", image="alpine:3.20")
    b = root.sh("echo ok", label="pass-step", image="alpine:3.20")
    return (a, b)
"#;

fn write_fork_pipeline(temp: &tempfile::TempDir) {
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), FORK_PIPELINE_PY).unwrap();
}

/// A->B->C linear `BuildsIn` chain where the root (A) fails. Under `-k` the
/// failure must propagate transitively: B is skipped (direct dependent) AND C
/// is skipped (grandchild). Distinct from the fork pipeline above, which only
/// covers independent branches off a single failed step.
const CHAIN_PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("chain")
def chain():
    a = hm.sh("exit 1", label="step-a", image="alpine:3.20")
    b = a.sh("echo b", label="step-b")
    c = b.sh("echo c", label="step-c")
    return c
"#;

fn write_chain_pipeline(temp: &tempfile::TempDir) {
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), CHAIN_PIPELINE_PY).unwrap();
}

/// Parse the JSON event stream and return the step keys that actually
/// started. `step_start` carries only `step_id`, so we first build a
/// `step_id -> key` map from the `step_queued` events, then resolve every
/// `step_start`'s id back to its key.
fn collect_step_starts(stdout: &str) -> Vec<String> {
    use std::collections::HashMap;

    let mut id_to_key: HashMap<String, String> = HashMap::new();
    let mut started_ids: Vec<String> = Vec::new();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("line did not parse as JSON: {line}\nerror: {e}"));
        match v.get("kind").and_then(|k| k.as_str()) {
            Some("step_queued") => {
                if let (Some(id), Some(key)) = (
                    v.get("step_id").and_then(|k| k.as_str()),
                    v.get("key").and_then(|k| k.as_str()),
                ) {
                    id_to_key.insert(id.to_string(), key.to_string());
                }
            }
            Some("step_start") => {
                if let Some(id) = v.get("step_id").and_then(|k| k.as_str()) {
                    started_ids.push(id.to_string());
                }
            }
            _ => {}
        }
    }

    started_ids
        .iter()
        .filter_map(|id| id_to_key.get(id).cloned())
        .collect()
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
    let step_starts = collect_step_starts(&stdout);

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
fn keep_going_skips_transitive_dependents() {
    // A->B->C linear chain, A fails. Under -k, B (child) and C (grandchild)
    // must both be skipped. The grandchild C is the regression guard: a
    // skipped B reports exit_code 0, so an exit-code-only gate would let C
    // run on a clean base image. Skips emit no `step_start`, so the absence
    // of one for B and C is the observable signal.
    let temp = tempfile::tempdir().unwrap();
    write_chain_pipeline(&temp);

    let out = Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "-k", "--format", "json", "chain"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "expected non-zero exit; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).unwrap();
    let step_starts = collect_step_starts(&stdout);

    assert!(
        step_starts.iter().any(|k| k == "step-a"),
        "expected step_start for 'step-a'; saw: {step_starts:?}\nstdout:\n{stdout}"
    );
    assert!(
        !step_starts.iter().any(|k| k == "step-b"),
        "'step-b' must be skipped (direct dependent of failed 'step-a'); saw: {step_starts:?}\nstdout:\n{stdout}"
    );
    assert!(
        !step_starts.iter().any(|k| k == "step-c"),
        "'step-c' must be skipped transitively (grandchild of failed 'step-a'); saw: {step_starts:?}\nstdout:\n{stdout}"
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
