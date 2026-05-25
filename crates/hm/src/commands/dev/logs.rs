//! `hm dev logs <slug>` — tail a live deployment's logs.

use std::io::Write;

use anyhow::Result;
use bollard::container::LogsOptions;
use futures_util::StreamExt;

use crate::cli::DevLogsArgs;
use crate::context::RunContext;
use crate::orchestrator::docker_client::DockerClient;

use super::naming::{
    LABEL_SESSION, LABEL_SLUG, LABEL_WORKTREE, resolve_worktree_root, worktree_hash,
};

/// Stream logs from a live deployment's container.
///
/// # Errors
///
/// Returns an error if Docker is unreachable.
pub async fn handle(args: DevLogsArgs, _ctx: RunContext) -> Result<i32> {
    let docker = DockerClient::connect()?;
    let worktree_root = resolve_worktree_root()?;
    let wt_hash = worktree_hash(&worktree_root);
    let containers = docker
        .list_containers_by_label(LABEL_WORKTREE, &wt_hash)
        .await?;
    let mut matches: Vec<(String, String, String)> = Vec::new();
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
            matches.push((args.slug.clone(), session, id.clone()));
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
    let (_, _, id) = &matches[0];
    let mut s = docker.inner_for_logs().logs::<String>(
        id,
        Some(LogsOptions {
            stdout: true,
            stderr: true,
            follow: args.follow,
            tail: "all".to_string(),
            ..Default::default()
        }),
    );
    while let Some(chunk) = s.next().await {
        if let Ok(c) = chunk {
            std::io::stdout().write_all(c.into_bytes().as_ref()).ok();
        }
    }
    Ok(0)
}
