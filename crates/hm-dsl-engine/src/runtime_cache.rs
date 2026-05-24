//! WASM runtime download and SHA-256 verified cache.
//!
//! Downloads WASM interpreter modules (e.g. CPython-WASI), verifies their
//! SHA-256 digest, and caches them under `~/.harmont/runtimes/`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tracing::info;

/// A local cache directory for WASM runtime modules.
#[derive(Debug)]
pub struct RuntimeCache {
    base: PathBuf,
}

/// Describes a WASM runtime module to fetch and verify.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeSpec {
    /// Short name, e.g. `"cpython"`.
    pub name: &'static str,
    /// SemVer version string, e.g. `"3.12.0"`.
    pub version: &'static str,
    /// HTTPS URL to download the `.wasm` module from.
    pub url: &'static str,
    /// Expected lowercase hex-encoded SHA-256 of the downloaded file.
    pub sha256: &'static str,
}

/// Placeholder — will be updated with a real hash once we test against the
/// actual cpython-wasi `.wasm` artifact.
pub const CPYTHON_WASI: RuntimeSpec = RuntimeSpec {
    name: "cpython",
    version: "3.12.0",
    url: "https://github.com/nicktimko/nicktimko.github.io/releases/download/cpython-3.12.0-wasi/python-3.12.0-wasi.wasm",
    sha256: "0000000000000000000000000000000000000000000000000000000000000000",
};

impl RuntimeCache {
    /// Create a cache rooted at `base`. The directory is created on first
    /// [`ensure`](Self::ensure) call if it does not already exist.
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }

    /// Default cache location: `~/.harmont/runtimes/`.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn default_path() -> Result<Self> {
        let home = dirs::home_dir().context("could not determine home directory")?;
        Ok(Self {
            base: home.join(".harmont").join("runtimes"),
        })
    }

    /// Deterministic path where a runtime module would be cached.
    pub fn module_path(&self, name: &str, version: &str) -> PathBuf {
        self.base.join(format!("{name}-{version}.wasm"))
    }

    /// Ensure the runtime described by `spec` is present on disk.
    ///
    /// 1. If the file already exists and passes SHA-256 verification, return
    ///    its path immediately.
    /// 2. Otherwise download from `spec.url` into a temporary file, verify the
    ///    digest, and atomically rename it into the cache directory.
    ///
    /// # Errors
    ///
    /// - Network / IO failures during download.
    /// - SHA-256 mismatch after download.
    pub async fn ensure(&self, spec: &RuntimeSpec) -> Result<PathBuf> {
        let target = self.module_path(spec.name, spec.version);

        // 1. Already cached?
        if target.exists() {
            verify_sha256(&target, spec.sha256)?;
            info!(
                runtime = spec.name,
                version = spec.version,
                "using cached WASM module"
            );
            return Ok(target);
        }

        // Make sure the cache directory exists.
        tokio::fs::create_dir_all(&self.base)
            .await
            .with_context(|| format!("failed to create cache dir {}", self.base.display()))?;

        // 2. Download to a temp file in the same directory (same filesystem →
        //    rename is atomic).
        let tmp = tempfile::NamedTempFile::new_in(&self.base)
            .context("failed to create temp file for WASM download")?;

        info!(
            runtime = spec.name,
            version = spec.version,
            url = spec.url,
            "downloading WASM runtime"
        );

        let response = reqwest::get(spec.url)
            .await
            .with_context(|| format!("failed to GET {}", spec.url))?;

        if !response.status().is_success() {
            bail!(
                "download failed: {} returned HTTP {}",
                spec.url,
                response.status()
            );
        }

        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("failed to read response body from {}", spec.url))?;

        tokio::fs::write(tmp.path(), &bytes)
            .await
            .context("failed to write downloaded WASM module to temp file")?;

        // 3. Verify SHA-256.
        verify_sha256(tmp.path(), spec.sha256)?;

        // 4. Rename into place.
        tmp.persist(&target)
            .with_context(|| format!("failed to persist WASM module to {}", target.display()))?;

        info!(
            runtime = spec.name,
            version = spec.version,
            path = %target.display(),
            "cached WASM module"
        );

        Ok(target)
    }
}

/// Verify that the SHA-256 digest of `path` matches `expected` (lowercase hex).
///
/// # Errors
///
/// - IO failure reading the file.
/// - Digest mismatch.
pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read {} for SHA-256 check", path.display()))?;

    let digest = Sha256::digest(&bytes);
    let actual = hex::encode(digest);

    if actual != expected {
        bail!(
            "SHA-256 mismatch for {}: expected {expected}, got {actual}",
            path.display()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn cache_dir_path_is_correct() {
        let dir = PathBuf::from("/tmp/test-runtimes");
        let cache = RuntimeCache::new(dir);
        let path = cache.module_path("cpython", "3.12.0");
        assert_eq!(path, PathBuf::from("/tmp/test-runtimes/cpython-3.12.0.wasm"));
    }

    #[test]
    fn sha256_verify_accepts_correct_hash() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.wasm");
        let content = b"hello, wasm runtime";
        fs::write(&file, content).unwrap();

        // Pre-computed SHA-256 of "hello, wasm runtime".
        let expected = hex::encode(Sha256::digest(content));
        verify_sha256(&file, &expected).unwrap();
    }

    #[test]
    fn sha256_verify_rejects_bad_hash() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.wasm");
        fs::write(&file, b"hello, wasm runtime").unwrap();

        let bad_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let err = verify_sha256(&file, bad_hash).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("SHA-256 mismatch"), "unexpected error: {msg}");
    }

    #[tokio::test]
    async fn ensure_returns_cached_path_if_exists() {
        let tmp = TempDir::new().unwrap();
        let cache = RuntimeCache::new(tmp.path().to_path_buf());

        let content = b"fake wasm module bytes";
        let sha = hex::encode(Sha256::digest(content));

        // Pre-create the cached file.
        let expected_path = cache.module_path("cpython", "3.12.0");
        fs::write(&expected_path, content).unwrap();

        // Build a spec whose sha256 matches.
        let spec = RuntimeSpec {
            name: "cpython",
            version: "3.12.0",
            url: "https://example.invalid/should-not-be-fetched",
            sha256: Box::leak(sha.into_boxed_str()),
        };

        let result = cache.ensure(&spec).await.unwrap();
        assert_eq!(result, expected_path);
    }
}
