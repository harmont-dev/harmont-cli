//! Built-in Docker step-executor plugin for the hm CLI.
//!
//! The host registers this plugin embedded (via `include_bytes!`) and
//! dispatches every `CommandStep` whose `runner` is `None` or
//! `"docker"` to it.

#![allow(unsafe_code, reason = "extism-pdk host_fn imports require unsafe")]
#![allow(
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo,
    clippy::multiple_crate_versions,
    clippy::cargo_common_metadata,
    clippy::missing_errors_doc,
    reason = "matches the test-fixtures allow-list; plugin authoring crate"
)]

use hm_plugin_sdk::*;

mod decision;
mod extism_host;
mod image_name;

#[derive(Default)]
struct DockerExec;

impl StepExecutor for DockerExec {
    fn run(&self, input: ExecutorInput) -> Result<StepResult, PluginError> {
        run_step(input)
    }
}

fn run_step(input: ExecutorInput) -> Result<StepResult, PluginError> {
    use crate::decision::plan;
    use crate::extism_host as host;
    use crate::image_name::resolve_image;
    use hm_plugin_protocol::{
        DockerCommitArgs, DockerExecArgs, DockerExtractArgs, DockerStartArgs,
    };

    let plan = plan(&input.cache_lookup);

    // Cache hit shortcut: no container, no exec; we still hand back
    // the hit tag so chain-downstream steps can boot from it.
    if !plan.run_command {
        return Ok(StepResult {
            exit_code: 0,
            committed_snapshot: plan.hit_tag.clone(),
            artifacts: vec![],
        });
    }

    let image = resolve_image(
        &input.step,
        plan.hit_tag.as_ref(),
        input.parent_snapshot.as_ref(),
    );
    let container_name = sanitize_container_name(&input.run_id.to_string(), &input.step.key);

    // Ensure the image is locally available — pull if needed.
    if !host::image_exists(&image) {
        host::pull(&image)
            .map_err(|e| PluginError::new("docker_pull_failed", format!("pull '{image}': {e}")))?;
    }

    let cid = host::start_container(DockerStartArgs {
        image: image.clone(),
        env: input.env.clone(),
        workdir: input.workdir.clone(),
        name_hint: container_name,
    })
    .map_err(|e| PluginError::new("docker_start_failed", e.to_string()))?;

    // RAII-equivalent cleanup tracker. We don't have Drop in WASM
    // host-fn land (panics there aren't recoverable cleanly), so use
    // an explicit cleanup helper at every early-return.
    macro_rules! cleanup_and_return {
        ($result:expr) => {{
            host::stop_remove(&cid);
            return $result;
        }};
    }

    // Extract the user's source archive onto /workspace.
    if let Err(e) = host::extract_workspace(DockerExtractArgs {
        container_id: cid.clone(),
        archive_id: input.workspace_archive_id,
        workdir: input.workdir.clone(),
    }) {
        cleanup_and_return!(Err(PluginError::new(
            "docker_extract_failed",
            e.to_string()
        )));
    }

    // Exec the step's command. Logs stream live into the event bus
    // via the host's StepLogWriter — the plugin only sees the exit
    // code.
    let exit_code = match host::exec(DockerExecArgs {
        container_id: cid.clone(),
        cmd: vec!["sh".into(), "-c".into(), input.step.cmd.clone()],
        env: input.env.clone(),
        workdir: input.workdir.clone(),
        stdin_archive_id: None,
    }) {
        Ok(rc) => rc,
        Err(e) => cleanup_and_return!(Err(PluginError::new("docker_exec_failed", e.to_string()))),
    };

    // Always commit on success — under the new orchestrator the
    // scheduler threads the committed snapshot to the next step in
    // the chain (and to fork children). If the host already chose a
    // tag (cache-build path), use it; otherwise mint an ephemeral
    // tag scoped by step_id so concurrent / replayed runs don't
    // collide.
    let committed = if exit_code == 0 {
        let target_tag = plan.commit_to.clone().unwrap_or_else(|| {
            let safe: String = input
                .step
                .key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect();
            hm_plugin_protocol::SnapshotRef::from(format!(
                "harmont-local-ephemeral/{safe}:run-{}",
                input.step_id.simple()
            ))
        });
        match host::commit(DockerCommitArgs {
            container_id: cid.clone(),
            tag: target_tag.to_string(),
        }) {
            Ok(_) => Some(target_tag),
            Err(e) => {
                cleanup_and_return!(Err(PluginError::new("docker_commit_failed", e.to_string())))
            }
        }
    } else {
        None
    };

    host::stop_remove(&cid);

    Ok(StepResult {
        exit_code,
        committed_snapshot: committed,
        artifacts: vec![],
    })
}

fn sanitize_container_name(run_id: &str, step_key: &str) -> String {
    let run_short: String = run_id.chars().take(8).collect();
    let key: String = step_key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("harmont-{run_short}-{key}")
}

register_plugin!(
    manifest = PluginManifest {
        api_version: HM_PLUGIN_API_VERSION,
        name: "harmont-docker".into(),
        version: semver::Version::new(0, 1, 0),
        description: "Docker step executor (default runner).".into(),
        capabilities: vec![Capability::StepExecutor(StepExecutorSpec {
            runner: "docker".into(),
            default: true,
            step_schema: None,
        })],
        required_host_fns: vec![
            "hm_log".into(),
            "hm_emit_step_log".into(),
            "hm_should_cancel".into(),
            "hm_docker_ping".into(),
            "hm_docker_image_exists".into(),
            "hm_docker_pull".into(),
            "hm_docker_start_container".into(),
            "hm_docker_extract_workspace".into(),
            "hm_docker_exec".into(),
            "hm_docker_commit".into(),
            "hm_docker_remove_image".into(),
            "hm_docker_stop_remove".into(),
        ],
        config_schema: None,
        allowed_hosts: vec![],
    },
    executor = DockerExec,
);
