//! `hm cloud org switch <slug>` — pick the active organization.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use hm_core::app_ctx::AppCtx;
use hm_core::config::domain::BackendConfig;
use hm_core::config::user::UserCloudConfig;

use crate::commands::cloud::cli::OrgCommand;
use crate::commands::cloud::settings;

pub(crate) async fn run(
    _env: &BTreeMap<String, String>,
    cmd: OrgCommand,
    app: &AppCtx,
) -> Result<()> {
    let (client, _ctx) = settings::client(app).await?;

    match cmd {
        OrgCommand::Switch { slug } => switch(&client, &slug, app).await,
    }
}

async fn switch(client: &harmont_cloud::HarmontClient, slug: &str, app: &AppCtx) -> Result<()> {
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

    // Set the org on the user config's cloud backend, preserving the domain.
    let mut user = app.user_config().cloned().unwrap_or_default();
    let cloud = match user.backend {
        Some(BackendConfig::Cloud(cloud)) => cloud,
        _ => UserCloudConfig::default(),
    };
    user.backend = Some(BackendConfig::Cloud(UserCloudConfig {
        org: Some(found.slug.clone()),
        ..cloud
    }));
    user.save(&app.user_config_path())
        .await
        .context("saving config")?;

    tracing::info!("active organization: {} ({})", found.name, found.slug);
    Ok(())
}
