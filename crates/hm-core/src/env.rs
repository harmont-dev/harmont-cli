//! Process environment: the environment facts the CLI reads, captured at
//! startup.

/// The environment facts the CLI reads, captured once at startup so the rest of
/// the CLI reads them from here rather than hitting `std::env` inline.
#[derive(Debug, Clone, Default)]
pub struct EnvVarProvider {
    ssh_connection: Option<String>,
    ssh_tty: Option<String>,
    ssh_client: Option<String>,
    display: Option<String>,
    wayland_display: Option<String>,
    ci: bool,
    no_color: bool,
}

impl EnvVarProvider {
    /// Capture the current environment facts the CLI reads.
    #[must_use]
    pub fn init() -> Self {
        Self {
            ssh_connection: std::env::var("SSH_CONNECTION").ok(),
            ssh_tty: std::env::var("SSH_TTY").ok(),
            ssh_client: std::env::var("SSH_CLIENT").ok(),
            display: std::env::var("DISPLAY").ok(),
            wayland_display: std::env::var("WAYLAND_DISPLAY").ok(),
            ci: is_ci::cached(),
            no_color: std::env::var_os("NO_COLOR").is_some(),
        }
    }

    /// Whether a CI runner is detected.
    #[must_use]
    pub const fn is_ci(&self) -> bool {
        self.ci
    }

    /// Whether the process runs inside an SSH session (any `SSH_*` var set).
    #[must_use]
    pub const fn is_ssh(&self) -> bool {
        self.ssh_connection.is_some() || self.ssh_tty.is_some() || self.ssh_client.is_some()
    }

    /// Whether a display server is reachable (`DISPLAY` or `WAYLAND_DISPLAY` set).
    #[must_use]
    pub const fn has_display(&self) -> bool {
        self.display.is_some() || self.wayland_display.is_some()
    }

    /// Whether `NO_COLOR` is set (to any value), disabling ANSI color.
    #[must_use]
    pub const fn no_color(&self) -> bool {
        self.no_color
    }
}
