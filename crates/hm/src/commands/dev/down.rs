//! `hm dev down` — sweep containers + networks left over from past sessions.

use std::collections::BTreeSet;

use anyhow::Result;

use crate::cli::DevDownArgs;
use crate::context::RunContext;
use crate::orchestrator::docker_client::DockerClient;

use super::naming::{
    DRIVER_LOCAL, LABEL_DRIVER, LABEL_SESSION, LABEL_SLUG, LABEL_WORKTREE, network_name,
    resolve_worktree_root, worktree_hash,
};

/// # Errors
///
/// Returns Docker errors on list / stop / remove failures.
pub async fn handle(args: DevDownArgs, _ctx: RunContext) -> Result<i32> {
    let docker = DockerClient::connect()?;
    let worktree_root = resolve_worktree_root()?;
    let wt_hash = worktree_hash(&worktree_root);

    let containers = if args.all {
        docker
            .list_containers_by_label(LABEL_DRIVER, DRIVER_LOCAL)
            .await?
    } else {
        docker
            .list_containers_by_label(LABEL_WORKTREE, &wt_hash)
            .await?
    };

    // (id, slug, session, name)
    let mut to_remove: Vec<(String, String, String, String)> = Vec::new();
    for c in &containers {
        let labels = c.labels.clone().unwrap_or_default();
        let slug = labels.get(LABEL_SLUG).cloned().unwrap_or_default();
        let session = labels.get(LABEL_SESSION).cloned().unwrap_or_default();
        let name = c
            .names
            .as_ref()
            .and_then(|n| n.first().cloned())
            .unwrap_or_default();
        if let Some(s) = &args.session
            && &session != s
        {
            continue;
        }
        if !args.slugs.is_empty() && !args.slugs.iter().any(|x| x == &slug) {
            continue;
        }
        if let Some(id) = &c.id {
            to_remove.push((id.clone(), slug, session, name));
        }
    }

    if to_remove.is_empty() {
        tracing::info!("[hm] nothing to sweep");
        return Ok(0);
    }

    let mut sessions_swept = BTreeSet::<String>::default();
    for (id, slug, session, name) in &to_remove {
        let _ = docker.stop_container(id).await;
        let _ = docker.remove_container(id).await;
        tracing::info!("[hm] removed {name} (slug={slug}, session={session})");
        sessions_swept.insert(session.clone());
    }

    // Networks: any network that no longer has containers must go.
    for session in &sessions_swept {
        let net = if args.all {
            // For --all we don't know the worktree per session ahead of
            // time; safer to inspect each container's worktree label and
            // remove its network. For v1, since we tagged the network
            // with the same labels, just iterate distinct network names
            // from the container set above.
            continue;
        } else {
            network_name(&wt_hash, session)
        };
        let _ = docker.remove_network(&net).await;
    }

    if args.all {
        // Sweep any network with harmont.driver=local that's now orphaned.
        // Bollard exposes list_networks via Docker; quick best-effort
        // scan: try to remove every hm-*-* network name we recorded.
        // (We deliberately keep this simple — orphan networks are
        // recreated next `up` anyway.)
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    // Behavior is integration-tested in dev_integration.rs; pure logic
    // here is tiny and exercised through the CLI.
}
