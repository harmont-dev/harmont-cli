#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive dependency version conflicts in rand/windows-sys/thiserror chains; not fixable without upstream updates"
)]

use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

use harmont_cli::cli::{self, Cli};
use harmont_cli::context::RunContext;
use harmont_cli::error::{self, HmError};

#[tokio::main]
async fn main() {
    let args = Cli::parse();

    let use_indicatif = matches!(
        &args.command,
        cli::Command::Run(r) if !r.logs && r.format == "human"
    );

    let default_level = if args.verbose { "debug" } else { "info" };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    if use_indicatif {
        let indicatif_layer = tracing_indicatif::IndicatifLayer::new();

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(indicatif_layer.get_stderr_writer())
                    .with_target(false)
                    .without_time()
                    .with_ansi(false)
                    .with_filter(filter),
            )
            .with(indicatif_layer)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .without_time()
            .with_ansi(false)
            .init();
    }

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
        tracing::error!("{hm_err}");
        return hm_err.exit_code();
    }

    let msg = format!("{err:#}");
    tracing::error!("error: {msg}");
    error::EXIT_BUILD_FAILED
}
