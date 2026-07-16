use std::io::IsTerminal;

use crate::cli::Cli;
use hm_core::Workspace;
use hm_render::OutputMode;
use thiserror::Error;

/// Failure building a [`RunContext`].
#[derive(Debug, Error)]
pub enum Error {
    /// Process current directory could not be resolved.
    #[error("cannot determine current directory")]
    CurrentDir(#[source] std::io::Error),

    /// No directory containing `.hm/` was found above the process cwd.
    #[error(
        "no harmont workspace found\n  → run from a directory that contains `.hm/`, or initialize one with `hm init`"
    )]
    NotFound,

    /// A discovered project root failed [`Workspace::load`].
    #[error(transparent)]
    Workspace(#[from] hm_core::WorkspaceLoadError),
}

/// Runtime context for commands that operate on a harmont project workspace.
///
/// After the plan-4 cloud-plugin cutover this is intentionally thin:
/// API client, credential store, and active-org resolution moved into
/// `hm-plugin-cloud`. Project config and well-known paths live on
/// [`Self::workspace`]; output mode is host-owned.
#[derive(Debug)]
pub struct RunContext {
    /// Loaded project workspace (paths + layered config).
    pub workspace: Workspace,
    /// Output mode for the residual built-in verbs (the legacy global
    /// `--format` flag was retired in plan 3; per-subcommand `--format`
    /// is the only currently-wired source, so this defaults to human).
    pub output: OutputMode,
}

impl RunContext {
    /// Build a [`RunContext`] from parsed CLI args.
    ///
    /// Discovers the project root by walking up from the current directory
    /// ([`hm_util::dirs::find_project_root`]), then loads a [`Workspace`] for
    /// that exact path. Walk-up is intentionally separate from
    /// [`Workspace::load`].
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the current directory cannot be determined, no
    /// project root is found, or workspace load fails.
    pub fn from_cli(cli: &Cli) -> Result<Self, Error> {
        let start_dir = std::env::current_dir().map_err(Error::CurrentDir)?;
        // Walk-up is separate from Workspace::load (which only validates the
        // exact path).
        let root = hm_util::dirs::find_project_root(&start_dir).ok_or(Error::NotFound)?;
        let workspace = Workspace::load(&root)?;

        let output = OutputMode::Human {
            // Single source of truth for the color/TTY rule (still honors --no-color).
            color: hm_render::color_enabled(cli.no_color),
            // Interactive prompts/spinners key off stdout being a TTY.
            interactive: std::io::stdout().is_terminal(),
        };

        Ok(Self { workspace, output })
    }
}
