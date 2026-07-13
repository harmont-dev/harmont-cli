//! Raw discovery envelope emitted by the Python DSL, and its lowering.
//!
//! The Python runtime emits a [`RawEnvelope`]: per-pipeline metadata plus each
//! pipeline's [`RawStepChain`] (not yet lowered IR). Rust lowers every chain via
//! [`lower::lower`] and produces a [`FinalEnvelope`] whose `definition` field
//! carries the canonical v0 [`hm_pipeline_ir::PipelineGraph`], ready for the
//! backend's pipeline discovery to consume.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::lower;
use crate::step_chain::RawStepChain;

/// The discovery envelope as emitted by the Python DSL, before lowering.
#[derive(Debug, Clone, Deserialize)]
pub struct RawEnvelope {
    pub schema_version: String,
    pub pipelines: Vec<RawPipelineEntry>,
}

/// One pipeline in a [`RawEnvelope`]: metadata plus its raw step chain.
#[derive(Debug, Clone, Deserialize)]
pub struct RawPipelineEntry {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub allow_manual: bool,
    #[serde(default)]
    pub triggers: Vec<serde_json::Value>,
    pub step_chain: RawStepChain,
}

/// The lowered discovery envelope handed to consumers.
#[derive(Debug, Clone, Serialize)]
pub struct FinalEnvelope {
    pub schema_version: String,
    pub pipelines: Vec<FinalPipelineEntry>,
}

/// One pipeline in a [`FinalEnvelope`]: metadata plus its lowered definition.
#[derive(Debug, Clone, Serialize)]
pub struct FinalPipelineEntry {
    pub slug: String,
    pub name: String,
    pub allow_manual: bool,
    pub triggers: Vec<serde_json::Value>,
    /// The serialized [`hm_pipeline_ir::PipelineGraph`] (v0 IR).
    pub definition: serde_json::Value,
}

/// Lower every pipeline's step chain and produce the final envelope.
///
/// # Errors
///
/// Returns an error if any pipeline's step chain fails to lower, or if the
/// resulting graph cannot be serialized.
pub fn process_raw_envelope(raw: RawEnvelope) -> Result<FinalEnvelope> {
    let pipelines = raw
        .pipelines
        .into_iter()
        .map(process_entry)
        .collect::<Result<Vec<_>>>()?;
    Ok(FinalEnvelope {
        schema_version: raw.schema_version,
        pipelines,
    })
}

fn process_entry(entry: RawPipelineEntry) -> Result<FinalPipelineEntry> {
    let graph = lower::lower(&entry.step_chain)
        .with_context(|| format!("failed to lower pipeline '{}'", entry.slug))?;
    let definition = serde_json::to_value(&graph)
        .with_context(|| format!("failed to serialize definition for pipeline '{}'", entry.slug))?;
    Ok(FinalPipelineEntry {
        slug: entry.slug,
        name: entry.name,
        allow_manual: entry.allow_manual,
        triggers: entry.triggers,
        definition,
    })
}
