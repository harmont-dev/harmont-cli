//! Static runner interface.
//!
//! This module replaces the old WASM plugin system with a static DI
//! approach. Step executors implement [`StepRunner`]; output formatters
//! implement [`OutputRenderer`]. A [`RunnerRegistry`] maps runner names
//! to concrete implementations at startup.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use hm_plugin_protocol::{BuildEvent, ExecutorInput, StepResult};
use tokio_util::sync::CancellationToken;

use crate::orchestrator::archive::ArchiveStore;
use crate::orchestrator::docker_client::DockerClient;
use crate::orchestrator::events::EventBus;
use crate::orchestrator::workspace::WorkspaceManager;

pub mod docker;

// ---------------------------------------------------------------------------
// RunContext
// ---------------------------------------------------------------------------

/// Shared context threaded into every runner invocation.
///
/// Replaces the monolithic `OrchestratorState` that the old plugin
/// system passed as opaque host memory. All fields are cheaply
/// cloneable (`Arc` / `CancellationToken` / `DockerClient`).
#[derive(Clone, Debug)]
pub struct RunContext {
    pub docker: DockerClient,
    pub event_bus: Arc<EventBus>,
    pub archives: Arc<ArchiveStore>,
    pub cancel: CancellationToken,
    pub workspace: Arc<Mutex<WorkspaceManager>>,
}

// ---------------------------------------------------------------------------
// StepRunner trait
// ---------------------------------------------------------------------------

/// Async trait implemented by step executors (e.g. the Docker runner).
///
/// Each runner is identified by a string [`Self::name`] that pipeline
/// authors reference in their step definitions.
///
/// The `execute` method returns a boxed future so the trait remains
/// dyn-compatible (async fn in trait is not object-safe).
pub trait StepRunner: Send + Sync + fmt::Debug {
    /// Unique name for this runner (e.g. `"docker"`).
    fn name(&self) -> &str;

    /// Execute a single pipeline step.
    ///
    /// # Errors
    ///
    /// Implementations should return `Err` for infrastructure failures
    /// (container boot failure, network error, etc.). A non-zero exit
    /// code from the user command is **not** an error — it is reported
    /// via [`StepResult::exit_code`].
    fn execute(
        &self,
        ctx: &RunContext,
        input: ExecutorInput,
    ) -> Pin<Box<dyn Future<Output = Result<StepResult>> + Send + '_>>;
}

// ---------------------------------------------------------------------------
// OutputRenderer trait
// ---------------------------------------------------------------------------

/// Synchronous observer of [`BuildEvent`]s.
///
/// Implementations format events for human consumption (progress bars,
/// coloured log lines) or machine consumption (JSON-lines).
pub trait OutputRenderer: Send + fmt::Debug {
    /// Called once per event in emission order.
    fn on_event(&mut self, event: &BuildEvent);
}

// ---------------------------------------------------------------------------
// RunnerRegistry
// ---------------------------------------------------------------------------

/// Maps runner names to [`StepRunner`] implementations.
///
/// Constructed once at startup and shared immutably for the duration
/// of the run.
#[derive(Default)]
pub struct RunnerRegistry {
    runners: HashMap<String, Arc<dyn StepRunner>>,
    default: Option<String>,
}

impl RunnerRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            runners: HashMap::new(),
            default: None,
        }
    }

    /// Register a runner. When `is_default` is true the runner's name
    /// becomes the fallback used by [`Self::resolve`] when no explicit
    /// name is given.
    pub fn register(&mut self, runner: Arc<dyn StepRunner>, is_default: bool) {
        let name = runner.name().to_owned();
        if is_default {
            self.default = Some(name.clone());
        }
        self.runners.insert(name, runner);
    }

    /// Look up a runner by name, falling back to the default when
    /// `name` is `None`.
    #[must_use]
    pub fn resolve(&self, name: Option<&str>) -> Option<Arc<dyn StepRunner>> {
        let key = name.or(self.default.as_deref())?;
        self.runners.get(key).cloned()
    }

    /// The name of the current default runner, if one has been set.
    #[must_use]
    pub fn default_runner_name(&self) -> Option<&str> {
        self.default.as_deref()
    }

    /// Sorted list of all registered runner names.
    #[must_use]
    pub fn runner_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.runners.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }
}

impl fmt::Debug for RunnerRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunnerRegistry")
            .field("runners", &self.runners.keys().collect::<Vec<_>>())
            .field("default", &self.default)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Minimal stub runner for unit tests.
    #[derive(Debug)]
    struct StubRunner {
        runner_name: String,
    }

    impl StubRunner {
        fn new(name: &str) -> Self {
            Self {
                runner_name: name.to_owned(),
            }
        }
    }

    impl StepRunner for StubRunner {
        fn name(&self) -> &str {
            &self.runner_name
        }

        fn execute(
            &self,
            _ctx: &RunContext,
            _input: ExecutorInput,
        ) -> Pin<Box<dyn Future<Output = Result<StepResult>> + Send + '_>> {
            Box::pin(async {
                Ok(StepResult {
                    exit_code: 0,
                    committed_snapshot: None,
                    artifacts: vec![],
                })
            })
        }
    }

    #[test]
    fn resolve_by_name() {
        let mut reg = RunnerRegistry::new();
        reg.register(Arc::new(StubRunner::new("docker")), false);
        reg.register(Arc::new(StubRunner::new("local")), false);

        let runner = reg.resolve(Some("docker")).unwrap();
        assert_eq!(runner.name(), "docker");

        let runner = reg.resolve(Some("local")).unwrap();
        assert_eq!(runner.name(), "local");

        assert!(reg.resolve(Some("nope")).is_none());
    }

    #[test]
    fn resolve_default() {
        let mut reg = RunnerRegistry::new();
        reg.register(Arc::new(StubRunner::new("docker")), true);
        reg.register(Arc::new(StubRunner::new("local")), false);

        // `None` name falls back to default.
        let runner = reg.resolve(None).unwrap();
        assert_eq!(runner.name(), "docker");
        assert_eq!(reg.default_runner_name(), Some("docker"));
    }

    #[test]
    fn no_default_returns_none() {
        let mut reg = RunnerRegistry::new();
        reg.register(Arc::new(StubRunner::new("docker")), false);

        assert!(reg.resolve(None).is_none());
        assert!(reg.default_runner_name().is_none());
    }

    #[test]
    fn runner_names_sorted() {
        let mut reg = RunnerRegistry::new();
        reg.register(Arc::new(StubRunner::new("zeta")), false);
        reg.register(Arc::new(StubRunner::new("alpha")), false);
        reg.register(Arc::new(StubRunner::new("mid")), false);

        assert_eq!(reg.runner_names(), vec!["alpha", "mid", "zeta"]);
    }

    #[test]
    fn debug_impl() {
        let reg = RunnerRegistry::new();
        // Just ensure it doesn't panic.
        let _ = format!("{reg:?}");
    }
}
