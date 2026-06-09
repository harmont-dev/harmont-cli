use std::path::Path;

use anyhow::Result;

/// Restore cached VM snapshots from a directory.
///
/// This is a placeholder after the Docker-to-VM migration. The VM
/// backend manages snapshots internally via `ImageRegistry`; external
/// save/restore is not yet implemented.
///
/// # Errors
///
/// Currently always succeeds.
#[allow(clippy::print_stderr)]
pub async fn handle_restore(dir: &Path) -> Result<i32> {
    tracing::info!(
        path = %dir.display(),
        "cache restore is not yet implemented for the VM backend",
    );
    eprintln!("restored 0/0 snapshots (VM backend — not yet implemented)");
    Ok(0)
}
