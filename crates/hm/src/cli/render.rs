use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use hm_dsl_engine::{detect, engine_for};

#[derive(Debug, Clone, Parser)]
pub struct RenderArgs {
    /// Pipeline slug to render.
    #[arg()]
    pub slug: String,

    /// Source root containing `.harmont/` (defaults to cwd).
    #[arg(short, long)]
    pub dir: Option<PathBuf>,
}

/// Render one pipeline's v0 IR JSON to stdout without executing it.
///
/// When both Python and TypeScript are present, Python wins (the supported
/// backend path), matching `hm pipelines`.
///
/// # Errors
///
/// Returns an error if the language can't be detected, the engine can't start,
/// or the slug is unknown / fails to render (the available slugs are written to
/// stderr by the DSL runtime).
pub async fn run(args: RenderArgs) -> Result<()> {
    let repo_root = match args.dir {
        Some(d) => d,
        None => std::env::current_dir().context("cannot determine current directory")?,
    };

    let lang =
        detect::detect_language_python_first(&repo_root).context("detecting pipeline language")?;
    let engine = engine_for(lang).context("initializing DSL engine")?;
    let json = engine
        .render_pipeline_json(&repo_root, &args.slug)
        .await
        .with_context(|| format!("rendering pipeline {:?}", args.slug))?;

    // Machine-facing: raw v0 IR JSON on stdout, nothing else.
    print!("{json}");
    Ok(())
}
