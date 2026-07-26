//! Running python3.

use std::ffi::OsStr;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::process::{AsyncCommandExt as _, CapturedError, CapturedOk, CapturedStreams as _};

/// A resolved `python3` interpreter.
#[derive(Debug, Clone, Copy)]
pub struct Python<'bin> {
    bin: &'bin Path,
}

impl<'bin> Python<'bin> {
    /// Wrap a `python3` executable, typically `AppRuntime::bins().python3()`.
    #[must_use]
    pub const fn new(bin: &'bin Path) -> Self {
        Self { bin }
    }

    /// An inline program: `python3 -c <script>`. Trailing [`arg`](PyCommand)s
    /// become `sys.argv[1:]`.
    pub fn program(&self, script: impl AsRef<OsStr>) -> PyCommand {
        let mut cmd = tokio::process::Command::new(self.bin);
        cmd.arg("-c").arg(script);
        PyCommand::wrap(cmd)
    }

    /// A module run: `python3 -m <module>`.
    pub fn module(&self, module: impl AsRef<OsStr>) -> PyCommand {
        let mut cmd = tokio::process::Command::new(self.bin);
        cmd.arg("-m").arg(module);
        PyCommand::wrap(cmd)
    }

    /// A script file: `python3 <path>`.
    pub fn script(&self, path: impl AsRef<Path>) -> PyCommand {
        let mut cmd = tokio::process::Command::new(self.bin);
        cmd.arg(path.as_ref());
        PyCommand::wrap(cmd)
    }
}

/// A `python3` invocation. Derefs to [`tokio::process::Command`].
#[derive(Debug)]
#[must_use = "a PyCommand does nothing until run"]
pub struct PyCommand {
    cmd: tokio::process::Command,
}

impl PyCommand {
    fn wrap(mut cmd: tokio::process::Command) -> Self {
        cmd.env("PYTHONDONTWRITEBYTECODE", "1");
        Self { cmd }
    }

    /// Append a `PYTHONPATH` entry.
    pub fn pythonpath(&mut self, path: impl AsRef<Path>) -> &mut Self {
        let mut entries: Vec<PathBuf> = self
            .cmd
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("PYTHONPATH"))
            .and_then(|(_, value)| value)
            .map(|value| std::env::split_paths(value).collect())
            .unwrap_or_default();
        entries.push(path.as_ref().to_path_buf());
        if let Ok(joined) = std::env::join_paths(entries) {
            self.cmd.env("PYTHONPATH", joined);
        }
        self
    }

    /// Run to completion.
    ///
    /// # Errors
    /// [`PyError::Spawn`] if `python3` cannot be spawned; [`PyError::Failed`]
    /// if it exits non-zero.
    pub async fn run(&mut self) -> Result<PyOutput, PyError> {
        let ok = self
            .cmd
            .captured()
            .await
            .map_err(PyError::Spawn)?
            .success()
            .map_err(PyError::Failed)?;
        Ok(PyOutput(ok))
    }
}

impl Deref for PyCommand {
    type Target = tokio::process::Command;
    fn deref(&self) -> &Self::Target {
        &self.cmd
    }
}

impl DerefMut for PyCommand {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cmd
    }
}

/// A successful `python3` run: [`CapturedOk`] plus JSON decoding of stdout.
#[derive(Debug)]
pub struct PyOutput(CapturedOk);

impl PyOutput {
    /// Deserialize stdout as JSON.
    ///
    /// # Errors
    /// If stdout is not valid JSON for `T`.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(self.0.stdout())
    }

    /// The underlying captured output.
    #[must_use]
    pub fn into_inner(self) -> CapturedOk {
        self.0
    }
}

impl Deref for PyOutput {
    type Target = CapturedOk;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A `python3` invocation that produced no output.
#[derive(Debug, thiserror::Error)]
pub enum PyError {
    /// `python3` could not be spawned.
    #[error("spawning `python3`")]
    Spawn(#[source] std::io::Error),
    /// `python3` exited non-zero.
    #[error(transparent)]
    Failed(#[from] CapturedError),
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::print_stderr,
    clippy::panic,
    reason = "test setup, assertions, and skip diagnostics"
)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// The system `python3`, or `None` to skip on hosts without it.
    fn python3() -> Option<PathBuf> {
        crate::process::pathbin("python3").ok()
    }

    #[rstest]
    #[case::print("print('hello')", "hello\n")]
    #[case::no_newline("import sys; sys.stdout.write('hi')", "hi")]
    #[case::empty("pass", "")]
    #[tokio::test]
    async fn program_captures_stdout(#[case] script: &str, #[case] expected: &str) {
        let Some(bin) = python3() else {
            eprintln!("skipping: python3 not on PATH");
            return;
        };
        let out = Python::new(&bin).program(script).run().await.unwrap();
        assert_eq!(out.stdout_string().unwrap(), expected);
    }

    #[rstest]
    #[tokio::test]
    async fn nonzero_exit_is_failed_carrying_stderr() {
        let Some(bin) = python3() else {
            eprintln!("skipping: python3 not on PATH");
            return;
        };
        let err = Python::new(&bin)
            .program("import sys; sys.stderr.write('boom'); sys.exit(3)")
            .run()
            .await
            .unwrap_err();
        match err {
            PyError::Failed(e) => {
                assert_eq!(e.code(), Some(3));
                assert!(e.stderr_string().unwrap().contains("boom"));
            }
            PyError::Spawn(e) => panic!("expected Failed, got Spawn({e:?})"),
        }
    }

    #[rstest]
    #[tokio::test]
    async fn missing_interpreter_is_a_spawn_error() {
        let bin = PathBuf::from("hm-common-no-such-python-xyz");
        let err = Python::new(&bin)
            .program("print(1)")
            .run()
            .await
            .unwrap_err();
        assert!(matches!(err, PyError::Spawn(_)));
    }

    #[rstest]
    #[tokio::test]
    async fn json_decodes_stdout() {
        let Some(bin) = python3() else {
            eprintln!("skipping: python3 not on PATH");
            return;
        };
        let out = Python::new(&bin)
            .program("import json; print(json.dumps({'a': 1, 'b': [2, 3]}))")
            .run()
            .await
            .unwrap();
        let value: serde_json::Value = out.json().unwrap();
        assert_eq!(value["a"], 1);
        assert_eq!(value["b"][1], 3);
    }

    #[rstest]
    #[tokio::test]
    async fn positional_args_become_sys_argv() {
        let Some(bin) = python3() else {
            eprintln!("skipping: python3 not on PATH");
            return;
        };
        let mut py = Python::new(&bin).program("import sys; print(' '.join(sys.argv[1:]))");
        py.args(["one", "two"]);
        let out = py.run().await.unwrap();
        assert_eq!(out.stdout_string().unwrap(), "one two\n");
    }

    #[rstest]
    #[tokio::test]
    async fn current_dir_sets_the_working_directory() {
        let Some(bin) = python3() else {
            eprintln!("skipping: python3 not on PATH");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let mut py = Python::new(&bin).program("import os; print(os.getcwd())");
        py.current_dir(dir.path());
        let out = py.run().await.unwrap();
        let got = std::fs::canonicalize(out.stdout_string().unwrap().trim()).unwrap();
        let want = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(got, want);
    }

    #[rstest]
    #[tokio::test]
    async fn pythonpath_makes_a_module_importable() {
        let Some(bin) = python3() else {
            eprintln!("skipping: python3 not on PATH");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mymod.py"), "VALUE = 42\n").unwrap();
        let mut py = Python::new(&bin).program("import mymod; print(mymod.VALUE)");
        py.pythonpath(dir.path());
        let out = py.run().await.unwrap();
        assert_eq!(out.stdout_string().unwrap().trim(), "42");
    }

    #[rstest]
    #[tokio::test]
    async fn pythonpath_appends_across_calls() {
        let Some(bin) = python3() else {
            eprintln!("skipping: python3 not on PATH");
            return;
        };
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("mod_a.py"), "A = 1\n").unwrap();
        std::fs::write(b.path().join("mod_b.py"), "B = 2\n").unwrap();
        let mut py = Python::new(&bin).program("import mod_a, mod_b; print(mod_a.A + mod_b.B)");
        py.pythonpath(a.path());
        py.pythonpath(b.path());
        let out = py.run().await.unwrap();
        assert_eq!(out.stdout_string().unwrap().trim(), "3");
    }

    #[rstest]
    #[tokio::test]
    async fn module_form_runs_a_module() {
        let Some(bin) = python3() else {
            eprintln!("skipping: python3 not on PATH");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mymod.py"), "print('from module')\n").unwrap();
        let mut py = Python::new(&bin).module("mymod");
        py.pythonpath(dir.path());
        let out = py.run().await.unwrap();
        assert_eq!(out.stdout_string().unwrap(), "from module\n");
    }

    #[rstest]
    #[tokio::test]
    async fn script_form_runs_a_file() {
        let Some(bin) = python3() else {
            eprintln!("skipping: python3 not on PATH");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("s.py");
        std::fs::write(&script, "print('from file')\n").unwrap();
        let out = Python::new(&bin).script(&script).run().await.unwrap();
        assert_eq!(out.stdout_string().unwrap(), "from file\n");
    }
}
