//! Cache-key resolver — byte-exact port of `harmont-py/harmont/keygen.py`.
//!
//! The output bytes MUST match the Python (and, before it, Scheme) resolver so
//! cache snapshots persisted by earlier versions stay reachable. The outer key
//! is the sha256 of the preimage
//!
//! ```text
//! pipeline_org NUL pipeline_slug NUL step_key NUL parent_resolved NUL policy_resolution
//! ```
//!
//! where `NUL` is a single `\x00` byte and `policy_resolution` branches on the
//! step's cache policy (see [`resolve_policy`]).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::step_chain::RawCachePolicy;

/// The `\x00` separator woven through every preimage.
const NUL: &str = "\x00";

/// Parameters cache-key resolution needs beyond the raw step chain.
///
/// Carried through the lowering pass so keys can be resolved while the rich
/// [`RawCachePolicy`] data (env keys, durations, paths, sub-policies) is still
/// in scope, before it is flattened into the IR `Cache` shape.
#[derive(Debug, Clone)]
pub struct LowerOptions {
    /// Owning organization slug — first field of the outer preimage.
    pub pipeline_org: String,
    /// Pipeline slug — second field of the outer preimage.
    pub pipeline_slug: String,
    /// Wall-clock unix timestamp used to bucket `ttl` policies.
    pub now: u64,
    /// Project directory that `on_change` paths and globs resolve against.
    pub base_path: PathBuf,
    /// Process environment sampled for `env_subset` of `forever`/`ttl` policies.
    pub env: BTreeMap<String, String>,
}

/// Hex-encode `sha256(s)` as lowercase, matching `hashlib.sha256(...).hexdigest()`.
#[must_use]
pub fn sha256_hex(s: &str) -> String {
    to_hex(&Sha256::digest(s.as_bytes()))
}

/// Resolve a step's full outer cache key.
///
/// `parent_resolved` is the already-resolved key of the `builds_in` parent, or
/// `"scratch"` when the step has no cached parent.
///
/// # Errors
///
/// Propagates errors from [`resolve_policy`] — chiefly missing `on_change`
/// paths or filesystem read failures.
pub fn compute_cache_key(
    step_key: &str,
    cmd: &str,
    policy: &RawCachePolicy,
    parent_resolved: &str,
    opts: &LowerOptions,
) -> Result<String> {
    let policy_res = resolve_policy(policy, cmd, opts)?;
    let preimage = [
        opts.pipeline_org.as_str(),
        opts.pipeline_slug.as_str(),
        step_key,
        parent_resolved,
        policy_res.as_str(),
    ]
    .join(NUL);
    Ok(sha256_hex(&preimage))
}

/// Resolve the `policy_resolution` fragment for a single policy.
///
/// # Errors
///
/// Returns an error if an `on_change` path is missing (and not silently
/// skippable) or a file/directory cannot be read.
pub fn resolve_policy(policy: &RawCachePolicy, cmd: &str, opts: &LowerOptions) -> Result<String> {
    match policy {
        RawCachePolicy::None => Ok("none".to_owned()),
        RawCachePolicy::Forever { env_keys } => {
            let inner = [cmd, &env_subset(env_keys, &opts.env)].join(NUL);
            Ok(format!("forever-{}", sha256_hex(&inner)))
        }
        RawCachePolicy::Ttl {
            duration_seconds,
            env_keys,
        } => {
            let bucket = opts.now / duration_seconds;
            let inner = [cmd, &env_subset(env_keys, &opts.env)].join(NUL);
            Ok(format!("ttl-{bucket}-{}", sha256_hex(&inner)))
        }
        RawCachePolicy::OnChange { paths } => {
            Ok(format!("sha-{}", sha256_hex(&on_change_preimage(paths, &opts.base_path)?)))
        }
        RawCachePolicy::Compose { sub_policies } => {
            let mut parts = String::new();
            for sub in sub_policies {
                if matches!(sub, RawCachePolicy::None) {
                    parts.push_str("none");
                } else {
                    parts.push_str(&resolve_policy(sub, cmd, opts)?);
                }
            }
            Ok(format!("compose-{}", sha256_hex(&parts)))
        }
    }
}

/// Concatenate `key=value\x00` for each sorted env key, reading `value` from
/// `env` (empty string when absent).
#[must_use]
pub fn env_subset(env_keys: &[String], env: &BTreeMap<String, String>) -> String {
    let mut sorted: Vec<&String> = env_keys.iter().collect();
    sorted.sort();
    let mut out = String::new();
    for k in sorted {
        out.push_str(k);
        out.push('=');
        out.push_str(env.get(k).map_or("", String::as_str));
        out.push_str(NUL);
    }
    out
}

/// Build the `on_change` preimage: `file_hash(p) NUL` for each resolved path.
///
/// Path strings are sorted first; glob patterns (`*`, `?`, `[`) expand against
/// `base` and their matches are sorted; plain paths that don't exist are
/// skipped (mirroring `keygen.py`).
fn on_change_preimage(paths: &[String], base: &Path) -> Result<String> {
    let mut sorted: Vec<&String> = paths.iter().collect();
    sorted.sort();

    let mut resolved: Vec<PathBuf> = Vec::new();
    for p in sorted {
        if p.contains('*') || p.contains('?') || p.contains('[') {
            let pattern = base.join(p);
            let pattern = pattern.to_str().with_context(|| {
                format!("on_change glob pattern is not valid UTF-8: {}", pattern.display())
            })?;
            let mut matches: Vec<PathBuf> = glob::glob(pattern)
                .with_context(|| format!("invalid on_change glob pattern: {pattern}"))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .with_context(|| format!("failed to read on_change glob matches for {pattern}"))?;
            matches.sort();
            resolved.extend(matches);
        } else {
            let full = base.join(p);
            if full.exists() {
                resolved.push(full);
            }
        }
    }

    let mut pre = String::new();
    for r in &resolved {
        pre.push_str(&path_hash(r)?);
        pre.push_str(NUL);
    }
    Ok(pre)
}

/// Hash a path for an `on_change` key.
///
/// Files hash their bytes. Directories fold each descendant file's POSIX
/// relative path + bytes into one stream in sorted order. Missing paths fail
/// loudly.
fn path_hash(path: &Path) -> Result<String> {
    if path.is_file() {
        let bytes =
            std::fs::read(path).with_context(|| format!("reading on_change file {}", path.display()))?;
        return Ok(to_hex(&Sha256::digest(&bytes)));
    }
    if path.is_dir() {
        let mut files: Vec<PathBuf> = Vec::new();
        collect_files(path, &mut files)?;
        files.sort();

        let mut h = Sha256::new();
        for child in &files {
            let rel = child
                .strip_prefix(path)
                .with_context(|| format!("relativizing {}", child.display()))?;
            let rel_posix = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            h.update(rel_posix.as_bytes());
            h.update([0u8]);
            let bytes = std::fs::read(child)
                .with_context(|| format!("reading on_change file {}", child.display()))?;
            h.update(&bytes);
            h.update([0u8]);
        }
        return Ok(to_hex(&h.finalize()));
    }
    bail!("on_change path does not exist: {}", path.display());
}

/// Collect every regular file under `dir`, recursing into subdirectories.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("reading directory {}", dir.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("reading entry in {}", dir.display()))?
            .path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

/// Lowercase hex encoding of a byte slice.
fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}
