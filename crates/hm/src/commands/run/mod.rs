use anyhow::Result;

use crate::cli::RunArgs;
use crate::context::RunContext;

mod local;

pub use local::handle as handle_local;
pub use local::run_pipeline_v0_one_shot;

/// Top-level dispatcher for `hm run`. After the plan-4 cloud-plugin
/// cutover, `hm run` always runs locally via Docker.
///
/// # Errors
///
/// Returns Docker, pipeline-render, or scheduler errors surfaced by the
/// local orchestrator.
pub async fn handle(args: RunArgs, ctx: RunContext) -> Result<i32> {
    handle_local(args, ctx).await
}
