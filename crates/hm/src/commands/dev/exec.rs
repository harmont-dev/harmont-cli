//! `hm dev exec <slug> [-- cmd...]` — one-shot exec.

use anyhow::Result;

use crate::cli::DevExecArgs;
use crate::context::RunContext;
use crate::orchestrator::docker_client::DockerClient;

use super::naming::{
    LABEL_SESSION, LABEL_SLUG, LABEL_WORKTREE, resolve_worktree_root, worktree_hash,
};

/// Exec a command inside a live deployment's container.
///
/// # Errors
///
/// Returns an error if Docker is unreachable or if the exec lifecycle
/// calls fail.
pub async fn handle(args: DevExecArgs, _ctx: RunContext) -> Result<i32> {
    let docker = DockerClient::connect()?;
    let worktree_root = resolve_worktree_root()?;
    let wt_hash = worktree_hash(&worktree_root);
    let containers = docker
        .list_containers_by_label(LABEL_WORKTREE, &wt_hash)
        .await?;
    let mut matches: Vec<String> = Vec::new();
    for c in &containers {
        let labels = c.labels.clone().unwrap_or_default();
        if labels.get(LABEL_SLUG).map(String::as_str) != Some(&args.slug) {
            continue;
        }
        let session = labels.get(LABEL_SESSION).cloned().unwrap_or_default();
        if let Some(s) = &args.session
            && &session != s
        {
            continue;
        }
        if let Some(id) = &c.id {
            matches.push(id.clone());
        }
    }
    if matches.is_empty() {
        tracing::error!(
            "hm: slug `{}` is not running in this worktree.\n  → run `hm dev up {}` first.",
            args.slug,
            args.slug,
        );
        return Ok(4);
    }
    if matches.len() > 1 {
        tracing::error!(
            "hm: slug `{}` matches multiple live sessions; pass --session <id>",
            args.slug
        );
        return Ok(5);
    }
    let id = &matches[0];
    let cmd = if args.cmd.is_empty() {
        vec!["sh".to_string(), "-l".to_string()]
    } else {
        args.cmd.clone()
    };
    let code = docker.exec_tty(id, &cmd).await?;
    Ok(code)
}
