use std::io::IsTerminal;
use std::path::Path;

use crate::cli::Cli;
use hm_core::{Workspace, WorkspaceLoadError};
use hm_render::OutputMode;

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
    /// Build a [`RunContext`] from parsed CLI args and the verb's `--dir`.
    ///
    /// Root resolution is [`Workspace::resolve`]'s, shared with every other
    /// `--dir` verb. Passing `dir` through matters: the workspace this loads is
    /// the one the run executes against, so resolving it from the cwd while the
    /// run used `--dir` would let config come from one tree and code from
    /// another.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceLoadError`] if the current directory cannot be
    /// determined, no project root is found, or workspace load fails.
    pub fn from_cli(cli: &Cli, dir: Option<&Path>) -> Result<Self, WorkspaceLoadError> {
        let workspace = Workspace::resolve(dir)?;

        let output = OutputMode::Human {
            // Single source of truth for the color/TTY rule (still honors --no-color).
            color: hm_render::color_enabled(cli.no_color),
            // Interactive prompts/spinners key off stdout being a TTY.
            interactive: std::io::stdout().is_terminal(),
        };

        Ok(Self { workspace, output })
    }
}
