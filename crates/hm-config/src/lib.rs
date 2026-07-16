//! Layered (project/user/env) configuration for the `hm` CLI. Shared between
//! the `hm` binary and `hm-plugin-cloud` so both sides resolve config through
//! one source of truth.
//!
//! Credentials live in `hm-core` (`hm_core::Sys::creds`), not here.

use std::path::{Path, PathBuf};

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

pub const DEFAULT_API_URL: &str = "https://api.harmont.dev";

/// Errors produced while loading configuration.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// Figment failed to merge/extract a config layer (malformed TOML, type mismatch, …).
    #[error(transparent)]
    Figment(#[from] figment::Error),
}

/// Errors produced while persisting configuration.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SaveError {
    /// Platform config directory (`~/.config/hm` or equivalent) could not be resolved.
    #[error("could not determine config directory")]
    ConfigDirUnavailable,

    /// TOML serialization of a [`Config`] failed.
    #[error("serializing config")]
    Serialize(#[from] toml::ser::Error),

    /// Atomic write of a config file failed.
    #[error("writing {}", .path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Execution backend for `hm run`.
///
/// Closed set parsed at the config boundary so invalid values are rejected at
/// deserialize time instead of mis-dispatching later, and every consumer match
/// is exhaustively checked by the compiler.
///
/// The `#[display(...)]` strings are the stable lowercase wire/CLI names and
/// must match the `#[serde(rename_all = "lowercase")]` representation.
#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    #[default]
    #[display("docker")]
    Docker,
    #[display("cloud")]
    Cloud,
}

/// Derive the SPA (dashboard) base URL from the API base.
///
/// The CLI talks to `api.harmont.dev`, but a human clicks through to the
/// dashboard at `app.harmont.dev`. A watch/login link built from the API host
/// lands on raw JSON, so every surface that emits a user-clickable URL must map
/// the host first.
///
/// Priority:
/// 1. `override_url` (e.g. the `HM_APP_URL` env override) when non-empty,
/// 2. heuristic mapping of `api.` → `app.` on the API host,
/// 3. the API base itself (last-resort dev fallback for hosts like
///    `localhost` that have no `api.`/`app.` split).
///
/// The returned URL never has a trailing slash.
#[must_use]
pub fn app_url(api: &str, override_url: Option<&str>) -> String {
    if let Some(u) = override_url.map(str::trim).filter(|u| !u.is_empty()) {
        return u.trim_end_matches('/').to_string();
    }
    let api = api.trim_end_matches('/');
    if let Some(rest) = api.strip_prefix("https://api.") {
        return format!("https://app.{rest}");
    }
    if let Some(rest) = api.strip_prefix("http://api.") {
        return format!("http://app.{rest}");
    }
    api.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
#[non_exhaustive]
pub struct CloudConfig {
    pub org: Option<String>,
    #[default(DEFAULT_API_URL.to_owned())]
    pub api_url: String,
    /// Org-global pipeline slug to submit builds to directly (set by `hm run`
    /// after registering a remoteless directory). When present, cloud runs
    /// submit by this slug instead of resolving by git-repo identity.
    pub pipeline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SmartDefault)]
#[non_exhaustive]
pub struct Preferences {
    #[default("human".to_owned())]
    pub format: String,
    pub auto_watch: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Config {
    #[serde(default)]
    pub backend: Backend,
    #[serde(default)]
    pub cloud: CloudConfig,
    #[serde(default)]
    pub preferences: Preferences,
}

impl Config {
    /// XDG-aware user config path (`~/.config/hm/config.toml`).
    ///
    /// Returns `None` if the platform config directory cannot be determined.
    #[must_use]
    pub fn user_config_path() -> Option<PathBuf> {
        hm_util::dirs::hm_config_dir().map(|d| d.join("config.toml"))
    }

    /// Project-level config path: `<root>/.hm/config.toml`.
    #[must_use]
    pub fn project_config_path(project_root: &Path) -> PathBuf {
        project_root.join(".hm").join("config.toml")
    }

    /// Load configuration with full layering: defaults -> user file -> project file -> env.
    ///
    /// `project_config_path` is the path to the project-level config file
    /// (typically `.hm/config.toml`), not the project root. Pass `None` to
    /// skip the project layer (user + env only). When the user config path
    /// cannot be resolved, the user layer is skipped as well.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] if figment extraction fails (malformed TOML, type
    /// mismatches).
    pub fn load(project_config_path: Option<&Path>) -> Result<Self, LoadError> {
        Self::load_from_paths(Self::user_config_path().as_deref(), project_config_path)
    }

    /// Testable core: build a `Config` from explicit file paths.
    ///
    /// Layering, lowest to highest precedence: defaults -> user file ->
    /// project file -> env.
    ///
    /// Env precedence (highest): both the `HM_`-prefixed split form
    /// (`HM_CLOUD__ORG`, `HM_CLOUD__API_URL`) and the documented
    /// `HM_ORG` / `HM_API_URL` are honored; the latter map onto
    /// `cloud.org` / `cloud.api_url`.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] if figment extraction fails (malformed TOML, type
    /// mismatches).
    pub fn load_from_paths(
        user_path: Option<&Path>,
        project_path: Option<&Path>,
    ) -> Result<Self, LoadError> {
        let mut figment = Figment::new().merge(Serialized::defaults(Self::default()));

        if let Some(p) = user_path {
            figment = figment.merge(Toml::file(p));
        }
        if let Some(p) = project_path {
            figment = figment.merge(Toml::file(p));
        }

        figment = figment
            .merge(Env::prefixed("HM_").split("__"))
            .merge(hm_alias_env());

        Ok(figment.extract()?)
    }

    /// Persist config to `path` atomically.
    ///
    /// # Errors
    ///
    /// Returns [`SaveError`] if TOML serialization fails or the atomic write
    /// fails.
    pub fn save_to(&self, path: &Path) -> Result<(), SaveError> {
        let serialized = toml::to_string_pretty(self)?;
        hm_util::os::fs::blocking::write_atomic_restricted(
            path,
            serialized.as_bytes(),
            hm_util::os::fs::FileMode(0o644),
            hm_util::os::fs::DirMode(0o700),
        )
        .map_err(|source| SaveError::Write {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Save to user-level config path (`~/.config/hm/config.toml`).
    ///
    /// # Errors
    ///
    /// Returns [`SaveError::ConfigDirUnavailable`] if the path cannot be
    /// determined, or any other [`SaveError`] if the write fails.
    pub fn save_user(&self) -> Result<(), SaveError> {
        let path = Self::user_config_path().ok_or(SaveError::ConfigDirUnavailable)?;
        self.save_to(&path)
    }
}

/// Figment env provider mapping the friendly `HM_ORG` / `HM_API_URL`
/// variables onto the nested `cloud` config keys.
///
/// The cloud settings docs and `hm`'s error messages tell users to
/// `set HM_ORG=<slug>` / `HM_API_URL=<url>`, so those flat names must feed
/// the config. This binds them to `cloud.org` / `cloud.api_url` alongside the
/// generic `HM_`-prefixed split layer (`HM_CLOUD__ORG`, …).
fn hm_alias_env() -> Env {
    Env::raw()
        .only(&["HM_ORG", "HM_API_URL"])
        .map(|key| match key.as_str() {
            "HM_ORG" => "cloud.org".into(),
            "HM_API_URL" => "cloud.api_url".into(),
            other => other.into(),
        })
        .split(".")
}
