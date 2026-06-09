use clap::Parser;
use std::path::PathBuf;

// RunArgs uses several bool flags (no_watch, logs, cloud): each is an
// independent clap switch and a state-machine or enum would be more confusing.
#[allow(clippy::struct_excessive_bools)]
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

    /// Stream full build logs instead of showing progress bars.
    /// Has no effect with `--format json`.
    #[arg(long)]
    pub logs: bool,

    /// Execution backend. `docker` (default) runs the build locally on the
    /// Docker VM backend; `cloud` submits it to Harmont Cloud and streams
    /// live logs. Layers over the `backend` config key when omitted.
    #[arg(long, value_name = "NAME")]
    pub backend: Option<String>,

    /// Deprecated alias for `--backend cloud`. Uploads the working tree
    /// (respecting .gitignore, excluding .git) and streams live logs.
    #[arg(long, hide = true)]
    pub cloud: bool,

    /// Cloud organization (defaults to the configured default org or
    /// `[cloud] org` in config). Used when the backend resolves to cloud.
    #[arg(long)]
    pub org: Option<String>,
}
