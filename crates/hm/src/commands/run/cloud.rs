use anyhow::{Context, Result};
use harmont_cloud::builds::NewBuild;
use hm_plugin_cloud::settings;

use crate::cli::RunArgs;
use crate::commands::run::local::render_pipeline;
use crate::context::RunContext;
use crate::orchestrator::source::build_archive_bytes;

/// `hm run --cloud`: render locally (fail fast), upload the worktree, submit.
///
/// # Errors
///
/// Returns an error if no token is present, the DSL render fails, no
/// organization can be resolved, archiving the worktree fails, or the
/// build-submission request is rejected by the API.
pub(super) async fn handle(args: RunArgs, ctx: RunContext) -> Result<i32> {
    // 1. Fail fast on auth BEFORE any work.
    let (client, rctx) = settings::client()
        .context("`hm run --cloud` requires authentication — run `hm login` or set HARMONT_API_TOKEN")?;

    // 2. Render locally — fails fast on DSL errors before upload.
    let (repo_root, slug, ir_json) = render_pipeline(&args, &ctx).await?;

    // 3. Resolve org (flag > config default).
    let org = args
        .org
        .clone()
        .or_else(|| rctx.default_org.clone())
        .context("no organization — pass --org or set default_org in ~/.harmont/config.toml")?;

    // 4. Git metadata (best-effort; this is a local run, not a push).
    let (branch, commit) = git_metadata(&repo_root, args.branch.clone());

    // 5. Archive the worktree.
    tracing::info!("archiving worktree…");
    let source_tgz = build_archive_bytes(&repo_root).context("archiving the worktree")?;

    // 6. Submit.
    tracing::info!("submitting build to {}…", rctx.api);
    let build = client
        .submit_build(NewBuild {
            org: org.clone(),
            pipeline: slug.clone(),
            branch,
            commit,
            message: args.message.clone(),
            pipeline_ir: ir_json,
            source_tgz,
            env: parse_env(&args.env),
        })
        .await
        .context("submitting the build")?;

    // 7. Tell the user where to watch.
    tracing::info!(
        "build #{} created ({})  org={} pipeline={}",
        build.number,
        build.state,
        org,
        slug
    );

    if args.no_watch {
        return Ok(0);
    }
    // Live streaming is wired in task F3.
    tracing::info!("(live log streaming lands in a follow-up; use `hm cloud build watch` meanwhile)");
    Ok(0)
}

fn parse_env(pairs: &[String]) -> std::collections::HashMap<String, String> {
    pairs
        .iter()
        .filter_map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

fn git_metadata(root: &std::path::Path, branch_override: Option<String>) -> (String, String) {
    let run = |a: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(a)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let branch = branch_override
        .or_else(|| run(&["rev-parse", "--abbrev-ref", "HEAD"]))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "HEAD".to_string());
    let commit = run(&["rev-parse", "HEAD"])
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "0".repeat(40));
    (branch, commit)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_splits_pairs() {
        let m = parse_env(&["A=1".into(), "B=x=y".into(), "bad".into()]);
        assert_eq!(m.get("A").unwrap(), "1");
        assert_eq!(m.get("B").unwrap(), "x=y");
        assert!(!m.contains_key("bad"));
    }

    #[test]
    fn git_metadata_falls_back_outside_repo() {
        let (b, c) = git_metadata(std::path::Path::new("/"), None);
        assert!(!b.is_empty() && !c.is_empty());
        assert_eq!(c.len(), 40); // zero-sha fallback
    }
}
