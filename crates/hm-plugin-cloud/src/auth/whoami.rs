//! `hm cloud whoami` — print the user the stored token belongs to.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::settings;

pub(crate) async fn run(_env: &BTreeMap<String, String>) -> Result<()> {
    let (client, _ctx) = settings::client()?;
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
        me.uuid,
    );
    Ok(())
}
