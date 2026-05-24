//! `hm cloud logout` — clears the stored bearer token.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::config::Config;
use crate::creds;

pub(crate) async fn run(env: &BTreeMap<String, String>) -> Result<()> {
    let cfg = Config::from_env(env);
    creds::clear_token(&cfg.api_base);
    tracing::info!("logged out of {}", cfg.api_base);
    Ok(())
}
