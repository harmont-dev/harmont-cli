//! The error type returned across the `ExecutionBackend` boundary.

/// A backend failure returned across the `ExecutionBackend` boundary.
///
/// Distinguishes *infrastructure* failures (return `Err`) from a *failed
/// build* (`Ok(BuildOutcome { status: Failed, .. })`). `#[non_exhaustive]`
/// so new backends can add variants without breaking callers.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BackendError {
    #[error("authentication required")]
    Unauthorized,
    #[error("backend rejected the build [{code}]: {message}")]
    Rejected { code: String, message: String },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("network error: {0}")]
    Transport(String),
    #[error("log stream error: {0}")]
    LogStream(String),
    #[error("local execution error: {0}")]
    Local(String),
    /// The worktree archive exceeds the upload cap. Carries the observed
    /// (compressed) size, the cap, and a human hint naming the largest
    /// top-level paths so the user can `.gitignore` the offenders. Fails fast
    /// BEFORE the upload (see the cloud backend's `start`).
    #[error("source archive is {observed_bytes} bytes (cap {cap_bytes})")]
    SourceTooLarge {
        observed_bytes: u64,
        cap_bytes: u64,
        largest_paths: Vec<(String, u64)>,
    },
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

pub type Result<T> = std::result::Result<T, BackendError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_is_matchable_and_displayed() {
        let e = BackendError::Unauthorized;
        assert!(matches!(e, BackendError::Unauthorized));
        assert!(e.to_string().contains("authentication"));
    }

    #[test]
    fn rejected_carries_code_and_message() {
        let e = BackendError::Rejected {
            code: "build_rejected".into(),
            message: "bad IR".into(),
        };
        let s = e.to_string();
        assert!(s.contains("build_rejected") && s.contains("bad IR"));
    }
}
