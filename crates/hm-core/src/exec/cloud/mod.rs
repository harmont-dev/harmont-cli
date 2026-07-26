//! Cloud execution backend (submit + watch over the SDK).
pub mod watch; // pub: the CLI's `cloud build watch`/`cloud job log` verbs reuse it

mod backend;
pub use backend::CloudBackend;
