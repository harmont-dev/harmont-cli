//! Cloud client library for the hm CLI.
//!
//! Implements `hm cloud {login,logout,whoami,org,pipeline,build,job,billing,run}`.

#![allow(
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo,
    clippy::multiple_crate_versions,
    clippy::cargo_common_metadata,
    clippy::missing_errors_doc,
    reason = "quick migration from plugin crate; polish later"
)]

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
pub async fn login_interactive() -> anyhow::Result<()> {
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    auth::login::run(&env, false).await
}
