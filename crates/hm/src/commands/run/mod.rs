use std::collections::HashMap;

use anyhow::{Context, Result};

use hm_dsl_engine::detect;

use crate::cli::RunArgs;
use crate::context::RunContext;
use crate::error::{ErrorCategory, HmError};

/// Top-level driver for `hm run`.
///
/// Runs the local worktree on the selected execution backend: `docker`
/// (default) runs it locally on the Docker VM backend; `cloud` submits it to
/// Harmont Cloud and streams logs.
///
/// Backend resolution (flag wins over config):
/// - `--backend <name>` → that backend (`cloud`, `docker`, …)
/// - `--cloud`          → `cloud` (deprecated alias)
/// - neither            → `ctx.config.backend` (figment-layered, default `docker`)
///
/// This is a THIN driver over the `hm-exec` backends: it builds an
/// [`hm_exec::ExecutionBackend`], renders the pipeline to v0 IR once, starts
/// the build, drives its event stream through an `hm_render` renderer, owns
/// Ctrl-C, and returns the build's process exit code. Cloud authentication is
/// resolved BEFORE the (local) render work so a missing token fails fast.
///
/// # Errors
///
/// Returns a doctrine-shaped error (carrying the right process exit code) when
/// the backend rejects the build, authentication fails, the network is
/// unreachable, the local daemon is down, or the pipeline fails to render.
pub async fn handle(args: RunArgs, ctx: RunContext) -> Result<i32> {
    // 1. Build the backend. Cloud needs auth + org resolution BEFORE any
    //    (local) render work — fail fast on a missing token.
    //    Resolution: explicit --backend > legacy --cloud alias > config.backend
    //    (figment-layered default "docker").
    let backend_name = args
        .backend
        .clone()
        .or_else(|| {
            if args.cloud {
                Some("cloud".to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| ctx.config.backend.clone());

    let backend: Box<dyn hm_exec::ExecutionBackend> = if backend_name == "cloud" {
        let api_url = ctx.config.cloud.api_url.clone();
        let token = hm_config::creds::cloud_token(&api_url).context(
            "`hm run --backend cloud` requires authentication — run `hm cloud login` or set HARMONT_API_TOKEN",
        )?;
        let org = args
            .org
            .clone()
            .or_else(|| ctx.config.cloud.org.clone())
            .context("no organization — pass --org or set `[cloud] org = \"…\"` in .hm/config.toml or ~/.config/hm/config.toml")?;
        let client = harmont_cloud::HarmontClient::with_base_url(token, &api_url);
        Box::new(hm_exec::CloudBackend::new(client, api_url, org))
    } else {
        // Local execution on a hm-vm VmBackend, selected by name.
        let vm_backend: std::sync::Arc<dyn hm_vm::VmBackend> = match backend_name.as_str() {
            "docker" => std::sync::Arc::new(
                hm_vm::docker::DockerBackend::connect().map_err(|e| anyhow::anyhow!("{e:#}"))?,
            ),
            other => anyhow::bail!("unknown --backend '{other}'\n  available: docker, cloud"),
        };
        Box::new(hm_exec::LocalBackend::new(
            resolve_parallelism(&args),
            vm_backend,
        ))
    };

    // 2. Capability-driven flag validation (replaces the old silent ignoring).
    let caps = backend.capabilities();
    if args.no_watch && !caps.supports_no_watch {
        anyhow::bail!(
            "--no-watch is not supported by the {} backend",
            backend.name()
        );
    }
    if args.parallelism.is_some() && !caps.honors_parallelism {
        tracing::warn!(
            "--parallelism is ignored by the {} backend (the server schedules)",
            backend.name()
        );
    }
    if args.keep_going && !caps.honors_keep_going {
        tracing::warn!(
            "-k/--keep-going is ignored by the {} backend (the server schedules)",
            backend.name()
        );
    }

    // 3. Render + parse the plan once (shared by every backend).
    let (repo_root, slug, ir_json) = render_pipeline(&args, &ctx).await?;
    let plan = hm_exec::Plan::parse(ir_json).map_err(|e| backend_anyhow(&e))?;
    let (branch, commit) = git_metadata(&repo_root, args.branch.clone());
    let req = hm_exec::RunRequest {
        plan,
        repo_root,
        pipeline_slug: slug,
        env: parse_env(&args.env).into_iter().collect(),
        source: hm_exec::SourceMeta {
            branch,
            commit,
            message: args.message.clone(),
        },
        options: hm_exec::RunOptions {
            no_cache: false,
            timeout: None,
            watch: !args.no_watch,
            keep_going: args.keep_going,
        },
    };

    // 4. Renderer selection (unchanged): logs stream in CI or with --logs.
    let use_logs =
        args.logs || std::env::var_os("CI").is_some_and(|v| !v.is_empty()) || !hm_render::stderr_interactive();
    let renderer = hm_render::renderer_for(&args.format, ctx.output.color_enabled(), use_logs)?;

    // 5. Start, drive events, own Ctrl-C, await the outcome.
    let handle = backend.start(req).await.map_err(|e| backend_anyhow(&e))?;
    let (events, control) = handle.into_parts();
    let _ctrlc = crate::signal::install_ctrlc(control.cancel_token());
    let render = tokio::spawn(hm_render::drive_stream(renderer, events));
    let outcome = control.wait().await.map_err(|e| backend_anyhow(&e))?;
    let _ = render.await;

    Ok(outcome.status.exit_code())
}

/// Resolve local-run parallelism: the explicit `--parallelism`, else the
/// number of logical CPUs (4 as a last resort). Matches `hm run`'s prior
/// behavior exactly.
fn resolve_parallelism(args: &RunArgs) -> usize {
    args.parallelism.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
    })
}

/// Parse `KEY=VALUE` pairs into a map, dropping malformed entries.
#[must_use]
fn parse_env(pairs: &[String]) -> HashMap<String, String> {
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

/// Resolve repo root, detect the DSL, select the pipeline slug, and render
/// the v0 IR JSON. Shared by local and cloud runs.
///
/// Returns `(repo_root, slug, ir_json_string)`. The JSON is returned as a
/// string so a backend (e.g. cloud) can ship it verbatim; the driver parses
/// it into an [`hm_exec::Plan`] once.
///
/// # Errors
///
/// Returns an error if the working directory cannot be resolved, no pipeline
/// slug was given when more than one is declared (or none are declared), or
/// the DSL detection / pipeline-render step fails.
async fn render_pipeline(
    args: &RunArgs,
    _ctx: &RunContext,
) -> Result<(std::path::PathBuf, String, String)> {
    let repo_root = match args.dir.clone() {
        Some(p) => p,
        None => std::env::current_dir().context("cannot determine current directory")?,
    };

    let lang =
        detect::detect_language(&repo_root).map_err(|e| HmError::DslEngine(format!("{e:#}")))?;
    let engine =
        hm_dsl_engine::engine_for(lang).map_err(|e| HmError::DslEngine(format!("{e:#}")))?;

    let slug = if let Some(s) = &args.pipeline {
        s.clone()
    } else {
        let metas: Vec<hm_dsl_engine::PipelineMeta> = engine
            .list_pipelines(&repo_root)
            .await
            .map_err(|e| HmError::PipelineRender(format!("{e:#}")))?;
        let slugs: Vec<String> = metas.into_iter().map(|m| m.slug).collect();
        match slugs.as_slice() {
            [only] => only.clone(),
            [] => anyhow::bail!(
                "no pipelines declared in this repo\n  \
                 hint: define one with `@hm.pipeline(\"slug\")` in `.hm/pipeline.py`"
            ),
            many => anyhow::bail!(
                "this repo declares pipelines: {}\n  → pass one as the first argument",
                many.join(", ")
            ),
        }
    };

    let json_str = engine
        .render_pipeline_json(&repo_root, &slug)
        .await
        .map_err(|e| HmError::PipelineRender(format!("{e:#}")))?;

    Ok((repo_root, slug, json_str))
}

/// Convert an [`hm_exec::BackendError`] into an [`anyhow::Error`] that carries
/// BOTH the doctrine message ([`explain`]) AND the right process exit code.
///
/// The exit code is preserved by wrapping in [`HmError::Backend`], whose
/// [`HmError::category`] returns the embedded [`ErrorCategory`]; `main`'s
/// `handle_error` downcasts to `HmError` and reads `exit_code()`.
fn backend_anyhow(err: &hm_exec::BackendError) -> anyhow::Error {
    HmError::Backend(explain(err), exit_category(err)).into()
}

/// Map a [`hm_exec::BackendError`] to the process exit-code category.
///
/// Note: the old taxonomy distinguished a downed Docker daemon
/// (`EXIT_NETWORK`) from an unknown-runner pipeline error
/// (`EXIT_PIPELINE_INVALID`). Both now arrive as
/// [`hm_exec::BackendError::Local`], so they collapse to a single category
/// (`Network`) here — an acceptable loss of resolution.
const fn exit_category(err: &hm_exec::BackendError) -> ErrorCategory {
    use hm_exec::BackendError as E;
    match err {
        // A plan/IR rejection is a pipeline-config problem.
        E::Rejected { .. } => ErrorCategory::PipelineInvalid,
        // Auth failures map to the dedicated auth exit code.
        E::Unauthorized => ErrorCategory::Auth,
        // Network unreachability and local-infra failures (Docker down) are
        // both "the thing that runs builds isn't reachable".
        E::Transport(_) | E::Local(_) => ErrorCategory::Network,
        // A NotFound is an API-level miss (bad org/pipeline/build).
        E::NotFound(_) => ErrorCategory::Api,
        // Everything else (interrupted log streams, opaque errors, and any
        // future `#[non_exhaustive]` variant) is a build-level failure.
        _ => ErrorCategory::BuildFailed,
    }
}

/// Render a [`hm_exec::BackendError`] in the project's error doctrine: point
/// precisely, say what was observed, say the fix, give a stable code + doc URL.
///
/// Adapted from the legacy `executor/cloud.rs::explain(&HarmontError)`.
fn explain(err: &hm_exec::BackendError) -> String {
    use hm_exec::BackendError as E;
    match err {
        E::Unauthorized => "\
error[auth_required]: not authenticated
  fix    run `hm cloud login` (or set HARMONT_API_TOKEN)
  docs   https://harmont.dev/docs/errors/auth_required"
            .to_string(),
        E::Rejected { code, message } => format!(
            "\
error[{code}]: {message}
  fix    fix the pipeline and re-run `hm run`
  docs   https://harmont.dev/docs/errors/{code}"
        ),
        E::NotFound(what) => format!(
            "\
error[not_found]: {what}
  fix    check the org, pipeline, and build number are correct
  docs   https://harmont.dev/docs/errors/not_found"
        ),
        E::Transport(m) => format!(
            "\
error[network]: {m}
  fix    check your connection and the API URL (HARMONT_API_URL)
  docs   https://harmont.dev/docs/errors/network"
        ),
        E::LogStream(m) => format!(
            "\
error[log_stream]: live logs interrupted — {m}
  fix    the build continues; re-attach with `hm cloud build show`
  docs   https://harmont.dev/docs/errors/log_stream"
        ),
        E::Local(m) => format!(
            "\
error[local]: {m}
  fix    check that the Docker daemon is running (`docker version`)
  docs   https://harmont.dev/docs/errors/local"
        ),
        other => format!(
            "\
error[backend]: {other}
  docs   https://harmont.dev/docs/errors/backend"
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
    fn explain_carries_stable_codes_and_docs() {
        use hm_exec::BackendError as E;
        assert!(explain(&E::Unauthorized).contains("error[auth_required]"));
        assert!(explain(&E::NotFound("x".into())).contains("error[not_found]"));
        assert!(explain(&E::LogStream("x".into())).contains("error[log_stream]"));
        assert!(explain(&E::Transport("x".into())).contains("error[network]"));
        assert!(explain(&E::Local("x".into())).contains("error[local]"));
        let r = explain(&E::Rejected {
            code: "invalid_ir".into(),
            message: "bad IR".into(),
        });
        assert!(r.contains("error[invalid_ir]") && r.contains("bad IR"));
        for s in [
            explain(&E::Unauthorized),
            explain(&E::NotFound("x".into())),
            explain(&E::Transport("x".into())),
            explain(&E::Local("x".into())),
        ] {
            assert!(s.contains("docs   https://harmont.dev/docs/errors/"));
        }
    }

    #[test]
    fn exit_category_preserves_taxonomy() {
        use hm_exec::BackendError as E;
        assert_eq!(
            exit_category(&E::Rejected {
                code: "invalid_ir".into(),
                message: String::new()
            }),
            ErrorCategory::PipelineInvalid
        );
        assert_eq!(exit_category(&E::Unauthorized), ErrorCategory::Auth);
        assert_eq!(
            exit_category(&E::Transport("x".into())),
            ErrorCategory::Network
        );
        assert_eq!(exit_category(&E::Local("x".into())), ErrorCategory::Network);
        assert_eq!(exit_category(&E::NotFound("x".into())), ErrorCategory::Api);
    }
}
