//! `hm cloud job list|show|log`.

use std::collections::BTreeMap;

use anyhow::Result;
use futures_util::StreamExt;
use harmont_cloud::HarmontClient;
use harmont_cloud::logs::{LogChunk, LogEvent};
use uuid::Uuid;

use crate::cli::JobCommand;
use crate::settings;

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
    // Mint a build-scoped log token, then stream this job's logs to terminal.
    let token = client.log_token(org, pipe, build).await?;
    let log_base = client.base_url().to_string();
    let stream = client
        .stream_job_logs(&log_base, job_id, &token.token)
        .await?;
    tokio::pin!(stream);
    while let Some(item) = stream.next().await {
        match item? {
            LogEvent::History(chunks) => {
                for c in &chunks {
                    print_chunk(c);
                }
            }
            LogEvent::Chunk(c) => print_chunk(&c),
            LogEvent::Done => break,
        }
    }
    Ok(())
}

fn print_chunk(c: &LogChunk) {
    // `content` may carry a trailing newline already; trim it so tracing's own
    // line framing doesn't double-space the output.
    let line = c.content.strip_suffix('\n').unwrap_or(&c.content);
    tracing::info!("{line}");
}
