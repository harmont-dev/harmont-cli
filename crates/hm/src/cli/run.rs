use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
pub struct RunArgs {
    /// Pipeline slug. Required when the repo declares more than one
    /// `@hm.pipeline`; the CLI lists available slugs when omitted.
    #[arg()]
    pub pipeline: Option<String>,

    /// Branch to record on the build.
    #[arg(short, long)]
    pub branch: Option<String>,

    /// Build message.
    #[arg(short, long)]
    pub message: Option<String>,

    /// Environment variables (KEY=VALUE).
    #[arg(short, long)]
    pub env: Vec<String>,

    /// Source root (defaults to cwd).
    #[arg(short, long)]
    pub dir: Option<PathBuf>,

    /// Skip watching the build after it's created.
    #[arg(long)]
    pub no_watch: bool,

    /// Maximum number of chains to run concurrently. Defaults to the
    /// host's available parallelism. `0` is treated as `1`.
    #[arg(long, value_name = "N")]
    pub parallelism: Option<usize>,

    /// Output formatter (matches an installed output-formatter plugin
    /// `name`). Built-ins: `human`, `json`. Default: `human`.
    #[arg(long, value_name = "NAME", default_value = "human", global = false)]
    pub format: String,
}
