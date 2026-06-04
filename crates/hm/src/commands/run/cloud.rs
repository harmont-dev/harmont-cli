use anyhow::Result;

use crate::cli::RunArgs;
use crate::context::RunContext;

/// `hm run --cloud`: render locally, upload the worktree, stream logs.
/// Implemented in task E2.
#[allow(clippy::unused_async)] // body will become async in E2
pub(super) async fn handle(_args: RunArgs, _ctx: RunContext) -> Result<i32> {
    anyhow::bail!("`hm run --cloud` is not yet implemented")
}
