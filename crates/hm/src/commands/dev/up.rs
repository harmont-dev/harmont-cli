//! `hm dev up` — bring deployments up in the foreground.
//!
//! Flow: registry dump (subprocess) → boot plan (topo) → create network
//! → boot containers per level (parallel) → log mux → wait signal →
//! teardown.

use anyhow::{Context, Result};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::cli::DevUpArgs;
use crate::context::RunContext;
use crate::orchestrator::docker_client::DockerClient;

use super::logmux::{LogLine, run as run_logmux};
use super::naming::{fresh_session_id, resolve_worktree_root, worktree_hash};
use super::network::{Network, create as create_network, remove as remove_network};
use super::registry::{FromSource, LocalSpec, RegEntry, dump};
use super::service_spec::build as build_spec;
use super::topo::plan;

/// One booted container in this session.
struct Booted {
    slug: String,
    container_id: String,
}

/// Context passed to [`boot_one`] to keep the argument count below the
/// `clippy::too-many-arguments` threshold.
#[derive(Clone)]
struct BootCtx {
    worktree_root: std::path::PathBuf,
    worktree_hash: String,
    session_id: String,
    network_name: String,
    rebuild: bool,
}

/// Handle `hm dev up`.
///
/// # Errors
///
/// Returns an error if the registry dump fails, Docker is unreachable,
/// network creation fails, or any container boot fails.
pub async fn handle(args: DevUpArgs, _ctx: RunContext) -> Result<i32> {
    let worktree_root = resolve_worktree_root()?;
    let wt_hash = worktree_hash(&worktree_root);
    let session_id = fresh_session_id();
    tracing::info!("[hm] session {session_id}. resolving deployments in .harmont/");

    let registry = dump(&worktree_root)
        .await
        .context("dump deployment registry")?;
    let boot_plan = plan(&registry, &args.slugs, args.no_deps)?;
    let docker = DockerClient::connect()?;
    docker.ping().await.context("docker daemon ping")?;

    let net = create_network(&docker, &wt_hash, &session_id).await?;
    tracing::info!("[hm] network {}: created", net.name);

    // Determine slug column width.
    let slug_width = boot_plan.slugs().map(str::len).max().unwrap_or(4);

    let (log_tx, log_rx) = mpsc::unbounded_channel::<LogLine>();
    let log_color = std::env::var("NO_COLOR").is_err();
    let log_task = tokio::spawn(run_logmux(log_rx, slug_width, log_color));

    let mut booted: Vec<Booted> = Vec::new();

    let ctx = BootCtx {
        worktree_root,
        worktree_hash: wt_hash,
        session_id: session_id.clone(),
        network_name: net.name.clone(),
        rebuild: args.rebuild,
    };

    // Boot levels in topo order.
    for level in &boot_plan.levels {
        let mut joinset: JoinSet<Result<Booted>> = JoinSet::new();
        for slug in level {
            let RegEntry::Local(spec) = &registry.deployments[slug] else {
                continue; // upstream plan already filtered to local
            };
            let docker = docker.clone();
            let spec = spec.clone();
            let slug = slug.clone();
            let log_tx = log_tx.clone();
            let ctx = ctx.clone();
            joinset.spawn(async move { boot_one(docker, slug, spec, ctx, log_tx).await });
        }
        while let Some(res) = joinset.join_next().await {
            let b = res??;
            booted.push(b);
        }
    }

    tracing::info!("[hm] all up. Ctrl-C to tear down. Logs follow.");

    // Wait for SIGINT/SIGTERM.
    wait_signal().await?;

    tracing::info!("[hm] tearing down...");
    teardown(&docker, &net, &booted).await;

    // Drop the sender so the logmux channel closes and the task can finish.
    drop(log_tx);
    let _ = log_task.await;

    Ok(0)
}

async fn boot_one(
    docker: DockerClient,
    slug: String,
    spec: LocalSpec,
    ctx: BootCtx,
    log_tx: mpsc::UnboundedSender<LogLine>,
) -> Result<Booted> {
    // Resolve image: raw or build-from-step.
    let image = resolve_image(&docker, &slug, &spec, &ctx.worktree_hash, ctx.rebuild).await?;
    let resolved = build_spec(
        &slug,
        &spec,
        &image,
        &ctx.worktree_root,
        &ctx.worktree_hash,
        &ctx.session_id,
        &ctx.network_name,
    )?;
    let container_id = docker.start_service(resolved.as_service_spec()).await?;
    let host_ports = docker.inspect_ports(&container_id).await?;
    // Log the ready line.
    let ports_str = if host_ports.is_empty() {
        String::new()
    } else {
        let mut entries: Vec<(u16, u16)> = host_ports.iter().map(|(c, h)| (*c, *h)).collect();
        entries.sort_unstable();
        let parts: Vec<String> = entries
            .iter()
            .map(|(c, h)| format!("localhost:{h} \u{2192} :{c}"))
            .collect();
        format!(" | {}", parts.join(", "))
    };
    tracing::info!("[{slug}] ready  ( {}{ports_str} )", resolved.container_name);
    // Spawn the log-stream consumer for this container.
    tokio::spawn(stream_logs(
        docker.clone(),
        container_id.clone(),
        slug.clone(),
        log_tx,
    ));
    Ok(Booted { slug, container_id })
}

async fn resolve_image(
    docker: &DockerClient,
    slug: &str,
    spec: &LocalSpec,
    worktree_hash: &str,
    rebuild: bool,
) -> Result<String> {
    if let Some(tag) = &spec.image {
        if !docker.image_exists(tag).await? {
            tracing::info!("[{slug}] pulling {tag}...");
            docker.pull_image(tag).await?;
        }
        return Ok(tag.clone());
    }
    if let Some(FromSource::StepChain { pipeline_v0 }) = &spec.from {
        let chain_key = extract_terminal_key(pipeline_v0).unwrap_or_else(|| "nocache".to_string());
        let tag = format!("hm-build-{worktree_hash}-{slug}:{chain_key}");
        if rebuild || !docker.image_exists(&tag).await? {
            tracing::info!("[{slug}] building from Step chain...");
            crate::orchestrator::build_image_from_pipeline(docker, pipeline_v0, &tag).await?;
        }
        return Ok(tag);
    }
    anyhow::bail!("deployment `{slug}` has neither image= nor from_=; registry-dump bug?")
}

/// Pull the terminal step's resolved cache-key from the v0 IR JSON.
/// The dumper (harmont-py) calls `resolve_pipeline_keys` so every step
/// carries `key`. We use the last step's key as the cache-tag.
fn extract_terminal_key(pipeline_v0: &serde_json::Value) -> Option<String> {
    let steps = pipeline_v0.get("steps")?.as_array()?;
    steps.last()?.get("key")?.as_str().map(str::to_string)
}

async fn stream_logs(
    docker: DockerClient,
    container_id: String,
    slug: String,
    tx: mpsc::UnboundedSender<LogLine>,
) {
    use bollard::container::LogsOptions;
    let mut s = docker.inner_for_logs().logs::<String>(
        &container_id,
        Some(LogsOptions {
            stdout: true,
            stderr: true,
            follow: true,
            tail: "all".to_string(),
            ..Default::default()
        }),
    );
    while let Some(item) = s.next().await {
        match item {
            Ok(chunk) => {
                let bytes = chunk.into_bytes().to_vec();
                if tx
                    .send(LogLine {
                        slug: slug.clone(),
                        bytes,
                    })
                    .is_err()
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// # Errors
///
/// Returns an error if signal handler registration fails.
async fn wait_signal() -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }
    Ok(())
}

async fn teardown(docker: &DockerClient, net: &Network, booted: &[Booted]) {
    // Reverse order so dependents stop before their deps.
    for b in booted.iter().rev() {
        let _ = docker.stop_container(&b.container_id).await;
        let _ = docker.remove_container(&b.container_id).await;
        tracing::info!("[{}] stopped", b.slug);
    }
    let _ = remove_network(docker, net).await;
    tracing::info!("[hm] network {}: removed", net.name);
}
