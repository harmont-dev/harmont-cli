use std::io::IsTerminal;

use hm_core::app_ctx::AppCtx;
use hm_render::OutputMode;

use crate::cli::Cli;

/// The app context and output mode for a dispatched command.
#[derive(Debug)]
pub struct RunContext<'app> {
    pub app: &'app AppCtx,
    /// Output mode for the built-in verbs.
    pub output: OutputMode,
}

impl<'app> RunContext<'app> {
    /// Build a [`RunContext`] from the app context and parsed CLI args.
    #[must_use]
    pub fn from_cli(app: &'app AppCtx, cli: &Cli) -> Self {
        let output = OutputMode::Human {
            // Single source of truth for the color/TTY rule (still honors --no-color).
            color: hm_render::color_enabled(cli.no_color),
            // Interactive prompts/spinners key off stdout being a TTY.
            interactive: std::io::stdout().is_terminal(),
        };

        Self { app, output }
    }
}
