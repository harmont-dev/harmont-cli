use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub images: BTreeMap<String, String>,
}

impl Manifest {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            version: 1,
            images: BTreeMap::new(),
        }
    }

    /// SHA-256 content hash of the JSON-serialized manifest, truncated to 16
    /// hex characters.
    ///
    /// # Panics
    ///
    /// Panics if the manifest cannot be serialized to JSON (should never
    /// happen for this type).
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn content_hash(&self) -> String {
        let json = serde_json::to_string(self).expect("manifest serialization cannot fail");
        let hash = Sha256::digest(json.as_bytes());
        hex::encode(&hash[..8])
    }
}

/// Convert a Docker image tag to the corresponding tar filename.
///
/// `"harmont-local/base:a1b2c3d4"` → `"base--a1b2c3d4.tar"`
#[must_use]
pub fn tar_name_for_tag(tag: &str) -> String {
    let stripped = tag.strip_prefix("harmont-local/").unwrap_or(tag);
    format!("{}.tar", stripped.replace(':', "--"))
}

/// Inverse of [`tar_name_for_tag`].
///
/// `"base--a1b2c3d4.tar"` → `Some("harmont-local/base:a1b2c3d4")`
#[must_use]
pub fn tag_from_tar_name(filename: &str) -> Option<String> {
    let stem = filename.strip_suffix(".tar")?;
    let (name, hash) = stem.split_once("--")?;
    Some(format!("harmont-local/{name}:{hash}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "unit tests")]
mod tests {
    use super::*;

    #[test]
    fn tar_filename_from_tag() {
        assert_eq!(
            tar_name_for_tag("harmont-local/base:a1b2c3d4"),
            "base--a1b2c3d4.tar"
        );
    }

    #[test]
    fn tag_from_tar_filename() {
        assert_eq!(
            tag_from_tar_name("base--a1b2c3d4.tar"),
            Some("harmont-local/base:a1b2c3d4".to_string())
        );
    }

    #[test]
    fn tag_from_bad_filename_returns_none() {
        assert_eq!(tag_from_tar_name("random-file.tar"), None);
        assert_eq!(tag_from_tar_name("no-extension"), None);
    }

    #[test]
    fn manifest_round_trip() {
        let mut m = Manifest::new();
        m.images
            .insert("base".to_string(), "harmont-local/base:abc123".to_string());

        let json = serde_json::to_string(&m).unwrap();
        let m2: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn manifest_content_hash_is_deterministic() {
        let mut m = Manifest::new();
        m.images.insert(
            "step1".to_string(),
            "harmont-local/step1:deadbeef".to_string(),
        );

        let h1 = m.content_hash();
        let h2 = m.content_hash();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
