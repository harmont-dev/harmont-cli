//! `hm dev ls` — list registered + running deployments.

use std::collections::HashMap;

use anyhow::Result;

use crate::context::RunContext;
use crate::orchestrator::docker_client::DockerClient;

use super::naming::{
    LABEL_SESSION, LABEL_SLUG, LABEL_WORKTREE, resolve_worktree_root, worktree_hash,
};
use super::registry::{RegEntry, dump};

/// List registered deployments merged with their live Docker state.
///
/// # Errors
///
/// Returns an error if the worktree root cannot be resolved or the
/// registry subprocess fails.
pub async fn handle(_ctx: RunContext) -> Result<i32> {
    let worktree_root = resolve_worktree_root()?;
    let wt_hash = worktree_hash(&worktree_root);
    let registry = dump(&worktree_root).await?;
    let docker = DockerClient::connect().ok();

    tracing::info!(
        "{:<10} {:<8} {:<10} {:<10} PORTS",
        "SLUG",
        "DRIVER",
        "SESSION",
        "STATUS"
    );

    // Pre-load running containers by (slug, session) key.
    let mut running: HashMap<(String, String), (String, HashMap<u16, u16>)> = HashMap::new();
    if let Some(d) = &docker {
        let containers = d
            .list_containers_by_label(LABEL_WORKTREE, &wt_hash)
            .await
            .unwrap_or_default();
        for c in &containers {
            let labels = c.labels.clone().unwrap_or_default();
            let slug = labels.get(LABEL_SLUG).cloned().unwrap_or_default();
            let session = labels.get(LABEL_SESSION).cloned().unwrap_or_default();
            let state = c.state.clone().unwrap_or_default();
            if let Some(id) = &c.id {
                let ports = d.inspect_ports(id).await.unwrap_or_default();
                running.insert((slug, session), (state, ports));
            }
        }
    }

    for (slug, entry) in &registry.deployments {
        match entry {
            RegEntry::Local(_) => {
                // Print one row per live session; fall back to "registered" if none.
                let mut matched = false;
                for ((s, sess), (state, ports)) in &running {
                    if s == slug {
                        matched = true;
                        let ports_s = format_ports(ports);
                        tracing::info!(
                            "{slug:<10} {:<8} {:<10} {:<10} {ports_s}",
                            "local",
                            sess,
                            state
                        );
                    }
                }
                if !matched {
                    tracing::info!(
                        "{slug:<10} {:<8} {:<10} {:<10} \u{2014}",
                        "local",
                        "\u{2014}",
                        "registered"
                    );
                }
            }
            RegEntry::Unhandled => {
                tracing::info!(
                    "{slug:<10} {:<8} {:<10} {:<10} (no local driver)",
                    "?",
                    "\u{2014}",
                    "registered"
                );
            }
        }
    }
    Ok(0)
}

fn format_ports(ports: &HashMap<u16, u16>) -> String {
    let mut entries: Vec<(u16, u16)> = ports.iter().map(|(c, h)| (*c, *h)).collect();
    entries.sort_unstable();
    entries
        .iter()
        .map(|(c, h)| format!("localhost:{h} \u{2192} :{c}"))
        .collect::<Vec<_>>()
        .join(", ")
}
