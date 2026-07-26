//! `hm cloud logout` — clears the stored bearer token.

use std::collections::BTreeMap;

use anyhow::Result;
use hm_core::app_context::AppContext;

use crate::settings;

pub(crate) async fn run(_env: &BTreeMap<String, String>, app: &AppContext) -> Result<()> {
    let (_client, domain) = settings::anon_client(app);
    let api = domain.api_url();
    hm_core::config::creds::forget_cloud_token(&api).await;
    tracing::info!("logged out of {api}");
    Ok(())
}
