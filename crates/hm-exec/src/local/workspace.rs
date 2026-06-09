//! Per-run workspace orchestration.
//!
//! [`WorkspaceManager`] auto-selects between two strategies:
//!
//! - **Clone strategy** (macOS APFS, Linux reflink, fallback `cp`):
//!   each step gets a full directory clone via [`hm_util::cow::cow_clone_dir`].
//! - **Overlay strategy** (Linux ext4 + `fuse-overlayfs`):
//!   each step gets a `fuse-overlayfs` mount with shared lower layers.
//!
//! The rest of the system (scheduler, runner) only sees
//! [`WorkspaceManager::workspace_path`] — strategy is transparent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use hm_util::cow::{CowStrategy, OverlayMount};

/// Manages workspace directories for a single pipeline run.
///
/// Each step gets an isolated directory that is either a full COW clone
/// or a `fuse-overlayfs` mount, depending on the platform.
pub struct WorkspaceManager {
    run_dir: PathBuf,
    base_dir: PathBuf,
    strategy: CowStrategy,
    workspaces: HashMap<String, PathBuf>,
    overlays: HashMap<String, OverlayLayer>,
}

struct OverlayLayer {
    upper_dir: PathBuf,
    merged_dir: PathBuf,
    ancestor_uppers: Vec<PathBuf>,
    _mount: Option<OverlayMount>,
}

impl WorkspaceManager {
    /// Create a new workspace manager that clones from `base_dir` into
    /// per-step sub-directories under `run_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if `run_dir` cannot be created.
    pub fn from_base(run_dir: PathBuf, base_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&run_dir)
            .with_context(|| format!("create run dir {}", run_dir.display()))?;
        let strategy = hm_util::cow::detect_strategy();
        tracing::info!(?strategy, "COW workspace strategy");
        if strategy == hm_util::cow::CowStrategy::FullCopy {
            tracing::warn!("using full-copy fallback — workspace cloning will be slow");
            for probe in hm_util::cow::diagnose_strategies() {
                if probe.available {
                    tracing::info!(strategy = ?probe.strategy, reason = probe.reason, "available");
                } else {
                    tracing::info!(strategy = ?probe.strategy, reason = probe.reason, "unavailable");
                }
            }
        }
        Ok(Self {
            run_dir,
            base_dir,
            strategy,
            workspaces: HashMap::new(),
            overlays: HashMap::new(),
        })
    }

    /// Create a new workspace manager that first extracts a tar.gz
    /// archive into `run_dir/base`, then delegates to [`Self::from_base`].
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be extracted or the run
    /// directory cannot be created.
    pub fn from_archive(run_dir: PathBuf, archive_bytes: &[u8]) -> Result<Self> {
        let base_dir = run_dir.join("base");
        std::fs::create_dir_all(&base_dir)
            .with_context(|| format!("create base dir {}", base_dir.display()))?;
        extract_tar_gz(archive_bytes, &base_dir)?;
        Self::from_base(run_dir, base_dir)
    }

    /// Create an isolated workspace directory for `step_key`.
    ///
    /// If `parent_key` is `Some`, the workspace inherits the contents of
    /// the parent workspace (including any modifications made after
    /// creation). If `None`, the workspace is cloned from the base
    /// directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent workspace is not registered, or if
    /// the clone / overlay operation fails.
    pub fn create_workspace(
        &mut self,
        step_key: &str,
        parent_key: Option<&str>,
    ) -> Result<PathBuf> {
        if self.workspaces.contains_key(step_key) || self.overlays.contains_key(step_key) {
            anyhow::bail!("workspace for step '{step_key}' already exists");
        }
        match self.strategy {
            CowStrategy::FuseOverlay => self.create_overlay(step_key, parent_key),
            _ => self.create_clone(step_key, parent_key, None),
        }
    }

    /// Create a workspace from a cached directory, bypassing parent
    /// relationships.
    ///
    /// # Errors
    ///
    /// Returns an error if the clone operation fails.
    pub fn create_workspace_from_cache(
        &mut self,
        step_key: &str,
        cached_workspace: &Path,
    ) -> Result<PathBuf> {
        if self.workspaces.contains_key(step_key) || self.overlays.contains_key(step_key) {
            anyhow::bail!("workspace for step '{step_key}' already exists");
        }
        self.create_clone(step_key, None, Some(cached_workspace))
    }

    /// Look up the filesystem path for a previously created workspace.
    #[must_use]
    pub fn workspace_path(&self, step_key: &str) -> Option<&Path> {
        if let Some(p) = self.workspaces.get(step_key) {
            return Some(p.as_path());
        }
        self.overlays.get(step_key).map(|l| l.merged_dir.as_path())
    }

    /// The base directory that root workspaces are cloned from.
    #[must_use]
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// The COW strategy in use for this run.
    #[must_use]
    pub const fn strategy(&self) -> CowStrategy {
        self.strategy
    }

    /// Remove the entire run directory, including all workspaces and
    /// overlay mounts.
    ///
    /// # Errors
    ///
    /// Returns an error if the run directory cannot be removed.
    pub fn cleanup(&mut self) -> Result<()> {
        // Drop overlay mounts before removing the filesystem tree.
        self.overlays.clear();
        if self.run_dir.exists() {
            std::fs::remove_dir_all(&self.run_dir)
                .with_context(|| format!("cleanup run dir {}", self.run_dir.display()))?;
        }
        Ok(())
    }

    fn create_clone(
        &mut self,
        step_key: &str,
        parent_key: Option<&str>,
        cached: Option<&Path>,
    ) -> Result<PathBuf> {
        let safe = sanitize_key(step_key);
        let ws_dir = self.run_dir.join("workspaces").join(&safe);

        let source = if let Some(c) = cached {
            c.to_path_buf()
        } else if let Some(pk) = parent_key {
            self.workspaces
                .get(pk)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("parent workspace '{pk}' not registered"))?
        } else {
            self.base_dir.clone()
        };

        hm_util::cow::cow_clone_dir(&source, &ws_dir)
            .with_context(|| format!("cow clone {} -> {}", source.display(), ws_dir.display()))?;

        self.workspaces.insert(step_key.to_string(), ws_dir.clone());
        Ok(ws_dir)
    }

    fn create_overlay(&mut self, step_key: &str, parent_key: Option<&str>) -> Result<PathBuf> {
        let safe = sanitize_key(step_key);
        let layer_dir = self.run_dir.join("layers").join(&safe);
        let upper_dir = layer_dir.join("upper");
        let work_dir = layer_dir.join("work");
        let merged_dir = layer_dir.join("merged");

        std::fs::create_dir_all(&upper_dir)?;
        std::fs::create_dir_all(&work_dir)?;
        std::fs::create_dir_all(&merged_dir)?;

        let ancestor_uppers = if let Some(pk) = parent_key {
            let parent = self
                .overlays
                .get(pk)
                .ok_or_else(|| anyhow::anyhow!("parent overlay '{pk}' not registered"))?;
            let mut ancestors = vec![parent.upper_dir.clone()];
            ancestors.extend(parent.ancestor_uppers.iter().cloned());
            ancestors
        } else {
            vec![]
        };

        let mut lower_dirs: Vec<&Path> = ancestor_uppers.iter().map(PathBuf::as_path).collect();
        lower_dirs.push(&self.base_dir);

        let mount = OverlayMount::mount(&lower_dirs, &upper_dir, &work_dir, &merged_dir)?;

        let merged_path = merged_dir.clone();
        self.overlays.insert(
            step_key.to_string(),
            OverlayLayer {
                upper_dir,
                merged_dir,
                ancestor_uppers,
                _mount: Some(mount),
            },
        );
        Ok(merged_path)
    }
}

impl std::fmt::Debug for WorkspaceManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceManager")
            .field("run_dir", &self.run_dir)
            .field("workspaces", &self.workspaces.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

fn sanitize_key(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<()> {
    use flate2::read::GzDecoder;

    let decoder = GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(dest)
        .with_context(|| format!("extract archive to {}", dest.display()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    fn make_base(tmp: &std::path::Path) -> PathBuf {
        let base = tmp.join("base");
        fs::create_dir(&base).unwrap();
        fs::write(base.join("main.rs"), b"fn main() {}").unwrap();
        base
    }

    #[test]
    fn root_step_clones_base() {
        let tmp = tempfile::tempdir().unwrap();
        let base = make_base(tmp.path());
        let mut mgr = WorkspaceManager::from_base(tmp.path().join("run"), base).unwrap();

        let ws = mgr.create_workspace("build", None).unwrap();
        assert_eq!(
            fs::read_to_string(ws.join("main.rs")).unwrap(),
            "fn main() {}"
        );
    }

    #[test]
    fn child_step_inherits_parent_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let base = make_base(tmp.path());
        let mut mgr = WorkspaceManager::from_base(tmp.path().join("run"), base).unwrap();

        let ws_a = mgr.create_workspace("a", None).unwrap();
        fs::write(ws_a.join("artifact.bin"), b"built").unwrap();

        let ws_b = mgr.create_workspace("b", Some("a")).unwrap();
        assert_eq!(
            fs::read_to_string(ws_b.join("main.rs")).unwrap(),
            "fn main() {}"
        );
        assert_eq!(
            fs::read_to_string(ws_b.join("artifact.bin")).unwrap(),
            "built"
        );
    }

    #[test]
    fn fork_children_are_isolated() {
        let tmp = tempfile::tempdir().unwrap();
        let base = make_base(tmp.path());
        let mut mgr = WorkspaceManager::from_base(tmp.path().join("run"), base).unwrap();

        let ws_a = mgr.create_workspace("a", None).unwrap();
        fs::write(ws_a.join("from_a"), b"a").unwrap();

        let ws_b = mgr.create_workspace("b", Some("a")).unwrap();
        let ws_c = mgr.create_workspace("c", Some("a")).unwrap();

        fs::write(ws_b.join("from_b"), b"b").unwrap();
        assert!(!ws_c.join("from_b").exists(), "c must not see b's changes");
    }

    #[test]
    fn workspace_path_returns_created() {
        let tmp = tempfile::tempdir().unwrap();
        let base = make_base(tmp.path());
        let mut mgr = WorkspaceManager::from_base(tmp.path().join("run"), base).unwrap();

        mgr.create_workspace("s", None).unwrap();
        assert!(mgr.workspace_path("s").is_some());
        assert!(mgr.workspace_path("nonexistent").is_none());
    }

    #[test]
    fn create_workspace_from_cache_clones_cached_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let base = make_base(tmp.path());
        let cached = tmp.path().join("cached");
        fs::create_dir(&cached).unwrap();
        fs::write(cached.join("cached_file.txt"), b"from_cache").unwrap();

        let mut mgr = WorkspaceManager::from_base(tmp.path().join("run"), base).unwrap();
        let ws = mgr.create_workspace_from_cache("s", &cached).unwrap();
        assert_eq!(
            fs::read_to_string(ws.join("cached_file.txt")).unwrap(),
            "from_cache"
        );
        assert!(!ws.join("main.rs").exists());
    }

    #[test]
    fn duplicate_step_key_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let base = make_base(tmp.path());
        let mut mgr = WorkspaceManager::from_base(tmp.path().join("run"), base).unwrap();
        mgr.create_workspace("dup", None).unwrap();
        assert!(mgr.create_workspace("dup", None).is_err());
    }

    #[test]
    fn cleanup_removes_run_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let base = make_base(tmp.path());
        let run_dir = tmp.path().join("run");
        let mut mgr = WorkspaceManager::from_base(run_dir.clone(), base).unwrap();
        mgr.create_workspace("s", None).unwrap();
        assert!(run_dir.exists());

        mgr.cleanup().unwrap();
        assert!(!run_dir.exists());
    }
}
