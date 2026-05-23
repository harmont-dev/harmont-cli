//! Pipeline IR, the v0 wire format consumed by the `hm` binary.
//!
//! Source of truth lives in two other places that must stay in sync
//! with this file: `harmont-pipeline/src/Harmont/Pipeline/Schema.hs`
//! (Haskell mirror) and `cidsl/py/harmont/__init__.py` (Python emitter).
//! Changing a field name here means changing it in both other places
//! in the same PR.

#![forbid(unsafe_code)]
#![allow(clippy::multiple_crate_versions, clippy::cargo_common_metadata)]
