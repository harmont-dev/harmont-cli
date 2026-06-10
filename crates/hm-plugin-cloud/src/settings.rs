//! Cloud client builders for the `hm cloud` verbs.
//!
//! Config and credentials are owned by the shared [`hm_config`] crate:
//!
//! - layered config (user `~/.config/hm/config.toml` + project
//!   `.hm/config.toml` + `HARMONT_*` env) supplies the API base
//!   (`cloud.api_url`) and the active org (`cloud.org`);
//! - bearer tokens live in `hm_config::creds`, keyed by API base, with
//!   `HARMONT_API_TOKEN` taking precedence.
//!
//! This module only assembles an SDK client from that config; it does not own
//! any config or credential storage of its own.

use anyhow::{Context, Result};
use harmont_cloud::HarmontClient;

/// Resolved cloud context for the `hm cloud` verbs.
#[derive(Debug, Clone)]
pub struct ResolvedCtx {
    /// Effective API base URL.
    pub api: String,
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
            "no organization — set `[cloud] org = \"…\"` in ~/.config/hm/config.toml (or .hm/config.toml), or run `hm cloud org switch <slug>`")
    }
}

/// An authenticated cloud client built from the layered config + stored token.
///
/// Fails fast with a clear message when no token is present.
///
/// # Errors
///
/// Returns an error if config can't be loaded or no token is available.
pub fn client() -> Result<(HarmontClient, ResolvedCtx)> {
    let cfg = hm_config::Config::load(None).context("loading config")?; // user + env layering
    let api = cfg.cloud.api_url.clone();
    let token = hm_config::creds::cloud_token(&api)
        .context("not logged in — run `hm cloud login` or set HARMONT_API_TOKEN")?;
    let client = HarmontClient::with_base_url(token, &api);
    Ok((
        client,
        ResolvedCtx {
            api,
            org: cfg.cloud.org,
        },
    ))
}

/// An anonymous client (for the login flow) + the resolved API base.
///
/// # Errors
///
/// Returns an error if config can't be loaded.
pub fn anon_client() -> Result<(HarmontClient, String)> {
    let cfg = hm_config::Config::load(None).context("loading config")?;
    let api = cfg.cloud.api_url.clone();
    Ok((HarmontClient::anonymous(&api), api))
}

/// Render preferences for cloud commands that stream through `hm-render`.
///
/// Both fields are derived from `hm-render`'s shared TTY/color helpers (the
/// single source of truth, also used by `hm/src/context.rs`).
#[derive(Debug, Clone, Copy)]
pub struct RenderPrefs {
    /// ANSI enabled when `NO_COLOR` is unset and stderr is a TTY.
    ///
    /// The plugin verbs have no `--no-color` flag, so we pass `false` for the
    /// flag; the `--no-color` asymmetry vs. the host `hm run` path is explicit
    /// here at the call site.
    pub color: bool,
    /// Force the streaming `HumanRenderer` over the live `ProgressRenderer`.
    ///
    /// True when stdout is **not** an interactive terminal (CI / pipe / log
    /// file), so nothing animates into a non-TTY sink.
    pub logs: bool,
}

impl RenderPrefs {
    /// Detect render preferences from the live environment via `hm-render`'s
    /// shared TTY/color helpers — the single source of truth, also used by
    /// `hm/src/context.rs`. This inspects `NO_COLOR` and the stdout/stderr
    /// TTY state at call time, so it is a deliberate observation of the
    /// environment rather than a constant default.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            color: hm_render::color_enabled(false),
            logs: hm_render::stdout_piped(),
        }
    }
}

/// Map a raw generated-client error into an `anyhow` error with a readable
/// message. The raw `Error<E>` renders the server's error body (status,
/// headers, decoded value) via its `Display` impl, which holds for any
/// `E: Debug` — true of the generated `types::Error` body.
pub fn map_raw<E: std::fmt::Debug>(e: harmont_cloud_raw::Error<E>) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}
