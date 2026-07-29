//! The process's runtime environment: terminal, session, and display facts
//! captured at startup.

use std::io::IsTerminal as _;

/// The runtime environment facts, captured once at startup.
#[derive(Debug, Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent environment facts, not a packed state machine"
)]
pub struct Term {
    stdin: bool,
    stdout: bool,
    stderr: bool,
    ci: bool,
    ssh: bool,
    display: bool,
    no_color: bool,
}

impl Term {
    /// Capture the environment from the standard streams, CI signals, the SSH
    /// session variables, and the display variables.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            stdin: std::io::stdin().is_terminal(),
            stdout: std::io::stdout().is_terminal(),
            stderr: std::io::stderr().is_terminal(),
            ci: is_ci::cached(),
            ssh: env_present("SSH_CONNECTION")
                || env_present("SSH_TTY")
                || env_present("SSH_CLIENT"),
            display: env_present("DISPLAY") || env_present("WAYLAND_DISPLAY"),
            no_color: std::env::var_os("NO_COLOR").is_some(),
        }
    }

    /// Whether the app can drive an interactive session: stdin and stdout are
    /// both terminals and no CI runner is present.
    #[must_use]
    pub const fn is_interactive(&self) -> bool {
        self.stdin && self.stdout && !self.ci
    }

    /// Whether the environment permits ANSI color: `NO_COLOR` is unset and
    /// stderr is a terminal.
    #[must_use]
    pub const fn wants_color(&self) -> bool {
        !self.no_color && self.stderr
    }

    /// Whether stdin is a terminal.
    #[must_use]
    pub const fn stdin_is_tty(&self) -> bool {
        self.stdin
    }

    /// Whether stdout is a terminal.
    #[must_use]
    pub const fn stdout_is_tty(&self) -> bool {
        self.stdout
    }

    /// Whether stderr is a terminal.
    #[must_use]
    pub const fn stderr_is_tty(&self) -> bool {
        self.stderr
    }

    /// Whether a CI runner is detected.
    #[must_use]
    pub const fn is_ci(&self) -> bool {
        self.ci
    }

    /// Whether the process is running inside an SSH session.
    #[must_use]
    pub const fn is_ssh(&self) -> bool {
        self.ssh
    }

    /// Whether a graphical browser could plausibly be opened for the user.
    #[must_use]
    pub const fn has_gui(&self) -> bool {
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            !self.ssh
        } else {
            self.display
        }
    }
}

/// Whether an environment variable is set to a non-empty value.
fn env_present(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}
