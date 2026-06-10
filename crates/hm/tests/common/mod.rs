#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

/// Construct a Command pointing at the freshly-built `hm` binary.
///
/// # Panics
///
/// Panics if the `hm` binary has not been built — `assert_cmd::cargo`
/// resolves the path lazily and only fails when the binary is genuinely
/// missing from `target/`.
#[must_use]
pub fn hm_bin() -> assert_cmd::Command {
    assert_cmd::Command::cargo_bin("hm").expect("binary 'hm' not found")
}

/// Build an `hm` command wired against a wiremock `MockServer`.
///
/// The harness sets:
/// - `HM_API_URL`  → the mock server's URI (random localhost port)
/// - `HM_API_TOKEN` → a fake bearer (`test-token`) so `require_auth`
///   passes without reading from the file credential store; the mock
///   accepts any value
/// - `HM_ORG` → the supplied slug, so subcommands that need an org
///   resolve it without reading a config file
///
/// The returned `Command` is ready to `.assert()`. Callers can chain
/// extra `.env(...)` for case-specific overrides (e.g. `NO_COLOR`).
#[must_use]
pub fn hm_command(server: &wiremock::MockServer, org: &str, args: &[&str]) -> assert_cmd::Command {
    let mut cmd = hm_bin();
    cmd.env("HM_API_URL", server.uri())
        .env("HM_API_TOKEN", "test-token")
        .env("HM_ORG", org)
        .args(args);
    cmd
}
