//! `hm cloud org switch <slug>` — pick the active organization.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::cli::OrgCommand;
use crate::settings;

pub(crate) async fn run(_env: &BTreeMap<String, String>, cmd: OrgCommand) -> Result<()> {
    let (client, _ctx) = settings::client()?;

    match cmd {
        OrgCommand::Switch { slug } => switch(&client, &slug).await,
    }
}

async fn switch(client: &harmont_cloud::HarmontClient, slug: &str) -> Result<()> {
    let orgs = client
        .raw()
        .list_organizations(None, None)
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    let found = orgs
        .data
        .iter()
        .find(|o| o.slug == slug)
        .ok_or_else(|| anyhow::anyhow!("no organization with slug '{slug}'"))?;
    settings::set_default_org(&found.slug)?;
    tracing::info!("active organization: {} ({})", found.name, found.slug);
    Ok(())
}
