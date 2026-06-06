//! `hm cloud job list|show|log`.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use harmont_cloud::HarmontClient;
use harmont_cloud::logs::{LogChunk, LogEvent, StreamKind};
use hm_plugin_protocol::events::{BuildEvent, PlanSummary, StdStream};
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
    // Mint a build-scoped log token, then stream this single job's logs through
    // the shared `hm-render` HumanRenderer (a one-step build wrapper).
    let token = client.log_token(org, pipe, build).await?;
    let log_base = client.base_url().to_string();

    let (color, _logs) = settings::render_prefs();
    // A single-job tail is always a flat log stream, so force the streaming
    // HumanRenderer (logs = true) regardless of TTY.
    let renderer = hm_render::renderer_for("human", color, true)?;
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

    let stream = client
        .stream_job_logs(&log_base, job_id, &token.token)
        .await?;
    tokio::pin!(stream);
    let mut buf = String::new();
    let mut last_stream = StreamKind::Stdout;
    'outer: while let Some(item) = stream.next().await {
        match item? {
            LogEvent::History(chunks) => {
                for c in &chunks {
                    last_stream = c.stream;
                    if emit_chunk(&tx, job_id, c, &mut buf).await.is_err() {
                        break 'outer;
                    }
                }
            }
            LogEvent::Chunk(c) => {
                last_stream = c.stream;
                if emit_chunk(&tx, job_id, &c, &mut buf).await.is_err() {
                    break 'outer;
                }
            }
            LogEvent::Done => break,
        }
    }
    // Flush any trailing partial line.
    if !buf.is_empty() {
        let line = std::mem::take(&mut buf);
        let _ = tx
            .send(BuildEvent::StepLog {
                step_id: job_id,
                stream: map_stream(last_stream),
                line,
                ts: Utc::now(),
            })
            .await;
    }

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

/// Map the SDK stream kind onto the renderer's two-way stream: `Meta` folds
/// into `Stderr` (out-of-band, not pipeline stdout).
fn map_stream(kind: StreamKind) -> StdStream {
    match kind {
        StreamKind::Stdout => StdStream::Stdout,
        StreamKind::Stderr | StreamKind::Meta => StdStream::Stderr,
    }
}

/// Buffer a chunk's content and forward each complete `\n`-terminated line as
/// a `StepLog` event. Returns `Err(())` if the renderer's receiver dropped.
async fn emit_chunk(
    tx: &tokio::sync::mpsc::Sender<BuildEvent>,
    job_id: Uuid,
    c: &LogChunk,
    buf: &mut String,
) -> std::result::Result<(), ()> {
    let ts = c
        .ts_unix_ns
        .map(DateTime::<Utc>::from_timestamp_nanos)
        .unwrap_or_else(Utc::now);
    buf.push_str(&c.content);
    while let Some(nl) = buf.find('\n') {
        let raw: String = buf.drain(..=nl).collect();
        let line = raw.trim_end_matches(['\r', '\n']).to_string();
        tx.send(BuildEvent::StepLog {
            step_id: job_id,
            stream: map_stream(c.stream),
            line,
            ts,
        })
        .await
        .map_err(|_| ())?;
    }
    Ok(())
}
