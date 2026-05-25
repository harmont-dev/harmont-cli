use std::path::Path;

use anyhow::{Context, Result};
use tracing::{info, warn};

use super::manifest;
use crate::orchestrator::docker_client::DockerClient;

/// Restore cached Docker images from tar files in the given directory.
///
/// Each `.tar` file is mapped back to its `harmont-local/*` tag via
/// [`manifest::tag_from_tar_name`]. Images that already exist in the
/// local Docker daemon are skipped.
///
/// # Errors
///
/// Returns an error if the Docker daemon is unreachable or a filesystem
/// operation on `dir` fails.
#[allow(clippy::print_stderr)]
pub async fn handle_restore(dir: &Path) -> Result<i32> {
    let docker = DockerClient::connect()?;
    docker.ping().await?;

    if !dir.exists() {
        info!("cache dir does not exist, nothing to restore");
        eprintln!("restored 0/0 images (cache dir missing)");
        return Ok(0);
    }

    let mut tars = Vec::new();
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .with_context(|| format!("read cache dir {}", dir.display()))?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if std::path::Path::new(&name_str)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tar"))
        {
            tars.push((name_str, entry.path()));
        }
    }

    let total = tars.len();
    let mut restored = 0u32;
    let mut skipped = 0u32;

    for (filename, tar_path) in &tars {
        let Some(tag) = manifest::tag_from_tar_name(filename) else {
            warn!("skip unrecognized tar: {filename}");
            continue;
        };

        if docker.image_exists(&tag).await? {
            info!("skip (present): {tag}");
            skipped += 1;
            continue;
        }

        info!("restore: {filename} → {tag}");
        match docker.import_image(tar_path).await {
            Ok(()) => restored += 1,
            Err(e) => {
                warn!("failed to load {filename}: {e}");
            }
        }
    }

    eprintln!("restored {restored}/{total} images ({skipped} already present)");
    Ok(0)
}
