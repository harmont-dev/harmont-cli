//! Raw discovery envelope emitted by the Python DSL, and its lowering.
//!
//! The Python runtime emits a [`RawEnvelope`]: per-pipeline metadata plus each
//! pipeline's [`RawStepChain`] (not yet lowered IR). Rust lowers every chain via
//! [`lower::lower`] and produces a [`FinalEnvelope`] whose `definition` field
//! carries the canonical v0 [`hm_pipeline_ir::PipelineGraph`], ready for the
//! backend's pipeline discovery to consume.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::keygen::LowerOptions;
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
        .map(|entry| process_entry(entry, None))
        .collect::<Result<Vec<_>>>()?;
    Ok(FinalEnvelope {
        schema_version: raw.schema_version,
        pipelines,
    })
}

/// Lower every pipeline's step chain with cache-key resolution enabled.
///
/// Each pipeline is lowered with its own [`LowerOptions`], carrying the
/// pipeline's slug so resolved cache keys are namespaced per pipeline —
/// byte-for-byte matching the Python resolver (`pipeline_slug = reg.slug`).
///
/// # Errors
///
/// Returns an error if any pipeline's step chain fails to lower, if
/// cache-key resolution fails (e.g. a missing `on_change` path), or if the
/// resulting graph cannot be serialized.
pub fn process_raw_envelope_with_options(
    raw: RawEnvelope,
    pipeline_org: &str,
    now: u64,
    base_path: &Path,
    env: &BTreeMap<String, String>,
) -> Result<FinalEnvelope> {
    let pipelines = raw
        .pipelines
        .into_iter()
        .map(|entry| {
            let opts = LowerOptions {
                pipeline_org: pipeline_org.to_owned(),
                pipeline_slug: entry.slug.clone(),
                now,
                base_path: base_path.to_path_buf(),
                env: env.clone(),
            };
            process_entry(entry, Some(&opts))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(FinalEnvelope {
        schema_version: raw.schema_version,
        pipelines,
    })
}

fn process_entry(entry: RawPipelineEntry, opts: Option<&LowerOptions>) -> Result<FinalPipelineEntry> {
    let graph = lower::lower_with_options(&entry.step_chain, opts)
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
