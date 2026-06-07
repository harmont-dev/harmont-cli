use anyhow::Result;

/// # Errors
/// Returns an error if workspace cache removal fails.
pub async fn handle_clean() -> Result<i32> {
    let ws_cleaned = if let Some(ws_cache) = hm_util::dirs::harmont_workspace_cache_dir()
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

    let db_cleaned = if let Some(cache_dir) = hm_util::dirs::harmont_cache_dir() {
        let db_path = cache_dir.join("registry.db");
        if db_path.exists() {
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
