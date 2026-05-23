use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::context::RunContext;

#[derive(Debug, Clone, Subcommand)]
pub enum DevCommand {
    /// Bring deployments up in the foreground. Blocks until Ctrl-C.
    Up(DevUpArgs),
    /// Tear down deployments owned by this worktree's sessions.
    Down(DevDownArgs),
    /// List registered + running deployments.
    Ls,
    /// Tail logs of a live deployment from another terminal.
    Logs(DevLogsArgs),
    /// Print the host port for a live deployment. Designed for $() use.
    PortOf(DevPortOfArgs),
    /// One-shot exec into a live deployment container.
    Exec(DevExecArgs),
}

#[derive(Debug, Clone, Parser)]
pub struct DevUpArgs {
    /// Deployment slugs to bring up. When empty, brings up everything
    /// registered in `.harmont/*.py`.
    #[arg()]
    pub slugs: Vec<String>,

    /// Skip transitive dependencies; bring up exactly the listed slugs.
    #[arg(long)]
    pub no_deps: bool,

    /// Force image rebuild on `from_=Step` deployments even if a cached
    /// build image exists.
    #[arg(long)]
    pub rebuild: bool,
}

#[derive(Debug, Clone, Parser)]
pub struct DevDownArgs {
    /// Slugs to sweep. When empty, sweeps all sessions of this worktree.
    #[arg()]
    pub slugs: Vec<String>,

    /// Sweep one specific session entirely (overrides `slugs`).
    #[arg(long, value_name = "ID")]
    pub session: Option<String>,

    /// Sweep system-wide instead of this worktree (every container
    /// labelled `harmont.driver=local`).
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Clone, Parser)]
pub struct DevLogsArgs {
    pub slug: String,

    #[arg(short, long)]
    pub follow: bool,

    #[arg(long, value_name = "ID")]
    pub session: Option<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct DevPortOfArgs {
    pub slug: String,

    /// Container-internal port whose host binding to print.
    pub container_port: u16,

    #[arg(long, value_name = "ID")]
    pub session: Option<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct DevExecArgs {
    pub slug: String,

    /// Command to run inside the container. Default `sh -l`.
    #[arg(trailing_var_arg = true)]
    pub cmd: Vec<String>,

    #[arg(long, value_name = "ID")]
    pub session: Option<String>,
}

/// Dispatch an `hm dev` subcommand to the appropriate handler.
///
/// # Errors
///
/// Returns an error if the subcommand handler fails.
pub async fn dispatch(command: DevCommand, ctx: RunContext) -> Result<i32> {
    use crate::commands::dev;
    match command {
        DevCommand::Up(args) => dev::up::handle(args, ctx).await,
        DevCommand::Down(args) => dev::down::handle(args, ctx).await,
        DevCommand::Ls => dev::ls::handle(ctx).await,
        DevCommand::Logs(args) => dev::logs::handle(args, ctx).await,
        DevCommand::PortOf(args) => dev::port_of::handle(args, ctx).await,
        DevCommand::Exec(args) => dev::exec::handle(args, ctx).await,
    }
}
