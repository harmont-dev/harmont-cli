use anyhow::Result;
use hm_plugin_protocol::HM_PLUGIN_API_VERSION;

use crate::plugin::{PluginRegistry, RegistryConfig};

#[allow(clippy::unused_async)]
/// Print version information to stdout.
///
/// # Errors
///
/// Returns an error if the plugin registry cannot be loaded.
pub async fn run() -> Result<()> {
    let reg = PluginRegistry::load(RegistryConfig::default())?;
    println!("hm {}", env!("CARGO_PKG_VERSION"));
    println!("plugin api version: {HM_PLUGIN_API_VERSION}");
    let count = reg.manifests().count();
    println!("plugins loaded: {count}");
    Ok(())
}
