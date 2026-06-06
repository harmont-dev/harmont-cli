use anyhow::{Context, Result};

use crate::cli::RunArgs;
use crate::context::RunContext;
use crate::executor::{CloudExecutor, Executor, LocalExecutor, Rendered, parse_env};

mod local;

use local::render_pipeline;

/// Top-level dispatcher for `hm run`. Local by default; `--cloud` uploads the
/// worktree to Harmont Cloud and streams logs.
///
/// Both paths share one shape: pick a `Box<dyn Executor>`, render the pipeline
/// to v0 IR, select an `hm_render` renderer, then `execute`. Cloud
/// authentication is resolved BEFORE the (local) render work so a missing
/// token fails fast.
///
/// # Errors
///
/// Returns Docker, pipeline-render, or scheduler errors surfaced by the
/// local orchestrator, or cloud-dispatch errors when `--cloud` is set.
pub async fn handle(args: RunArgs, ctx: RunContext) -> Result<i32> {
    // Cloud needs auth + org resolution BEFORE the (local) render work — fail fast.
    let executor: Box<dyn Executor> = if args.cloud {
        let (client, rctx) = hm_plugin_cloud::settings::client().context(
            "`hm run --cloud` requires authentication — run `hm cloud login` or set HARMONT_API_TOKEN",
        )?;
        let org = args
            .org
            .clone()
            .or_else(|| rctx.default_org.clone())
            .context("no organization — pass --org or set default_org in ~/.harmont/config.toml")?;
        Box::new(CloudExecutor::new(
            client,
            rctx.api,
            org,
            args.message.clone(),
            parse_env(&args.env),
            args.branch.clone(),
            args.no_watch,
            ctx.output.interactive(),
        ))
    } else {
        Box::new(LocalExecutor::new(resolve_parallelism(&args)))
    };

    let (repo_root, slug, ir_json) = render_pipeline(&args, &ctx).await?;
    let use_logs = args.logs || std::env::var_os("CI").is_some_and(|v| !v.is_empty());
    let renderer = hm_render::renderer_for(&args.format, ctx.output.color_enabled(), use_logs)?;

    executor
        .execute(Rendered { repo_root, slug, ir_json }, renderer)
        .await
}

/// Resolve local-run parallelism: the explicit `--parallelism`, else the
/// number of logical CPUs (4 as a last resort). Matches `hm run`'s prior
/// behavior exactly.
fn resolve_parallelism(args: &RunArgs) -> usize {
    args.parallelism.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
    })
}
