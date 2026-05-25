//! `hm cloud pipeline list|show`.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::api::types::{Pipeline, PipelineList};
use crate::cli::PipelineCommand;
use crate::config::Config;
use crate::creds;
use crate::http::Client;
use crate::state::CloudState;

pub(crate) async fn run(env: &BTreeMap<String, String>, cmd: PipelineCommand) -> Result<()> {
    let cfg = Config::from_env(env);
    let token = creds::load_token(&cfg.api_base, env)
        .ok_or_else(|| anyhow::anyhow!("not logged in; run `hm cloud login`"))?;
    let client = Client::new(&cfg, Some(token));
    let org = active_org()?;

    match cmd {
        PipelineCommand::List => list(&client, &org).await,
        PipelineCommand::Show { slug } => show(&client, &org, &slug).await,
    }
}

async fn list(client: &Client, org: &str) -> Result<()> {
    let pipes: PipelineList = client
        .get(&format!("/organizations/{org}/pipelines"))
        .await?;
    for p in &pipes.data {
        tracing::info!(
            "{:<24} {}",
            p.slug,
            p.label.as_deref().unwrap_or("(no label)")
        );
    }
    Ok(())
}

async fn show(client: &Client, org: &str, slug: &str) -> Result<()> {
    let p: Pipeline = client
        .get(&format!("/organizations/{org}/pipelines/{slug}"))
        .await?;
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "id": p.id,
        "slug": p.slug,
        "label": p.label,
        "default_branch": p.default_branch,
    }))
    .unwrap_or_default();
    tracing::info!("{json}");
    Ok(())
}

fn active_org() -> Result<String> {
    CloudState::load()
        .active_org
        .ok_or_else(|| anyhow::anyhow!("no active organization; run `hm cloud org switch <slug>`"))
}
