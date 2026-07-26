//! `hm cloud whoami` — print the user the stored token belongs to.

use std::collections::BTreeMap;

use anyhow::Result;
use hm_core::app_ctx::AppCtx;

use crate::commands::cloud::settings;

pub(crate) async fn run(_env: &BTreeMap<String, String>, app: &AppCtx) -> Result<()> {
    let (client, _ctx) = settings::client(app).await?;
    let me = client
        .raw()
        .get_current_user()
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    tracing::info!(
        "{} <{}> (id {})",
        me.name.clone().unwrap_or_else(|| me.email.clone()),
        me.email,
        me.id,
    );
    Ok(())
}
