#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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

#[test]
#[ignore = "requires a running Docker daemon"]
fn independent_chains_overlap_with_parallelism_two() {
    let dir = tempdir().expect("tempdir");
    write_pipeline(dir.path());

    let bin = env!("CARGO_BIN_EXE_hm");
    let start = Instant::now();
    let output = Command::new(bin)
        .args(["run", "--parallelism", "2"])
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
    // Sequential would be >=6s of sleeps plus container start cost.
    // Parallel should land near 3s + start cost. Allow a generous
    // 5.5s budget to absorb Docker cold-start jitter.
    assert!(
        elapsed.as_secs_f64() < 5.5,
        "expected concurrent execution (<5.5s), got {elapsed:?}",
    );
}

#[test]
#[ignore = "requires a running Docker daemon"]
fn parallelism_one_serialises_chains() {
    let dir = tempdir().expect("tempdir");
    write_pipeline(dir.path());

    let bin = env!("CARGO_BIN_EXE_hm");
    let start = Instant::now();
    let output = Command::new(bin)
        .args(["run", "--parallelism", "1"])
        .current_dir(dir.path())
        .output()
        .expect("spawn harmont");
    let elapsed = start.elapsed();

    assert!(output.status.success(), "hm exited non-zero");
    assert!(
        elapsed.as_secs_f64() >= 6.0,
        "with --parallelism 1 the two sleeps must serialise; got {elapsed:?}",
    );
}
