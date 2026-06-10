//! `hm cloud job list|show|log`.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::Utc;
use harmont_cloud::HarmontClient;
use hm_plugin_protocol::events::{BuildEvent, PlanSummary};
use uuid::Uuid;

use crate::cli::JobCommand;
use crate::settings;
use hm_exec::cloud::watch::stream_job_logs_as_events;

pub(crate) async fn run(_env: &BTreeMap<String, String>, cmd: JobCommand) -> Result<()> {
    let (client, ctx) = settings::client()?;
    let org = ctx.org()?;

    match cmd {
        JobCommand::List { pipeline, build } => list(&client, &org, &pipeline, build).await,
        JobCommand::Show {
            pipeline,
            build,
            job_id,
        } => show(&client, &org, &pipeline, build, &job_id).await,
        JobCommand::Log {
            pipeline,
            build,
            job_id,
        } => log_cmd(&client, &org, &pipeline, build, &job_id).await,
    }
}

async fn list(client: &HarmontClient, org: &str, pipe: &str, build: i64) -> Result<()> {
    let jobs = client.list_jobs(org, pipe, build).await?;
    for j in &jobs {
        tracing::info!(
            "{}  {:<10}  {}",
            j.id,
            j.state.to_string(),
            j.name.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

async fn show(client: &HarmontClient, org: &str, pipe: &str, build: i64, jid: &str) -> Result<()> {
    let j = client
        .raw()
        .get_job(org, pipe, build, jid)
        .await
        .map_err(settings::map_raw)?
        .into_inner();
    tracing::info!("{}", serde_json::to_string_pretty(&j).unwrap_or_default());
    Ok(())
}

async fn log_cmd(
    client: &HarmontClient,
    org: &str,
    pipe: &str,
    build: i64,
    jid: &str,
) -> Result<()> {
    let job_id = Uuid::parse_str(jid)
        .map_err(|e| anyhow::anyhow!("job id '{jid}' is not a valid UUID: {e}"))?;
    // Mint a build-scoped log token, then stream this single job's logs through
    // the shared `hm-render` HumanRenderer (a one-step build wrapper).
    let token = client.log_token(org, pipe, build).await?;
    let log_base = client.base_url().to_string();

    let prefs = settings::RenderPrefs::detect();
    // A single-job tail is always a flat log stream, so force the streaming
    // HumanRenderer (logs = true) regardless of TTY.
    let renderer = hm_render::renderer_for("human", prefs.color, true)?;
    let (tx, rx) = tokio::sync::mpsc::channel(1024);
    let driver = tokio::spawn(hm_render::drive(renderer, rx));

    // Wrap the lone job in a minimal one-step build so the renderer's lifecycle
    // (BuildStart … BuildEnd) is well-formed and `drive` closes cleanly.
    let name = jid.to_string();
    let _ = tx
        .send(BuildEvent::BuildStart {
            run_id: Uuid::new_v4(),
            plan: PlanSummary {
                step_count: 1,
                chain_count: 1,
                default_runner: "cloud".to_string(),
            },
            started_at: Utc::now(),
        })
        .await;
    let _ = tx
        .send(BuildEvent::StepQueued {
            step_id: job_id,
            key: name.clone(),
            chain_idx: 0,
            parent_key: None,
            display_name: name,
        })
        .await;
    let _ = tx
        .send(BuildEvent::StepStart {
            step_id: job_id,
            runner: "cloud".to_string(),
            image: None,
        })
        .await;

    // Stream this job's logs. A transport error is fatal for a single-job
    // tail (propagated via `?`), unlike the multi-job watcher which swallows
    // per-job stream errors to keep watching remaining jobs.
    stream_job_logs_as_events(client, &log_base, job_id, &token.token, &tx).await?;

    // Close the build so the renderer's `drive` loop returns.
    let _ = tx
        .send(BuildEvent::StepEnd {
            step_id: job_id,
            exit_code: 0,
            duration_ms: 0,
            snapshot: None,
        })
        .await;
    let _ = tx
        .send(BuildEvent::BuildEnd {
            exit_code: 0,
            duration_ms: 0,
        })
        .await;
    let _ = driver.await;
    Ok(())
}
