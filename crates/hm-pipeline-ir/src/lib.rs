//! Pipeline IR, the v0 wire format consumed by the `hm` binary.
//!
//! The wire format is a petgraph-serde graph. Nodes carry
//! `CommandStep` + resolved env; edges are `EdgeKind` (`BuildsIn` or
//! `DependsOn`). See `graph::PipelineGraph` for the top-level type.

#![forbid(unsafe_code)]
#![allow(clippy::multiple_crate_versions, clippy::cargo_common_metadata)]

use std::collections::BTreeMap;

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

pub mod graph;
