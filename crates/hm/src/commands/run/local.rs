use anyhow::{Context, Result};

use super::render::{ToolPaths, list_pipelines, render_pipeline_json};
use crate::cli::RunArgs;
use crate::context::RunContext;
use crate::output::format::banner;

/// Execute a v0 IR pipeline locally; return the final container id.
///
/// Distinct from `handle()` — does not use the user-facing run UI.
/// Used by `hm dev up` to build deployment images from `from_=Step` chains.
///
/// # Not yet implemented
///
/// The existing orchestrator (`crate::orchestrator::run`) does not expose
/// the final container id — it commits each step to a new image tag
/// (`SnapshotRef`) and does not preserve the container after commit. To
/// implement this properly we would need to either:
///   1. Thread an optional "keep final container" flag through the
///      scheduler's `run_chain` / `StepResult` path, or
///   2. Deserialize the final `SnapshotRef` tag and use it directly as
///      the build image (skipping the commit+remove round-trip).
///
/// This is a non-trivial change to the scheduler (> 50 lines, separate
/// review). For v1, `hm dev up` callers that encounter a `from_=Step`
/// deployment receive this error and are expected to use `image=`
/// deployments instead.
///
/// # Errors
///
/// Always returns `Err` in the current stub implementation.
pub async fn run_pipeline_v0_one_shot(
    _docker: &crate::orchestrator::docker_client::DockerClient,
    _pipeline_v0: &serde_json::Value,
) -> anyhow::Result<String> {
    // STUB: wiring the local executor to return the final container id
    // requires refactoring `crate::orchestrator::scheduler::run_chain`
    // to optionally preserve the container after the last step commit.
    // That change is > 50 lines and is deferred to a dedicated task.
    // See: crates/hm/src/orchestrator/scheduler.rs run_chain() and
    // the SnapshotRef / committed_snapshot pipeline.
    Err(anyhow::anyhow!(
        "from_=Step builds not yet wired: \
         run_pipeline_v0_one_shot is a stub pending scheduler refactor. \
         Use image= in your @hm.deploy() call for v1 `hm dev up` support."
    ))
}

fn decode_plan_to_wire(bytes: &[u8]) -> anyhow::Result<hm_pipeline_ir::PipelineGraph> {
    serde_json::from_slice(bytes).map_err(|e| anyhow::anyhow!("decode pipeline JSON: {e}"))
}

/// Run a pipeline locally via Docker.
///
/// # Errors
///
/// Returns an error if the working directory cannot be resolved, no
/// pipeline slug was given when more than one is declared (or none are
/// declared), the Python DSL transpile or Scheme evaluator step fails,
/// the resulting plan does not decode, the Docker daemon is unreachable,
/// or the orchestrator surfaces an internal scheduler error. Non-zero
/// step exit codes are returned as the `i32`, not as an Err.
pub async fn handle(args: RunArgs, _ctx: RunContext) -> Result<i32> {
    let repo_root = match args.dir.clone() {
        Some(p) => p,
        None => std::env::current_dir().context("cannot determine current directory")?,
    };

    let tools = ToolPaths::discover()?;

    let slug = if let Some(s) = &args.pipeline {
        s.clone()
    } else {
        let metas = list_pipelines(&tools, &repo_root).await?;
        let slugs: Vec<String> = metas.into_iter().map(|m| m.slug).collect();
        match slugs.as_slice() {
            [only] => only.clone(),
            [] => anyhow::bail!(
                "no pipelines declared in this repo\n  \
                 hint: define one with `@hm.pipeline(\"slug\")` in `.harmont/pipeline.py`"
            ),
            many => anyhow::bail!(
                "this repo declares pipelines: {}\n  → pass one as the first argument",
                many.join(", ")
            ),
        }
    };

    if args.format == "human" {
        banner("run --local", &format!("slug={slug}"));
    }

    let json = render_pipeline_json(&tools, &repo_root, &slug).await?;
    let graph = decode_plan_to_wire(&json)?;
    let parallelism = args.parallelism.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
    });
    let exit_code =
        crate::orchestrator::run(graph, repo_root, parallelism, args.format.clone())
            .await?;
    Ok(exit_code)
}
