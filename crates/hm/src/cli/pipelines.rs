use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use hm_dsl_engine::{DslEngine, detect, python_engine};

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
pub async fn run(args: PipelinesArgs) -> Result<()> {
    use hm_core::WorkspaceLoadError as WsErr;
    let workspace = match hm_core::Workspace::resolve(args.dir.as_deref()) {
        Ok(w) => w,
        // "Not a harmont project" is not an error here — it means this repo
        // declares no pipelines. A malformed `.hm/config.toml` still fails
        // loudly rather than masquerading as an empty repo.
        Err(WsErr::NotFound | WsErr::InvalidPath(_) | WsErr::InvalidWorkspace(_)) => {
            print!("{EMPTY_ENVELOPE}");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    let repo_root = workspace.path().as_path().to_path_buf();

    if !detect::has_pipeline_files(&repo_root) {
        print!("{EMPTY_ENVELOPE}");
        return Ok(());
    }

    detect::check_python(&repo_root).context("detecting pipeline language")?;
    let engine = python_engine().context("initializing DSL engine")?;
    let json = engine
        .registry_json(&repo_root)
        .await
        .context("dumping pipeline registry")?;

    // Machine-facing: raw envelope JSON on stdout, nothing else.
    print!("{json}");
    Ok(())
}
