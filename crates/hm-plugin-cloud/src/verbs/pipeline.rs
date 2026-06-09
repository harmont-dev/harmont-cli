//! `hm cloud pipeline list|show`.

use std::collections::BTreeMap;

use anyhow::Result;
use harmont_cloud::HarmontClient;

use crate::cli::PipelineCommand;
use crate::settings;

pub(crate) async fn run(_env: &BTreeMap<String, String>, cmd: PipelineCommand) -> Result<()> {
    let (client, ctx) = settings::client()?;
    let org = ctx.org()?;

    match cmd {
        PipelineCommand::List => list(&client, &org).await,
        PipelineCommand::Show { slug } => show(&client, &org, &slug).await,
    }
}

async fn list(client: &HarmontClient, org: &str) -> Result<()> {
    let pipes = client
        .raw()
        .list_pipelines(org, None, None)
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    for p in &pipes.data {
        tracing::info!("{:<24} {}", p.slug, p.name);
    }
    Ok(())
}

async fn show(client: &HarmontClient, org: &str, slug: &str) -> Result<()> {
    let p = client
        .raw()
        .get_pipeline(org, slug)
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "slug": p.slug,
        "name": p.name,
        "default_branch": p.default_branch,
        "repository": p.repository,
        "visibility": p.visibility.to_string(),
        "description": p.description,
    }))
    .unwrap_or_default();
    tracing::info!("{json}");
    Ok(())
}
