//! Local Docker execution backend.
pub mod runner;
mod backend;
mod scheduler;
mod events;
mod archive;
mod cache;
mod docker_client;
mod source;
// WorkspaceManager is a complete module carried forward from the Haskell
// migration. It is not yet wired into the Docker runner (the runner uses
// container filesystems instead). Scoped allows keep the whole `local`
// tree clean; remove when workspace is wired in.
#[allow(dead_code, unreachable_pub)]
mod workspace;

pub use backend::LocalDockerBackend;
pub use source::{build_archive_bytes, write_archive}; // also used by CloudBackend
pub use events::EventBus;
pub use archive::ArchiveStore;
pub use docker_client::DockerClient;
pub use runner::docker::DockerRunner;
pub use runner::{RunnerRegistry, StepContext, StepRunner};
pub(crate) use scheduler::run;
