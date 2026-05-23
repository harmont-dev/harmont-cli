use anyhow::{Context, Result};
use clap::Subcommand;

use crate::plugin::{PluginRegistry, RegistryConfig, paths};

#[derive(Debug, Clone, Subcommand)]
pub enum PluginCommand {
    /// List installed plugins (embedded + user + project).
    List,

    /// Show one plugin's manifest in detail.
    Info {
        /// Plugin name (matches `name` field of the manifest).
        name: String,
    },

    /// Install a plugin from a file path or HTTPS URL.
    ///
    /// HTTPS URLs require `--pin <sha256>` for integrity.
    Install {
        /// Plugin source: local path (`./foo.wasm`) or HTTPS URL.
        source: String,

        /// SHA-256 hex digest to verify against. Required for HTTPS
        /// sources; optional for local paths.
        #[arg(long, value_name = "SHA256_HEX")]
        pin: Option<String>,
    },

    /// Remove an installed plugin by name.
    Remove {
        /// Plugin name.
        name: String,
    },
}

/// Run an `hm plugin` subcommand.
///
/// # Errors
///
/// Returns an error if the plugin operation fails.
pub async fn run(cmd: PluginCommand) -> Result<()> {
    match cmd {
        PluginCommand::List => list().await,
        PluginCommand::Info { name } => info(&name).await,
        PluginCommand::Install { source, pin } => install_cmd(&source, pin.as_deref()).await,
        PluginCommand::Remove { name } => remove(&name).await,
    }
}

#[allow(clippy::unused_async)]
async fn list() -> Result<()> {
    let reg = PluginRegistry::load(RegistryConfig::default())?;
    if reg.manifests().count() == 0 {
        println!("No plugins installed.");
        println!();
        println!("Plugins live in:");
        if let Some(p) = paths::user_plugins_dir() {
            println!("  {}", p.display());
        }
        if let Some(p) = paths::project_plugins_dir() {
            println!("  {}", p.display());
        }
        println!();
        println!("Install one with `hm plugin install <path-or-url>`.");
        return Ok(());
    }
    println!("{:<28} {:>10}  capabilities", "name", "version");
    for m in reg.manifests() {
        let caps: Vec<String> = m.capabilities.iter().map(capability_summary).collect();
        println!("{:<28} {:>10}  {}", m.name, m.version, caps.join(", "));
    }
    Ok(())
}

#[allow(clippy::unused_async)]
async fn info(name: &str) -> Result<()> {
    let reg = PluginRegistry::load(RegistryConfig::default())?;
    let m = reg
        .manifests()
        .find(|m| m.name == name)
        .with_context(|| format!("no plugin named '{name}' is installed"))?;
    let json = serde_json::to_string_pretty(m)?;
    println!("{json}");
    Ok(())
}

async fn install_cmd(source: &str, pin: Option<&str>) -> Result<()> {
    let path = crate::plugin::install::install(source, pin).await?;
    println!("Installed plugin to {}", path.display());
    Ok(())
}

#[allow(clippy::unused_async)]
async fn remove(name: &str) -> Result<()> {
    let dir = crate::plugin::paths::install_dir().context("no install dir")?;
    let target = dir.join(format!("{name}.wasm"));
    if !target.is_file() {
        anyhow::bail!("no plugin file at {}", target.display());
    }
    std::fs::remove_file(&target).context("remove plugin")?;
    println!("Removed {}", target.display());
    Ok(())
}

fn capability_summary(cap: &hm_plugin_protocol::Capability) -> String {
    use hm_plugin_protocol::Capability::{
        LifecycleHook, OutputFormatter, StepExecutor, Subcommand,
    };
    match cap {
        Subcommand(s) => format!("subcmd:{}", s.verb),
        StepExecutor(s) => {
            if s.default {
                format!("runner:{}(*)", s.runner)
            } else {
                format!("runner:{}", s.runner)
            }
        }
        LifecycleHook(_) => "hook".into(),
        OutputFormatter(s) => format!("format:{}", s.name),
    }
}
