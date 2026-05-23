use futures::stream::FuturesOrdered;
use std::io;

/// Find the best harmont config directory.
///
/// The harmont config directory is found by searching for
/// ```txt
/// ~/.hm
/// /etc/hm
/// ```
///
/// in that order. If any of these directories are found, then the first one,
/// in that precedence, will be returned.
///
/// Note that the directory does not need to be well-formed to be considered.
///
/// Note Windows uses `C:\ProgramData`. Note we do not respect `Application Support` on mac because
/// it confuses everyone.
pub async fn config_dir() -> io::Result<PathBuf> {
    // TODO(markovejnovic): Send out multtiple parallel tokio tasks which return stuff in order.
}
