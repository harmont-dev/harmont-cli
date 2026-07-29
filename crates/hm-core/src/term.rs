//! The process's runtime environment: terminal, session, and display facts
//! captured at startup.

use std::io::IsTerminal as _;

use crate::env::EnvVarProvider;

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
    /// Capture terminal state from the standard streams and CI signals, and the
    /// session/display facts from `env`.
    #[must_use]
    pub fn detect(env: &EnvVarProvider) -> Self {
        Self {
            stdin: std::io::stdin().is_terminal(),
            stdout: std::io::stdout().is_terminal(),
            stderr: std::io::stderr().is_terminal(),
            ci: is_ci::cached(),
            ssh: env.is_set("SSH_CONNECTION") || env.is_set("SSH_TTY") || env.is_set("SSH_CLIENT"),
            display: env.is_set("DISPLAY") || env.is_set("WAYLAND_DISPLAY"),
            no_color: env.is_present("NO_COLOR"),
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
