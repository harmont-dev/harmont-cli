//! `hm cloud logout` — clears the stored bearer token.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::settings;

#[allow(
    clippy::unused_async,
    reason = "async to match the uniform await arm in cli::dispatch_command"
)]
pub(crate) async fn run(_env: &BTreeMap<String, String>) -> Result<()> {
    let (_client, api) = settings::anon_client()?;
    hm_config::creds::forget_cloud_token(&api);
    tracing::info!("logged out of {api}");
    Ok(())
}
