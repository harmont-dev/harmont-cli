//! `hm cloud build list|show|cancel|watch`.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use harmont_cloud::HarmontClient;
use harmont_cloud::models::build_is_terminal;

use crate::cli::BuildCommand;
use crate::settings;

pub(crate) async fn run(_env: &BTreeMap<String, String>, cmd: BuildCommand) -> Result<()> {
    let (client, ctx) = settings::client()?;
    let org = ctx.org()?;

    match cmd {
        BuildCommand::List { pipeline } => list(&client, &org, &pipeline).await,
        BuildCommand::Show { pipeline, number } => show(&client, &org, &pipeline, number).await,
        BuildCommand::Cancel { pipeline, number } => cancel(&client, &org, &pipeline, number).await,
        BuildCommand::Watch { pipeline, number } => watch(&client, &org, &pipeline, number).await,
    }
}

async fn list(client: &HarmontClient, org: &str, pipe: &str) -> Result<()> {
    let builds = client
        .raw()
        .list_builds(org, pipe, None, None)
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    for b in &builds.data {
        tracing::info!(
            "#{:<5} {:<10} {}",
            b.number,
            b.state.to_string(),
            b.message.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

async fn show(client: &HarmontClient, org: &str, pipe: &str, number: i64) -> Result<()> {
    let b = client.get_build(org, pipe, number).await?;
    let json = serde_json::to_string_pretty(&b).unwrap_or_default();
    tracing::info!("{json}");
    Ok(())
}

async fn cancel(client: &HarmontClient, org: &str, pipe: &str, number: i64) -> Result<()> {
    client.cancel_build(org, pipe, number).await?;
    tracing::info!("build #{number} cancelled");
    Ok(())
}

async fn watch(client: &HarmontClient, org: &str, pipe: &str, number: i64) -> Result<()> {
    let mut last_state = String::new();
    loop {
        let b = client.get_build(org, pipe, number).await?;
        let state = b.state.to_string();
        if state != last_state {
            tracing::info!("state: {last_state} -> {state}");
            last_state.clone_from(&state);
        }
        if build_is_terminal(&state) {
            return match state.as_str() {
                "passed" => Ok(()),
                _ => bail!("build {state} (#{number})"),
            };
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
