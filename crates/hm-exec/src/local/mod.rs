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
pub(crate) use source::build_archive_bytes; // intra-crate: cloud/backend.rs via crate::local::
pub use docker_client::DockerClient; // external: hm/src/commands/cache/{save,restore,clean}.rs
pub(crate) use runner::docker::DockerRunner; // intra-crate: local/backend.rs via crate::local::
pub(crate) use runner::RunnerRegistry; // intra-crate: local/backend.rs via crate::local::
pub(crate) use scheduler::run;
pub(crate) use scheduler::chain_count;
