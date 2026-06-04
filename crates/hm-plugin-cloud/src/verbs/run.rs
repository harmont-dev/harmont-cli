//! `hm cloud run <PIPELINE>` — submit a pre-rendered pipeline plan to the
//! cloud and watch the resulting build.
//!
//! This is the minimal, file-based path: the caller supplies a pre-rendered v0
//! IR plan via `--plan-file` (or `plan.json` by convention) and **no source
//! archive** is uploaded. The full local-worktree flow — rendering the DSL and
//! archiving the working tree — is implemented by `hm run --cloud` (plan task
//! E2), which lives in the `hm` crate where the renderer and archiver are.

use std::collections::BTreeMap;

use anyhow::Result;
use clap::Parser;
use harmont_cloud::builds::NewBuild;

use crate::settings;

#[derive(Debug, Clone, Parser)]
pub struct RunArgs {
    /// Pipeline slug. Required.
    pub pipeline: String,
    /// Branch to record on the build.
    #[arg(short, long, default_value = "main")]
    pub branch: String,
    /// Commit SHA to record on the build.
    #[arg(short, long, default_value = "0000000000000000000000000000000000000000")]
    pub commit: String,
    /// Build message.
    #[arg(short, long)]
    pub message: Option<String>,
    /// Path to a pre-rendered v0 IR plan file. Defaults to `plan.json`.
    #[arg(long)]
    pub plan_file: Option<String>,
    /// Don't watch; print the build number and exit.
    #[arg(long)]
    pub no_watch: bool,
}

pub(crate) async fn run(env: &BTreeMap<String, String>, args: RunArgs) -> Result<()> {
    let (client, ctx) = settings::client()?;
    let org = ctx.org()?;

    let plan_path = args.plan_file.as_deref().unwrap_or("plan.json");
    let pipeline_ir = std::fs::read_to_string(plan_path)
        .map_err(|e| anyhow::anyhow!("could not read plan file '{plan_path}': {e}"))?;
    // Validate it parses as JSON before we ship it.
    serde_json::from_str::<serde_json::Value>(&pipeline_ir)
        .map_err(|e| anyhow::anyhow!("invalid JSON in plan file '{plan_path}': {e}"))?;

    let build = client
        .submit_build(NewBuild {
            org: org.clone(),
            pipeline: args.pipeline.clone(),
            branch: args.branch.clone(),
            commit: args.commit.clone(),
            message: args.message.clone(),
            pipeline_ir,
            // Full worktree archiving lands in `hm run --cloud` (task E2).
            source_tgz: Vec::new(),
            env: env
                .iter()
                .filter(|(k, _)| k.starts_with("HM_RUN_ENV_"))
                .map(|(k, v)| (k.trim_start_matches("HM_RUN_ENV_").to_string(), v.clone()))
                .collect(),
        })
        .await?;

    tracing::info!("submitted build #{}", build.number);
    if args.no_watch {
        return Ok(());
    }
    crate::verbs::build::run(
        env,
        crate::cli::BuildCommand::Watch {
            pipeline: args.pipeline.clone(),
            number: build.number,
        },
    )
    .await
}
