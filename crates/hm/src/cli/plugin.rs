use anyhow::Result;
use clap::Subcommand;

#[derive(Debug, Clone, Subcommand)]
pub enum PluginCommand {
    /// List registered runners.
    List,
}

/// Run an `hm plugin` subcommand.
///
/// # Errors
///
/// Returns an error if the plugin operation fails.
pub async fn run(cmd: PluginCommand) -> Result<()> {
    match cmd {
        PluginCommand::List => list().await,
    }
}

#[allow(clippy::unused_async)]
async fn list() -> Result<()> {
    tracing::info!(target: "user::stdout", "Registered runners:");
    tracing::info!(target: "user::stdout", "  docker (default, built-in)");
    Ok(())
}
