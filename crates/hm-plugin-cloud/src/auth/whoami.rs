//! `hm cloud whoami` — print the user the stored token belongs to.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::api::types::User;
use crate::config::Config;
use crate::creds;
use crate::http::Client;

pub(crate) async fn run(env: &BTreeMap<String, String>) -> Result<()> {
    let cfg = Config::from_env(env);
    let token = creds::load_token(&cfg.api_base, env).ok_or_else(|| {
        anyhow::anyhow!("not logged in to {}\n  fix: `hm cloud login`", cfg.api_base)
    })?;
    let client = Client::new(&cfg, Some(token));
    let me: User = client.get("/auth/me").await?;
    tracing::info!(
        "{} <{}> (id {})",
        me.display_name.clone().unwrap_or_else(|| me.email.clone()),
        me.email,
        me.id,
    );
    Ok(())
}
