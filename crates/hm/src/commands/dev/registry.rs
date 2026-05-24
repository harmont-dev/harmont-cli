//! Read the deployment registry from `python -m harmont.dev --dump-registry`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DevRegistry {
    pub schema_version: String,
    pub worktree: String,
    pub deployments: BTreeMap<String, RegEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "driver")]
pub enum RegEntry {
    #[serde(rename = "local")]
    Local(LocalSpec),
    /// Any other driver. Carries the discriminator + `_unhandled: true`.
    /// Used by `hm dev ls` to display non-local deployments.
    #[serde(other)]
    Unhandled,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LocalSpec {
    pub image: Option<String>,
    #[serde(default)]
    pub from: Option<FromSource>,
    #[serde(default)]
    pub cmd: Option<Vec<String>>,
    pub port_mapping: BTreeMap<String, String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub volumes: BTreeMap<String, String>,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum FromSource {
    #[serde(rename = "step_chain")]
    StepChain { pipeline_v0: serde_json::Value },
}

/// Wire sentinel for `hm.dev.port()` — emitted by `harmont.dev.dump_registry_json`.
pub const PORT_SENTINEL: &str = "__hm_dev_port__";

/// Invoke `python -m harmont.dev --dump-registry --worktree-root <root>`
/// and deserialize the output.
///
/// # Errors
///
/// Returns an error if python is missing on PATH, the subprocess exits
/// non-zero (stderr is included in the message), or the JSON is malformed.
pub async fn dump(worktree_root: &Path) -> Result<DevRegistry> {
    let py = std::env::var("HARMONT_PYTHON").unwrap_or_else(|_| "python3".to_string());
    let output = Command::new(&py)
        .args(["-m", "harmont.dev", "--dump-registry", "--worktree-root"])
        .arg(worktree_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("invoke `{py} -m harmont.dev`; is harmont-py installed?"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "python -m harmont.dev --dump-registry exited {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    serde_json::from_slice(&output.stdout).context("parse deployment registry JSON")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal_local() {
        let raw = r#"{
          "schema_version": "0",
          "worktree": "/tmp/wt",
          "deployments": {
            "db": {
              "driver": "local",
              "image": "postgres:16",
              "from": null,
              "cmd": null,
              "port_mapping": {"5432": "__hm_dev_port__"},
              "env": {"POSTGRES_PASSWORD": "dev"},
              "volumes": {},
              "workdir": null,
              "deps": []
            }
          }
        }"#;
        let reg: DevRegistry = serde_json::from_str(raw).unwrap();
        assert_eq!(reg.schema_version, "0");
        let RegEntry::Local(spec) = &reg.deployments["db"] else {
            panic!("local expected")
        };
        assert_eq!(spec.image.as_deref(), Some("postgres:16"));
        assert_eq!(spec.port_mapping["5432"], PORT_SENTINEL);
    }

    #[test]
    fn deserialize_step_chain_from() {
        let raw = r#"{
          "schema_version": "0",
          "worktree": "/tmp/wt",
          "deployments": {
            "api": {
              "driver": "local",
              "image": null,
              "from": {"type": "step_chain", "pipeline_v0": {"version":"0","steps":[]}},
              "cmd": null,
              "port_mapping": {"8000": "__hm_dev_port__"},
              "env": {},
              "volumes": {},
              "workdir": null,
              "deps": ["db"]
            }
          }
        }"#;
        let reg: DevRegistry = serde_json::from_str(raw).unwrap();
        let RegEntry::Local(spec) = &reg.deployments["api"] else {
            panic!()
        };
        assert!(matches!(spec.from, Some(FromSource::StepChain { .. })));
        assert_eq!(spec.deps, vec!["db"]);
    }

    #[test]
    fn unknown_driver_maps_to_unhandled() {
        let raw = r#"{
          "schema_version": "0",
          "worktree": "/tmp/wt",
          "deployments": {
            "prod": {"driver": "aws", "_unhandled": true}
          }
        }"#;
        let reg: DevRegistry = serde_json::from_str(raw).unwrap();
        assert!(matches!(reg.deployments["prod"], RegEntry::Unhandled));
    }
}
