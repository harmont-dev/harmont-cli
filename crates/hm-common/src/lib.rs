//! Harmont common utilities shared across the `hm` workspace.

#[cfg(feature = "app-runtime")]
pub mod app_runtime;
pub mod dirs;
pub mod format;
pub mod fs;
pub mod git;
pub mod process;
pub mod python;
pub mod string;
pub mod time;

/// Raw platform primitives. Not part of the public surface — callers use
/// [`dirs`], which wraps these with Harmont-specific paths.
pub(crate) mod os;
