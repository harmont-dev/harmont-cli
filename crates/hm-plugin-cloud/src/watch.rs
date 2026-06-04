//! Watch a cloud build to completion: discover jobs, stream each job's logs
//! concurrently, render through the Reporter + CloudJobView, and return a
//! process exit code (0 = passed, 1 = failed/canceled).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures_util::StreamExt;
use harmont_cloud::{
    logs::LogEvent,
    models::{build_is_terminal, job_is_terminal},
    HarmontClient,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::reporter::{Level, Reporter};
use crate::ui::cloud_view::CloudJobView;

/// Poll-interval for build/job status.
const POLL: Duration = Duration::from_millis(1500);

/// Watch `build #number` until terminal. `log_base` is the host serving the
/// SSE log stream (the API base in prod). Returns 0 if the build passed, else 1.
pub async fn watch_build(
    client: &HarmontClient,
    log_base: &str,
    org: &str,
    pipeline: &str,
    number: i64,
    reporter: Arc<dyn Reporter>,
    view: Arc<Mutex<CloudJobView>>,
) -> Result<i32> {
    let token = client.log_token(org, pipeline, number).await?.token;
    let mut streaming: HashSet<Uuid> = HashSet::new();
    let mut handles = Vec::new();

    let final_state = loop {
        // Discover jobs; start a log stream for each job that has reached a
        // state where logs exist (running or already terminal).
        let jobs = client.list_jobs(org, pipeline, number).await?;
        for job in &jobs {
            let state = job.state.to_string();
            let logs_available = matches!(
                state.as_str(),
                "running" | "passed" | "failed" | "timed_out"
            );
            if logs_available && streaming.insert(job.id) {
                let name = job.name.clone().unwrap_or_else(|| "job".to_string());
                view.lock().await.job_running(job.id, &name);
                handles.push(tokio::spawn(stream_one(
                    client.clone(),
                    log_base.to_string(),
                    job.id,
                    name,
                    token.clone(),
                    view.clone(),
                )));
            }
        }

        let build = client.get_build(org, pipeline, number).await?;
        if build_is_terminal(&build.state.to_string()) {
            break build.state.to_string();
        }
        tokio::time::sleep(POLL).await;
    };

    // Drain all log streams.
    for h in handles.drain(..) {
        let _ = h.await;
    }

    // Mark each job's final glyph.
    if let Ok(jobs) = client.list_jobs(org, pipeline, number).await {
        for job in jobs {
            let state = job.state.to_string();
            if job_is_terminal(&state) {
                let name = job.name.clone().unwrap_or_else(|| "job".to_string());
                let passed = matches!(state.as_str(), "passed" | "skipped");
                view.lock().await.job_done(job.id, &name, passed);
            }
        }
    }

    let passed = final_state == "passed";
    reporter.status(
        if passed { Level::Success } else { Level::Error },
        &format!("Build #{number} {final_state}"),
    );
    Ok(if passed { 0 } else { 1 })
}

/// Stream one job's logs until the `done` event, splitting content into lines.
async fn stream_one(
    client: HarmontClient,
    log_base: String,
    job_id: Uuid,
    name: String,
    token: String,
    view: Arc<Mutex<CloudJobView>>,
) {
    let stream = match client.stream_job_logs(&log_base, job_id, &token).await {
        Ok(s) => s,
        Err(_) => return,
    };
    futures_util::pin_mut!(stream);
    let mut buf = String::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(LogEvent::History(chunks)) => {
                for c in chunks {
                    emit(&view, job_id, &name, &mut buf, &c.content).await;
                }
            }
            Ok(LogEvent::Chunk(c)) => emit(&view, job_id, &name, &mut buf, &c.content).await,
            Ok(LogEvent::Done) => break,
            Err(_) => break,
        }
    }
    // Flush any trailing partial line.
    if !buf.is_empty() {
        let line = std::mem::take(&mut buf);
        view.lock().await.job_log(job_id, &name, &line);
    }
}

/// Buffer content and emit complete `\n`-terminated lines.
async fn emit(
    view: &Arc<Mutex<CloudJobView>>,
    id: Uuid,
    name: &str,
    buf: &mut String,
    content: &str,
) {
    buf.push_str(content);
    while let Some(nl) = buf.find('\n') {
        let line: String = buf.drain(..=nl).collect();
        view.lock()
            .await
            .job_log(id, name, line.trim_end_matches(['\r', '\n']));
    }
}
