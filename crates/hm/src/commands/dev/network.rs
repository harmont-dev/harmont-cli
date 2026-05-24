//! Per-session bridge network for `hm dev up`.

use std::collections::HashMap;

use anyhow::Result;

use crate::orchestrator::docker_client::DockerClient;

use super::naming::{DRIVER_LOCAL, LABEL_DRIVER, LABEL_SESSION, LABEL_WORKTREE, network_name};

#[derive(Debug, Clone)]
pub struct Network {
    pub name: String,
}

/// Create the per-session bridge network with the canonical labels.
///
/// # Errors
///
/// Returns the docker error if the daemon rejects creation.
pub async fn create(docker: &DockerClient, worktree_hash: &str, session: &str) -> Result<Network> {
    let name = network_name(worktree_hash, session);
    let mut labels = HashMap::new();
    labels.insert(LABEL_WORKTREE.to_string(), worktree_hash.to_string());
    labels.insert(LABEL_SESSION.to_string(), session.to_string());
    labels.insert(LABEL_DRIVER.to_string(), DRIVER_LOCAL.to_string());
    docker.create_network(&name, labels).await?;
    Ok(Network { name })
}

/// Remove the per-session bridge network. Idempotent.
///
/// # Errors
///
/// Returns the docker error if removal fails for non-404 reasons.
pub async fn remove(docker: &DockerClient, net: &Network) -> Result<()> {
    docker.remove_network(&net.name).await
}
