//! Watch a cloud build to completion, emitting [`BuildEvent`]s.
//!
//! Discovers jobs, streams each job's logs concurrently, and maps cloud job
//! lifecycle + SSE logs to the shared [`BuildEvent`] vocabulary so the cloud
//! path renders through the same `hm-render` renderers as a local run.
//!
//! A cloud job maps to a pipeline step (keyed by `job.id`); the cloud build
//! is modeled as a single chain (`chain_idx == 0`, `chain_count == 1`).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use harmont_cloud::{
    logs::{LogEvent, StreamKind},
    models::{build_is_terminal, job_is_terminal},
    HarmontClient,
};
use hm_plugin_protocol::events::{BuildEvent, PlanSummary, StdStream};
use uuid::Uuid;

/// Poll-interval for build/job status.
const POLL: Duration = Duration::from_millis(1500);

/// Aborts any still-running stream tasks when dropped (covers early-return
/// error paths so no detached ghost tasks outlive `watch_build`).
#[derive(Debug)]
struct AbortGuard(Vec<tokio::task::JoinHandle<()>>);
impl Drop for AbortGuard {
    fn drop(&mut self) {
        for h in &self.0 {
            h.abort();
        }
    }
}

/// Convert a unix-nanosecond timestamp to a UTC datetime, falling back to
/// "now" when absent or out of range.
pub(crate) fn ts_or_now(ts_unix_ns: Option<i64>) -> DateTime<Utc> {
    ts_unix_ns.map_or_else(Utc::now, DateTime::<Utc>::from_timestamp_nanos)
}

/// Duration between two optional timestamps, in milliseconds (0 if either is
/// missing or the interval is negative).
fn duration_ms(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> u64 {
    match (start, end) {
        (Some(s), Some(e)) => (e - s).num_milliseconds().max(0).cast_unsigned(),
        _ => 0,
    }
}

/// Watch `build #number` until terminal, emitting [`BuildEvent`]s on `tx`.
///
/// `log_base` is the host serving the SSE log stream (the API base in prod).
/// Returns 0 if the build passed, else 1.
///
/// # Errors
/// Returns an error if any SDK call fails (build status poll, job list, or log
/// token fetch). A dropped receiver (`tx`) is treated as a clean early exit
/// (`Ok(1)`) — not an error.
#[allow(clippy::too_many_lines)] // single-responsibility poll loop; split would obscure flow
pub async fn watch_build(
    client: &HarmontClient,
    log_base: &str,
    org: &str,
    pipeline: &str,
    number: i64,
    tx: tokio::sync::mpsc::Sender<BuildEvent>,
) -> Result<i32> {
    // TODO: log token has a ~1h TTL; very long builds will 401 mid-stream and
    // lose remaining logs (build-status poll still drives completion).
    // Refresh via LogToken.expires_at if needed.
    let token = client.log_token(org, pipeline, number).await?.token;

    let started = Instant::now();
    if tx
        .send(BuildEvent::BuildStart {
            run_id: Uuid::new_v4(),
            plan: PlanSummary {
                // #jobs isn't known until the first list_jobs; 0 is a fine
                // placeholder (renderers treat it as "not yet known").
                step_count: 0,
                chain_count: 1,
                default_runner: "cloud".to_string(),
            },
            started_at: Utc::now(),
        })
        .await
        .is_err()
    {
        // Renderer side went away — nothing left to drive.
        return Ok(1);
    }

    // Jobs we've started a log stream for.
    let mut streaming: HashSet<Uuid> = HashSet::new();
    // Deduplicates the post-drain StepEnd sweep: if `list_jobs` returns the
    // same job ID more than once we emit only one StepEnd per job.
    let mut ended: HashSet<Uuid> = HashSet::new();
    // Stable chain-local index assigned in discovery order.
    let mut chain_idx: HashMap<Uuid, usize> = HashMap::new();
    let mut next_idx: usize = 0;
    let mut guard = AbortGuard(Vec::new());

    let final_state = loop {
        // Discover jobs; start a log stream for each job that has reached a
        // state where logs exist (running or already terminal).
        let jobs = client.list_jobs(org, pipeline, number).await?;
        for job in &jobs {
            let state = job.state.to_string();
            let logs_available = matches!(
                state.as_str(),
                "running"
                    | "passed"
                    | "failed"
                    | "timed_out"
                    | "canceling"
                    | "canceled"
                    | "timing_out"
            );
            if logs_available && streaming.insert(job.id) {
                let name = job.name.clone().unwrap_or_else(|| "job".to_string());
                let idx = *chain_idx.entry(job.id).or_insert_with(|| {
                    let i = next_idx;
                    next_idx += 1;
                    i
                });
                if tx
                    .send(BuildEvent::StepQueued {
                        step_id: job.id,
                        key: name.clone(),
                        chain_idx: idx,
                        parent_key: None,
                        display_name: name.clone(),
                    })
                    .await
                    .is_err()
                {
                    return Ok(1);
                }
                if tx
                    .send(BuildEvent::StepStart {
                        step_id: job.id,
                        runner: "cloud".to_string(),
                        image: None,
                    })
                    .await
                    .is_err()
                {
                    return Ok(1);
                }
                guard.0.push(tokio::spawn(stream_one(
                    client.clone(),
                    log_base.to_string(),
                    job.id,
                    token.clone(),
                    tx.clone(),
                )));
            }
            // NOTE: StepEnd is intentionally NOT emitted here. A job's log
            // stream runs in a spawned task concurrently with this poll loop;
            // emitting StepEnd now could order it ahead of that job's still-
            // in-flight StepLog lines. We drain every stream below, then emit
            // all StepEnds — guaranteeing logs precede the step's terminal mark.
        }

        let build = client.get_build(org, pipeline, number).await?;
        if build_is_terminal(&build.state.to_string()) {
            break build.state.to_string();
        }
        // TODO: no overall deadline; a build stuck non-terminal loops forever
        // (matches `hm cloud build watch`). Consider a --timeout ceiling.
        tokio::time::sleep(POLL).await;
    };

    // Drain all log streams (empties the guard so Drop aborts nothing on the
    // success path).
    for h in guard.0.drain(..) {
        let _ = h.await;
    }

    // Emit StepEnd for any terminal job not yet ended (e.g. a job that went
    // straight to terminal in the same poll the build did).
    if let Ok(jobs) = client.list_jobs(org, pipeline, number).await {
        for job in &jobs {
            if job_is_terminal(&job.state.to_string())
                && ended.insert(job.id)
                && tx.send(step_end(job)).await.is_err()
            {
                return Ok(1);
            }
        }
    }

    let passed = final_state == "passed";
    let code = i32::from(!passed);
    // Best-effort close; ignore a dropped receiver.
    let _ = tx
        .send(BuildEvent::BuildEnd {
            exit_code: code,
            // Saturate at u64::MAX (~584 million years) rather than panic.
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
        .await;
    Ok(code)
}

/// Build a `StepEnd` event from a (terminal) job's recorded fields.
fn step_end(job: &harmont_cloud::models::Job) -> BuildEvent {
    let state = job.state.to_string();
    let passed = matches!(state.as_str(), "passed" | "skipped");
    let exit_code = job
        .exit_code
        // Saturate exit codes outside [i32::MIN, i32::MAX] rather than panic.
        .map_or_else(|| i32::from(!passed), |c| i32::try_from(c).unwrap_or(1));
    BuildEvent::StepEnd {
        step_id: job.id,
        exit_code,
        duration_ms: duration_ms(job.started_at, job.finished_at),
        snapshot: None,
    }
}

/// Stream one job's SSE logs as [`BuildEvent::StepLog`] events.
///
/// Emits a `StepLog` per complete line (keyed by `step_id`) to `tx`, until
/// the job's `done` event. Buffers partial lines and flushes the trailing
/// remainder. Used by both the multi-job watch loop and the single-job
/// `hm cloud job log` tail.
///
/// Returns `Ok(())` on a clean `done` close. A dropped receiver (`tx.send`
/// fails) is treated as a clean stop — the caller has gone away, not the job.
///
/// **Error semantics are caller-controlled:**
/// - The multi-job watcher (`stream_one`) swallows the error (best-effort: log
///   other jobs, keep watching).
/// - The single-job tail (`hm cloud job log`) propagates it (`?`) so the
///   command surfaces transport failures to the user.
///
/// # Errors
/// Returns an error on transport or SSE stream failure (the underlying
/// [`HarmontClient::stream_job_logs`] call or a non-`Done` error event).
pub async fn stream_job_logs_as_events(
    client: &HarmontClient,
    log_base: &str,
    step_id: Uuid,
    token: &str,
    tx: &tokio::sync::mpsc::Sender<BuildEvent>,
) -> Result<()> {
    let stream = client.stream_job_logs(log_base, step_id, token).await?;
    futures_util::pin_mut!(stream);
    let mut buf = String::new();
    let mut last_stream = StreamKind::Stdout;
    while let Some(item) = stream.next().await {
        match item {
            Ok(LogEvent::History(chunks)) => {
                for c in chunks {
                    last_stream = c.stream;
                    if emit(tx, step_id, c.stream, c.ts_unix_ns, &mut buf, &c.content)
                        .await
                        .is_err()
                    {
                        // Receiver dropped — treat as clean stop.
                        return Ok(());
                    }
                }
            }
            Ok(LogEvent::Chunk(c)) => {
                last_stream = c.stream;
                if emit(tx, step_id, c.stream, c.ts_unix_ns, &mut buf, &c.content)
                    .await
                    .is_err()
                {
                    // Receiver dropped — treat as clean stop.
                    return Ok(());
                }
            }
            Ok(LogEvent::Done) => break,
            Err(e) => return Err(e.into()),
        }
    }
    // Flush any trailing partial line.
    if !buf.is_empty() {
        let line = std::mem::take(&mut buf);
        // Ignore send failure: receiver dropping at flush time is still a
        // clean stop.
        let _ = tx
            .send(BuildEvent::StepLog {
                step_id,
                stream: map_stream(last_stream),
                line,
                ts: Utc::now(),
            })
            .await;
    }
    Ok(())
}

/// Thin wrapper used by the multi-job watch loop. Errors are treated as
/// best-effort (log stream for this job stops, other jobs continue).
async fn stream_one(
    client: HarmontClient,
    log_base: String,
    job_id: Uuid,
    token: String,
    tx: tokio::sync::mpsc::Sender<BuildEvent>,
) {
    let _ = stream_job_logs_as_events(&client, &log_base, job_id, &token, &tx).await;
}

/// Map the SDK stream kind onto the renderer's two-way stream: `Meta` folds
/// into `Stderr` (it's out-of-band, not pipeline stdout).
pub(crate) const fn map_stream(kind: StreamKind) -> StdStream {
    match kind {
        StreamKind::Stdout => StdStream::Stdout,
        StreamKind::Stderr | StreamKind::Meta => StdStream::Stderr,
    }
}

/// Buffer content and emit complete `\n`-terminated lines as `StepLog`
/// events. Returns `Err(())` if the receiver dropped (caller should stop).
async fn emit(
    tx: &tokio::sync::mpsc::Sender<BuildEvent>,
    job_id: Uuid,
    kind: StreamKind,
    ts_unix_ns: Option<i64>,
    buf: &mut String,
    content: &str,
) -> std::result::Result<(), ()> {
    buf.push_str(content);
    while let Some(nl) = buf.find('\n') {
        let raw: String = buf.drain(..=nl).collect();
        let line = raw.trim_end_matches(['\r', '\n']).to_string();
        tx.send(BuildEvent::StepLog {
            step_id: job_id,
            stream: map_stream(kind),
            line,
            ts: ts_or_now(ts_unix_ns),
        })
        .await
        .map_err(|_| ())?;
    }
    Ok(())
}
