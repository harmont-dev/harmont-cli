//! `hm cloud build list|show|cancel|watch`.

use std::collections::BTreeMap;

use anyhow::Result;
use harmont_cloud::HarmontClient;

use crate::cli::BuildCommand;
use crate::settings;
use hm_exec::cloud::watch::watch_build;

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
    // Render the live build through the shared `hm-render` renderers (the same
    // ones a local `hm run` uses), driven by the `BuildEvent`s `watch_build`
    // emits over an mpsc channel.
    let prefs = crate::settings::RenderPrefs::detect();
    let renderer = hm_render::renderer_for("human", prefs.color, prefs.logs)?;
    let (tx, rx) = tokio::sync::mpsc::channel(1024);
    let driver = tokio::spawn(hm_render::drive(renderer, rx));

    let log_base = client.base_url().to_string();
    let code = watch_build(client, &log_base, org, pipe, number, tx).await?;
    let _ = driver.await;

    if code == 0 {
        Ok(())
    } else {
        anyhow::bail!("build #{number} did not pass")
    }
}
