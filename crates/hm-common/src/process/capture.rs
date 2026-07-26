//! Spawn a command and capture its output, with a typed success/failure split.

use std::borrow::Cow;
use std::future::Future;
use std::io;
use std::process::ExitStatus;
use std::str::Utf8Error;

mod sealed {
    pub trait Sealed {}
    impl Sealed for std::process::Command {}
    impl Sealed for tokio::process::Command {}
    impl Sealed for super::CapturedOk {}
    impl Sealed for super::CapturedError {}
}

/// A finished process: its captured stdout/stderr and exit status.
///
/// The output bytes are only readable after resolving the exit status with
/// [`success`](Captured::success).
#[derive(Debug, Clone)]
pub struct Captured {
    program: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: ExitStatus,
}

impl Captured {
    /// Split on the exit status: `Ok` if the process exited 0, else `Err`.
    pub fn success(self) -> Result<CapturedOk, CapturedError> {
        if self.status.success() {
            Ok(CapturedOk(self))
        } else {
            Err(CapturedError(self))
        }
    }

    /// The raw exit status.
    #[must_use]
    pub const fn status(&self) -> ExitStatus {
        self.status
    }

    /// The exit code, or `None` if the process was killed by a signal.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }
}

/// A process that exited successfully (status 0). Read output via [`CapturedStreams`].
#[derive(Debug, Clone)]
pub struct CapturedOk(Captured);

/// A process that exited non-zero.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{}", render_capture_error(.0))]
pub struct CapturedError(Captured);

fn describe_status(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "terminated by signal".to_string(),
        |code| format!("exited with status {code}"),
    )
}

fn render_capture_error(c: &Captured) -> String {
    use std::fmt::Write as _;

    let mut msg = format!("`{}` {}", c.program, describe_status(c.status));
    let stderr = String::from_utf8_lossy(&c.stderr);
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        const MAX: usize = 2000;
        let snippet: String = stderr.chars().take(MAX).collect();
        let ellipsis = if snippet.len() < stderr.len() { "…" } else { "" };
        let _ = write!(msg, ": {snippet}{ellipsis}");
    }
    msg
}

/// Accessors over a captured process's output, implemented by [`CapturedOk`] and
/// [`CapturedError`].
pub trait CapturedStreams: sealed::Sealed {
    /// Raw stdout bytes.
    fn stdout(&self) -> &[u8];
    /// Raw stderr bytes.
    fn stderr(&self) -> &[u8];
    /// The exit status.
    fn status(&self) -> ExitStatus;

    /// The exit code, or `None` if killed by a signal.
    fn code(&self) -> Option<i32> {
        self.status().code()
    }

    /// stdout as UTF-8, borrowed.
    fn stdout_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(self.stdout())
    }
    /// stderr as UTF-8, borrowed.
    fn stderr_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(self.stderr())
    }

    /// stdout as an owned UTF-8 `String`.
    fn stdout_string(&self) -> Result<String, Utf8Error> {
        self.stdout_str().map(str::to_owned)
    }
    /// stderr as an owned UTF-8 `String`.
    fn stderr_string(&self) -> Result<String, Utf8Error> {
        self.stderr_str().map(str::to_owned)
    }

    /// stdout as `&str`, **panicking** if not valid UTF-8.
    #[allow(clippy::expect_used, reason = "callers opt into panic-on-invalid by calling _unwrap")]
    fn stdout_str_unwrap(&self) -> &str {
        self.stdout_str().expect("stdout was not valid UTF-8")
    }
    /// stderr as `&str`, **panicking** if not valid UTF-8.
    #[allow(clippy::expect_used, reason = "callers opt into panic-on-invalid by calling _unwrap")]
    fn stderr_str_unwrap(&self) -> &str {
        self.stderr_str().expect("stderr was not valid UTF-8")
    }

    /// stdout as an owned `String`, **panicking** if not valid UTF-8.
    #[allow(clippy::expect_used, reason = "callers opt into panic-on-invalid by calling _unwrap")]
    fn stdout_string_unwrap(&self) -> String {
        self.stdout_string().expect("stdout was not valid UTF-8")
    }
    /// stderr as an owned `String`, **panicking** if not valid UTF-8.
    #[allow(clippy::expect_used, reason = "callers opt into panic-on-invalid by calling _unwrap")]
    fn stderr_string_unwrap(&self) -> String {
        self.stderr_string().expect("stderr was not valid UTF-8")
    }

    /// stdout decoded lossily (invalid sequences become U+FFFD). Never fails.
    fn stdout_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.stdout())
    }
    /// stderr decoded lossily (invalid sequences become U+FFFD). Never fails.
    fn stderr_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.stderr())
    }
}

impl CapturedStreams for CapturedOk {
    fn stdout(&self) -> &[u8] {
        &self.0.stdout
    }
    fn stderr(&self) -> &[u8] {
        &self.0.stderr
    }
    fn status(&self) -> ExitStatus {
        self.0.status
    }
}

impl CapturedStreams for CapturedError {
    fn stdout(&self) -> &[u8] {
        &self.0.stdout
    }
    fn stderr(&self) -> &[u8] {
        &self.0.stderr
    }
    fn status(&self) -> ExitStatus {
        self.0.status
    }
}

/// Adds [`captured`](CommandExt::captured) to [`std::process::Command`].
pub trait CommandExt: sealed::Sealed {
    /// Spawn, wait for completion, and capture stdout/stderr and exit status.
    fn captured(&mut self) -> io::Result<Captured>;
}

impl CommandExt for std::process::Command {
    #[tracing::instrument(skip(self), fields(program = %self.get_program().to_string_lossy()))]
    fn captured(&mut self) -> io::Result<Captured> {
        let program = self.get_program().to_string_lossy().into_owned();
        let out = self.output()?;
        Ok(Captured {
            program,
            stdout: out.stdout,
            stderr: out.stderr,
            status: out.status,
        })
    }
}

/// Adds [`captured`](AsyncCommandExt::captured) to [`tokio::process::Command`].
pub trait AsyncCommandExt: sealed::Sealed {
    /// Spawn, await completion, and capture stdout/stderr and exit status.
    fn captured(&mut self) -> impl Future<Output = io::Result<Captured>>;
}

impl AsyncCommandExt for tokio::process::Command {
    #[tracing::instrument(skip(self), fields(program = %self.as_std().get_program().to_string_lossy()))]
    async fn captured(&mut self) -> io::Result<Captured> {
        let program = self.as_std().get_program().to_string_lossy().into_owned();
        let out = self.output().await?;
        Ok(Captured {
            program,
            stdout: out.stdout,
            stderr: out.stderr,
            status: out.status,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use rstest::rstest;

    /// `sh -c <script>` — portable across this repo's unix CI targets.
    fn sh(script: &str) -> std::process::Command {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }

    #[rstest]
    #[case::plain("printf hello", "hello")]
    #[case::trailing_newline("echo hi", "hi\n")]
    #[case::multiline("printf 'a\\nb'", "a\nb")]
    #[case::empty("true", "")]
    fn captures_stdout_on_success(#[case] script: &str, #[case] expected: &str) {
        let ok = sh(script).captured().unwrap().success().unwrap();
        assert_eq!(ok.stdout_string().unwrap(), expected);
    }

    #[rstest]
    #[case::exit_1("exit 1", 1)]
    #[case::exit_3("exit 3", 3)]
    #[case::false_builtin("false", 1)]
    fn nonzero_exit_is_error_carrying_the_code(#[case] script: &str, #[case] code: i32) {
        let err = sh(script).captured().unwrap().success().unwrap_err();
        assert_eq!(err.code(), Some(code));
    }

    #[rstest]
    fn error_display_names_program_status_and_stderr() {
        let err = sh("echo boom 1>&2; exit 2")
            .captured()
            .unwrap()
            .success()
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("sh"), "program missing: {msg}");
        assert!(msg.contains("status 2"), "status missing: {msg}");
        assert!(msg.contains("boom"), "stderr missing: {msg}");
    }

    #[rstest]
    fn stderr_is_captured_on_success() {
        let ok = sh("echo note 1>&2").captured().unwrap().success().unwrap();
        assert_eq!(ok.stderr_string().unwrap().trim(), "note");
    }

    #[rstest]
    fn spawn_failure_is_an_io_error() {
        let result = std::process::Command::new("hm-common-no-such-binary-xyz").captured();
        assert!(result.is_err());
    }

    #[rstest]
    fn non_utf8_stdout_reads_as_bytes_and_lossy() {
        let ok = sh(r"printf '\377'").captured().unwrap().success().unwrap();
        assert_eq!(ok.stdout(), b"\xff");
        assert!(ok.stdout_str().is_err());
        assert!(ok.stdout_lossy().contains('\u{FFFD}'));
    }

    #[rstest]
    #[should_panic(expected = "not valid UTF-8")]
    fn string_unwrap_panics_on_invalid_utf8() {
        let ok = sh(r"printf '\377'").captured().unwrap().success().unwrap();
        let _ = ok.stdout_string_unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn async_captured_reads_stdout() {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg("printf async-hi");
        let ok = cmd.captured().await.unwrap().success().unwrap();
        assert_eq!(ok.stdout_string().unwrap(), "async-hi");
    }
}
