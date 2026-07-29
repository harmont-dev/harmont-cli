//! Process environment: the environment variables the CLI reads, captured at
//! startup.

/// The environment variables the CLI reads, captured once at startup so the
/// rest of the CLI reads them from here rather than hitting `std::env` inline.
#[derive(Debug, Clone, Default)]
pub struct EnvVarProvider {
    ssh_connection: Option<String>,
    ssh_tty: Option<String>,
    ssh_client: Option<String>,
    display: Option<String>,
    wayland_display: Option<String>,
    no_color: bool,
}

impl EnvVarProvider {
    /// Capture the current values of the environment variables the CLI reads.
    #[must_use]
    pub fn init() -> Self {
        Self {
            ssh_connection: std::env::var("SSH_CONNECTION").ok(),
            ssh_tty: std::env::var("SSH_TTY").ok(),
            ssh_client: std::env::var("SSH_CLIENT").ok(),
            display: std::env::var("DISPLAY").ok(),
            wayland_display: std::env::var("WAYLAND_DISPLAY").ok(),
            no_color: std::env::var_os("NO_COLOR").is_some(),
        }
    }

    /// `SSH_CONNECTION` — set for a session opened over SSH.
    #[must_use]
    pub fn ssh_connection(&self) -> Option<&str> {
        self.ssh_connection.as_deref()
    }

    /// `SSH_TTY` — the SSH session's controlling terminal.
    #[must_use]
    pub fn ssh_tty(&self) -> Option<&str> {
        self.ssh_tty.as_deref()
    }

    /// `SSH_CLIENT` — the SSH client's address.
    #[must_use]
    pub fn ssh_client(&self) -> Option<&str> {
        self.ssh_client.as_deref()
    }

    /// `DISPLAY` — the X11 display, when a graphical session is reachable.
    #[must_use]
    pub fn display(&self) -> Option<&str> {
        self.display.as_deref()
    }

    /// `WAYLAND_DISPLAY` — the Wayland display, when a graphical session is
    /// reachable.
    #[must_use]
    pub fn wayland_display(&self) -> Option<&str> {
        self.wayland_display.as_deref()
    }

    /// Whether `NO_COLOR` is set (to any value), disabling ANSI color.
    #[must_use]
    pub const fn no_color(&self) -> bool {
        self.no_color
    }
}
