use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use hm_core::app_ctx::AppCtx;
use hm_dsl_engine::{DslEngine, SubprocessPythonEngine, detect};

#[derive(Debug, Clone, Parser)]
pub struct PipelinesArgs {
    /// Source root containing `.hm/` (defaults to cwd).
    #[arg(short, long)]
    pub dir: Option<PathBuf>,
}

/// Empty discovery envelope, emitted when a repo declares no pipelines. Mirrors
/// the shape of `harmont.dump_registry_json()` so backend discovery parses it
/// the same way (it reads only the `pipelines` array).
const EMPTY_ENVELOPE: &str = r#"{"schema_version":"1","pipelines":[]}"#;

/// Print the discovery envelope JSON (all pipelines) to stdout.
///
/// A repo with no `.hm/` directory (or one with no `.py` files)
/// declares no pipelines and yields the empty envelope rather than an error —
/// the backend fans discovery out across every repo in an installation, most of
/// which carry no pipelines.
///
/// # Errors
///
/// Returns an error if the engine can't start or the DSL runtime fails to
/// evaluate the pipelines.
pub async fn run(args: PipelinesArgs, app: &AppCtx) -> Result<()> {
    let repo_root = match args.dir {
        Some(d) => d,
        None => std::env::current_dir().context("cannot determine current directory")?,
    };

    if !detect::has_pipeline_files(&repo_root) {
        print!("{EMPTY_ENVELOPE}");
        return Ok(());
    }

    detect::check_python(&repo_root).context("detecting pipeline language")?;
    let engine = SubprocessPythonEngine::new(app);
    let json = engine
        .registry_json(&repo_root)
        .await
        .context("dumping pipeline registry")?;

    // Machine-facing: raw envelope JSON on stdout, nothing else.
    print!("{json}");
    Ok(())
}
