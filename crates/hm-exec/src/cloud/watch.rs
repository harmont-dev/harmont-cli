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
    HarmontClient, HarmontError,
    logs::{LogEvent, StreamKind},
    models::{HarmontJob, OpenJobState, build_is_terminal, job_is_terminal},
};
use hm_pipeline_ir::DurationMs;
use hm_plugin_protocol::events::{BuildEvent, PlanSummary, StdStream};
use uuid::Uuid;

/// Poll-interval for build/job status.
const POLL: Duration = Duration::from_millis(1500);

/// Re-mint the log token when its remaining lifetime drops below this margin,
/// so a stream spawned late in a long build starts with a fresh, valid token
/// instead of one that 401s within seconds.
const TOKEN_REFRESH_MARGIN: chrono::Duration = chrono::Duration::minutes(5);

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
fn duration_ms(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> DurationMs {
    match (start, end) {
        (Some(s), Some(e)) => DurationMs((e - s).num_milliseconds().max(0).cast_unsigned()),
        _ => DurationMs(0),
    }
}

/// Parse an optional RFC 3339 timestamp string (the form the v1 API serializes
/// `started_at` / `finished_at` as) into a UTC datetime, dropping unparseable
/// or absent values to `None`.
fn parse_rfc3339(ts: Option<&str>) -> Option<DateTime<Utc>> {
    ts.and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

/// Whether a job has reached a state where its logs exist (running or already
/// terminal), and so a log stream should be started for it.
///
/// Matching the typed [`OpenJobState`] enum (rather than `to_string()`/`as_str()`
/// against string literals) keeps the known states exhaustive: when the cloud
/// adds a new state the compiler forces this decision to be revisited, and a
/// misspelled state can no longer silently drop a job's logs. An `Unknown`
/// state (a value introduced after this SDK was built) is treated as
/// logs-available — starting a stream for a job that has none yet is harmless,
/// whereas skipping one that does have logs would silently lose output.
const fn job_logs_available(state: &OpenJobState) -> bool {
    match state {
        OpenJobState::Running
        | OpenJobState::Passed
        | OpenJobState::Failed
        | OpenJobState::TimedOut
        | OpenJobState::Canceling
        | OpenJobState::Canceled
        | OpenJobState::TimingOut
        | OpenJobState::Unknown => true,
        // No logs yet (not started) or never produced (skipped).
        OpenJobState::Pending
        | OpenJobState::Scheduled
        | OpenJobState::Assigned
        | OpenJobState::Skipped => false,
    }
}

/// Map a terminal build state to the process exit code the renderer and the
/// `hm run` driver use. `passed` → 0, `canceled` → 130 (SIGINT-cancel, mirrors
/// [`crate::BuildStatus::Canceled`]), everything else (`failed`, and any
/// unexpected state) → 1. Kept in lockstep with the backend's state→status map
/// so a server-side cancel is never reported as a failure.
pub(crate) fn exit_code_for_state(state: &str) -> i32 {
    match state {
        "passed" => 0,
        "canceled" => 130,
        _ => 1,
    }
}

/// Watch `build #number` until terminal, emitting [`BuildEvent`]s on `tx`.
///
/// `log_base` is the host serving the SSE log stream (the API base in prod).
/// Returns the terminal exit code via [`exit_code_for_state`]: 0 passed, 130
/// canceled, 1 otherwise.
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
    // Log tokens carry a ~1h TTL. A long build outlives a single mint, so a
    // job whose stream starts late in the build would 401 mid-stream. We keep
    // the minted token (with its `expires_at`) and re-mint before spawning a
    // new stream once we're within `TOKEN_REFRESH_MARGIN` of expiry, so every
    // later-starting step gets a valid token. Streams that 401 anyway surface a
    // one-line notice (see `stream_one`) rather than silently dropping logs.
    let mut log_token = client.log_token(org, pipeline, number).await?;

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
            // The v1 API exposes a job's primary key as the string `id` (a UUID
            // in canonical form); parse it into the `Uuid` that `BuildEvent`
            // and the local step maps key on.
            let job_id = Uuid::parse_str(&job.id)?;
            if job_logs_available(&job.state) && streaming.insert(job_id) {
                let name = job.name.clone().unwrap_or_else(|| "job".to_string());
                let idx = *chain_idx.entry(job_id).or_insert_with(|| {
                    let i = next_idx;
                    next_idx += 1;
                    i
                });
                if tx
                    .send(BuildEvent::StepQueued {
                        step_id: job_id,
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
                        step_id: job_id,
                        runner: "cloud".to_string(),
                        image: None,
                    })
                    .await
                    .is_err()
                {
                    return Ok(1);
                }
                // Re-mint the token if it's near expiry before this (possibly
                // late-starting) stream begins. A re-mint failure is
                // non-fatal: fall back to the existing token and let
                // `stream_one` surface a notice if the server rejects it.
                if log_token.expires_at - Utc::now() < TOKEN_REFRESH_MARGIN {
                    match client.log_token(org, pipeline, number).await {
                        Ok(fresh) => log_token = fresh,
                        Err(e) => tracing::warn!("log-token refresh failed: {e}"),
                    }
                }
                guard.0.push(tokio::spawn(stream_one(
                    client.clone(),
                    log_base.to_string(),
                    job_id,
                    log_token.token.clone(),
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
        if build_is_terminal(&build.state) {
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
            if job_is_terminal(&job.state) {
                let job_id = Uuid::parse_str(&job.id)?;
                if ended.insert(job_id) && tx.send(step_end(job, job_id)).await.is_err() {
                    return Ok(1);
                }
            }
        }
    }

    let code = exit_code_for_state(&final_state);
    // Best-effort close; ignore a dropped receiver.
    let _ = tx
        .send(BuildEvent::BuildEnd {
            exit_code: code,
            duration_ms: DurationMs::from(started.elapsed()),
        })
        .await;
    Ok(code)
}

/// Build a `StepEnd` event from a (terminal) job's recorded fields. `step_id`
/// is the job's `id` already parsed into a `Uuid` by the caller.
fn step_end(job: &HarmontJob, step_id: Uuid) -> BuildEvent {
    let state = job.state.to_string();
    let passed = matches!(state.as_str(), "passed" | "skipped");
    let exit_code = job
        .exit_code
        // Saturate exit codes outside [i32::MIN, i32::MAX] rather than panic.
        .map_or_else(|| i32::from(!passed), |c| i32::try_from(c).unwrap_or(1));
    BuildEvent::StepEnd {
        step_id,
        exit_code,
        duration_ms: duration_ms(
            parse_rfc3339(job.started_at.as_deref()),
            parse_rfc3339(job.finished_at.as_deref()),
        ),
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
/// best-effort (log stream for this job stops, other jobs continue) — with one
/// exception: a `401 Unauthorized` (the log token expired mid-build) is
/// surfaced as a single one-line notice on the step's stream instead of being
/// dropped silently, so the gulf of evaluation ("why did my logs stop?") stays
/// narrow. The build-status poll still drives the build to its real verdict.
async fn stream_one(
    client: HarmontClient,
    log_base: String,
    job_id: Uuid,
    token: String,
    tx: tokio::sync::mpsc::Sender<BuildEvent>,
) {
    let expired = stream_job_logs_as_events(&client, &log_base, job_id, &token, &tx)
        .await
        .err()
        .and_then(|e| {
            e.downcast_ref::<HarmontError>()
                .map(|h| matches!(h, HarmontError::Unauthorized))
        })
        .unwrap_or(false);
    if expired {
        let _ = tx
            .send(BuildEvent::StepLog {
                step_id: job_id,
                stream: StdStream::Stderr,
                line: "live logs expired; full logs available via `hm cloud build show`"
                    .to_string(),
                ts: Utc::now(),
            })
            .await;
    }
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

#[cfg(test)]
mod tests {
    use super::{OpenJobState, job_logs_available};
    use rstest::rstest;

    // A future state we don't recognize (`Unknown`) is streamed rather than
    // silently dropped.
    #[rstest]
    #[case::running(OpenJobState::Running, true)]
    #[case::passed(OpenJobState::Passed, true)]
    #[case::failed(OpenJobState::Failed, true)]
    #[case::timed_out(OpenJobState::TimedOut, true)]
    #[case::canceling(OpenJobState::Canceling, true)]
    #[case::canceled(OpenJobState::Canceled, true)]
    #[case::timing_out(OpenJobState::TimingOut, true)]
    #[case::unknown(OpenJobState::Unknown, true)]
    #[case::pending(OpenJobState::Pending, false)]
    #[case::scheduled(OpenJobState::Scheduled, false)]
    #[case::assigned(OpenJobState::Assigned, false)]
    #[case::skipped(OpenJobState::Skipped, false)]
    fn job_logs_available_matches_state(#[case] state: OpenJobState, #[case] expected: bool) {
        assert_eq!(job_logs_available(&state), expected);
    }

    // A server-side cancel must NOT collapse to the generic failure code;
    // unexpected/unknown terminal states fail closed.
    #[rstest]
    #[case::passed("passed", 0)]
    #[case::canceled("canceled", 130)]
    #[case::failed("failed", 1)]
    #[case::timed_out("timed_out", 1)]
    fn exit_code_for_state(#[case] state: &str, #[case] code: i32) {
        assert_eq!(super::exit_code_for_state(state), code);
    }
}
