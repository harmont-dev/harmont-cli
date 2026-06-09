use std::path::Path;

use anyhow::Result;

/// Save cached VM snapshots to a directory for CI warm-start.
///
/// This is a placeholder after the Docker-to-VM migration. The VM
/// backend manages snapshots internally via `ImageRegistry`; external
/// save/restore is not yet implemented.
///
/// # Errors
///
/// Currently always succeeds.
#[allow(clippy::print_stdout)]
pub async fn handle_save(dir: &Path) -> Result<i32> {
    tracing::info!(
        path = %dir.display(),
        "cache save is not yet implemented for the VM backend",
    );
    // Print an empty hash so callers that capture stdout don't break.
    println!("0000000000000000");
    Ok(0)
}
