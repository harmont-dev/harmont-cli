#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! HAR-22 regression: a cache hit at a chain root must NOT pin the
//! forked children to stale /workspace. We build a tiny pipeline that
//! caches a cheap "echo" step on a forever policy, then forks off a
//! child that prints a marker file. Two runs with different marker
//! contents must print different markers — proving the second run
//! saw the refreshed source even though the parent cache hit.
//!
//! Requires Docker. Marked `#[ignore]` so CI without a daemon skips
//! it: `cargo test --test local_fork_cache -- --ignored`.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn write_pipeline(dir: &Path, marker_contents: &str) {
    let harmont = dir.join(".hm");
    fs::create_dir_all(&harmont).expect("mkdir .hm");
    fs::write(dir.join("marker.txt"), marker_contents).expect("marker.txt");
    fs::write(
        harmont.join("pipeline.py"),
        r#"
import harmont as hm

def build():
    base = hm.scratch().run(
        "echo base-ran",
        label="base",
        cache=hm.forever(),
    )
    child = base.fork(label="child").run(
        "cat /workspace/marker.txt",
        label="child",
    )
    return hm.pipeline(child, default_image="alpine:3.20")
"#,
    )
    .expect("pipeline.py");
}

fn run_harmont(repo: &Path) -> String {
    let bin = env!("CARGO_BIN_EXE_hm");
    let output = Command::new(bin)
        .args(["run"])
        .current_dir(repo)
        .output()
        .expect("spawn harmont");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "hm run --local failed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stderr
}

#[test]
#[ignore = "requires a running Docker daemon and network for image pulls"]
fn fork_child_sees_refreshed_source_after_parent_cache_hit() {
    let dir = tempdir().expect("tempdir");

    // First run: cold cache, marker = "v1".
    write_pipeline(dir.path(), "v1");
    let out1 = run_harmont(dir.path());
    assert!(
        out1.contains("v1"),
        "first run did not print marker v1:\n{out1}"
    );

    // Update the source — leave the pipeline alone so the cache key
    // for `base` stays identical (forever policy ignores file content
    // outside the cmd / env_keys).
    fs::write(dir.path().join("marker.txt"), "v2").expect("rewrite marker.txt");

    // Second run: `base` cache should HIT. The child must see v2.
    let out2 = run_harmont(dir.path());
    assert!(
        out2.contains("v2"),
        "second run did not see refreshed source (regression of HAR-22):\n{out2}"
    );
    assert!(
        !out2.contains("v1"),
        "second run leaked stale source (regression of HAR-22):\n{out2}"
    );
}
