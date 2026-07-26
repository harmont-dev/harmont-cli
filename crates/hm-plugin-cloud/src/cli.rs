//! CLI parsing for cloud subcommands.

use std::collections::BTreeMap;

use anyhow::Result;
use clap::Subcommand;

use crate::{auth, verbs};

/// Process exit status for the cloud subcommands.
enum ExitCode {
    /// The command completed successfully.
    Success,
    /// The command ran but failed at runtime.
    RuntimeError,
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        match code {
            ExitCode::Success => 0,
            ExitCode::RuntimeError => 1,
        }
    }
}

#[derive(Debug, Clone, Subcommand)]
pub enum CloudCommand {
    /// Authenticate this CLI against the Harmont API.
    Login {
        /// Skip the loopback flow and prompt for a paste-in code.
        #[arg(long)]
        paste: bool,
    },
    /// Remove stored credentials.
    Logout,
    /// Show the authenticated user.
    Whoami,
    /// Manage organizations.
    #[command(subcommand)]
    Org(OrgCommand),
    /// Manage pipelines.
    #[command(subcommand)]
    Pipeline(PipelineCommand),
    /// Manage builds.
    #[command(subcommand)]
    Build(BuildCommand),
    /// Manage jobs.
    #[command(subcommand)]
    Job(JobCommand),
    /// Manage credits, top-ups, and usage.
    #[command(subcommand)]
    Billing(BillingCommand),
    /// Submit the local pipeline to the cloud and watch its build.
    Run(verbs::run::RunArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrgCommand {
    /// Set the active organization.
    Switch {
        /// Organization slug.
        slug: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum PipelineCommand {
    /// List pipelines for the active organization.
    List,
    /// Show pipeline details by slug.
    Show { slug: String },
}

#[derive(Debug, Clone, Subcommand)]
pub enum BuildCommand {
    /// List builds for a pipeline.
    List {
        #[arg(short, long)]
        pipeline: String,
    },
    /// Show a build by number.
    Show {
        #[arg(short, long)]
        pipeline: String,
        number: i64,
    },
    /// Cancel a build.
    Cancel {
        #[arg(short, long)]
        pipeline: String,
        number: i64,
    },
    /// Watch a build until it reaches a terminal state.
    Watch {
        #[arg(short, long)]
        pipeline: String,
        number: i64,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum JobCommand {
    /// List jobs in a build.
    List {
        #[arg(short, long)]
        pipeline: String,
        #[arg(short, long)]
        build: i64,
    },
    /// Show a job by id.
    Show {
        #[arg(short, long)]
        pipeline: String,
        #[arg(short, long)]
        build: i64,
        job_id: String,
    },
    /// Print the job log.
    Log {
        #[arg(short, long)]
        pipeline: String,
        #[arg(short, long)]
        build: i64,
        job_id: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum BillingCommand {
    /// Print the current credit balance.
    Balance,
    /// List billing transactions.
    Transactions {
        #[arg(long, default_value = "100")]
        limit: u32,
    },
    /// Show usage over a time window.
    Usage {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
    },
    /// Top up credits via Stripe checkout.
    Topup {
        amount_usd: u32,
        #[arg(long)]
        no_browser: bool,
    },
    /// Redeem a coupon code.
    Redeem { code: String },
}

/// Dispatch a parsed `CloudCommand`, returning its process exit code.
///
/// # Errors
///
/// Returns an error only if dispatch itself fails; a verb's own runtime
/// failure is logged and mapped to a non-zero exit code.
pub async fn dispatch_command(command: CloudCommand, env: BTreeMap<String, String>) -> Result<i32> {
    let result = match command {
        CloudCommand::Login { paste } => auth::login::run(&env, paste).await,
        CloudCommand::Logout => auth::logout::run(&env).await,
        CloudCommand::Whoami => auth::whoami::run(&env).await,
        CloudCommand::Org(cmd) => verbs::org::run(&env, cmd).await,
        CloudCommand::Pipeline(cmd) => verbs::pipeline::run(&env, cmd).await,
        CloudCommand::Build(cmd) => verbs::build::run(&env, cmd).await,
        CloudCommand::Job(cmd) => verbs::job::run(&env, cmd).await,
        CloudCommand::Billing(cmd) => verbs::billing::run(&env, cmd).await,
        CloudCommand::Run(args) => verbs::run::run(&env, args).await,
    };
    match result {
        Ok(()) => Ok(ExitCode::Success.into()),
        Err(e) => {
            tracing::error!("{e:#}");
            Ok(ExitCode::RuntimeError.into())
        }
    }
}
