#![allow(
    clippy::print_stderr,
    reason = "the panic banner in handle_error is the last-resort stderr writer"
)]
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

    // Initialize tracing if --verbose.
    if args.verbose {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")),
            )
            .with_target(false)
            .init();
    }

    // Color override propagates to every OwoColorize call site. We
    // respect three signals, in priority order: explicit `--no-color`,
    // the `NO_COLOR` env var (https://no-color.org), and finally
    // TTY-ness of stderr. When stderr isn't a terminal — pipe to a
    // file, `head`, or a test harness — turning colors off keeps the
    // bytes downstream clean.
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
    // Try to downcast to our typed error for a specific exit code.
    if let Some(hm_err) = err.downcast_ref::<HmError>() {
        status::print_error(&format!("{hm_err}"));
        return hm_err.exit_code();
    }

    // Generic error.
    let msg = format!("{err:#}");
    eprintln!("{} {msg}", "error:".red().bold());
    error::EXIT_BUILD_FAILED
}
