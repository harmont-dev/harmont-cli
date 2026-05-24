//! `hm cloud build list|show|cancel|watch`.

use std::collections::BTreeMap;

use anyhow::{Result, bail};

use crate::api::types::{Build, BuildList};
use crate::cli::BuildCommand;
use crate::config::Config;
use crate::creds;
use crate::http::Client;
use crate::state::CloudState;

pub(crate) async fn run(env: &BTreeMap<String, String>, cmd: BuildCommand) -> Result<()> {
    let cfg = Config::from_env(env);
    let token = creds::load_token(&cfg.api_base, env)
        .ok_or_else(|| anyhow::anyhow!("not logged in; run `hm cloud login`"))?;
    let client = Client::new(&cfg, Some(token));
    let org = active_org()?;

    match cmd {
        BuildCommand::List { pipeline } => list(&client, &org, &pipeline).await,
        BuildCommand::Show { pipeline, number } => show(&client, &org, &pipeline, number).await,
        BuildCommand::Cancel { pipeline, number } => cancel(&client, &org, &pipeline, number).await,
        BuildCommand::Watch { pipeline, number } => watch(&client, &org, &pipeline, number).await,
    }
}

async fn list(client: &Client, org: &str, pipe: &str) -> Result<()> {
    let builds: BuildList = client
        .get(&format!("/organizations/{org}/pipelines/{pipe}/builds"))
        .await?;
    for b in &builds.data {
        tracing::info!(
            "#{:<5} {:<10} {}",
            b.number,
            b.state,
            b.message.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

async fn show(client: &Client, org: &str, pipe: &str, number: i64) -> Result<()> {
    let b: Build = client
        .get(&format!(
            "/organizations/{org}/pipelines/{pipe}/builds/{number}"
        ))
        .await?;
    let json = serde_json::to_string_pretty(&b).unwrap_or_default();
    tracing::info!("{json}");
    Ok(())
}

async fn cancel(client: &Client, org: &str, pipe: &str, number: i64) -> Result<()> {
    let _: serde_json::Value = client
        .post(
            &format!("/organizations/{org}/pipelines/{pipe}/builds/{number}/cancel"),
            &serde_json::json!({}),
        )
        .await?;
    tracing::info!("build #{number} cancelled");
    Ok(())
}

async fn watch(client: &Client, org: &str, pipe: &str, number: i64) -> Result<()> {
    let mut last_state = String::new();
    loop {
        let b: Build = client
            .get(&format!(
                "/organizations/{org}/pipelines/{pipe}/builds/{number}"
            ))
            .await?;
        if b.state != last_state {
            tracing::info!("state: {last_state} -> {}", b.state);
            last_state = b.state.clone();
        }
        match b.state.as_str() {
            "passed" => return Ok(()),
            "failed" | "canceled" => {
                bail!("build {} ({})", b.state, number);
            }
            _ => {}
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

fn active_org() -> Result<String> {
    CloudState::load()
        .active_org
        .ok_or_else(|| anyhow::anyhow!("no active organization; run `hm cloud org switch <slug>`"))
}
