//! Cloud execution backend (submit + watch over the SDK).
pub mod watch; // pub: hm-plugin-cloud's `build watch`/`job log` verbs reuse it

mod backend;
pub use backend::CloudBackend;
