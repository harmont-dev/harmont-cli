//! `hm cloud {login,logout,whoami,org,pipeline,build,job,billing,run}`.

pub mod cli;
pub mod settings;

mod auth;
mod verbs;

/// Run the interactive browser-loopback login flow.
///
/// Designed for embedding in host commands (e.g. `hm init`) that need
/// the user to authenticate before proceeding.
///
/// # Errors
///
/// Returns an error if the browser cannot be opened, the login times
/// out, or the token cannot be persisted.
pub async fn login_interactive(app: &hm_core::app_ctx::AppCtx) -> anyhow::Result<()> {
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    auth::login::run(&env, false, app).await
}
