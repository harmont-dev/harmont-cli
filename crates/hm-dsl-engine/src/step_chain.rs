//! Raw step-chain wire format emitted by the Python DSL.
//!
//! Python serializes each pipeline's `Step` chain into a [`RawStepChain`]: a
//! flat list of steps that reference their parents by index, plus the indices
//! of the chain's leaves and pipeline-level metadata. Rust lowers this into the
//! canonical [`hm_pipeline_ir::PipelineGraph`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A pipeline's step chain as emitted by the Python DSL.
///
/// Steps form a forest referenced by index: each [`RawStep::parent_idx`] points
/// at an earlier entry in [`steps`](Self::steps), and [`leaf_indices`](Self::leaf_indices)
/// names the terminal steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawStepChain {
    pub steps: Vec<RawStep>,
    pub leaf_indices: Vec<usize>,
    #[serde(default)]
    pub pipeline_env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub pipeline_timeout_seconds: Option<u32>,
}

/// A single step in a [`RawStepChain`].
///
/// A step is either a command (`cmd` set) or a `wait` barrier (`is_wait`).
/// `parent_idx` is `None` for a step that boots from a fresh image (a root).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawStep {
    pub cmd: Option<String>,
    pub parent_idx: Option<usize>,
    #[serde(default)]
    pub is_wait: bool,
    #[serde(default)]
    pub continue_on_failure: bool,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub cache: Option<RawCachePolicy>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub runner: Option<String>,
    #[serde(default)]
    pub runner_args: Option<serde_json::Value>,
    #[serde(default)]
    pub key_override: Option<String>,
}

/// Cache policy for a step, tagged by its `policy` field on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "policy")]
pub enum RawCachePolicy {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "forever")]
    Forever {
        #[serde(default)]
        env_keys: Vec<String>,
    },
    #[serde(rename = "ttl")]
    Ttl {
        duration_seconds: u64,
        #[serde(default)]
        env_keys: Vec<String>,
    },
    #[serde(rename = "on_change")]
    OnChange { paths: Vec<String> },
    #[serde(rename = "compose")]
    Compose { sub_policies: Vec<Self> },
}
