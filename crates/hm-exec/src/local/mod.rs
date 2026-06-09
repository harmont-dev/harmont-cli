//! Local Docker execution backend.
pub mod runner;
mod scheduler;
mod events;
mod archive;
mod cache;
mod docker_client;
mod source;
mod workspace;

pub use source::{build_archive_bytes, write_archive}; // also used by CloudBackend
pub use events::EventBus;
pub use archive::ArchiveStore;
pub use docker_client::DockerClient;
pub use runner::docker::DockerRunner;
pub use runner::{RunnerRegistry, StepContext, StepRunner};
// Driven by `LocalDockerBackend` in the next task; unused until then.
#[allow(unused_imports)]
pub(crate) use scheduler::run;
