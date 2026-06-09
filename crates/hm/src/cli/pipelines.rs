use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use hm_dsl_engine::{detect, engine_for};

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
/// A repo with no `.hm/` directory (or one with no `.py`/`.ts` files)
/// declares no pipelines and yields the empty envelope rather than an error —
/// the backend fans discovery out across every repo in an installation, most of
/// which carry no pipelines. When both Python and TypeScript are present, Python
/// wins (the registry dump is Python-only today).
///
/// # Errors
///
/// Returns an error if the engine can't start or the DSL runtime fails to
/// evaluate the pipelines.
pub async fn run(args: PipelinesArgs) -> Result<()> {
    let repo_root = match args.dir {
        Some(d) => d,
        None => std::env::current_dir().context("cannot determine current directory")?,
    };

    if !detect::has_pipeline_files(&repo_root) {
        print!("{EMPTY_ENVELOPE}");
        return Ok(());
    }

    let lang =
        detect::detect_language_python_first(&repo_root).context("detecting pipeline language")?;
    let engine = engine_for(lang).context("initializing DSL engine")?;
    let json = engine
        .registry_json(&repo_root)
        .await
        .context("dumping pipeline registry")?;

    // Machine-facing: raw envelope JSON on stdout, nothing else.
    print!("{json}");
    Ok(())
}
