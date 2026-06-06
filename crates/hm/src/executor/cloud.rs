//! Cloud executor: uploads the rendered worktree to Harmont Cloud, submits a
//! build, and streams its events through the shared `hm_render` renderers.
use std::collections::HashMap;

use anyhow::{Context as _, Result};
use harmont_cloud::builds::NewBuild;
use hm_plugin_cloud::watch::watch_build;
use hm_render::OutputRenderer;

use crate::executor::{Executor, Rendered};

/// Runs the build on Harmont Cloud: archive + submit the worktree, then watch
/// the build to completion (unless `no_watch`).
#[derive(Debug)]
pub struct CloudExecutor {
    client: harmont_cloud::HarmontClient,
    /// API base used for SSE log streaming during `watch_build`.
    api_base: String,
    org: String,
    message: Option<String>,
    env: HashMap<String, String>,
    branch_override: Option<String>,
    no_watch: bool,
    /// Gates the upload spinner (interactive TTY only).
    interactive: bool,
}

impl CloudExecutor {
    /// Construct a `CloudExecutor`.
    ///
    /// `client`/`api_base` come from `hm_plugin_cloud::settings::client()`;
    /// `org` is the resolved organization; `interactive` is
    /// `ctx.output.interactive()` (controls the upload spinner).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        client: harmont_cloud::HarmontClient,
        api_base: String,
        org: String,
        message: Option<String>,
        env: HashMap<String, String>,
        branch_override: Option<String>,
        no_watch: bool,
        interactive: bool,
    ) -> Self {
        Self {
            client,
            api_base,
            org,
            message,
            env,
            branch_override,
            no_watch,
            interactive,
        }
    }
}

#[async_trait::async_trait]
impl Executor for CloudExecutor {
    async fn execute(&self, plan: Rendered, output: Box<dyn OutputRenderer>) -> Result<i32> {
        // 1. Archive + submit. In interactive mode an indeterminate spinner gives
        //    feedback during the (multi-second) archive + upload; in non-interactive
        //    mode (CI / `--format json`) we keep the quiet tracing breadcrumbs so
        //    nothing animates into a pipe.
        let uploading = if self.interactive {
            let pb = indicatif::ProgressBar::new_spinner();
            let style = indicatif::ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .unwrap_or_else(|_| indicatif::ProgressStyle::default_spinner())
                .tick_strings(&[
                    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠀",
                ]);
            pb.set_style(style);
            pb.set_message("uploading worktree…");
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
            Some(pb)
        } else {
            tracing::info!("archiving worktree…");
            None
        };

        // 2. Git metadata (best-effort; this is a local run, not a push).
        let (branch, commit) = git_metadata(&plan.repo_root, self.branch_override.clone());

        // 3. Archive the worktree. Clear the spinner before any error message prints.
        let source_tgz = match crate::orchestrator::source::build_archive_bytes(&plan.repo_root)
            .context("archiving the worktree")
        {
            Ok(bytes) => bytes,
            Err(e) => {
                if let Some(pb) = uploading {
                    pb.finish_and_clear();
                }
                return Err(e);
            }
        };

        if uploading.is_none() {
            tracing::info!("submitting build to {}…", self.api_base);
        }
        // 4. Submit. On a `HarmontError`, surface the project's error doctrine
        //    rather than an opaque chain (see `explain`).
        let submit = self
            .client
            .submit_build(NewBuild {
                org: self.org.clone(),
                pipeline: plan.slug.clone(),
                branch,
                commit,
                message: self.message.clone(),
                pipeline_ir: plan.ir_json,
                source_tgz,
                env: self.env.clone(),
            })
            .await;
        // 5. Clear the spinner on BOTH paths before printing anything.
        if let Some(pb) = uploading {
            pb.finish_and_clear();
        }
        let build = submit.map_err(|e| anyhow::anyhow!("{}", explain(&e)))?;

        tracing::info!(
            "Build #{} submitted ({}/{} on {})",
            build.number,
            self.org,
            plan.slug,
            self.api_base
        );

        // 6. `--no-watch`: submit and exit without streaming.
        if self.no_watch {
            return Ok(0);
        }

        // 7. Watch the build to completion, rendering through the shared
        //    `hm-render` renderers. `watch_build` emits `BuildEvent`s on a
        //    channel; `drive` consumes them with the renderer chosen by the
        //    caller (`--format json` → JsonRenderer; interactive →
        //    ProgressRenderer; non-TTY → HumanRenderer).
        let (tx, rx) = tokio::sync::mpsc::channel(1024);
        let driver = tokio::spawn(hm_render::drive(output, rx));

        let code = watch_build(
            &self.client,
            &self.api_base,
            &self.org,
            &plan.slug,
            build.number,
            tx,
        )
        .await
        // Re-render a wrapped `HarmontError` (log-token mint, status polls,
        // stream setup) in the error doctrine; pass other anyhow chains through.
        .map_err(|e| {
            e.downcast_ref::<harmont_cloud::HarmontError>()
                .map(|he| anyhow::anyhow!("{}", explain(he)))
                .unwrap_or(e)
        })?;
        let _ = driver.await;
        Ok(code)
    }
}

/// Parse `KEY=VALUE` pairs into a map, dropping malformed entries.
#[must_use]
pub fn parse_env(pairs: &[String]) -> HashMap<String, String> {
    pairs
        .iter()
        .filter_map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// Resolve `(branch, commit)` from git at `root`, best-effort. An explicit
/// `branch_override` wins; missing values fall back to `HEAD` / the zero SHA.
fn git_metadata(root: &std::path::Path, branch_override: Option<String>) -> (String, String) {
    let run = |a: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(a)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let branch = branch_override
        .or_else(|| run(&["rev-parse", "--abbrev-ref", "HEAD"]))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "HEAD".to_string());
    let commit = run(&["rev-parse", "HEAD"])
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "0".repeat(40));
    (branch, commit)
}

/// Render a `HarmontError` in the project's error doctrine: point precisely,
/// say what was observed, say the fix, give a stable code + doc URL.
fn explain(err: &harmont_cloud::HarmontError) -> String {
    use harmont_cloud::HarmontError as E;
    match err {
        E::Unauthorized => "\
error[auth_required]: not authenticated
  fix    run `hm cloud login` (or set HARMONT_API_TOKEN)
  docs   https://harmont.dev/docs/errors/auth_required"
            .to_string(),
        E::Api { status, code, message } => format!(
            "\
error[{code}]: {message}
  status {status}
  docs   https://harmont.dev/docs/errors/{code}"
        ),
        E::NotFound(what) => format!(
            "\
error[not_found]: {what}
  fix    check the org, pipeline, and build number are correct
  docs   https://harmont.dev/docs/errors/not_found"
        ),
        E::LogStream(m) => format!(
            "\
error[log_stream]: live logs interrupted — {m}
  fix    the build continues; re-attach with `hm cloud build show`
  docs   https://harmont.dev/docs/errors/log_stream"
        ),
        E::Transport(m) => format!(
            "\
error[network]: {m}
  fix    check your connection and the API URL (HARMONT_API_URL)
  docs   https://harmont.dev/docs/errors/network"
        ),
        E::Decode(m) => format!(
            "\
error[decode]: unexpected response from the API — {m}
  fix    update `hm` (the API contract may have changed) or report a bug
  docs   https://harmont.dev/docs/errors/decode"
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_splits_pairs() {
        let m = parse_env(&["A=1".into(), "B=x=y".into(), "bad".into()]);
        assert_eq!(m.get("A").unwrap(), "1");
        assert_eq!(m.get("B").unwrap(), "x=y");
        assert!(!m.contains_key("bad"));
    }

    #[test]
    fn git_metadata_falls_back_outside_repo() {
        let (b, c) = git_metadata(std::path::Path::new("/"), None);
        assert!(!b.is_empty() && !c.is_empty());
        assert_eq!(c.len(), 40); // zero-sha fallback
    }

    #[test]
    fn stable_codes_present() {
        use harmont_cloud::HarmontError as E;
        assert!(explain(&E::Unauthorized).contains("error[auth_required]"));
        assert!(explain(&E::NotFound("x".into())).contains("error[not_found]"));
        assert!(explain(&E::LogStream("x".into())).contains("error[log_stream]"));
        assert!(explain(&E::Transport("x".into())).contains("error[network]"));
        assert!(explain(&E::Decode("x".into())).contains("error[decode]"));
        let api = explain(&E::Api {
            status: 422,
            code: "build_rejected".into(),
            message: "bad".into(),
        });
        assert!(api.contains("error[build_rejected]"));
        assert!(api.contains("422"));
        assert!(api.contains("bad"));
    }

    #[test]
    fn every_variant_has_docs() {
        use harmont_cloud::HarmontError as E;
        for s in [
            explain(&E::Unauthorized),
            explain(&E::NotFound("x".into())),
            explain(&E::LogStream("x".into())),
            explain(&E::Transport("x".into())),
            explain(&E::Decode("x".into())),
        ] {
            assert!(s.contains("docs   https://harmont.dev/docs/errors/"));
        }
    }
}
