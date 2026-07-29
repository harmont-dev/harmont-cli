//! The process's runtime environment: terminal, session, and display facts.

use std::io::IsTerminal as _;

use crate::env::EnvVarProvider;

/// The terminal state of the standard streams, paired with the environment
/// facts they are interpreted against.
#[derive(Debug, Clone, Copy)]
pub struct Term<'env> {
    stdin: bool,
    stdout: bool,
    stderr: bool,
    env: &'env EnvVarProvider,
}

impl<'env> Term<'env> {
    /// Capture terminal state from the standard streams, interpreted against
    /// `env`.
    #[must_use]
    pub fn detect(env: &'env EnvVarProvider) -> Self {
        Self {
            stdin: std::io::stdin().is_terminal(),
            stdout: std::io::stdout().is_terminal(),
            stderr: std::io::stderr().is_terminal(),
            env,
        }
    }

    /// Whether the app can drive an interactive session: stdin and stdout are
    /// both terminals and no CI runner is present.
    #[must_use]
    pub const fn is_interactive(&self) -> bool {
        self.stdin && self.stdout && !self.env.is_ci()
    }

    /// Whether the environment permits ANSI color: `NO_COLOR` is unset and
    /// stderr is a terminal.
    #[must_use]
    pub const fn wants_color(&self) -> bool {
        !self.env.no_color() && self.stderr
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
        self.env.is_ci()
    }

    /// Whether the process is running inside an SSH session.
    #[must_use]
    pub const fn is_ssh(&self) -> bool {
        self.env.is_ssh()
    }

    /// Whether a graphical browser could plausibly be opened for the user.
    #[must_use]
    pub const fn has_gui(&self) -> bool {
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            !self.env.is_ssh()
        } else {
            self.env.has_display()
        }
    }
}
