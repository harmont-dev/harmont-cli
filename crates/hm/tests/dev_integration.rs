//! Docker-gated integration tests.
//!
//! Run with: `cargo test -p harmont-cli --features docker-integration -- --ignored`
//! Requires:
//!   * A reachable Docker daemon
//!   * harmont-py installed in the env at `HARMONT_PYTHON` (defaults to python3)
//!     with the `feat/hm-dev-deploy` branch checked out (or merged to main)
//!
//! Each test creates its own .harmont/ in a tmpdir to avoid step-on
//! between concurrent runs.

#![cfg(feature = "docker-integration")]
// Integration tests intentionally use unwrap/expect/panic to fail loudly on
// docker-state mismatches; that's the correct behaviour for test code.
#![allow(
    clippy::unwrap_used,
    reason = "integration test helpers panic on docker-state mismatch"
)]
#![allow(
    clippy::expect_used,
    reason = "integration test helpers panic on docker-state mismatch"
)]
#![allow(
    clippy::panic,
    reason = "poll_http panics after timeout — correct for test code"
)]
#![allow(
    clippy::cast_possible_wrap,
    reason = "pid fits in i32 on all platforms we target"
)]
#![allow(
    clippy::ignore_without_reason,
    reason = "reason is in the test name and doc comment above"
)]

use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

fn write_deploys_py(dir: &std::path::Path, body: &str) {
    let h = dir.join(".harmont");
    std::fs::create_dir_all(&h).unwrap();
    std::fs::write(h.join("deploys.py"), body).unwrap();
}

fn hm_bin() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop(); // /target/debug/deps -> /target/debug
    p.pop();
    p.push("hm");
    p
}

#[test]
#[ignore]
fn up_serves_http_and_tears_down() {
    let tmp = tempfile::tempdir().unwrap();
    write_deploys_py(
        tmp.path(),
        r#"
import harmont as hm

@hm.deploy("hello")
def hello():
    return hm.dev.deploy(
        image="python:3.12-alpine",
        cmd=["python", "-m", "http.server", "5678"],
        port_mapping={5678: hm.dev.port()},
    )
"#,
    );

    let mut up = Command::new(hm_bin())
        .args(["dev", "up"])
        .current_dir(tmp.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn hm dev up");

    let stderr = up.stderr.as_mut().unwrap();
    let mut buf = String::new();
    let mut chunk = [0u8; 1024];
    let started = std::time::Instant::now();
    while started.elapsed().as_secs() < 60 {
        let n = stderr.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
        if buf.contains("all up.") {
            break;
        }
    }
    assert!(
        buf.contains("all up."),
        "up did not become ready; stderr:\n{buf}"
    );

    let port_of = Command::new(hm_bin())
        .args(["dev", "port-of", "hello", "5678"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(
        port_of.status.success(),
        "port-of failed: {}",
        String::from_utf8_lossy(&port_of.stderr)
    );
    let host_port: u16 = String::from_utf8(port_of.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        host_port > 1024,
        "expected ephemeral host port, got {host_port}"
    );

    // python -m http.server returns an HTML directory listing whose
    // body always contains the literal "Directory listing for /".
    let body = poll_http(&format!("http://127.0.0.1:{host_port}"));
    assert!(
        body.contains("Directory listing"),
        "expected python http.server directory listing; got {body:?}",
    );

    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(up.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    );
    let _ = up.wait();

    let port_of_after = Command::new(hm_bin())
        .args(["dev", "port-of", "hello", "5678"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(
        port_of_after.status.code(),
        Some(4),
        "stopped slug should exit 4: {}",
        String::from_utf8_lossy(&port_of_after.stderr)
    );
}

fn poll_http(url: &str) -> String {
    let started = std::time::Instant::now();
    let mut last_err = String::new();
    while started.elapsed().as_secs() < 15 {
        match ureq::get(url).call() {
            Ok(resp) => {
                if resp.status() == 200 {
                    return resp.into_string().unwrap_or_default();
                }
                last_err = format!("status {}", resp.status());
            }
            Err(e) => last_err = e.to_string(),
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    panic!("HTTP poll failed against {url}: {last_err}");
}
