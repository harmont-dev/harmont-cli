//! Plugin manifest types. A plugin advertises what it provides by
//! returning a [`PluginManifest`] from its mandatory `hm_manifest`
//! export at load time.

use schemars::JsonSchema as DeriveJsonSchema;
use serde::{Deserialize, Serialize};

use crate::hook::{HookEventKind, HookPhase};

/// JSON Schema fragment (serde-passthrough). Used to validate
/// plugin-specific config blobs and `runner_args`.
pub type JsonSchema = serde_json::Value;

/// Clap-derived JSON describing a subcommand's argument schema.
/// Produced by the SDK helper [`crate::manifest::clap_json_from`]
/// (added in [`hm-plugin-sdk`]).
pub type ClapJson = serde_json::Value;

/// Returned by an Extism plugin's `hm_manifest()` export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct PluginManifest {
    /// Must equal [`crate::HM_PLUGIN_API_VERSION`] or the host rejects
    /// the plugin at load time.
    pub api_version: u32,
    /// Stable plugin identifier, e.g. `harmont-docker`. Used as the
    /// key in the registry and in error messages.
    pub name: String,
    pub version: semver::Version,
    pub description: String,
    pub capabilities: Vec<Capability>,
    /// Host functions the plugin needs. Load fails fast if any are
    /// not exported by this build of `hm`.
    pub required_host_fns: Vec<String>,
    /// Optional JSON Schema describing plugin-specific configuration
    /// that lives in the project's `.harmont/plugins.toml`.
    pub config_schema: Option<JsonSchema>,
    /// HTTPS hosts the plugin is permitted to contact via
    /// `extism_pdk::http::request`. Defaults to empty (no HTTP).
    /// The host wires this into extism's per-instance manifest at
    /// load time; attempting to contact a host not in this list
    /// fails inside the plugin.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema, derive_more::IsVariant)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capability {
    Subcommand(SubcommandSpec),
    StepExecutor(StepExecutorSpec),
    LifecycleHook(LifecycleHookSpec),
    OutputFormatter(OutputFormatterSpec),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct SubcommandSpec {
    /// Top-level verb under `hm`. Two plugins may not claim the
    /// same `verb`.
    pub verb: String,
    pub about: String,
    /// Clap-shaped JSON for argument parsing (the host re-parses on
    /// the plugin's behalf via `clap`).
    pub args_schema: ClapJson,
    pub subcommands: Vec<Self>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct StepExecutorSpec {
    /// Matched against `CommandStep.runner` at dispatch time.
    pub runner: String,
    /// At most one plugin may set `default: true`. The host runs that
    /// executor when a step omits `runner`.
    pub default: bool,
    /// Optional JSON Schema for `CommandStep.runner_args`. The host
    /// validates `runner_args` against this schema before dispatch.
    pub step_schema: Option<JsonSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct LifecycleHookSpec {
    pub events: Vec<HookEventKind>,
    pub phase: HookPhase,
    pub timeout_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct OutputFormatterSpec {
    /// Selected via `--format <name>` on the command line.
    pub name: String,
    /// Advisory MIME type written into `--format <name> --output <file>` headers.
    pub mime: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn capability_tagged_serialization() {
        let cap = Capability::StepExecutor(StepExecutorSpec {
            runner: "docker".into(),
            default: true,
            step_schema: None,
        });
        let s = serde_json::to_string(&cap).unwrap();
        assert!(s.contains(r#""kind":"step_executor""#), "got: {s}");
        assert!(s.contains(r#""runner":"docker""#), "got: {s}");
    }
}
