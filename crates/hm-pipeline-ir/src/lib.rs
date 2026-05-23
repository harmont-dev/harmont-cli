//! Pipeline IR, the v0 wire format consumed by the `hm` binary.
//!
//! Source of truth lives in two other places that must stay in sync
//! with this file: `harmont-pipeline/src/Harmont/Pipeline/Schema.hs`
//! (Haskell mirror) and `cidsl/py/harmont/__init__.py` (Python emitter).
//! Changing a field name here means changing it in both other places
//! in the same PR.

#![forbid(unsafe_code)]
#![allow(clippy::multiple_crate_versions, clippy::cargo_common_metadata)]

use std::collections::BTreeMap;

use schemars::JsonSchema as DeriveJsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct Pipeline {
    /// Must equal `"0"` — bumping this is reserved for breaking
    /// schema changes, none of which are scheduled. The v0 schema
    /// gains optional fields in-place (see `runner` below).
    pub version: String,
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub default_image: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    Command(Box<CommandStep>),
    Wait(WaitStep),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct CommandStep {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
    pub cmd: String,
    #[serde(default)]
    pub builds_in: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub cache: Option<Cache>,
    /// Names the step-executor plugin that should run this step.
    /// `None` ⇒ the default executor handles it (Docker, in the
    /// shipped configuration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    /// Plugin-specific extra fields. Validated by the executor
    /// plugin's `StepExecutorSpec::step_schema` if it set one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct WaitStep {
    #[serde(default)]
    pub continue_on_failure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct Cache {
    pub policy: String,
    #[serde(default)]
    pub key: Option<String>,
}
