#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! End-to-end tests for `harmont run --local`.
//!
//! Skipped unless `HARMONT_LOCAL_E2E=1` is set AND a Docker daemon is
//! reachable — we don't want CI matrices that lack docker-in-docker
//! to fail.
//!
//! Run sequentially; multiple tests in this file would otherwise race
//! on identical container names like `harmont-local-<pid>-<step>`:
//!
//! ```sh
//! HARMONT_LOCAL_E2E=1 cargo test --test local_e2e -- --test-threads=1
//! ```
//!
//! The harness auto-discovers the Python `cidsl` package by walking up
//! from `CARGO_MANIFEST_DIR` (= `cli/`). No env-var setup required from
//! the operator beyond `HARMONT_LOCAL_E2E=1`.

use std::path::PathBuf;
use std::process::Command;

/// Returns true when the test should no-op. Either the gate env var is
/// missing or the Docker daemon is unreachable.
///
/// Note: we probe with `docker version --format '{{.Server.Version}}'`
/// rather than `docker ping` — the latter is not a real subcommand and
/// exits 0 (printing help) on a healthy CLI even when the daemon is
/// down, which would defeat the gate.
fn skip_if_no_docker() -> bool {
    if std::env::var_os("HARMONT_LOCAL_E2E").is_none() {
        return true;
    }
    let out = Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output();
    match out {
        Ok(o) => !o.status.success(),
        Err(_) => true,
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pipelines")
        .join(name)
}

/// Map a fixture filename (e.g. `failing_step.py`) to the slug its
/// `@hm.pipeline(...)` decorator registers (e.g. `failing-step`).
fn fixture_slug(fixture_name: &str) -> &'static str {
    match fixture_name {
        "scratch.py" => "scratch",
        "chain.py" => "chain",
        "cached.py" => "cached",
        "failing_step.py" => "failing-step",
        "fork.py" => "fork",
        "mid_chain_cache.py" => "mid-chain-cache",
        other => panic!("unknown fixture {other}"),
    }
}

fn run_local(fixture_name: &str) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_hm");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR (cli/) must have a parent (repo root)");

    // Stage the fixture under `<tmp>/.hm/`. The new harness walks
    // every `.hm/*.py`; filename does not have to match the slug.
    let tmp = tempfile::tempdir().expect("mktempdir");
    let harmont_dir = tmp.path().join(".hm");
    std::fs::create_dir(&harmont_dir).expect("mkdir .hm");
    std::fs::copy(fixture(fixture_name), harmont_dir.join(fixture_name))
        .expect("copy fixture into .hm/");

    Command::new(bin)
        .args(["run", "--dir"])
        .arg(tmp.path())
        .arg(fixture_slug(fixture_name))
        .env("HARMONT_CIDSL_PY", repo_root.join("cidsl/py"))
        .output()
        .expect("spawning harmont binary should not fail")
}

#[test]
fn scratch_one_step() {
    if skip_if_no_docker() {
        return;
    }
    let out = run_local("scratch.py");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("[a]") && stderr.contains("hello"),
        "stderr: {stderr}"
    );
}

#[test]
fn chain_inherits_state() {
    if skip_if_no_docker() {
        return;
    }
    let out = run_local("chain.py");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("b-saw-a"), "stderr: {stderr}");
    assert!(stderr.contains("c-also-saw-a"), "stderr: {stderr}");
}

#[test]
fn fork_children_share_parent_state() {
    if skip_if_no_docker() {
        return;
    }
    let out = run_local("fork.py");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("c1"), "stderr: {stderr}");
    assert!(stderr.contains("c2"), "stderr: {stderr}");
}

#[test]
fn mid_chain_cache_hit_reboots_container() {
    if skip_if_no_docker() {
        return;
    }
    // First run: cache miss; commits an image for `b` and runs `c`
    // inside the same container as `a`+`b`.
    let _ = run_local("mid_chain_cache.py");
    // Second run: `b` should hit. The executor must reboot the chain
    // container from b's cached snapshot before running `c`, so that
    // `c` sees /tmp/b (from the snapshot) — not just `a`'s state.
    let out = run_local("mid_chain_cache.py");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "second run failed: {stderr}");
    assert!(
        stderr.contains("[b] cache hit"),
        "expected [b] cache hit, got: {stderr}"
    );
    assert!(
        stderr.contains("c-saw-b"),
        "step c must see /tmp/b from b's cached snapshot, got: {stderr}"
    );
}

#[test]
fn ttl_cache_hits_on_second_run() {
    if skip_if_no_docker() {
        return;
    }
    // First run: cache miss; commits an image.
    let first = run_local("cached.py");
    let first_stderr = String::from_utf8_lossy(&first.stderr);
    assert!(first.status.success(), "first run failed: {first_stderr}");

    // Second run: cache should hit on step `t`.
    let second = run_local("cached.py");
    let second_stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        second.status.success(),
        "second run failed: {second_stderr}"
    );
    assert!(
        second_stderr.contains("[t] cache hit"),
        "expected cache hit on second run, got: {second_stderr}"
    );
}

#[test]
fn failing_step_stops_execution_and_cleans_up() {
    if skip_if_no_docker() {
        return;
    }

    // Snapshot existing harmont-local containers before run so we can
    // assert no new ones leaked. Other tests share `--test-threads=1`
    // so we don't race a sibling test's containers into the diff.
    let before = list_harmont_local_containers();

    let out = run_local("failing_step.py");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        !out.status.success(),
        "expected non-zero exit, got success: {stderr}"
    );
    assert!(stderr.contains("a-ran"), "step a should have run: {stderr}");
    assert!(
        stderr.contains("step 'b' failed"),
        "step b's failure should surface: {stderr}"
    );
    assert!(
        !stderr.contains("c-ran"),
        "step c must not have run: {stderr}"
    );

    // Wait briefly for cleanup to settle, then assert no leaked
    // containers.
    std::thread::sleep(std::time::Duration::from_secs(1));
    let after = list_harmont_local_containers();
    let leaked: Vec<_> = after.difference(&before).collect();
    assert!(leaked.is_empty(), "leaked containers: {leaked:?}");
}

fn list_harmont_local_containers() -> std::collections::HashSet<String> {
    let out = std::process::Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            "name=harmont-local",
            "--format",
            "{{.ID}}",
        ])
        .output()
        .expect("docker ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[test]
fn resolves_pipeline_via_slug() {
    if skip_if_no_docker() {
        return;
    }

    // The new harness walks every `.hm/*.py` and resolves by the
    // decorator-registered slug — the filename can be anything. Copy
    // `scratch.py` (slug `scratch`) under a deliberately different
    // filename to exercise that.
    let tmp = tempfile::tempdir().expect("mktempdir");
    let harmont_dir = tmp.path().join(".hm");
    std::fs::create_dir(&harmont_dir).unwrap();
    std::fs::copy(fixture("scratch.py"), harmont_dir.join("renamed.py")).unwrap();

    let bin = env!("CARGO_BIN_EXE_hm");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap();
    let out = std::process::Command::new(bin)
        .args(["run", "--dir"])
        .arg(tmp.path())
        .arg("scratch")
        .env("HARMONT_CIDSL_PY", repo_root.join("cidsl/py"))
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(
        stderr.contains("hello"),
        "expected scratch fixture output: {stderr}"
    );
}
