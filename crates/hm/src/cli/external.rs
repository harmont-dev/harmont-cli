use std::collections::BTreeMap;

use anyhow::{Context, Result};
use hm_plugin_protocol::{ExitInfo, SubcommandInput};

use crate::error::HmError;
use crate::plugin::{PluginRegistry, RegistryConfig};

/// Run a plugin-provided external subcommand.
///
/// # Errors
///
/// Returns an error if plugin lookup or invocation fails.
pub async fn run(argv: Vec<String>) -> Result<i32> {
    let verb = argv
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("dispatcher called with empty argv (clap bug)"))?;

    let registry = PluginRegistry::load(RegistryConfig {
        auto_discover: true,
        extra_paths: vec![],
        embedded: vec![
            (
                "harmont-docker",
                crate::plugin::embedded::DOCKER_PLUGIN_WASM,
            ),
            (
                "harmont-output-human",
                crate::plugin::embedded::OUTPUT_HUMAN_PLUGIN_WASM,
            ),
            (
                "harmont-output-json",
                crate::plugin::embedded::OUTPUT_JSON_PLUGIN_WASM,
            ),
            ("harmont-cloud", crate::plugin::embedded::CLOUD_PLUGIN_WASM),
        ],
        pool_sizes: BTreeMap::new(),
    })
    .context("load plugin registry")?;

    let idx = registry
        .subcommand_index
        .get(&verb)
        .copied()
        .ok_or_else(|| HmError::UnknownVerb {
            verb: verb.clone(),
            available: registry.subcommand_index.keys().cloned().collect(),
        })?;

    let plugin = registry
        .get(idx)
        .context("plugin moved away during dispatch")?;

    let env: BTreeMap<String, String> = std::env::vars()
        .filter(|(k, _)| k.starts_with("HARMONT_"))
        .collect();

    let input = SubcommandInput {
        verb_path: argv.clone(),
        args: serde_json::Value::Null, // plugin parses raw argv itself
        env,
    };

    let info: ExitInfo = plugin
        .call_capability("hm_subcommand_run", &input)
        .await
        .with_context(|| format!("invoke plugin for verb '{verb}'"))?;

    if let Some(msg) = info.message {
        eprintln!("{msg}");
    }
    Ok(info.exit_code)
}
