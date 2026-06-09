pub mod init;
pub mod pipelines;
pub mod plugin;
pub mod render;
pub mod run;
pub mod version;

pub use plugin::PluginCommand;
pub use run::RunArgs;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::context::RunContext;

#[derive(Debug, Parser)]
#[command(
    name = "hm",
    version,
    about = "hm — CLI for the Harmont CI platform",
    long_about = "hm is the command-line interface for Harmont.\n\n\
                   Run `hm run` to push local code through a pipeline without committing.",
    propagate_version = true,
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Override the API base URL. Hidden flag — set `HARMONT_API_URL` instead.
    #[arg(long, global = true, env = "HARMONT_API_URL", hide = true)]
    pub api_url: Option<String>,

    /// Enable verbose/debug logging.
    #[arg(long, short, global = true)]
    pub verbose: bool,

    /// Disable colored output.
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Write a Chrome trace JSON to the given path for performance analysis.
    #[arg(long, global = true, hide = true, value_name = "PATH")]
    pub debug_trace: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Initialize a .hm/ pipeline from a project template.
    Init(init::InitArgs),

    /// Run a pipeline locally via Docker.
    Run(RunArgs),

    /// Print the pipeline discovery envelope (JSON) for every pipeline in the repo.
    Pipelines(pipelines::PipelinesArgs),

    /// Render one pipeline's v0 IR (JSON) without running it.
    Render(render::RenderArgs),

    /// Show hm version.
    Version,

    /// Manage plugins.
    #[command(subcommand)]
    Plugin(PluginCommand),

    /// Manage harmont Docker image cache.
    #[command(subcommand)]
    Cache(CacheCommand),

    /// Interact with the Harmont cloud API.
    #[command(subcommand)]
    Cloud(hm_plugin_cloud::cli::CloudCommand),
}

#[derive(Debug, Clone, Subcommand)]
pub enum CacheCommand {
    /// Save harmont Docker images to a cache directory.
    Save(CacheSaveArgs),
    /// Restore harmont Docker images from a cache directory.
    Restore(CacheRestoreArgs),
    /// Remove all cached workspaces and Docker images.
    Clean,
}

#[derive(Debug, Clone, clap::Args)]
pub struct CacheSaveArgs {
    /// Directory to save image tars into.
    pub dir: PathBuf,
}

#[derive(Debug, Clone, clap::Args)]
pub struct CacheRestoreArgs {
    /// Directory containing cached image tars.
    pub dir: PathBuf,
}

/// Dispatch a parsed CLI command to the appropriate handler. Returns an exit code.
///
/// # Errors
///
/// Returns an error if the dispatched handler fails.
pub async fn dispatch(command: Command, ctx: RunContext) -> Result<i32> {
    match command {
        Command::Init(args) => crate::commands::init::handle(args).await.map(|()| 0),
        Command::Run(args) => crate::commands::run::handle(args, ctx).await,
        Command::Pipelines(args) => crate::cli::pipelines::run(args).await.map(|()| 0),
        Command::Render(args) => crate::cli::render::run(args).await.map(|()| 0),
        Command::Cache(cmd) => match cmd {
            CacheCommand::Save(args) => crate::commands::cache::handle_save(&args.dir).await,
            CacheCommand::Restore(args) => crate::commands::cache::handle_restore(&args.dir).await,
            CacheCommand::Clean => crate::commands::cache::handle_clean().await,
        },
        Command::Version => version::run().await.map(|()| 0),
        Command::Plugin(cmd) => plugin::run(cmd).await.map(|()| 0),
        Command::Cloud(cmd) => {
            let env = std::env::vars().collect();
            hm_plugin_cloud::cli::dispatch_command(cmd, env).await
        }
    }
}
