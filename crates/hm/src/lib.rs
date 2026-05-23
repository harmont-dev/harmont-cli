#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive dependency version conflicts in rand/windows-sys/thiserror chains; not fixable without upstream updates"
)]

pub mod builtin;
pub mod cli;
pub mod commands;
pub mod config;
pub mod context;
pub mod creds_store;
pub mod dispatcher;
pub mod error;
pub mod orchestrator;
pub mod output;
pub mod plugin;
