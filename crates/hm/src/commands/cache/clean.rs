use anyhow::Result;
use hm_vm::VmBackend as _;

/// # Errors
/// Returns an error if workspace cache removal fails.
pub async fn handle_clean() -> Result<i32> {
    let ws_cleaned = if let Some(ws_cache) = hm_util::dirs::hm_workspace_cache_dir()
        && ws_cache.exists()
    {
        let size = dir_size(&ws_cache);
        std::fs::remove_dir_all(&ws_cache)?;
        tracing::info!(
            path = %ws_cache.display(),
            "removed workspace cache ({})",
            human_bytes(size),
        );
        true
    } else {
        false
    };

    let db_cleaned = if let Some(cache_dir) = hm_util::dirs::hm_cache_dir() {
        let db_path = cache_dir.join("registry.db");
        if db_path.exists() {
            // Remove the backing Docker images BEFORE deleting registry.db.
            // The registry is the only index from a cache key to its tagged
            // image (`forever-*`, etc.); once the DB is gone the images can't
            // be located by key, and `docker image prune` only reclaims
            // *dangling* images, so a tagged snapshot survives it. So we
            // enumerate the registry, remove each image via the Docker
            // backend (best-effort), then drop the DB.
            remove_registered_images(&db_path).await;

            std::fs::remove_file(&db_path)?;
            tracing::info!(path = %db_path.display(), "removed VM image registry");
            true
        } else {
            false
        }
    } else {
        false
    };

    if !ws_cleaned && !db_cleaned {
        tracing::info!("nothing to clean");
    }

    Ok(0)
}

/// Remove every Docker image tracked by the registry at `db_path`.
///
/// Best-effort: a missing Docker daemon or an already-deleted image is logged
/// and skipped, never fatal — `clean` must still delete the registry DB so the
/// cache index is reset.
async fn remove_registered_images(db_path: &std::path::Path) {
    // Capacity here is irrelevant — we only read existing rows, never insert.
    let registry = match hm_vm::ImageRegistry::open(db_path, u64::MAX) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "could not open image registry; skipping image removal");
            return;
        }
    };

    let snapshots = registry.all_snapshot_ids();
    if snapshots.is_empty() {
        return;
    }

    let backend = match hm_vm::docker::DockerBackend::connect() {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "could not connect to Docker; {} cached image(s) may remain — remove them with `docker image rm`",
                snapshots.len(),
            );
            return;
        }
    };

    let mut removed = 0usize;
    for snap in &snapshots {
        match backend.remove_snapshot(snap).await {
            Ok(()) => removed += 1,
            Err(e) => {
                tracing::warn!(image = %snap, error = %e, "failed to remove cached image");
            }
        }
    }
    tracing::info!(
        "removed {removed} of {} cached Docker image(s)",
        snapshots.len()
    );
}

fn dir_size(path: &std::path::Path) -> u64 {
    fn walk(p: &std::path::Path) -> u64 {
        std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .filter_map(std::result::Result::ok)
            .map(|e| {
                let path = e.path();
                if path.is_dir() {
                    walk(&path)
                } else {
                    e.metadata().map_or(0, |m| m.len())
                }
            })
            .sum()
    }
    walk(path)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "human-readable display; sub-byte precision irrelevant"
)]
fn human_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", b / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1}MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.1}GB", b / (1024.0 * 1024.0 * 1024.0))
    }
}
