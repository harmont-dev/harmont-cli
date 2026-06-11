//! End-to-end: `hm run --local` against a real Docker daemon, driving
//! the new orchestrator + embedded docker plugin.
//!
//! Gated `#[ignore]` because it shells out to a real Docker daemon —
//! opt-in with `cargo test -p harmont-cli --test cmd_run_local_orchestrated -- --ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::Command;
use predicates::str::contains;

/// A trivial one-step pipeline that doesn't need any user source — just
/// runs a single `echo` in an alpine container.
///
/// Note: the plan's pseudocode used a `hm.command(...)` API that does
/// not exist on the cidsl/py public surface. The real entry point is
/// `hm.sh(cmd, image="...")` (== `scratch().sh(...)`), wrapped in the
/// `@hm.pipeline(slug)` decorator (mirrors the existing
/// `tests/fixtures/pipelines/scratch.py`).
const PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("orchestrated")
def orchestrated() -> hm.Step:
    return hm.sh("echo orchestrated hello", label="hi", image="alpine:3.20")
"#;

#[test]
#[ignore = "requires Docker daemon; opt-in with `cargo test -- --ignored`"]
fn hm_run_local_executes_through_orchestrator() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), PIPELINE_PY).unwrap();

    Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "orchestrated"])
        .current_dir(temp.path())
        .assert()
        .success()
        .stderr(contains("orchestrated hello"));
}

/// Chain-lineage regression: a 2-step chain (`b.builds_in = a`) must
/// inherit `a`'s filesystem mutations into `b`'s container. Pre-fix,
/// the docker plugin booted a fresh container per step, losing
/// `/tmp/a`.
const CHAIN_PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("chain")
def chain() -> hm.Step:
    a = hm.sh("echo step-a > /tmp/a && cat /tmp/a", label="a", image="alpine:3.20")
    return a.sh("cat /tmp/a && echo step-b", label="b")
"#;

#[test]
#[ignore = "requires Docker daemon; opt-in with `cargo test -- --ignored`"]
fn hm_run_local_chain_inherits_filesystem() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), CHAIN_PIPELINE_PY).unwrap();

    Command::cargo_bin("hm")
        .unwrap()
        .args(["run", "chain"])
        .current_dir(temp.path())
        .assert()
        .success()
        .stderr(contains("step-a"))
        .stderr(contains("step-b"));
}
