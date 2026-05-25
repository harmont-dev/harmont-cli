//! `hm dev port-of <slug> <container-port>` — print host port for a live deployment.

use anyhow::Result;

use crate::cli::DevPortOfArgs;
use crate::context::RunContext;
use crate::orchestrator::docker_client::DockerClient;

use super::naming::{
    LABEL_SESSION, LABEL_SLUG, LABEL_WORKTREE, resolve_worktree_root, worktree_hash,
};

/// Look up the host port that a live deployment's container port is bound to.
///
/// # Errors
///
/// Returns an error if Docker is unreachable or if the registry subprocess
/// invocation fails.
pub async fn handle(args: DevPortOfArgs, _ctx: RunContext) -> Result<i32> {
    let docker = DockerClient::connect()?;
    let worktree_root = resolve_worktree_root()?;
    let wt_hash = worktree_hash(&worktree_root);
    let containers = docker
        .list_containers_by_label(LABEL_WORKTREE, &wt_hash)
        .await?;
    let mut matches: Vec<(String, String, std::collections::HashMap<u16, u16>)> = Vec::new();
    for c in &containers {
        let labels = c.labels.clone().unwrap_or_default();
        let slug = labels.get(LABEL_SLUG).cloned().unwrap_or_default();
        let session = labels.get(LABEL_SESSION).cloned().unwrap_or_default();
        if slug != args.slug {
            continue;
        }
        if let Some(s) = &args.session
            && &session != s
        {
            continue;
        }
        if let Some(id) = &c.id {
            let ports = docker.inspect_ports(id).await?;
            matches.push((slug, session, ports));
        }
    }

    if matches.is_empty() {
        // Was the slug registered at all?
        match super::registry::dump(&worktree_root).await {
            Ok(reg) if reg.deployments.contains_key(&args.slug) => {
                tracing::error!(
                    "hm: slug `{}` registered but not running in this worktree.\n  → run `hm dev up {}` first.",
                    args.slug,
                    args.slug,
                );
                return Ok(4);
            }
            _ => {
                tracing::error!(
                    "hm: slug `{}` not registered in this worktree's .harmont/.\n  → run `hm dev ls` to see registered slugs.",
                    args.slug,
                );
                return Ok(5);
            }
        }
    }
    if matches.len() > 1 {
        tracing::error!(
            "hm: slug `{}` matches multiple live sessions in this worktree:",
            args.slug
        );
        for (_, sess, ports) in &matches {
            let p = format_ports(ports);
            tracing::error!("  {sess}  {p}");
        }
        tracing::error!("pass `--session <id>` or run `hm dev ls`.");
        return Ok(5);
    }

    let (_, _, ports) = &matches[0];
    let Some(host_port) = ports.get(&args.container_port) else {
        tracing::error!(
            "hm: container port `{}` is not published by `{}`.\n  → check the deployment's port_mapping.",
            args.container_port,
            args.slug,
        );
        return Ok(5);
    };
    #[allow(clippy::print_stdout)]
    {
        println!("{host_port}");
    }
    Ok(0)
}

fn format_ports(ports: &std::collections::HashMap<u16, u16>) -> String {
    let mut entries: Vec<(u16, u16)> = ports.iter().map(|(c, h)| (*c, *h)).collect();
    entries.sort_unstable();
    entries
        .iter()
        .map(|(c, h)| format!("localhost:{h} → :{c}"))
        .collect::<Vec<_>>()
        .join(", ")
}
