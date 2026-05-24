#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive dependency version conflicts in rand/windows-sys/thiserror chains; not fixable without upstream updates"
)]

use clap::Parser;
use owo_colors::OwoColorize;
use tracing_subscriber::EnvFilter;

use harmont_cli::cli::{self, Cli};
use harmont_cli::context::RunContext;
use harmont_cli::error::{self, HmError};
use harmont_cli::output::status;

#[tokio::main]
async fn main() {
    let args = Cli::parse();

    let default_level = if args.verbose { "debug" } else { "info" };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .init();

    let color_enabled = !args.no_color
        && std::env::var_os("NO_COLOR").is_none()
        && console::Term::stderr().is_term();
    owo_colors::set_override(color_enabled);

    let code = match run(args).await {
        Ok(code) => code,
        Err(e) => handle_error(&e),
    };

    std::process::exit(code);
}

async fn run(args: Cli) -> Result<i32, anyhow::Error> {
    let command = args.command.clone();
    let ctx = RunContext::from_cli(&args)?;
    cli::dispatch(command, ctx).await
}

fn handle_error(err: &anyhow::Error) -> i32 {
    if let Some(hm_err) = err.downcast_ref::<HmError>() {
        status::print_error(&format!("{hm_err}"));
        return hm_err.exit_code();
    }

    let msg = format!("{err:#}");
    let red = "error:".red();
    let prefix = red.bold();
    tracing::error!("{prefix} {msg}");
    error::EXIT_BUILD_FAILED
}
