use std::sync::Arc;

use anyhow::{Context, Result};

use hm_dsl_engine::detect;

use crate::cli::RunArgs;
use crate::context::RunContext;
use crate::runner::{RunnerRegistry, docker::DockerRunner};

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
pub async fn handle(args: RunArgs, ctx: RunContext) -> Result<i32> {
    let repo_root = match args.dir.clone() {
        Some(p) => p,
        None => std::env::current_dir().context("cannot determine current directory")?,
    };

    let lang = detect::detect_language(&repo_root)
        .map_err(|e| crate::error::HmError::DslEngine(format!("{e:#}")))?;
    let engine = hm_dsl_engine::engine_for(lang)
        .map_err(|e| crate::error::HmError::DslEngine(format!("{e:#}")))?;

    let slug = if let Some(s) = &args.pipeline {
        s.clone()
    } else {
        let metas: Vec<hm_dsl_engine::PipelineMeta> = engine
            .list_pipelines(&repo_root)
            .await
            .map_err(|e| crate::error::HmError::PipelineRender(format!("{e:#}")))?;
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
        .map_err(|e| crate::error::HmError::PipelineRender(format!("{e:#}")))?;
    let json = json_str.into_bytes();
    let graph = decode_plan_to_wire(&json)?;
    let parallelism = args.parallelism.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
    });

    let mut runner_registry = RunnerRegistry::new();
    runner_registry.register(Arc::new(DockerRunner), true);
    let runner_registry = Arc::new(runner_registry);

    let use_logs = args.logs || std::env::var_os("CI").is_some_and(|v| !v.is_empty());

    let renderer: Box<dyn crate::runner::OutputRenderer> = match args.format.as_str() {
        "json" => Box::new(crate::output::json::JsonRenderer::new(std::io::stdout())),
        "human" if use_logs => Box::new(crate::output::human::HumanRenderer::new(
            std::io::stderr(),
            ctx.output.color_enabled(),
        )),
        "human" => Box::new(crate::output::progress::ProgressRenderer::new(
            std::io::stderr(),
            ctx.output.color_enabled(),
        )),
        other => anyhow::bail!("unknown --format '{other}'\n  available: human, json"),
    };

    let exit_code =
        crate::orchestrator::run(graph, repo_root, parallelism, runner_registry, renderer).await?;
    Ok(exit_code)
}
