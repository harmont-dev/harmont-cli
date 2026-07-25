#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration test setup and assertions"
)]

use rstest::rstest;
use std::fs;
use tempfile::tempdir;

#[rstest]
fn project_overrides_user() {
    let user_dir = tempdir().unwrap();
    let user_path = user_dir.path().join("config.toml");
    fs::write(
        &user_path,
        b"[cloud]\norg = \"user-org\"\napi_url = \"https://user.api\"\n\n[preferences]\nformat = \"json\"\n",
    )
    .unwrap();

    let project_dir = tempdir().unwrap();
    let project_path = project_dir.path().join("config.toml");
    fs::write(&project_path, b"[cloud]\norg = \"project-org\"\n").unwrap();

    let config =
        harmont_cli::config::Config::load_from_paths(Some(&user_path), Some(&project_path))
            .unwrap();

    assert_eq!(config.cloud.org.as_deref(), Some("project-org"));
    assert_eq!(config.cloud.api_url, "https://user.api");
    assert_eq!(config.preferences.format, "json");
}

#[rstest]
fn missing_files_resolve_to_defaults() {
    let config = harmont_cli::config::Config::load_from_paths(None, None).unwrap();
    assert_eq!(config.cloud.api_url, harmont_cli::config::DEFAULT_API_URL);
    assert_eq!(config.preferences.format, "human");
    assert!(!config.preferences.auto_watch);
    assert!(config.cloud.org.is_none());
}

/// A single config file resolves its `cloud.org` while leaving `api_url` at the
/// default. Covers project-only loads, user-only loads, and files carrying
/// unknown keys/sections (figment ignores those by default).
#[rstest]
#[case::project_only("[cloud]\norg = \"proj\"\n", "proj")]
#[case::file_values("[cloud]\norg = \"file-org\"\n", "file-org")]
#[case::unknown_keys_ignored(
    "[cloud]\norg = \"ok\"\nunknown_key = 42\n\n[unknown_section]\nfoo = true\n",
    "ok"
)]
fn single_file_resolves_org(#[case] toml_body: &str, #[case] expected_org: &str) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, toml_body).unwrap();

    let config = harmont_cli::config::Config::load_from_paths(Some(&path), None).unwrap();

    assert_eq!(config.cloud.org.as_deref(), Some(expected_org));
    assert_eq!(config.cloud.api_url, harmont_cli::config::DEFAULT_API_URL);
}

#[rstest]
#[case::malformed("this is not [valid toml\n")]
#[case::type_mismatch("[preferences]\nauto_watch = \"not-a-bool\"\n")]
fn invalid_toml_returns_error(#[case] toml_body: &str) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, toml_body).unwrap();

    let result = harmont_cli::config::Config::load_from_paths(Some(&path), None);
    assert!(result.is_err());
}

#[rstest]
fn load_resolves_project_root() {
    let project_dir = tempdir().unwrap();
    let harmont_dir = project_dir.path().join(".hm");
    fs::create_dir_all(&harmont_dir).unwrap();
    fs::write(
        harmont_dir.join("config.toml"),
        b"[cloud]\norg = \"proj-root\"\n",
    )
    .unwrap();

    let found = hm_util::dirs::find_project_root(project_dir.path());
    assert_eq!(found, Some(project_dir.path().to_path_buf()));

    let config_path = harmont_cli::config::Config::project_config_path(project_dir.path());
    assert_eq!(config_path, harmont_dir.join("config.toml"));
}
