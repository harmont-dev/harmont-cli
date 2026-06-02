use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use hm_dsl_engine::{detect, engine_for};

#[derive(Debug, Clone, Parser)]
pub struct PipelinesArgs {
    /// Source root containing `.harmont/` (defaults to cwd).
    #[arg(short, long)]
    pub dir: Option<PathBuf>,
}

/// Print the discovery envelope JSON (all pipelines) to stdout.
///
/// # Errors
///
/// Returns an error if the language can't be detected, the engine can't start,
/// or the DSL runtime fails to evaluate the pipelines.
pub async fn run(args: PipelinesArgs) -> Result<()> {
    let repo_root = match args.dir {
        Some(d) => d,
        None => std::env::current_dir().context("cannot determine current directory")?,
    };

    let lang = detect::detect_language(&repo_root).context("detecting pipeline language")?;
    let engine = engine_for(lang).context("initializing DSL engine")?;
    let json = engine
        .registry_json(&repo_root)
        .await
        .context("dumping pipeline registry")?;

    // Machine-facing: raw envelope JSON on stdout, nothing else.
    print!("{json}");
    Ok(())
}
