//! Process environment: a snapshot of the environment variables the CLI reads.

use std::collections::HashMap;

/// A snapshot of the process environment, captured once at startup so the CLI
/// reads variables from one place rather than hitting `std::env` ad hoc.
pub struct EnvVarProvider {
    vars: HashMap<String, String>,
}

impl EnvVarProvider {
    /// Capture the current environment. Variables whose name or value is not
    /// valid UTF-8 are skipped.
    #[must_use]
    pub fn init() -> Self {
        let vars = std::env::vars_os()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        Self { vars }
    }

    /// The value of `name`, if present — which may be an empty string.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    /// Whether `name` is present at all, even set to an empty string.
    #[must_use]
    pub fn is_present(&self, name: &str) -> bool {
        self.vars.contains_key(name)
    }

    /// Whether `name` is present and non-empty.
    #[must_use]
    pub fn is_set(&self, name: &str) -> bool {
        self.get(name).is_some_and(|value| !value.is_empty())
    }

    /// Parse `name`'s value into `T`, if it is present and parses.
    #[must_use]
    pub fn parse<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.get(name)?.parse().ok()
    }
}

impl std::fmt::Debug for EnvVarProvider {
    /// Redacted: environment variables can hold secrets, so only the count of
    /// captured variables is shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvVarProvider")
            .field("vars", &format_args!("<{} variables>", self.vars.len()))
            .finish()
    }
}
