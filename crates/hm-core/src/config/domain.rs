//! Shared config types: the backend selector and the cloud domain model.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

/// Default Harmont domain used when a cloud backend omits one.
pub const DEFAULT_DOMAIN: &str = "harmont.dev";

/// Failure to read or parse a config file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigLoadingError {
    /// The file could not be read.
    #[error("reading config file")]
    Io(#[from] std::io::Error),
    /// The file was not valid config TOML.
    #[error("parsing config TOML")]
    Deser(#[from] toml::de::Error),
}

/// Execution backend selected by a config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BackendConfig<C> {
    /// Run builds on the local Docker backend.
    #[default]
    Docker,
    /// Run builds on Harmont Cloud.
    Cloud(C),
}

/// Base Harmont domain for a cloud backend.
///
/// Parses from either a bare domain (`harmont.dev`) or a full URL (`https://harmont.dev`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDomain(Url);

impl BackendDomain {
    /// Parse from a bare domain or a full URL; a missing scheme becomes `https`.
    ///
    /// # Errors
    /// [`url::ParseError`] when the scheme-completed string is not a URL.
    pub fn parse(s: &str) -> Result<Self, url::ParseError> {
        let s = s.trim();
        let normalized = if s.contains("://") {
            s.to_owned()
        } else {
            format!("https://{s}")
        };
        Ok(Self(Url::parse(&normalized)?))
    }

    // TODO: return `Url` from api_url()/app_url() once the harmont-cloud client
    // accepts a URL base instead of string-concatenating (`format!("{base}{path}")`).
    // Its naive concat is why these must hand back a trailing-slash-trimmed String.

    /// The API base URL (`https://api.<domain>`), without a trailing slash.
    #[must_use]
    pub fn api_url(&self) -> String {
        self.subdomain("api")
            .as_str()
            .trim_end_matches('/')
            .to_owned()
    }

    /// The dashboard base URL (`https://app.<domain>`), without a trailing slash.
    #[must_use]
    pub fn app_url(&self) -> String {
        self.app().as_str().trim_end_matches('/').to_owned()
    }

    /// The dashboard base URL (`https://app.<domain>`), for extending with a
    /// path and query pairs.
    #[must_use]
    pub fn app(&self) -> Url {
        self.subdomain("app")
    }

    fn subdomain(&self, sub: &str) -> Url {
        match self.0.host_str() {
            Some(host) if host.contains('.') && host.parse::<std::net::IpAddr>().is_err() => {
                let mut url = self.0.clone();
                let _ = url.set_host(Some(&format!("{sub}.{host}")));
                url
            }
            _ => self.0.clone(),
        }
    }
}

impl Default for BackendDomain {
    fn default() -> Self {
        #[allow(
            clippy::expect_used,
            reason = "DEFAULT_DOMAIN is a compile-time constant known to parse"
        )]
        Self::parse(DEFAULT_DOMAIN).expect("DEFAULT_DOMAIN must be a valid domain")
    }
}

impl Serialize for BackendDomain {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str().trim_end_matches('/'))
    }
}

impl<'de> Deserialize<'de> for BackendDomain {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}
