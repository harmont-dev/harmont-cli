//! `hm cloud logout` — clears the stored bearer token.

use std::collections::BTreeMap;

use anyhow::{Context, Result};

use crate::settings;

pub(crate) async fn run(_env: &BTreeMap<String, String>) -> Result<()> {
    let (_client, api) = settings::anon_client()?;
    hm_core::Sys::load()
        .context("loading credentials")?
        .creds_mut()
        .clear();
    tracing::info!("logged out of {api}");
    Ok(())
}
