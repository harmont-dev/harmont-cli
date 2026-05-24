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
    tracing::info!("Registered runners:");
    tracing::info!("  docker (default, built-in)");
    Ok(())
}
