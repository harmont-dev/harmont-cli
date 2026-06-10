//! Layered (project/user/env) configuration and credential storage for the
//! `hm` CLI. Shared between the `hm` binary and `hm-plugin-cloud` so both sides
//! resolve config and credentials through one source of truth.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};

pub mod creds;

pub const DEFAULT_API_URL: &str = "https://api.harmont.dev";

/// Execution backend for `hm run`. Closed set parsed at the config boundary so
/// invalid values are rejected at deserialize time instead of mis-dispatching
/// later, and every consumer match is exhaustively checked by the compiler.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Docker,
    Cloud,
}

impl Default for Backend {
    fn default() -> Self {
        Backend::Docker
    }
}

impl Backend {
    /// Stable lowercase wire/CLI name (matches the `serde` representation).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Docker => "docker",
            Backend::Cloud => "cloud",
        }
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudConfig {
    pub org: Option<String>,
    pub api_url: String,
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            org: None,
            api_url: DEFAULT_API_URL.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    pub format: String,
    pub auto_watch: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            format: "human".to_owned(),
            auto_watch: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub backend: Backend,
    #[serde(default)]
    pub cloud: CloudConfig,
    #[serde(default)]
    pub preferences: Preferences,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            cloud: CloudConfig::default(),
            preferences: Preferences::default(),
        }
    }
}

impl Config {
    /// XDG-aware user config path (`~/.config/hm/config.toml`).
    ///
    /// # Errors
    ///
    /// Returns an error if the platform config directory cannot be determined.
    pub fn user_config_path() -> Result<PathBuf> {
        let dir = hm_util::dirs::hm_config_dir().context("could not determine config directory")?;
        Ok(dir.join("config.toml"))
    }

    /// Project-level config path: `<root>/.hm/config.toml`.
    #[must_use]
    pub fn project_config_path(project_root: &Path) -> PathBuf {
        project_root.join(".hm").join("config.toml")
    }

    /// Load configuration with full layering: defaults -> user file -> project file -> env.
    ///
    /// # Errors
    ///
    /// Returns an error if the user config path cannot be determined or
    /// figment extraction fails (malformed TOML, type mismatches).
    pub fn load(project_root: Option<&Path>) -> Result<Self> {
        let user_path = Self::user_config_path()?;
        let project_path = project_root.map(Self::project_config_path);
        Self::load_from_paths(Some(&user_path), project_path.as_deref())
            .context("loading configuration")
    }

    /// Testable core: build a `Config` from explicit file paths.
    ///
    /// # Errors
    ///
    /// Returns an error if figment extraction fails (malformed TOML, type mismatches).
    pub fn load_from_paths(user_path: Option<&Path>, project_path: Option<&Path>) -> Result<Self> {
        let mut figment = Figment::new().merge(Serialized::defaults(Self::default()));

        if let Some(p) = user_path {
            figment = figment.merge(Toml::file(p));
        }
        if let Some(p) = project_path {
            figment = figment.merge(Toml::file(p));
        }

        figment = figment.merge(Env::prefixed("HM_").split("__"));

        Ok(figment.extract()?)
    }

    /// Persist config to `path` atomically.
    ///
    /// # Errors
    ///
    /// Returns an error if TOML serialization fails or the atomic write fails.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let serialized = toml::to_string_pretty(self).context("serializing config")?;
        hm_util::os::fs::blocking::write_atomic_restricted(
            path,
            serialized.as_bytes(),
            0o644,
            0o700,
        )
        .with_context(|| format!("writing {}", path.display()))
    }

    /// Save to user-level config path (`~/.config/hm/config.toml`).
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be determined or the write fails.
    pub fn save_user(&self) -> Result<()> {
        self.save_to(&Self::user_config_path()?)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn default_config_values() {
        let cfg = Config::default();
        assert_eq!(cfg.backend, Backend::Docker);
        assert_eq!(cfg.cloud.api_url, DEFAULT_API_URL);
        assert!(cfg.cloud.org.is_none());
        assert_eq!(cfg.preferences.format, "human");
        assert!(!cfg.preferences.auto_watch);
    }

    #[test]
    fn deserialize_full_toml() {
        let toml_str = r#"
[cloud]
org = "acme"
api_url = "https://custom.api"

[preferences]
format = "json"
auto_watch = true
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.cloud.org.as_deref(), Some("acme"));
        assert_eq!(cfg.cloud.api_url, "https://custom.api");
        assert_eq!(cfg.preferences.format, "json");
        assert!(cfg.preferences.auto_watch);
    }

    #[test]
    fn deserialize_sparse_toml() {
        let toml_str = r#"
[cloud]
org = "sparse-co"
"#;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(toml_str.as_bytes()).unwrap();

        let cfg = Config::load_from_paths(Some(f.path()), None).unwrap();
        assert_eq!(cfg.cloud.org.as_deref(), Some("sparse-co"));
        assert_eq!(cfg.cloud.api_url, DEFAULT_API_URL);
        assert_eq!(cfg.preferences.format, "human");
        assert!(!cfg.preferences.auto_watch);
    }

    #[test]
    fn deserialize_empty_toml() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"").unwrap();

        let cfg = Config::load_from_paths(Some(f.path()), None).unwrap();
        assert_eq!(cfg.cloud.api_url, DEFAULT_API_URL);
        assert!(cfg.cloud.org.is_none());
        assert_eq!(cfg.preferences.format, "human");
        assert!(!cfg.preferences.auto_watch);
    }

    #[test]
    fn figment_project_overrides_user() {
        let user_toml = r#"
[cloud]
org = "user-org"
api_url = "https://user.api"

[preferences]
format = "json"
"#;
        let project_toml = r#"
[cloud]
org = "project-org"
"#;

        let mut user_file = tempfile::NamedTempFile::new().unwrap();
        user_file.write_all(user_toml.as_bytes()).unwrap();

        let mut project_file = tempfile::NamedTempFile::new().unwrap();
        project_file.write_all(project_toml.as_bytes()).unwrap();

        let cfg =
            Config::load_from_paths(Some(user_file.path()), Some(project_file.path())).unwrap();

        assert_eq!(cfg.cloud.org.as_deref(), Some("project-org"));
        assert_eq!(cfg.cloud.api_url, "https://user.api");
        assert_eq!(cfg.preferences.format, "json");
    }

    #[test]
    fn backend_defaults_docker_and_parses_and_layers() {
        // default
        assert_eq!(Config::default().backend, Backend::Docker);

        // user file sets cloud; project file sets docker -> project wins.
        let mut user_file = tempfile::NamedTempFile::new().unwrap();
        user_file.write_all(br#"backend = "cloud""#).unwrap();

        let mut project_file = tempfile::NamedTempFile::new().unwrap();
        project_file.write_all(br#"backend = "docker""#).unwrap();

        let cfg =
            Config::load_from_paths(Some(user_file.path()), Some(project_file.path())).unwrap();
        assert_eq!(cfg.backend, Backend::Docker);

        // user file alone parses "cloud".
        let cfg_user = Config::load_from_paths(Some(user_file.path()), None).unwrap();
        assert_eq!(cfg_user.backend, Backend::Cloud);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn save_and_reload_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let cfg = Config {
            backend: Backend::default(),
            cloud: CloudConfig {
                org: Some("saved-org".into()),
                api_url: DEFAULT_API_URL.to_owned(),
            },
            preferences: Preferences::default(),
        };
        cfg.save_to(&path).unwrap();

        let loaded = Config::load_from_paths(Some(&path), None).unwrap();
        assert_eq!(loaded.cloud.org.as_deref(), Some("saved-org"));
        assert_eq!(loaded.cloud.api_url, DEFAULT_API_URL);
        assert_eq!(loaded.preferences.format, "human");
    }

    #[test]
    fn figment_missing_files_still_resolve() {
        let nonexistent_user = Path::new("/tmp/harmont-test-nonexistent-user/config.toml");
        let nonexistent_project = Path::new("/tmp/harmont-test-nonexistent-project/config.toml");

        let cfg =
            Config::load_from_paths(Some(nonexistent_user), Some(nonexistent_project)).unwrap();

        assert_eq!(cfg.cloud.api_url, DEFAULT_API_URL);
        assert!(cfg.cloud.org.is_none());
        assert_eq!(cfg.preferences.format, "human");
        assert!(!cfg.preferences.auto_watch);
    }
}
