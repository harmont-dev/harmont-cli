//! `hm cloud org switch <slug>` — pick the active organization.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::api::types::OrganizationList;
use crate::cli::OrgCommand;
use crate::config::Config;
use crate::creds;
use crate::http::Client;
use crate::state::CloudState;

pub(crate) async fn run(env: &BTreeMap<String, String>, cmd: OrgCommand) -> Result<()> {
    let cfg = Config::from_env(env);
    let token = creds::load_token(&cfg.api_base, env)
        .ok_or_else(|| anyhow::anyhow!("not logged in; run `hm cloud login`"))?;
    let client = Client::new(&cfg, Some(token));

    match cmd {
        OrgCommand::Switch { slug } => switch(&client, &slug).await,
    }
}

async fn switch(client: &Client, slug: &str) -> Result<()> {
    let orgs: OrganizationList = client.get("/organizations").await?;
    let found = orgs
        .data
        .iter()
        .find(|o| o.slug == slug)
        .ok_or_else(|| anyhow::anyhow!("no organization with slug '{slug}'"))?;
    let mut state = CloudState::load();
    state.active_org = Some(found.slug.clone());
    state.save();
    tracing::info!("active organization: {} ({})", found.name, found.slug);
    Ok(())
}
