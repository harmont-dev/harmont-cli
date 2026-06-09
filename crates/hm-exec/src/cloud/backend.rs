//! The cloud [`ExecutionBackend`]: archive the worktree, submit the whole build
//! to Harmont Cloud, and watch it to completion. The server schedules and runs;
//! this backend is an *observer* (see [`Capabilities::cloud`]).

use harmont_cloud::{builds::NewBuild, HarmontClient, HarmontError};
use hm_plugin_protocol::events::{BuildEvent, BuildRef};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    BackendError, BackendHandle, BuildOutcome, BuildStatus, Capabilities, ExecutionBackend, Result,
    RunRequest,
};

/// Submits the whole build to Harmont Cloud and watches it; the server schedules.
#[derive(Debug)]
pub struct CloudBackend {
    client: HarmontClient,
    /// API base used as the SSE log stream host during `watch_build`.
    api_base: String,
    org: String,
}

impl CloudBackend {
    /// Construct a `CloudBackend`.
    ///
    /// `client`/`api_base` come from the CLI's resolved cloud settings; `org`
    /// is the resolved organization slug.
    #[must_use]
    pub const fn new(client: HarmontClient, api_base: String, org: String) -> Self {
        Self {
            client,
            api_base,
            org,
        }
    }
}

#[async_trait::async_trait]
impl ExecutionBackend for CloudBackend {
    fn name(&self) -> &'static str {
        "cloud"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::cloud()
    }

    async fn start(&self, req: RunRequest) -> Result<BackendHandle> {
        // Archive the worktree (fail fast as a setup error).
        let source_tgz = crate::local::build_archive_bytes(&req.repo_root)
            .map_err(|e| BackendError::Local(format!("archiving worktree: {e}")))?;

        // Submit. Fail fast on auth/rejection BEFORE returning a handle so the
        // CLI can surface the doctrine error without a half-started stream.
        let build = self
            .client
            .submit_build(NewBuild {
                org: self.org.clone(),
                pipeline: req.pipeline_slug.clone(),
                branch: req.source.branch.clone(),
                commit: req.source.commit.clone(),
                message: req.source.message.clone(),
                pipeline_ir: req.plan.ir_json.clone(), // verbatim
                source_tgz,
                env: req.env.clone().into_iter().collect(),
            })
            .await
            .map_err(map_harmont_err)?;

        let watch_url = Some(format!(
            "{}/{}/{}/builds/{}",
            self.api_base, self.org, req.pipeline_slug, build.number
        ));
        let build_ref = BuildRef {
            run_id: uuid::Uuid::new_v4(),
            number: Some(build.number),
            org: Some(self.org.clone()),
            pipeline: req.pipeline_slug.clone(),
        };

        let (tx, rx) = mpsc::channel(1024);
        let cancel = CancellationToken::new();

        // Emit `BuildAccepted` immediately (the CLI prints the watch line from
        // this). `try_send` can't fail: the receiver is alive and the buffer
        // is empty.
        let _ = tx.try_send(BuildEvent::BuildAccepted {
            build: build_ref.clone(),
            watch_url: watch_url.clone(),
        });

        // `--no-watch`: detach. The build was accepted server-side; resolve to
        // a terminal "submitted" outcome at once. The server keeps running it.
        if !req.options.watch {
            let now = chrono::Utc::now();
            let outcome = BuildOutcome {
                build: build_ref,
                status: BuildStatus::Passed,
                steps: vec![],
                started_at: now,
                finished_at: now,
                watch_url,
            };
            let join = tokio::spawn(async move {
                drop(tx); // close the stream so the renderer terminates
                Ok(outcome)
            });
            return Ok(BackendHandle::spawn(rx, cancel, join));
        }

        let client = self.client.clone();
        let api_base = self.api_base.clone();
        let org = self.org.clone();
        let pipeline = req.pipeline_slug.clone();
        let number = build.number;
        let token = cancel.clone();
        let started = chrono::Utc::now();
        let join = tokio::spawn(async move {
            let exit = tokio::select! {
                biased;
                () = token.cancelled() => {
                    // Cancel server-side (best-effort) and report Canceled.
                    let _ = client.cancel_build(&org, &pipeline, number).await;
                    return Ok(BuildOutcome {
                        build: build_ref,
                        status: BuildStatus::Canceled,
                        steps: vec![],
                        started_at: started,
                        finished_at: chrono::Utc::now(),
                        watch_url,
                    });
                }
                r = crate::cloud::watch::watch_build(&client, &api_base, &org, &pipeline, number, tx) => {
                    r.map_err(|e| BackendError::LogStream(e.to_string()))?
                }
            };
            let status = if exit == 0 {
                BuildStatus::Passed
            } else {
                BuildStatus::Failed
            };
            Ok(BuildOutcome {
                build: build_ref,
                status,
                // TODO(v1 follow-up): collect per-step summaries from the
                // `StepEnd` events `watch_build` already emits.
                steps: vec![],
                started_at: started,
                finished_at: chrono::Utc::now(),
                watch_url,
            })
        });
        Ok(BackendHandle::spawn(rx, cancel, join))
    }
}

/// Map the SDK error onto the backend-boundary error (the CLI maps THIS to the
/// project error doctrine). Exhaustive over [`HarmontError`].
fn map_harmont_err(e: HarmontError) -> BackendError {
    match e {
        HarmontError::Unauthorized => BackendError::Unauthorized,
        HarmontError::Api {
            status: _,
            code,
            message,
        } => BackendError::Rejected { code, message },
        HarmontError::NotFound(w) => BackendError::NotFound(w),
        HarmontError::Transport(m) => BackendError::Transport(m),
        HarmontError::Decode(m) | HarmontError::LogStream(m) => BackendError::LogStream(m),
    }
}
