//! Lifecycle hook wire types.

use schemars::JsonSchema as DeriveJsonSchema;
use serde::{Deserialize, Serialize};

use crate::events::BuildEvent;

/// Hook entry-point input. The host wraps a `BuildEvent` and tells
/// the plugin which phase this call is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HookEvent {
    pub event: BuildEvent,
    pub phase: HookPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema, derive_more::IsVariant)]
#[serde(rename_all = "snake_case")]
pub enum HookPhase {
    /// May return [`HookOutcome::Abort`] to fail the build.
    Before,
    /// Read-only; the return value is discarded.
    After,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema, derive_more::IsVariant)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookOutcome {
    /// Continue the build.
    Continue,
    /// Abort the build. Only honoured for `phase: Before`; ignored on
    /// `After` (with a host-side warning).
    Abort { reason: String },
}

/// Subset of [`crate::hook::HookEvent`] discriminants used at manifest time.
///
/// The manifest declares *what* events the plugin wants, not the per-event
/// payload. Kept in this file so plugin authors only import one module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DeriveJsonSchema, derive_more::IsVariant)]
#[serde(rename_all = "snake_case")]
pub enum HookEventKind {
    BuildStart,
    StepQueued,
    StepStart,
    StepLog,
    StepCacheHit,
    StepEnd,
    BuildEnd,
}
