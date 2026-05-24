use thiserror::Error;

/// Exit codes for the CLI.
pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_BUILD_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_AUTH: i32 = 3;
pub const EXIT_NETWORK: i32 = 4;
pub const EXIT_API: i32 = 5;
/// Pipeline-level invalid configuration (unknown runner, no default executor).
pub const EXIT_PIPELINE_INVALID: i32 = 7;

#[derive(Debug, Error)]
pub enum HmError {
    #[error("not authenticated\n  → run `hm login`")]
    NotAuthenticated,

    #[error("no active organization\n  → run `hm org switch <slug>` or set HARMONT_ORG=<slug>")]
    NoOrganization,

    #[error("API error (HTTP {status}): {message}")]
    Api { status: u16, message: String },

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error(
        "source archive exceeds the {max_mb} MB limit\n  → trim the source tree or add ignores in .harmontignore"
    )]
    ArchiveTooLarge { max_mb: u64 },

    #[error("pipeline not found: {slug}\n  → list available pipelines with `hm pipeline list`")]
    PipelineNotFound { slug: String },

    #[error(
        "error: manual builds are disabled for this pipeline\n  \u{2192} ask the pipeline owner to set allow_manual=True\n\nhm run --help   for more"
    )]
    PipelineManualDisabled,

    #[error("configuration error: {0}")]
    Config(String),

    #[error("docker error: {0}\n  → check that the Docker daemon is running (`docker version`)")]
    Docker(String),

    #[error("DSL engine error: {0}")]
    DslEngine(String),

    #[error("pipeline render error: {0}")]
    PipelineRender(String),

    #[error("local scheduler error: {0}")]
    LocalScheduling(String),

    #[error(
        "step '{step_key}' requested runner '{runner}', but no runner provides it (available: {available:?})"
    )]
    UnknownRunner {
        step_key: String,
        runner: String,
        available: Vec<String>,
    },

    #[error("no default step executor is registered")]
    NoDefaultExecutor,

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Coarse error category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    BuildFailed,
    Usage,
    Auth,
    Network,
    Api,
    PipelineInvalid,
}

impl ErrorCategory {
    #[must_use]
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::BuildFailed => EXIT_BUILD_FAILED,
            Self::Usage => EXIT_USAGE,
            Self::Auth => EXIT_AUTH,
            Self::Network => EXIT_NETWORK,
            Self::Api => EXIT_API,
            Self::PipelineInvalid => EXIT_PIPELINE_INVALID,
        }
    }
}

impl HmError {
    #[must_use]
    pub const fn category(&self) -> ErrorCategory {
        match self {
            Self::NotAuthenticated => ErrorCategory::Auth,
            Self::Api { status, .. } if *status == 401 || *status == 403 => ErrorCategory::Auth,
            Self::NoOrganization
            | Self::ArchiveTooLarge { .. }
            | Self::Config(_)
            | Self::DslEngine(_)
            | Self::PipelineRender(_) => ErrorCategory::Usage,
            Self::Api { .. }
            | Self::PipelineNotFound { .. }
            | Self::PipelineManualDisabled
            | Self::LocalScheduling(_) => ErrorCategory::Api,
            Self::Network(_) | Self::Docker(_) => ErrorCategory::Network,
            Self::UnknownRunner { .. } | Self::NoDefaultExecutor => ErrorCategory::PipelineInvalid,
            Self::Other(_) => ErrorCategory::BuildFailed,
        }
    }

    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        self.category().exit_code()
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorCategory, HmError};

    #[test]
    fn pipeline_manual_disabled_renders_section5_shape() {
        let s = format!("{}", HmError::PipelineManualDisabled);
        assert_eq!(
            s,
            "error: manual builds are disabled for this pipeline\n  \u{2192} ask the pipeline owner to set allow_manual=True\n\nhm run --help   for more"
        );
    }

    #[test]
    fn pipeline_manual_disabled_is_api_category() {
        assert_eq!(
            HmError::PipelineManualDisabled.category(),
            ErrorCategory::Api
        );
        assert_eq!(HmError::PipelineManualDisabled.exit_code(), super::EXIT_API);
    }
}
