//! `hm cloud run [TASK]` — submit the local pipeline plan to the cloud
//! and watch the resulting build.
//!
//! For plan 4 the caller supplies a pre-rendered plan JSON via
//! `--plan-file` (or `plan.json` by convention). Source-archive
//! upload — required by the live API — lands in plan 5.

use std::collections::BTreeMap;

use anyhow::Result;
use clap::Parser;

use crate::api::types::{Build, CreateBuildRequest};
use crate::config::Config;
use crate::creds;
use crate::http::Client;
use crate::state::CloudState;

#[derive(Debug, Clone, Parser)]
pub struct RunArgs {
    /// Pipeline slug. Required.
    pub pipeline: String,
    /// Branch to record on the build.
    #[arg(short, long)]
    pub branch: Option<String>,
    /// Build message.
    #[arg(short, long)]
    pub message: Option<String>,
    /// Path to a pre-rendered pipeline JSON file.
    /// If unset, reads `plan.json`.
    #[arg(long)]
    pub plan_file: Option<String>,
    /// Don't watch; print the build URL and exit.
    #[arg(long)]
    pub no_watch: bool,
}

pub(crate) async fn run(env: &BTreeMap<String, String>, args: RunArgs) -> Result<()> {
    let cfg = Config::from_env(env);
    let token = creds::load_token(&cfg.api_base, env)
        .ok_or_else(|| anyhow::anyhow!("not logged in; run `hm cloud login`"))?;
    let client = Client::new(&cfg, Some(token));
    let org = CloudState::load().active_org.ok_or_else(|| {
        anyhow::anyhow!("no active organization; run `hm cloud org switch <slug>`")
    })?;

    // Read the pipeline plan.
    let plan_path = args.plan_file.as_deref().unwrap_or("plan.json");
    let bytes = std::fs::read(plan_path)
        .map_err(|e| anyhow::anyhow!("could not read plan file '{plan_path}': {e}"))?;
    let plan_json: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| anyhow::anyhow!("invalid JSON in plan file '{plan_path}': {e}"))?;

    let req = CreateBuildRequest {
        pipeline_slug: args.pipeline.clone(),
        branch: args.branch.clone(),
        message: args.message.clone(),
        env: env
            .iter()
            .filter(|(k, _)| k.starts_with("HM_RUN_ENV_"))
            .map(|(k, v)| (k.trim_start_matches("HM_RUN_ENV_").to_string(), v.clone()))
            .collect(),
        plan_json,
    };
    let build: Build = client
        .post(
            &format!("/organizations/{org}/pipelines/{}/builds", args.pipeline),
            &req,
        )
        .await?;
    let url = format!(
        "{}/{}/{}/builds/{}",
        cfg.api_base.trim_end_matches("/api"),
        org,
        args.pipeline,
        build.number
    );
    tracing::info!("submitted build #{}: {url}", build.number);
    if args.no_watch {
        return Ok(());
    }
    // Watch loop: reuse build::run.
    crate::verbs::build::run(
        env,
        crate::cli::BuildCommand::Watch {
            pipeline: args.pipeline.clone(),
            number: build.number,
        },
    )
    .await
}
