//! Module which exposes executor operations on secrets.
use std::borrow::Borrow;

use secrecy::{ExposeSecret, SecretString};

/// Key reference to a secret.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct KeyRef<'a>(&'a str);

impl<'a> KeyRef<'a> {
    pub(crate) fn new(s: &'a str) -> Self {
        Self(s)
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0
    }
}

/// Stored key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct KeyBuf(String);

impl KeyBuf {
    pub(crate) fn new(s: String) -> Self {
        Self(s)
    }
}

impl AsRef<str> for KeyBuf {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for KeyBuf {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// Value reference to a secret.
#[derive(Debug, Clone)]
pub(crate) struct Value(SecretString);

impl Value {
    pub(crate) fn new(s: impl Into<String>) -> Self {
        Self(SecretString::from(s.into()))
    }

    pub(crate) fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

/// Collection of multiple secrets.
///
/// Normally, this is associated with a step and is injected into a step.
pub(crate) struct Bundle;

/// A generic provider which exposes secrets for different executors in harmont-cli.
pub(crate) trait Provider {
    /// Retrieve the given secret by key.
    fn get(&self, secret: &KeyRef<'_>) -> Option<&Value>;

    /// List all available key references.
    fn list(&self) -> impl Iterator<Item=KeyRef<'_>>;
}
