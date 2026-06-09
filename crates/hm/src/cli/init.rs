use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TemplateKind {
    Cmake,
    Elixir,
    Nextjs,
    Js,
    Rust,
    Zig,
    Python,
}

#[derive(Debug, Clone, Parser)]
pub struct InitArgs {
    /// Project template.
    #[arg(short, long)]
    pub template: Option<TemplateKind>,

    /// Target directory.
    #[arg(short, long, default_value = ".")]
    pub dir: PathBuf,

    /// Overwrite existing .hm/ directory.
    #[arg(long)]
    pub force: bool,
}
