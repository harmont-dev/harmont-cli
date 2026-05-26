use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

use super::manifest::{self, Manifest};
use crate::orchestrator::docker_client::DockerClient;

/// Save all `harmont-local/*` images to a cache directory as tar files,
/// write a manifest, and prune stale tars that no longer correspond to
/// any known image.
///
/// Prints the manifest's content hash to stdout so CI runners (e.g.
/// GitHub Actions) can capture it for use as a cache key.
///
/// # Errors
///
/// Returns an error if the Docker daemon is unreachable, an image
/// export fails, or any filesystem operation on `dir` fails.
#[allow(clippy::print_stdout)]
pub async fn handle_save(dir: &Path) -> Result<i32> {
    let docker = DockerClient::connect()?;
    docker.ping().await?;

    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("create cache dir {}", dir.display()))?;

    let tags = docker.list_images_by_prefix("harmont-local/").await?;

    let mut manifest = Manifest::new();

    for tag in &tags {
        let filename = manifest::tar_name_for_tag(tag);
        let tar_path = dir.join(&filename);

        if tar_path.exists() {
            info!("skip (exists): {filename}");
        } else {
            info!("save: {tag} → {filename}");
            docker.export_image(tag, &tar_path).await?;
        }

        manifest.images.insert(filename, tag.clone());
    }

    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    tokio::fs::write(dir.join("manifest.json"), &manifest_json)
        .await
        .context("write manifest.json")?;

    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.ends_with(".tar") && !manifest.images.contains_key(name_str.as_ref()) {
            info!("prune stale: {name_str}");
            tokio::fs::remove_file(entry.path()).await.ok();
        }
    }

    let hash = manifest.content_hash();
    println!("{hash}");

    Ok(0)
}
