use anyhow::{Context, Result};

use hm_dsl_engine::detect;

use crate::cli::RunArgs;
use crate::context::RunContext;

/// Resolve repo root, detect the DSL, select the pipeline slug, and render
/// the v0 IR JSON. Shared by local and cloud runs.
///
/// Returns `(repo_root, slug, ir_json_string)`. The JSON is returned as a
/// string (before it is decoded into a [`hm_pipeline_ir::PipelineGraph`]) so
/// the cloud path can ship it verbatim as `pipeline_ir`.
///
/// # Errors
///
/// Returns an error if the working directory cannot be resolved, no pipeline
/// slug was given when more than one is declared (or none are declared), or
/// the DSL detection / pipeline-render step fails.
pub(crate) async fn render_pipeline(
    args: &RunArgs,
    _ctx: &RunContext,
) -> Result<(std::path::PathBuf, String, String)> {
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
                 hint: define one with `@hm.pipeline(\"slug\")` in `.harmont/pipeline.py`"
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

    Ok((repo_root, slug, json_str))
}
