//! `hm cloud job list|show|log`.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::api::types::{Job, JobList, JobLog};
use crate::cli::JobCommand;
use crate::config::Config;
use crate::creds;
use crate::http::Client;
use crate::state::CloudState;

pub(crate) async fn run(env: &BTreeMap<String, String>, cmd: JobCommand) -> Result<()> {
    let cfg = Config::from_env(env);
    let token = creds::load_token(&cfg.api_base, env)
        .ok_or_else(|| anyhow::anyhow!("not logged in; run `hm cloud login`"))?;
    let client = Client::new(&cfg, Some(token));
    let org = active_org()?;

    match cmd {
        JobCommand::List { pipeline, build } => list(&client, &org, &pipeline, build).await,
        JobCommand::Show {
            pipeline,
            build,
            job_id,
        } => show(&client, &org, &pipeline, build, &job_id).await,
        JobCommand::Log {
            pipeline,
            build,
            job_id,
        } => log_cmd(&client, &org, &pipeline, build, &job_id).await,
    }
}

async fn list(client: &Client, org: &str, pipe: &str, build: i64) -> Result<()> {
    let jobs: JobList = client
        .get(&format!(
            "/organizations/{org}/pipelines/{pipe}/builds/{build}/jobs"
        ))
        .await?;
    for j in &jobs.data {
        tracing::info!(
            "{}  {:<10}  {}",
            j.id,
            j.state,
            j.label.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

async fn show(client: &Client, org: &str, pipe: &str, build: i64, jid: &str) -> Result<()> {
    let j: Job = client
        .get(&format!(
            "/organizations/{org}/pipelines/{pipe}/builds/{build}/jobs/{jid}"
        ))
        .await?;
    tracing::info!("{}", serde_json::to_string_pretty(&j).unwrap_or_default());
    Ok(())
}

async fn log_cmd(client: &Client, org: &str, pipe: &str, build: i64, jid: &str) -> Result<()> {
    let log: JobLog = client
        .get(&format!(
            "/organizations/{org}/pipelines/{pipe}/builds/{build}/jobs/{jid}/log"
        ))
        .await?;
    for chunk in &log.data {
        tracing::info!("{}", chunk.line);
    }
    Ok(())
}

fn active_org() -> Result<String> {
    CloudState::load()
        .active_org
        .ok_or_else(|| anyhow::anyhow!("no active organization; run `hm cloud org switch <slug>`"))
}
