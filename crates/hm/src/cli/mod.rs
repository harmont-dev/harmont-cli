pub mod plugin;
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

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Run a pipeline locally via Docker.
    Run(RunArgs),

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
        Command::Run(args) => crate::commands::run::handle(args, ctx).await,
        Command::Cache(cmd) => match cmd {
            CacheCommand::Save(args) => crate::commands::cache::handle_save(&args.dir).await,
            CacheCommand::Restore(args) => crate::commands::cache::handle_restore(&args.dir).await,
        },
        Command::Version => version::run().await.map(|()| 0),
        Command::Plugin(cmd) => plugin::run(cmd).await.map(|()| 0),
        Command::Cloud(_cmd) => {
            tracing::info!(
                "Harmont Cloud is not yet available.\n\
                 \n\
                 Sign up for the waitlist at https://harmont.dev to get early access."
            );
            Ok(0)
        }
    }
}
