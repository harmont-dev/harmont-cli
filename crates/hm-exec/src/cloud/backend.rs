//! The cloud [`ExecutionBackend`]: archive the worktree, submit the whole build
//! to Harmont Cloud, and watch it to completion. The server schedules and runs;
//! this backend is an *observer* (see [`Capabilities::cloud`]).

use harmont_cloud::{HarmontClient, HarmontError, builds::NewBuild};
use hm_plugin_protocol::events::{BuildEvent, BuildRef};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use std::path::Path;

use crate::{
    BackendError, BackendHandle, BuildOutcome, BuildStatus, Capabilities, ExecutionBackend, Result,
    RunRequest,
};

/// Soft warning threshold for the (compressed) source archive. Above this we
/// nudge the user toward a `.gitignore` fix but still upload.
const ARCHIVE_WARN_BYTES: u64 = 4 * 1024 * 1024;

/// Hard cap for the (compressed) source archive. The build request base64-
/// encodes this blob into a single JSON POST; base64 inflates by ~4/3, and the
/// backend's JSON body limit is ~8 MB, so a 6 MiB raw cap keeps the encoded
/// body (plus the IR + envelope) comfortably under that limit. Over the cap we
/// fail fast BEFORE the upload.
const ARCHIVE_CAP_BYTES: u64 = 6 * 1024 * 1024;

/// Submits the whole build to Harmont Cloud and watches it; the server schedules.
#[derive(Debug)]
pub struct CloudBackend {
    client: HarmontClient,
    /// API base used as the SSE log stream host during `watch_build`.
    api_base: String,
    /// Dashboard (SPA) base used to build the human-clickable watch URL. This
    /// is the `app.` host, NOT `api.` — a link built from `api_base` lands on
    /// raw JSON. Resolved via [`hm_config::app_url`] at the call site.
    app_base: String,
    org: String,
}

impl CloudBackend {
    /// Construct a `CloudBackend`.
    ///
    /// `client`/`api_base` come from the CLI's resolved cloud settings;
    /// `app_base` is the dashboard host the watch URL points at (see the field
    /// docs); `org` is the resolved organization slug.
    #[must_use]
    // `api_base` (the API host) and `app_base` (the dashboard host) are two
    // distinct hosts that must not be confused — the whole point of this fix —
    // so the near-identical names are deliberate and documented.
    #[allow(clippy::similar_names)]
    pub const fn new(
        client: HarmontClient,
        api_base: String,
        app_base: String,
        org: String,
    ) -> Self {
        Self {
            client,
            api_base,
            app_base,
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

        // Guard the upload size BEFORE the POST: warn when large, fail fast over
        // the cap (so the user never waits on a doomed upload), and always show
        // the size so the upload isn't a silent gulf of evaluation.
        guard_archive_size(source_tgz.len(), &req.repo_root)?;

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

        // Build the dashboard URL from the app host (NOT the API host) and the
        // SPA route shape `/:orgSlug/pipelines/:slug/builds/:number`. A link
        // built from `api_base` or without the `pipelines/` segment is
        // unclickable — it lands on raw JSON or a 404.
        let watch_url = Some(dashboard_build_url(
            &self.app_base,
            &self.org,
            &req.pipeline_slug,
            build.number,
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
            // Map the terminal exit code `watch_build` reports back to a
            // verdict. 130 is a server-side cancel (see
            // `watch::exit_code_for_state`) — report it as Canceled, NOT a
            // failure, mirroring the local scheduler.
            let status = match exit {
                0 => BuildStatus::Passed,
                130 => BuildStatus::Canceled,
                _ => BuildStatus::Failed,
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

/// Build the human-clickable dashboard URL for a build, matching the SPA route
/// `/:orgSlug/pipelines/:slug/builds/:number`. `app_base` is the dashboard
/// host (see [`CloudBackend`]'s `app_base` field) with no trailing slash.
fn dashboard_build_url(app_base: &str, org: &str, slug: &str, number: i64) -> String {
    format!("{app_base}/{org}/pipelines/{slug}/builds/{number}")
}

/// Render a byte count as a human "N.N MB" (mebibytes, one decimal).
fn human_mb(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // display-only; precision is irrelevant
    let mb = bytes as f64 / (1024.0 * 1024.0);
    format!("{mb:.1} MB")
}

/// Guard the source-archive upload: announce its size, warn when large, and
/// reject (fail fast) when over the cap.
///
/// `archive_bytes` is the compressed (`.tar.gz`) length actually shipped.
/// `repo_root` is only walked for the "biggest paths" hint, and only when the
/// archive is large enough to warn or reject — so the common (small) case pays
/// nothing extra.
fn guard_archive_size(archive_len: usize, repo_root: &Path) -> Result<()> {
    let bytes = archive_len as u64;

    // Always close the gulf of evaluation: a multi-second silent upload is a
    // wide gulf. Name the size up front.
    tracing::info!("uploading source archive ({})", human_mb(bytes));

    if bytes <= ARCHIVE_WARN_BYTES {
        return Ok(());
    }

    let largest = crate::local::top_level_sizes(repo_root);
    let offenders: Vec<(String, u64)> = largest.into_iter().take(3).collect();

    if bytes > ARCHIVE_CAP_BYTES {
        return Err(BackendError::SourceTooLarge {
            observed_bytes: bytes,
            cap_bytes: ARCHIVE_CAP_BYTES,
            largest_paths: offenders,
        });
    }

    // Over the warn threshold but under the cap: nudge toward a .gitignore fix.
    let hint = offenders
        .iter()
        .map(|(name, sz)| format!("{name} ({})", human_mb(*sz)))
        .collect::<Vec<_>>()
        .join(", ");
    tracing::warn!(
        "source archive is {} (largest: {}). Add big build artifacts to .gitignore to speed up uploads.",
        human_mb(bytes),
        if hint.is_empty() {
            "—".to_string()
        } else {
            hint
        },
    );
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    #[test]
    fn watch_url_uses_app_host_and_pipelines_path() {
        // Mirrors hm_config::app_url(DEFAULT_API_URL) -> https://app.harmont.dev.
        assert_eq!(
            dashboard_build_url("https://app.harmont.dev", "acme", "web", 42),
            "https://app.harmont.dev/acme/pipelines/web/builds/42"
        );
    }

    #[test]
    fn archive_under_warn_passes() {
        // A tiny archive (well under the warn threshold) never walks the tree
        // and never errors; repo_root is irrelevant.
        guard_archive_size(1024, std::path::Path::new("/nonexistent")).unwrap();
    }

    #[test]
    fn archive_over_cap_fails_with_source_too_large() {
        let tmp = tempfile::tempdir().unwrap();
        let err = guard_archive_size(ARCHIVE_CAP_BYTES as usize + 1, tmp.path()).unwrap_err();
        match err {
            BackendError::SourceTooLarge {
                observed_bytes,
                cap_bytes,
                ..
            } => {
                assert_eq!(observed_bytes, ARCHIVE_CAP_BYTES + 1);
                assert_eq!(cap_bytes, ARCHIVE_CAP_BYTES);
            }
            other => panic!("expected SourceTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn human_mb_formats_one_decimal() {
        assert_eq!(human_mb(6 * 1024 * 1024), "6.0 MB");
        assert_eq!(human_mb(1536 * 1024), "1.5 MB");
    }
}
