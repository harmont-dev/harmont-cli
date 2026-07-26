//! `hm cloud logout` — clears the stored bearer token.

use std::collections::BTreeMap;

use anyhow::Result;
use hm_core::app_ctx::AppCtx;

pub(crate) async fn run(_env: &BTreeMap<String, String>, app: &AppCtx) -> Result<()> {
    app.creds().clear().await;
    tracing::info!("logged out");
    Ok(())
}
