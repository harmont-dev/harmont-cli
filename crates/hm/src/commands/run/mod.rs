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
#[allow(clippy::too_many_lines)] // thin top-level driver: linear, no good split point
pub async fn handle(args: RunArgs, ctx: RunContext) -> Result<i32> {
    // 1. Resolve the backend name: explicit --backend > legacy --cloud alias >
    //    config.backend (figment-layered default "docker").
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
        .unwrap_or_else(|| ctx.config.backend.to_string());

    // 2. Cloud needs auth + org resolution up front — fail fast on a missing
    //    token before any render work. We resolve the credentials here but
    //    defer *constructing* the backend (and, for local runs, *connecting* to
    //    Docker) until after the pipeline renders, so an unknown slug or a
    //    missing/ambiguous pipeline argument fails with a helpful message
    //    instead of a daemon-connection error.
    let cloud_creds = if backend_name == "cloud" {
        let api_url = ctx.config.cloud.api_url.clone();
        let token = hm_config::creds::cloud_token(&api_url).context(
            "`hm run --backend cloud` requires authentication — run `hm cloud login` or set HM_API_TOKEN",
        )?;
        let org = args
            .org
            .clone()
            .or_else(|| ctx.config.cloud.org.clone())
            .context("no organization — pass --org or set `[cloud] org = \"…\"` in .hm/config.toml or ~/.config/hm/config.toml")?;
        Some((api_url, token, org))
    } else if backend_name != "docker" {
        anyhow::bail!("unknown --backend '{backend_name}'\n  available: docker, cloud");
    } else {
        None
    };

    // 3. Render + parse the plan once (shared by every backend). This validates
    //    the pipeline argument — unknown slug, or zero/many declared pipelines
    //    — before we connect to any daemon.
    let (repo_root, slug, ir_json) = render_pipeline(&args, &ctx).await?;
    let plan = hm_exec::Plan::parse(ir_json).map_err(|e| backend_anyhow(&e))?;

    // 4. Pick the renderer — this validates `--format` — before any daemon
    //    connection, so an unknown format fails fast without a running Docker.
    let use_logs = args.logs
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty())
        || !hm_render::stderr_interactive();
    let renderer = hm_render::renderer_for(&args.format, ctx.output.color_enabled(), use_logs)?;

    // 5. Build the backend. For local runs this is where we connect to Docker.
    // For cloud runs, keep a cloned client + org so a `pipeline_not_found` on
    // the first submit can create the pipeline and retry. `None` for local.
    let mut autocreate_client: Option<(harmont_cloud::HarmontClient, String)> = None;
    let backend: Box<dyn hm_exec::ExecutionBackend> =
        if let Some((api_url, token, org)) = cloud_creds {
            let client = harmont_cloud::HarmontClient::with_base_url(token, &api_url);
            autocreate_client = Some((client.clone(), org.clone()));
            // The watch link must point at the dashboard (app.) host, not the
            // API host — a link built from `api_url` lands on raw JSON.
            let app_url = hm_config::app_url(&api_url, std::env::var("HM_APP_URL").ok().as_deref());
            Box::new(hm_exec::CloudBackend::new(client, api_url, app_url, org))
        } else {
            // Local execution on a hm-vm VmBackend (docker).
            let vm_backend: std::sync::Arc<dyn hm_vm::VmBackend> = std::sync::Arc::new(
                hm_vm::docker::DockerBackend::connect().map_err(|e| anyhow::anyhow!("{e:#}"))?,
            );
            Box::new(hm_exec::LocalBackend::new(
                resolve_parallelism(&args),
                vm_backend,
            ))
        };

    // 6. Capability-driven flag validation (replaces the old silent ignoring).
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

    // 7. Assemble the run request.
    let (branch, commit) = git_metadata(&repo_root, args.branch.clone());
    let repo_name = git_remote_repo_name(&repo_root);
    let mut req = hm_exec::RunRequest {
        plan,
        repo_root,
        pipeline_slug: slug,
        env: parse_env(&args.env).into_iter().collect(),
        source: hm_exec::SourceMeta {
            branch,
            commit,
            message: args.message.clone(),
            repo_name,
        },
        options: hm_exec::RunOptions {
            no_cache: false,
            timeout: None,
            watch: !args.no_watch,
            keep_going: args.keep_going,
        },
        cloud_pipeline_slug: None,
    };

    // Cloud target resolution (before the first submit): a persisted pipeline
    // slug wins; otherwise a remoteless worktree (no git remote) is registered
    // interactively now and its slug persisted. A worktree WITH a remote falls
    // through to the repo-identity submit + get-or-create fallback below.
    if let Some((client, org)) = autocreate_client.as_ref() {
        if let Some(slug) = ctx.config.cloud.pipeline.clone() {
            req.cloud_pipeline_slug = Some(slug);
        } else if req.source.repo_name.is_none() {
            let default_branch =
                git_default_branch(&req.repo_root).unwrap_or_else(|| req.source.branch.clone());
            let slug = register_remoteless_pipeline(
                client,
                org,
                &req.pipeline_slug,
                &req.repo_root,
                &default_branch,
            )
            .await?;
            req.cloud_pipeline_slug = Some(slug);
        }
    }

    // Cloud-only auto-create context. Borrow `req` here (before it's moved into
    // `start`): the repository URL and default branch come from the worktree's
    // git remote; the pipeline name is the in-repo source slug.
    let autocreate = autocreate_client.map(|(client, org)| AutoCreate {
        client,
        org,
        repo_name: req.source.repo_name.clone(),
        repository: git_remote_url(&req.repo_root),
        name: req.pipeline_slug.clone(),
        default_branch: git_default_branch(&req.repo_root)
            .unwrap_or_else(|| req.source.branch.clone()),
    });

    // 8. Start, drive events, own Ctrl-C, await the outcome.
    // Submit. If the pipeline doesn't exist yet, resolve-or-create it and retry
    // by submitting directly to its global slug (the repo-identity path can't
    // see API-created pipelines). Clone the request up front so the retry has
    // its own copy (the first `start` consumes it).
    let req_retry = req.clone();
    let handle = match backend.start(req).await {
        Ok(handle) => handle,
        Err(err) => match resolve_or_create_cloud_pipeline(&err, autocreate.as_ref()).await? {
            Some(slug) => {
                let mut retry = req_retry;
                retry.cloud_pipeline_slug = Some(slug);
                backend.start(retry).await.map_err(|e| backend_anyhow(&e))?
            }
            None => return Err(backend_anyhow(&err)),
        },
    };
    let (events, control) = handle.into_parts();
    let _ctrlc = crate::signal::install_ctrlc(control.cancel_token());
    let render = tokio::spawn(hm_render::drive_stream(renderer, events));
    let outcome = control.wait().await.map_err(|e| backend_anyhow(&e))?;
    let _ = render.await;

    Ok(outcome.status.exit_code())
}

/// Resolve local-run parallelism: the explicit `--parallelism`, else the
/// number of logical CPUs (4 as a last resort). Matches `hm run`'s prior
/// behavior exactly. A `--parallelism 0` is clamped to `1` at this boundary
/// so the backend never has to defend against a zero count.
fn resolve_parallelism(args: &RunArgs) -> std::num::NonZeroUsize {
    use std::num::NonZeroUsize;
    /// Last-resort parallelism when neither `--parallelism` nor
    /// `available_parallelism()` yields a usable value.
    const FALLBACK: NonZeroUsize = NonZeroUsize::new(4).unwrap();
    args.parallelism.map_or_else(
        || std::thread::available_parallelism().unwrap_or(FALLBACK),
        |n| NonZeroUsize::new(n).unwrap_or(NonZeroUsize::MIN),
    )
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

/// Parse `owner/repo` from a git remote URL, mirroring the backend's
/// `Harmont.Pipelines.RepoName`: drop scheme/host and a trailing `.git`, then
/// take the last two non-empty path segments. `None` when fewer than two
/// segments remain.
fn parse_repo_name(url: &str) -> Option<String> {
    let url = url.trim();
    let path = if let Some((_, rest)) = url.split_once("://") {
        // scheme://host/owner/repo  → strip host
        rest.split_once('/').map_or(rest, |(_, p)| p)
    } else if url.contains('@') && url.contains(':') {
        // scp-style git@host:owner/repo → strip "git@host:"
        let after_at = url.split_once('@').map_or(url, |(_, r)| r);
        after_at.split_once(':').map_or(after_at, |(_, p)| p)
    } else {
        url.split_once('/').map_or(url, |(_, p)| p)
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return None;
    }
    Some(segs[segs.len() - 2..].join("/"))
}

/// Extract the default branch name from a `git symbolic-ref
/// refs/remotes/origin/HEAD` result (e.g. `refs/remotes/origin/main` → `main`).
/// `None` when the line is empty or lacks the expected prefix.
fn parse_default_branch(symbolic_ref: &str) -> Option<String> {
    let branch = symbolic_ref.trim().strip_prefix("refs/remotes/origin/")?;
    (!branch.is_empty()).then(|| branch.to_string())
}

/// The worktree's raw `origin` remote URL (the pipeline's `repository`).
fn git_remote_url(root: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!url.is_empty()).then_some(url)
}

/// The repo's default branch, from `origin/HEAD`. `None` when `origin/HEAD`
/// isn't set (common on fresh clones without `git remote set-head`).
fn git_default_branch(root: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    parse_default_branch(&String::from_utf8_lossy(&out.stdout))
}

/// Best-effort `owner/repo` from the worktree's `origin` remote.
fn git_remote_repo_name(root: &std::path::Path) -> Option<String> {
    parse_repo_name(&git_remote_url(root)?)
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

/// The server's structured error code for "no pipeline matches this
/// `(repo_name, source_slug)`" — the signal that `hm run --cloud` is the
/// repo's first cloud build and the pipeline must be created.
const PIPELINE_NOT_FOUND_CODE: &str = "pipeline_not_found";

/// Whether a backend error means "the pipeline doesn't exist yet". The cloud
/// submit path surfaces this as a structured `Rejected { code }` (current SDK);
/// we also accept an opaque `NotFound` body carrying the code, for robustness
/// against older servers that took the un-structured 404 path.
fn is_missing_pipeline(err: &hm_exec::BackendError) -> bool {
    use hm_exec::BackendError as E;
    match err {
        E::Rejected { code, .. } => code == PIPELINE_NOT_FOUND_CODE,
        E::NotFound(body) => body.contains(PIPELINE_NOT_FOUND_CODE),
        _ => false,
    }
}

/// Build the create-pipeline request body. `name` is the in-repo source slug
/// (so the server's derived global slug matches what the retry submits);
/// `repo_name` (`owner/repo`) is passed explicitly so the server need not
/// re-parse the clone URL.
fn build_create_pipeline_request(
    name: &str,
    default_branch: &str,
    repository: &str,
    repo_name: Option<&str>,
) -> harmont_cloud::types::CreatePipelineRequest {
    harmont_cloud::types::CreatePipelineRequest {
        default_branch: default_branch.to_string(),
        description: None,
        name: name.to_string(),
        repo_name: repo_name.map(str::to_string),
        repository: repository.to_string(),
    }
}

/// Merge `backend = "cloud"` and `[cloud] org/pipeline = …` into the project's
/// `.hm/config.toml`, preserving any other keys already in the file. Creates
/// the file (and `.hm/`) when absent. Used after registering a remoteless
/// directory so later runs submit by the persisted slug without prompting.
fn persist_project_pipeline(dir: &std::path::Path, org: &str, slug: &str) -> Result<()> {
    let path = dir.join(".hm/config.toml");
    let mut doc: toml::Table = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();
    doc.insert("backend".into(), toml::Value::String("cloud".into()));
    let cloud = doc
        .entry("cloud".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let Some(t) = cloud.as_table_mut() {
        t.insert("org".into(), toml::Value::String(org.to_string()));
        t.insert("pipeline".into(), toml::Value::String(slug.to_string()));
    }
    let serialized = toml::to_string_pretty(&doc).context("serializing .hm/config.toml")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, serialized).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Register the in-repo pipeline (`pipeline_name`, the `@hm.pipeline("…")` slug
/// from the `.py`/`.ts`) with Harmont when there's no git remote to identify
/// its repository. Reuses an existing pipeline of that name; otherwise prompts
/// the user for the repository name (`owner/repo`, default: the directory name)
/// and creates it. Persists the resolved slug to `.hm/config.toml` so later
/// runs submit by slug without prompting. Returns the org-global slug.
async fn register_remoteless_pipeline(
    client: &harmont_cloud::HarmontClient,
    org: &str,
    pipeline_name: &str,
    dir: &std::path::Path,
    default_branch: &str,
) -> Result<String> {
    use std::io::IsTerminal;

    // Reuse the pipeline if it already exists (resolved by its slug, which is
    // derived from the in-repo name); otherwise create it, asking for the repo
    // identity we can't read from a (missing) git remote.
    let slug = match client.raw().get_pipeline(org, pipeline_name).await {
        Ok(p) => p.into_inner().slug,
        Err(e) if e.status().is_some_and(|s| s.as_u16() == 404) => {
            let dirname = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("repo")
                .to_string();
            let repo_name = if std::io::stdin().is_terminal() {
                // Propagate a genuine prompt I/O error rather than silently
                // creating a pipeline under the default name — a create is a
                // side effect we shouldn't perform on an interrupted read.
                // (Hitting Enter accepts the default and returns `Ok`, so this
                // only fires on a real failure.)
                dialoguer::Input::<String>::new()
                    .with_prompt(format!(
                        "No git remote — register pipeline '{pipeline_name}'. Repository name (owner/repo)"
                    ))
                    .default(dirname)
                    .interact_text()
                    .context("reading the repository name")?
            } else {
                tracing::info!(
                    "no git remote — registering pipeline '{pipeline_name}' for repo '{dirname}' in org {org}"
                );
                dirname
            };
            let body = build_create_pipeline_request(
                pipeline_name,
                default_branch,
                &repo_name,
                Some(&repo_name),
            );
            let created = client
                .raw()
                .create_pipeline(org, &body)
                .await
                .map_err(hm_plugin_cloud::settings::map_raw)
                .with_context(|| format!("registering pipeline '{pipeline_name}' in org {org}"))?;
            created.into_inner().slug
        }
        Err(e) => {
            return Err(hm_plugin_cloud::settings::map_raw(e))
                .with_context(|| format!("looking up pipeline '{pipeline_name}' in org {org}"));
        }
    };

    persist_project_pipeline(dir, org, &slug).context("saving .hm/config.toml")?;
    tracing::info!("registered pipeline '{slug}' — submitting build");
    Ok(slug)
}

/// Everything the run driver needs to create a missing cloud pipeline and
/// retry the build. Built only for cloud runs; `repo_name`/`repository` are
/// `Option` because a remoteless worktree can't be auto-created (the cloud
/// backend already rejects those earlier with a clear "need a git remote"
/// message, so in practice both are `Some` whenever we reach the create path).
struct AutoCreate {
    client: harmont_cloud::HarmontClient,
    org: String,
    repo_name: Option<String>,
    repository: Option<String>,
    /// The in-repo source slug — becomes the new pipeline's `name`.
    name: String,
    default_branch: String,
}

/// On a `pipeline_not_found`, resolve the build's target pipeline and return
/// its org-global slug so the caller can retry by submitting directly to it
/// (`RunRequest::cloud_pipeline_slug`), bypassing the repo-identity resolution
/// that can't see API-created pipelines.
///
/// The pipeline may already exist from a prior `hm run` (the repo-identity path
/// can't find it, so a `pipeline_not_found` does NOT mean it's absent). So we
/// look it up by slug first and only create — after confirming on a TTY, or
/// automatically when non-interactive — when it's truly missing.
///
/// Returns `Ok(Some(slug))` to retry by that slug; `Ok(None)` when the error
/// isn't a missing pipeline, there's no auto-create context, the repo can't be
/// identified, or the user declined (caller then surfaces the original error).
/// Returns `Err` only when a lookup or create request itself failed.
async fn resolve_or_create_cloud_pipeline(
    err: &hm_exec::BackendError,
    ac: Option<&AutoCreate>,
) -> Result<Option<String>> {
    use std::io::IsTerminal;

    if !is_missing_pipeline(err) {
        return Ok(None);
    }
    let Some(ac) = ac else { return Ok(None) };
    let (Some(repo_name), Some(repository)) = (&ac.repo_name, &ac.repository) else {
        return Ok(None);
    };

    // Already created on a prior run? Look it up by slug; the repo-identity
    // submit can't see it, but `get_pipeline` (by global slug) can. This
    // assumes the org-global slug equals `ac.name` (the source slug we create
    // the pipeline under) — true for a slug-shaped name; a server-normalized
    // mismatch falls through to a clear create-collision error below.
    match ac.client.raw().get_pipeline(&ac.org, &ac.name).await {
        Ok(p) => return Ok(Some(p.into_inner().slug)),
        Err(e) if e.status().is_some_and(|s| s.as_u16() == 404) => {} // truly absent → create
        Err(e) => {
            return Err(hm_plugin_cloud::settings::map_raw(e))
                .with_context(|| format!("looking up pipeline '{}' in org {}", ac.name, ac.org));
        }
    }

    // Truly missing — confirm on a TTY, auto-create when non-interactive.
    if std::io::stdin().is_terminal() {
        let ok = dialoguer::Confirm::new()
            .with_prompt(format!(
                "No pipeline for {repo_name} in org {}. Create it?",
                ac.org
            ))
            .default(true)
            .interact()
            .unwrap_or(false);
        if !ok {
            return Ok(None);
        }
    } else {
        tracing::info!(
            "no pipeline for {repo_name} yet — creating it in org {}",
            ac.org
        );
    }

    let body =
        build_create_pipeline_request(&ac.name, &ac.default_branch, repository, Some(repo_name));
    let created = ac
        .client
        .raw()
        .create_pipeline(&ac.org, &body)
        .await
        .map_err(hm_plugin_cloud::settings::map_raw)
        .with_context(|| format!("creating pipeline '{}' in org {}", ac.name, ac.org))?;
    let slug = created.into_inner().slug;
    tracing::info!("created pipeline '{slug}' — submitting build");
    Ok(Some(slug))
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
        // An oversized source archive is a user-fixable setup mistake.
        E::SourceTooLarge { .. } => ErrorCategory::Usage,
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
  fix    run `hm cloud login` (or set HM_API_TOKEN)"
            .to_string(),
        E::Rejected { code, message } => format!(
            "\
error[{code}]: {message}
  fix    fix the pipeline and re-run `hm run`"
        ),
        E::NotFound(what) => format!(
            "\
error[not_found]: {what}
  fix    check the org, pipeline, and build number are correct"
        ),
        E::Transport(m) => format!(
            "\
error[network]: {m}
  fix    check your connection and the API URL (HM_API_URL)"
        ),
        E::LogStream(m) => format!(
            "\
error[log_stream]: live logs interrupted — {m}
  fix    the build continues; re-attach with `hm cloud build show`"
        ),
        E::Local(m) => format!("error[local]: {m}"),
        E::SourceTooLarge {
            observed_bytes,
            cap_bytes,
            largest_paths,
        } => {
            #[allow(clippy::cast_precision_loss)] // display-only
            let mb = |b: u64| format!("{:.1} MB", b as f64 / (1024.0 * 1024.0));
            let biggest = if largest_paths.is_empty() {
                "  (no large top-level paths identified)".to_string()
            } else {
                largest_paths
                    .iter()
                    .map(|(name, sz)| format!("           {name} — {}", mb(*sz)))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            format!(
                "\
error[source_too_large]: worktree archive is {observed} (cap {cap})
  biggest\n{biggest}
  fix    add the offending paths to .gitignore (build output, caches, vendored deps), then re-run `hm run`",
                observed = mb(*observed_bytes),
                cap = mb(*cap_bytes),
            )
        }
        other => format!("error[backend]: {other}"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn missing_pipeline_detected_from_structured_reject() {
        let err = hm_exec::BackendError::Rejected {
            code: "pipeline_not_found".into(),
            message: "No pipeline with that slug exists in this organization.".into(),
        };
        assert!(is_missing_pipeline(&err));
    }

    #[test]
    fn other_reject_is_not_missing_pipeline() {
        let err = hm_exec::BackendError::Rejected {
            code: "build_rejected".into(),
            message: "pipeline_ir invalid".into(),
        };
        assert!(!is_missing_pipeline(&err));
    }

    #[test]
    fn missing_pipeline_detected_from_opaque_not_found() {
        let err =
            hm_exec::BackendError::NotFound(r#"{"error":{"code":"pipeline_not_found"}}"#.into());
        assert!(is_missing_pipeline(&err));
    }

    #[test]
    fn transport_error_is_not_missing_pipeline() {
        let err = hm_exec::BackendError::Transport("connection refused".into());
        assert!(!is_missing_pipeline(&err));
    }

    #[test]
    fn parse_repo_name_handles_https_ssh_and_scp() {
        assert_eq!(
            parse_repo_name("https://github.com/harmont-dev/harmont-cli.git").as_deref(),
            Some("harmont-dev/harmont-cli")
        );
        assert_eq!(
            parse_repo_name("git@github.com:harmont-dev/harmont-cli.git").as_deref(),
            Some("harmont-dev/harmont-cli")
        );
        assert_eq!(
            parse_repo_name("ssh://git@github.com/harmont-dev/harmont-cli").as_deref(),
            Some("harmont-dev/harmont-cli")
        );
        assert_eq!(
            parse_repo_name("https://example.com/a/b/c/repo").as_deref(),
            Some("c/repo")
        );
    }

    #[test]
    fn parse_repo_name_rejects_unparseable() {
        assert_eq!(parse_repo_name(""), None);
        assert_eq!(parse_repo_name("not-a-url"), None);
    }

    #[test]
    fn parses_default_branch_from_symbolic_ref() {
        assert_eq!(
            parse_default_branch("refs/remotes/origin/main\n").as_deref(),
            Some("main")
        );
        assert_eq!(
            parse_default_branch("refs/remotes/origin/master").as_deref(),
            Some("master")
        );
    }

    #[test]
    fn default_branch_none_when_unexpected_or_empty() {
        assert_eq!(parse_default_branch(""), None);
        assert_eq!(parse_default_branch("refs/heads/main"), None);
        assert_eq!(parse_default_branch("refs/remotes/origin/"), None);
    }

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
    fn explain_carries_stable_codes() {
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
        let big = explain(&E::SourceTooLarge {
            observed_bytes: 7 * 1024 * 1024,
            cap_bytes: 6 * 1024 * 1024,
            largest_paths: vec![("node_modules".into(), 5 * 1024 * 1024)],
        });
        assert!(big.contains("error[source_too_large]"));
        // Points precisely (observed + cap), names the offender, states the fix.
        assert!(big.contains("7.0 MB") && big.contains("6.0 MB"));
        assert!(big.contains("node_modules") && big.contains(".gitignore"));
        // Doc URLs were removed (the pages 404); no error should link to them.
        for s in [
            explain(&E::Unauthorized),
            explain(&E::NotFound("x".into())),
            explain(&E::Transport("x".into())),
            explain(&E::Local("x".into())),
        ] {
            assert!(!s.contains("docs   https://harmont.dev/docs/errors/"));
        }
        // The Local arm no longer gives misleading Docker advice.
        assert!(!explain(&E::Local("archiving worktree: boom".into())).contains("Docker"));
        assert!(explain(&E::Local("archiving worktree: boom".into())).contains("error[local]"));
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
        assert_eq!(
            exit_category(&E::SourceTooLarge {
                observed_bytes: 1,
                cap_bytes: 0,
                largest_paths: vec![],
            }),
            ErrorCategory::Usage
        );
    }

    #[test]
    fn create_request_maps_fields_and_sets_repo_name() {
        let body = build_create_pipeline_request(
            "web",
            "main",
            "git@github.com:acme/my-app.git",
            Some("acme/my-app"),
        );
        assert_eq!(body.name, "web");
        assert_eq!(body.default_branch, "main");
        assert_eq!(body.repository, "git@github.com:acme/my-app.git");
        assert_eq!(body.repo_name.as_deref(), Some("acme/my-app"));
        assert!(body.description.is_none());

        // The remoteless path passes `None`: no `repo_name`, and `repository`
        // falls back to the pipeline name itself.
        let body = build_create_pipeline_request("my-app-2", "main", "my-app-2", None);
        assert!(body.repo_name.is_none());
        assert_eq!(body.repository, "my-app-2");
        assert_eq!(body.name, "my-app-2");
    }

    #[test]
    fn persist_creates_config_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        persist_project_pipeline(dir.path(), "acme", "my-app-2").unwrap();

        let raw = std::fs::read_to_string(dir.path().join(".hm/config.toml")).unwrap();
        let doc: toml::Table = toml::from_str(&raw).unwrap();
        assert_eq!(doc["backend"].as_str(), Some("cloud"));
        let cloud = doc["cloud"].as_table().unwrap();
        assert_eq!(cloud["org"].as_str(), Some("acme"));
        assert_eq!(cloud["pipeline"].as_str(), Some("my-app-2"));
    }

    #[test]
    fn persist_preserves_existing_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".hm")).unwrap();
        std::fs::write(
            dir.path().join(".hm/config.toml"),
            "backend = \"docker\"\n[cloud]\norg = \"old\"\napi_url = \"https://example.test\"\n",
        )
        .unwrap();

        persist_project_pipeline(dir.path(), "acme", "web").unwrap();

        let raw = std::fs::read_to_string(dir.path().join(".hm/config.toml")).unwrap();
        let doc: toml::Table = toml::from_str(&raw).unwrap();
        assert_eq!(doc["backend"].as_str(), Some("cloud"));
        let cloud = doc["cloud"].as_table().unwrap();
        assert_eq!(cloud["pipeline"].as_str(), Some("web"));
        assert_eq!(cloud["org"].as_str(), Some("acme"));
        // The unrelated key is preserved across the merge.
        assert_eq!(cloud["api_url"].as_str(), Some("https://example.test"));
    }
}
