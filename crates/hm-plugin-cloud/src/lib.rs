//! Cloud client library for the hm CLI.
//!
//! Implements `hm cloud {login,logout,whoami,org,pipeline,build,job,billing,run}`.

#![allow(
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo,
    clippy::multiple_crate_versions,
    clippy::cargo_common_metadata,
    clippy::missing_errors_doc,
    reason = "quick migration from plugin crate; polish later"
)]

pub mod cli;

mod api;
mod auth;
mod config;
mod creds;
mod http;
mod state;
mod verbs;
