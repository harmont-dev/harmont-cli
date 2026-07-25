#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup and assertions"
)]
//! HAR-22 regression: independent chains must run in parallel under
//! `--local`. We build a pipeline with two sibling chains each
//! sleeping 3 seconds. With `--parallelism 2` and no other work,
//! the executor should finish well under 6 seconds.
//!
//! Requires Docker. Marked `#[ignore]`.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use rstest::rstest;
use tempfile::tempdir;

fn write_pipeline(dir: &Path) {
    let harmont = dir.join(".hm");
    fs::create_dir_all(&harmont).expect("mkdir .hm");
    fs::write(dir.join("placeholder.txt"), "x").expect("placeholder");
    fs::write(
        harmont.join("pipeline.py"),
        r#"
import harmont as hm

def build():
    a = hm.scratch().sh("sleep 3", label="sleep-a", image="alpine:3.20")
    b = hm.scratch().sh("sleep 3", label="sleep-b", image="alpine:3.20")
    return hm.pipeline([a, b])
"#,
    )
    .expect("pipeline.py");
}

/// Whether the elapsed wall-clock must land below (parallel) or at/above
/// (serial) the configured bound.
enum TimingBound {
    /// Parallel: independent sleeps overlap, so total must stay under.
    Below,
    /// Serial: the two sleeps must add up, so total must reach at least.
    AtLeast,
}

#[rstest]
#[case::parallelism_two_overlaps("2", TimingBound::Below, 5.5)]
#[case::parallelism_one_serialises("1", TimingBound::AtLeast, 6.0)]
#[ignore = "requires a running Docker daemon"]
fn parallelism_controls_chain_overlap(
    #[case] parallelism: &str,
    #[case] bound: TimingBound,
    #[case] threshold_secs: f64,
) {
    let dir = tempdir().expect("tempdir");
    write_pipeline(dir.path());

    let bin = env!("CARGO_BIN_EXE_hm");
    let start = Instant::now();
    let output = Command::new(bin)
        .args(["run", "--parallelism", parallelism])
        .current_dir(dir.path())
        .output()
        .expect("spawn harmont");
    let elapsed = start.elapsed();

    assert!(
        output.status.success(),
        "hm exited non-zero:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    // With --parallelism 2 the two independent 3s sleeps overlap and the
    // build lands near 3s + start cost (generous 5.5s budget absorbs
    // Docker cold-start jitter). With --parallelism 1 they serialise and
    // total must reach >=6s.
    match bound {
        TimingBound::Below => assert!(
            elapsed.as_secs_f64() < threshold_secs,
            "expected concurrent execution (<{threshold_secs}s), got {elapsed:?}",
        ),
        TimingBound::AtLeast => assert!(
            elapsed.as_secs_f64() >= threshold_secs,
            "with --parallelism {parallelism} the two sleeps must serialise; got {elapsed:?}",
        ),
    }
}
