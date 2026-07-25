//! Verb implementations for `hm cloud <subcommand>`. Each module
//! exposes a `run(env, cmd)` entry point that `cli::dispatch` calls
//! after argv has been parsed.

pub(crate) mod billing;
pub(crate) mod build;
pub(crate) mod job;
pub(crate) mod org;
pub(crate) mod pipeline;
pub(crate) mod run;
pub(crate) mod secret;
