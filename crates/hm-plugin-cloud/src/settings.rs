//! Cloud client builders for the `hm cloud` verbs.
//!
//! Tokens come from [`hm_core::creds`] (`HM_API_TOKEN` wins); the domain and org
//! come from the user config.

use anyhow::{Context, Result};
use harmont_cloud::HarmontClient;
use hm_core::app_ctx::AppCtx;
use hm_core::config::domain::{BackendConfig, BackendDomain};
use hm_core::config::user::UserCloudConfig;
use secrecy::ExposeSecret as _;

/// Resolved cloud context for the `hm cloud` verbs.
#[derive(Debug, Clone)]
pub struct ResolvedCtx {
    /// Effective API base URL.
    pub api: String,
    /// Base Harmont domain the API/dashboard hosts derive from.
    pub domain: BackendDomain,
    /// Configured organization slug, if set.
    pub org: Option<String>,
}

impl ResolvedCtx {
    /// The configured org, or a clear error telling the user how to set it.
    ///
    /// # Errors
    ///
    /// Returns an error if no organization is configured.
    pub fn org(&self) -> Result<String> {
        self.org.clone().context(
            "no organization — run `hm cloud org switch <slug>`, or set one in ~/.hm/config.toml")
    }
}

/// The user's cloud settings, when the user config selects a cloud backend.
fn user_cloud(app: &AppCtx) -> Option<&UserCloudConfig> {
    match app.user_config().and_then(|u| u.backend.as_ref()) {
        Some(BackendConfig::Cloud(cloud)) => Some(cloud),
        _ => None,
    }
}

/// The cloud domain from the user config, or the default.
#[must_use]
pub fn domain(app: &AppCtx) -> BackendDomain {
    user_cloud(app)
        .and_then(|c| c.domain.clone())
        .unwrap_or_default()
}

/// An authenticated cloud client built from the user config + stored token.
///
/// Fails fast with a clear message when no token is present.
///
/// # Errors
///
/// Returns an error if no token is available.
pub async fn client(app: &AppCtx) -> Result<(HarmontClient, ResolvedCtx)> {
    let domain = domain(app);
    let api = domain.api_url();
    let token = app
        .creds()
        .get()
        .await
        .context("not logged in — run `hm cloud login` or set HM_API_TOKEN")?;
    let client = HarmontClient::with_base_url(token.expose_secret().to_owned(), &api);
    let org = user_cloud(app).and_then(|c| c.org.clone());
    Ok((client, ResolvedCtx { api, domain, org }))
}

/// An anonymous client (for the login flow) + the resolved cloud domain.
#[must_use]
pub fn anon_client(app: &AppCtx) -> (HarmontClient, BackendDomain) {
    let domain = domain(app);
    (HarmontClient::anonymous(domain.api_url()), domain)
}

/// Render preferences for cloud commands that stream through `hm-render`.
///
/// Both fields are derived from `hm-render`'s shared TTY/color helpers (the
/// single source of truth, also used by `hm/src/context.rs`).
#[derive(Debug, Clone, Copy)]
pub struct RenderPrefs {
    /// ANSI enabled when `NO_COLOR` is unset and stderr is a TTY.
    pub color: bool,
    /// Force the streaming `HumanRenderer` over the live `ProgressRenderer`.
    ///
    /// True when stdout is **not** an interactive terminal (CI / pipe / log
    /// file), so nothing animates into a non-TTY sink.
    pub logs: bool,
}

impl RenderPrefs {
    /// Detect render preferences from the current `NO_COLOR` and TTY state.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            color: hm_render::color_enabled(false),
            logs: hm_render::stdout_piped(),
        }
    }
}

/// Map a raw generated-client error into a readable `anyhow` error.
///
/// The server's error body (status, headers, decoded value) is rendered
/// via the raw `Error<E>`'s `Display` impl.
#[allow(
    clippy::needless_pass_by_value,
    reason = "by-value signature lets this be used directly as `.map_err(map_raw)`"
)]
pub fn map_raw<E: std::fmt::Debug>(e: harmont_cloud_raw::Error<E>) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}
