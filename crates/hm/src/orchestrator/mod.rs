//! Local-first build orchestration.
//!
//! The orchestrator owns the per-run state: the event bus that
//! announces `BuildEvent`s, the source-archive store served to
//! step-executor plugins, the cancellation atomic, and the chain
//! scheduler that dispatches each step to a plugin via the plan-1
//! plugin host.

pub mod archive;
pub mod cache;
pub mod docker_client;
pub mod events;
pub mod output_subscriber;
pub mod scheduler;
pub mod signal;
pub mod source;

pub use scheduler::run;

/// Build a Docker image by running a v0 IR pipeline as a one-shot build
/// container and committing the result to `image_tag`.
///
/// Used by `hm dev up` for `from_=Step` deployments. The pipeline's final
/// container becomes the new image; intermediate steps run in the same
/// container as the existing local executor does for `hm run`. On success
/// the build container is removed.
///
/// # Current status (v1 stub)
///
/// The inner call to `run_pipeline_v0_one_shot` is a stub. The existing
/// orchestrator (`scheduler::run`) commits each step's container to a new
/// image tag (`SnapshotRef`) but does not preserve the container id.
/// Wiring the return value requires extending `run_chain` and `StepResult`
/// — a change that exceeds the 50-line threshold specified in the plan
/// and is deferred to a dedicated task.
///
/// Raw `image=` deployments in `hm dev up` work end-to-end without this
/// wrapper; only `from_=Step` deployments are affected.
///
/// # Errors
///
/// Returns an error from the pipeline runner (currently always `Err` as
/// a stub) or from `commit_container` / `remove_container` if those are
/// ever reached.
pub async fn build_image_from_pipeline(
    docker: &crate::orchestrator::docker_client::DockerClient,
    pipeline_v0: &serde_json::Value,
    image_tag: &str,
) -> anyhow::Result<()> {
    // Reuse the existing local runner to execute the pipeline. It should
    // return the container id of the final step's container; we commit
    // that container into `image_tag`.
    //
    // NOTE: run_pipeline_v0_one_shot is currently a stub (see its doc
    // comment in commands/run/local.rs for the full rationale).
    let container_id = crate::commands::run::run_pipeline_v0_one_shot(docker, pipeline_v0).await?;
    docker.commit_container(&container_id, image_tag).await?;
    docker.remove_container(&container_id).await?;
    Ok(())
}

#[cfg(test)]
mod build_tests {
    // Compile-time existence check; the real integration is in
    // crates/hm/tests/dev_integration.rs.
    #[test]
    fn build_image_from_pipeline_is_callable() {
        // We can't run it without docker; just confirm the symbol exists
        // by taking a function pointer to it. The pointer is never called.
        // Lifetime annotation: the async fn borrows its args, so we use
        // a for<'a, 'b, 'c> higher-ranked bound.
        fn _assert_callable(
            d: &crate::orchestrator::docker_client::DockerClient,
            p: &serde_json::Value,
            t: &str,
        ) {
            // Calling build_image_from_pipeline returns a future; we just
            // verify the call type-checks, not that it runs.
            let _fut = super::build_image_from_pipeline(d, p, t);
            // `_fut` is dropped immediately — this is a compile check only.
        }
        // No assert; the test passes if it compiles.
    }
}
