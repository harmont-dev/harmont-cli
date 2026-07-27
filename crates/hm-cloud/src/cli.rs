//! The `hm cloud` command tree.

use clap::Subcommand;

#[derive(Debug, Clone, Subcommand)]
pub enum CloudCommand {
    /// Authenticate and inspect the active session.
    #[command(subcommand)]
    Auth(AuthCommand),
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
}

#[derive(Debug, Clone, Subcommand)]
pub enum AuthCommand {
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
    /// Show pipeline details by slug. Defaults to the configured pipeline.
    Show {
        /// Pipeline slug; defaults to the configured pipeline.
        slug: Option<String>,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum BuildCommand {
    /// List builds for a pipeline.
    List {
        /// Pipeline slug; defaults to the configured pipeline.
        #[arg(short, long)]
        pipeline: Option<String>,
    },
    /// Show a build by number.
    Show {
        /// Pipeline slug; defaults to the configured pipeline.
        #[arg(short, long)]
        pipeline: Option<String>,
        /// Build number.
        number: i64,
    },
    /// Cancel a build.
    Cancel {
        /// Pipeline slug; defaults to the configured pipeline.
        #[arg(short, long)]
        pipeline: Option<String>,
        /// Build number.
        number: i64,
    },
    /// Watch a build until it reaches a terminal state.
    Watch {
        /// Pipeline slug; defaults to the configured pipeline.
        #[arg(short, long)]
        pipeline: Option<String>,
        /// Build number.
        number: i64,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum JobCommand {
    /// List jobs in a build.
    List {
        /// Pipeline slug; defaults to the configured pipeline.
        #[arg(short, long)]
        pipeline: Option<String>,
        /// Build number.
        #[arg(short, long)]
        build: i64,
    },
    /// Show a job by id.
    Show {
        /// Pipeline slug; defaults to the configured pipeline.
        #[arg(short, long)]
        pipeline: Option<String>,
        /// Build number.
        #[arg(short, long)]
        build: i64,
        /// Job id.
        job_id: String,
    },
    /// Print the job log.
    Log {
        /// Pipeline slug; defaults to the configured pipeline.
        #[arg(short, long)]
        pipeline: Option<String>,
        /// Build number.
        #[arg(short, long)]
        build: i64,
        /// Job id.
        job_id: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum BillingCommand {
    /// Print the current credit balance.
    Balance,
    /// List billing transactions.
    Transactions {
        /// Maximum number of transactions to return.
        #[arg(long, default_value = "100")]
        limit: u32,
    },
    /// Show usage over a time window.
    Usage {
        /// Start of the usage window.
        #[arg(long)]
        from: Option<String>,
        /// End of the usage window.
        #[arg(long)]
        to: Option<String>,
    },
    /// Top up credits via Stripe checkout.
    Topup {
        /// Amount to add, in whole US dollars.
        amount_usd: u32,
        /// Print the checkout URL instead of opening a browser.
        #[arg(long)]
        no_browser: bool,
    },
    /// Redeem a coupon code.
    Redeem {
        /// Coupon code.
        code: String,
    },
}
