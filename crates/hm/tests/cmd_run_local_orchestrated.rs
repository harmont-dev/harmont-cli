//! End-to-end: `hm run --local` against a real Docker daemon, driving
//! the orchestrator and the docker VM runner.
//!
//! Gated `#[ignore]` because it shells out to a real Docker daemon —
//! opt-in with `cargo test -p harmont-cli --test cmd_run_local_orchestrated -- --ignored`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup and assertions"
)]

use assert_cmd::Command;
use predicates::str::contains;
use rstest::rstest;

/// A trivial one-step pipeline that doesn't need any user source — just
/// runs a single `echo` in an alpine container.
const PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("orchestrated")
def orchestrated() -> hm.Step:
    return hm.sh("echo orchestrated hello", label="hi", image="alpine:3.20")
"#;

/// Chain-lineage regression: a 2-step chain (`b.builds_in = a`) must
/// inherit `a`'s filesystem mutations into `b`'s container. Pre-fix,
/// the docker runner booted a fresh container per step, losing
/// `/tmp/a`.
const CHAIN_PIPELINE_PY: &str = r#"
import harmont as hm


@hm.pipeline("chain")
def chain() -> hm.Step:
    a = hm.sh("echo step-a > /tmp/a && cat /tmp/a", label="a", image="alpine:3.20")
    return a.sh("cat /tmp/a && echo step-b", label="b")
"#;

#[rstest]
#[case::single(PIPELINE_PY, "orchestrated", &["orchestrated hello"])]
#[case::chain(CHAIN_PIPELINE_PY, "chain", &["step-a", "step-b"])]
#[ignore = "requires Docker daemon"]
fn hm_run_local_executes_through_orchestrator(
    #[case] pipeline_src: &str,
    #[case] slug: &str,
    #[case] expected_stderr: &[&str],
) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".hm")).unwrap();
    std::fs::write(temp.path().join(".hm/pipeline.py"), pipeline_src).unwrap();

    let mut assert = Command::cargo_bin("hm")
        .unwrap()
        .args(["run", slug])
        .current_dir(temp.path())
        .assert()
        .success();
    for substr in expected_stderr {
        assert = assert.stderr(contains(*substr));
    }
}
