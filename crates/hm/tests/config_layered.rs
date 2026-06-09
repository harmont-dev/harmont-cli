#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use tempfile::tempdir;

#[test]
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

    let config = harmont_cli::config::Config::load_from_paths(
        Some(&user_path),
        Some(&project_path),
    )
    .unwrap();

    assert_eq!(config.cloud.org.as_deref(), Some("project-org"));
    assert_eq!(config.cloud.api_url, "https://user.api");
    assert_eq!(config.preferences.format, "json");
}

#[test]
fn missing_files_resolve_to_defaults() {
    let config = harmont_cli::config::Config::load_from_paths(None, None).unwrap();
    assert_eq!(config.cloud.api_url, harmont_cli::config::DEFAULT_API_URL);
    assert_eq!(config.preferences.format, "human");
    assert!(!config.preferences.auto_watch);
    assert!(config.cloud.org.is_none());
}

#[test]
fn project_only_no_user() {
    let project_dir = tempdir().unwrap();
    let project_path = project_dir.path().join("config.toml");
    fs::write(&project_path, b"[cloud]\norg = \"proj\"\n").unwrap();

    let config = harmont_cli::config::Config::load_from_paths(
        None,
        Some(&project_path),
    )
    .unwrap();

    assert_eq!(config.cloud.org.as_deref(), Some("proj"));
    assert_eq!(config.cloud.api_url, harmont_cli::config::DEFAULT_API_URL);
}

#[test]
fn file_values_survive_without_env_override() {
    let user_dir = tempdir().unwrap();
    let user_path = user_dir.path().join("config.toml");
    fs::write(&user_path, b"[cloud]\norg = \"file-org\"\n").unwrap();

    let config = harmont_cli::config::Config::load_from_paths(
        Some(&user_path),
        None,
    )
    .unwrap();
    assert_eq!(config.cloud.org.as_deref(), Some("file-org"));
}

#[test]
fn unknown_keys_are_ignored() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, b"[cloud]\norg = \"ok\"\nunknown_key = 42\n\n[unknown_section]\nfoo = true\n").unwrap();

    // Figment with serde by default ignores unknown fields.
    let config = harmont_cli::config::Config::load_from_paths(
        Some(&path),
        None,
    )
    .unwrap();
    assert_eq!(config.cloud.org.as_deref(), Some("ok"));
}

#[test]
fn malformed_toml_returns_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, b"this is not [valid toml\n").unwrap();

    let result = harmont_cli::config::Config::load_from_paths(
        Some(&path),
        None,
    );
    assert!(result.is_err());
}

#[test]
fn type_mismatch_returns_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    // auto_watch should be bool, not string
    fs::write(&path, b"[preferences]\nauto_watch = \"not-a-bool\"\n").unwrap();

    let result = harmont_cli::config::Config::load_from_paths(
        Some(&path),
        None,
    );
    assert!(result.is_err());
}

#[test]
fn load_resolves_project_root() {
    let project_dir = tempdir().unwrap();
    let harmont_dir = project_dir.path().join(".hm");
    fs::create_dir_all(&harmont_dir).unwrap();
    fs::write(harmont_dir.join("config.toml"), b"[cloud]\norg = \"proj-root\"\n").unwrap();

    let found = hm_util::dirs::find_project_root(project_dir.path());
    assert_eq!(found, Some(project_dir.path().to_path_buf()));

    let config_path = harmont_cli::config::Config::project_config_path(project_dir.path());
    assert_eq!(config_path, harmont_dir.join("config.toml"));
}
