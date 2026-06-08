use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
pub struct InitArgs {
    /// Project template (cmake, elixir, nextjs, js, rust, zig, python).
    #[arg(short, long)]
    pub template: Option<String>,

    /// Target directory (defaults to cwd).
    #[arg(short, long)]
    pub dir: Option<PathBuf>,

    /// Overwrite existing .harmont/ directory.
    #[arg(long)]
    pub force: bool,
}
