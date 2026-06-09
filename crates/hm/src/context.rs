use std::io::IsTerminal;

use anyhow::{Context, Result};

use crate::cli::Cli;
use crate::config::Config;
use hm_render::OutputMode;

/// Runtime context that bundles resolved config and output preferences.
///
/// After the plan-4 cloud-plugin cutover this is intentionally thin:
/// API client, credential store, and active-org resolution moved into
/// `hm-plugin-cloud`. The host context retains the config file (for
/// future use) and the output mode.
#[derive(Debug)]
pub struct RunContext {
    pub config: Config,
    /// Output mode for the residual built-in verbs (the legacy global
    /// `--format` flag was retired in plan 3; per-subcommand `--format`
    /// is the only currently-wired source, so this defaults to human).
    pub output: OutputMode,
}

impl RunContext {
    /// Build a [`RunContext`] from parsed CLI args.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file is unreadable or malformed.
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        let start_dir =
            std::env::current_dir().context("cannot determine current directory")?;
        let project_root = hm_util::dirs::find_project_root(&start_dir);
        let config = Config::load(project_root.as_deref())?;

        let output = OutputMode::Human {
            // Single source of truth for the color/TTY rule (still honors --no-color).
            color: hm_render::color_enabled(cli.no_color),
            // Interactive prompts/spinners key off stdout being a TTY.
            interactive: std::io::stdout().is_terminal(),
        };

        Ok(Self { config, output })
    }
}
