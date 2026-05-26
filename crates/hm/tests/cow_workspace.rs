#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! End-to-end test for COW workspace mode.
//!
//! Verifies that `hm run --cow` correctly propagates workspace state
//! across a three-step chain: step `a` writes a file, step `b` reads
//! it and writes another, step `c` reads both — proving COW workspace
//! inheritance works through the entire chain.
//!
//! Skipped unless `HARMONT_LOCAL_E2E=1` is set AND a Docker daemon is
//! reachable.
//!
//! ```sh
//! HARMONT_LOCAL_E2E=1 cargo test --test cow_workspace -- --test-threads=1
//! ```

use std::path::PathBuf;
use std::process::Command;

/// Returns true when the test should no-op.
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

#[test]
fn cow_chain_inherits_workspace() {
    if skip_if_no_docker() {
        return;
    }

    let bin = env!("CARGO_BIN_EXE_hm");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR must have a parent (repo root)");

    let tmp = tempfile::tempdir().expect("mktempdir");
    let harmont_dir = tmp.path().join(".harmont");
    std::fs::create_dir(&harmont_dir).expect("mkdir .harmont");
    std::fs::copy(fixture("cow_chain.py"), harmont_dir.join("cow_chain.py"))
        .expect("copy fixture into .harmont/");

    let out = Command::new(bin)
        .args(["run", "--cow", "--logs", "--dir"])
        .arg(tmp.path())
        .arg("cow-chain")
        .env("HARMONT_CIDSL_PY", repo_root.join("cidsl/py"))
        .output()
        .expect("spawning harmont binary should not fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "hm run --cow failed.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("c-saw-both"),
        "step c must see files from a and b via COW workspace.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
