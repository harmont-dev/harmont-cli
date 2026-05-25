use anyhow::Result;

#[allow(clippy::unused_async)]
/// Print version information to stdout.
///
/// # Errors
///
/// Returns an error on I/O failure.
pub async fn run() -> Result<()> {
    tracing::info!("hm {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
