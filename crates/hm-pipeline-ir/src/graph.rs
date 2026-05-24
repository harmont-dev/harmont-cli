use std::collections::BTreeMap;

use daggy::Dag;

use schemars::JsonSchema as DeriveJsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct CommandStep {
    pub key: String,
    #[serde(default)]
    pub label: Option<String>,
    pub cmd: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub cache: Option<Cache>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct Cache {
    pub policy: String,
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub step: CommandStep,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    BuildsIn,
    DependsOn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineGraph {
    #[serde(default = "default_version")]
    version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_image: Option<String>,
    #[serde(rename = "graph")]
    inner: Dag<Transition, EdgeKind>,
}

fn default_version() -> String {
    "0".to_string()
}

impl PipelineGraph {
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    #[must_use]
    pub fn default_image(&self) -> Option<&str> {
        self.default_image.as_deref()
    }

    #[must_use]
    pub fn dag(&self) -> &Dag<Transition, EdgeKind> {
        &self.inner
    }
}
