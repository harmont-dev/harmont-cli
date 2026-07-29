//! `hm cloud auth login | logout | whoami`.
//!
//! Thin adapters over [`hm_cloud::auth::AuthProvider`]; each exposes a single
//! `run(app)` entry point.

pub(crate) mod login;
pub(crate) mod logout;
pub(crate) mod whoami;
