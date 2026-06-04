use anyhow::Result;

use crate::cli::RunArgs;
use crate::context::RunContext;

mod cloud;
mod local;

pub use local::handle as handle_local;

/// Top-level dispatcher for `hm run`. Local by default; `--cloud` uploads the
/// worktree to Harmont Cloud and streams logs.
///
/// # Errors
///
/// Returns Docker, pipeline-render, or scheduler errors surfaced by the
/// local orchestrator, or cloud-dispatch errors when `--cloud` is set.
pub async fn handle(args: RunArgs, ctx: RunContext) -> Result<i32> {
    if args.cloud {
        cloud::handle(args, ctx).await
    } else {
        handle_local(args, ctx).await
    }
}
