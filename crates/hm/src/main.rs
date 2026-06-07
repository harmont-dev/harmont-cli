#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive dependency version conflicts in rand/windows-sys/thiserror chains; not fixable without upstream updates"
)]

use clap::Parser;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use harmont_cli::cli::{self, Cli};
use harmont_cli::context::RunContext;
use harmont_cli::error::{self, HmError};

#[tokio::main]
async fn main() {
    let args = Cli::parse();

    let color = !args.no_color
        && std::env::var("NO_COLOR").is_err()
        && std::io::IsTerminal::is_terminal(&std::io::stderr());

    let use_indicatif = !is_ci::cached()
        && matches!(
            &args.command,
            cli::Command::Run(r) if !r.logs && r.format == "human"
        );

    let default_level = if args.verbose { "debug" } else { "info" };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    let trace_path = args.debug_trace.as_deref();

    let chrome_guard = if use_indicatif {
        let max_bars =
            terminal_size::terminal_size().map_or(32, |(_, h)| u64::from(h.0.saturating_sub(2)));
        let indicatif_layer =
            tracing_indicatif::IndicatifLayer::new().with_max_progress_bars(max_bars, None);

        let (chrome_layer, guard) = build_chrome_layer(trace_path);

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(indicatif_layer.get_stderr_writer())
                    .with_target(false)
                    .without_time()
                    .with_ansi(color)
                    .with_filter(filter),
            )
            .with(indicatif_layer)
            .with(chrome_layer)
            .init();
        guard
    } else {
        let (chrome_layer, guard) = build_chrome_layer(trace_path);

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_target(false)
                    .without_time()
                    .with_ansi(color)
                    .with_filter(filter),
            )
            .with(chrome_layer)
            .init();
        guard
    };

    let code = match run(args).await {
        Ok(code) => code,
        Err(e) => handle_error(&e),
    };

    // Drop the chrome trace guard before exit so it flushes the JSON file.
    // std::process::exit skips destructors.
    drop(chrome_guard);

    std::process::exit(code);
}

async fn run(args: Cli) -> Result<i32, anyhow::Error> {
    let command = args.command.clone();
    let ctx = RunContext::from_cli(&args)?;
    cli::dispatch(command, ctx).await
}

fn build_chrome_layer<S>(
    path: Option<&std::path::Path>,
) -> (
    Option<tracing_chrome::ChromeLayer<S>>,
    Option<tracing_chrome::FlushGuard>,
)
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a> + Send + Sync,
{
    path.map_or_else(
        || (None, None),
        |p| {
            let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
                .file(p)
                .include_args(true)
                .build();
            (Some(layer), Some(guard))
        },
    )
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
