//! Dispatch for `hm cloud` subcommands.

use std::collections::BTreeMap;

use anyhow::Result;
use hm_cloud::cli::{AuthCommand, CloudCommand};
use hm_core::app_ctx::AppCtx;

use crate::commands::cloud::{auth, verbs};

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

/// Dispatch a parsed `CloudCommand`, returning its process exit code.
///
/// # Errors
///
/// Returns an error only if dispatch itself fails; a verb's own runtime
/// failure is logged and mapped to a non-zero exit code.
pub async fn dispatch_command(
    command: CloudCommand,
    env: BTreeMap<String, String>,
    app: &AppCtx,
) -> Result<i32> {
    let result = match command {
        CloudCommand::Auth(cmd) => match cmd {
            AuthCommand::Login => auth::login::run(app).await,
            AuthCommand::Logout => auth::logout::run(&env, app).await,
            AuthCommand::Whoami => auth::whoami::run(&env, app).await,
        },
        CloudCommand::Org(cmd) => verbs::org::run(&env, cmd, app).await,
        CloudCommand::Pipeline(cmd) => verbs::pipeline::run(&env, cmd, app).await,
        CloudCommand::Build(cmd) => verbs::build::run(&env, cmd, app).await,
        CloudCommand::Job(cmd) => verbs::job::run(&env, cmd, app).await,
        CloudCommand::Billing(cmd) => verbs::billing::run(&env, cmd, app).await,
    };
    match result {
        Ok(()) => Ok(ExitCode::Success.into()),
        Err(e) => {
            tracing::error!("{e:#}");
            Ok(ExitCode::RuntimeError.into())
        }
    }
}
