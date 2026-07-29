//! `hm cloud build list|show|cancel|watch`.

use anyhow::Result;
use harmont_cloud::HarmontClient;

use crate::commands::cloud::settings;
use hm_cloud::cli::BuildCommand;
use hm_core::app_ctx::AppCtx;
use hm_core::exec::cloud::watch::watch_build;
use hm_core::term::Term;

pub(crate) async fn run(cmd: BuildCommand, app: &AppCtx) -> Result<()> {
    let (client, ctx) = settings::client(app).await?;
    let org = ctx.org()?;

    match cmd {
        BuildCommand::List { pipeline } => {
            let pipe = settings::resolve_pipeline(app, pipeline).await?;
            list(&client, &org, &pipe).await
        }
        BuildCommand::Show { pipeline, number } => {
            let pipe = settings::resolve_pipeline(app, pipeline).await?;
            show(&client, &org, &pipe, number).await
        }
        BuildCommand::Cancel { pipeline, number } => {
            let pipe = settings::resolve_pipeline(app, pipeline).await?;
            cancel(&client, &org, &pipe, number).await
        }
        BuildCommand::Watch { pipeline, number } => {
            let pipe = settings::resolve_pipeline(app, pipeline).await?;
            watch(&client, &org, &pipe, number, app.term()).await
        }
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

async fn watch(
    client: &HarmontClient,
    org: &str,
    pipe: &str,
    number: i64,
    term: Term<'_>,
) -> Result<()> {
    // Render the live build through the shared `hm-render` renderers (the same
    // ones a local `hm run` uses), driven by the `BuildEvent`s `watch_build`
    // emits over an mpsc channel.
    let prefs = crate::commands::cloud::settings::RenderPrefs::detect(term);
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
