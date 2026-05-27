//! Platform-native copy-on-write directory cloning.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CowStrategy {
    ApfsClone,
    Reflink,
    FuseOverlay,
    FullCopy,
}

/// Detect the best available COW strategy for the current platform.
/// Result is cached after the first call.
#[must_use]
pub fn detect_strategy() -> CowStrategy {
    static STRATEGY: OnceLock<CowStrategy> = OnceLock::new();
    *STRATEGY.get_or_init(detect_strategy_inner)
}

/// Probe result for a single strategy.
#[derive(Debug, Clone)]
pub struct StrategyProbe {
    pub strategy: CowStrategy,
    pub available: bool,
    pub reason: &'static str,
}

/// Test all strategies and report which are available.
/// Used for diagnostics / user-facing warnings.
#[must_use]
#[allow(clippy::vec_init_then_push)]
pub fn diagnose_strategies() -> Vec<StrategyProbe> {
    let mut probes = Vec::new();

    #[cfg(target_os = "macos")]
    probes.push(StrategyProbe {
        strategy: CowStrategy::ApfsClone,
        available: true,
        reason: "macOS APFS detected",
    });

    #[cfg(target_os = "linux")]
    {
        probes.push(StrategyProbe {
            strategy: CowStrategy::Reflink,
            available: probe_reflink(),
            reason: if probe_reflink() {
                "filesystem supports reflinks"
            } else {
                "filesystem does not support reflinks (btrfs/XFS required)"
            },
        });
        let fuse_ok = probe_fuse_overlayfs();
        probes.push(StrategyProbe {
            strategy: CowStrategy::FuseOverlay,
            available: fuse_ok,
            reason: if fuse_ok {
                "fuse-overlayfs mount succeeded"
            } else if which::which("fuse-overlayfs").is_err() {
                "fuse-overlayfs not installed"
            } else {
                "fuse-overlayfs mount failed (missing /dev/fuse or user_allow_other?)"
            },
        });
    }

    probes.push(StrategyProbe {
        strategy: CowStrategy::FullCopy,
        available: true,
        reason: "always available (slow)",
    });

    probes
}

#[allow(clippy::missing_const_for_fn)]
fn detect_strategy_inner() -> CowStrategy {
    #[cfg(target_os = "macos")]
    {
        return CowStrategy::ApfsClone;
    }

    #[cfg(target_os = "linux")]
    {
        if probe_reflink() {
            return CowStrategy::Reflink;
        }
        if probe_fuse_overlayfs() {
            return CowStrategy::FuseOverlay;
        }
        return CowStrategy::FullCopy;
    }

    #[allow(unreachable_code)]
    CowStrategy::FullCopy
}

#[cfg(target_os = "linux")]
fn probe_reflink() -> bool {
    let Ok(tmp) = tempfile::tempdir() else {
        return false;
    };
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    if std::fs::write(&src, b"x").is_err() {
        return false;
    }
    Command::new("cp")
        .args(["--reflink=always"])
        .arg(&src)
        .arg(&dst)
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(target_os = "linux")]
fn probe_fuse_overlayfs() -> bool {
    if which::which("fuse-overlayfs").is_err() {
        return false;
    }
    let Ok(tmp) = tempfile::tempdir() else {
        return false;
    };
    let lower = tmp.path().join("lower");
    let upper = tmp.path().join("upper");
    let work = tmp.path().join("work");
    let merged = tmp.path().join("merged");
    for d in [&lower, &upper, &work, &merged] {
        if std::fs::create_dir(d).is_err() {
            return false;
        }
    }
    let opts = format!(
        "lowerdir={},upperdir={},workdir={},allow_other,squash_to_uid=0,squash_to_gid=0",
        lower.display(),
        upper.display(),
        work.display(),
    );
    let ok = Command::new("fuse-overlayfs")
        .args(["-o", &opts])
        .arg(&merged)
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        let bin = if which::which("fusermount3").is_ok() {
            "fusermount3"
        } else {
            "fusermount"
        };
        let _ = Command::new(bin)
            .args(["-u"])
            .arg(&merged)
            .stderr(std::process::Stdio::null())
            .status();
    }
    ok
}

/// Clone `src` to `dst` using the best available COW mechanism.
///
/// # Errors
///
/// Returns an error if `dst` already exists, if parent directories cannot
/// be created, or if the underlying copy operation fails.
pub fn cow_clone_dir(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        bail!("destination already exists: {}", dst.display());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create parent dirs for {}", dst.display()))?;
    }

    if try_platform_cow(src, dst)? {
        return Ok(());
    }

    copy_dir_recursive(src, dst)
}

fn try_platform_cow(src: &Path, dst: &Path) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("cp")
            .args(["-c", "-R", "-p"])
            .arg(src)
            .arg(dst)
            .stderr(std::process::Stdio::null())
            .status()
            .context("spawn cp -c")?;
        if status.success() {
            return Ok(true);
        }
        let _ = std::fs::remove_dir_all(dst);
    }

    #[cfg(target_os = "linux")]
    {
        let status = Command::new("cp")
            .args(["--reflink=always", "-a"])
            .arg(src)
            .arg(dst)
            .stderr(std::process::Stdio::null())
            .status()
            .context("spawn cp --reflink")?;
        if status.success() {
            return Ok(true);
        }
        let _ = std::fs::remove_dir_all(dst);

        let status = Command::new("cp")
            .args(["-a"])
            .arg(src)
            .arg(dst)
            .stderr(std::process::Stdio::null())
            .status()
            .context("spawn cp -a")?;
        if status.success() {
            return Ok(true);
        }
        let _ = std::fs::remove_dir_all(dst);
    }

    Ok(false)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
    for entry in std::fs::read_dir(src).with_context(|| format!("read dir {}", src.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_symlink() {
            let target = std::fs::read_link(&src_path)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &dst_path)?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&target, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .with_context(|| format!("copy {}", src_path.display()))?;
        }
    }
    Ok(())
}

pub struct OverlayMount {
    merged: PathBuf,
    upper: PathBuf,
    mounted: std::sync::atomic::AtomicBool,
}

impl OverlayMount {
    /// Mount a fuse-overlayfs filesystem merging the given layers.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation fails or `fuse-overlayfs`
    /// exits with a non-zero status.
    pub fn mount(
        lower_dirs: &[&Path],
        upper_dir: &Path,
        work_dir: &Path,
        merged_path: &Path,
    ) -> Result<Self> {
        std::fs::create_dir_all(upper_dir)?;
        std::fs::create_dir_all(work_dir)?;
        std::fs::create_dir_all(merged_path)?;

        let lowerdir: String = lower_dirs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(":");

        let opts = format!(
            "lowerdir={lowerdir},upperdir={},workdir={},allow_other,squash_to_uid=0,squash_to_gid=0",
            upper_dir.display(),
            work_dir.display(),
        );

        let output = Command::new("fuse-overlayfs")
            .args(["-o", &opts])
            .arg(merged_path)
            .output()
            .context("spawn fuse-overlayfs")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "fuse-overlayfs mount failed (exit {}): {stderr}\nlowerdir={}, upper={}, merged={}",
                output.status.code().unwrap_or(-1),
                lowerdir,
                upper_dir.display(),
                merged_path.display(),
            );
        }

        Ok(Self {
            merged: merged_path.to_path_buf(),
            upper: upper_dir.to_path_buf(),
            mounted: std::sync::atomic::AtomicBool::new(true),
        })
    }

    #[must_use]
    pub fn merged_path(&self) -> &Path {
        &self.merged
    }

    #[must_use]
    pub fn upper_dir(&self) -> &Path {
        &self.upper
    }

    /// Unmount the fuse-overlayfs filesystem. Safe to call multiple times.
    ///
    /// # Errors
    ///
    /// Returns an error if `fusermount` cannot be spawned or exits
    /// with a non-zero status.
    pub fn unmount(&self) -> Result<()> {
        if !self
            .mounted
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return Ok(());
        }
        let bin = if which::which("fusermount3").is_ok() {
            "fusermount3"
        } else {
            "fusermount"
        };
        let status = Command::new(bin)
            .args(["-u"])
            .arg(&self.merged)
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| format!("spawn {bin} -u"))?;
        if !status.success() {
            bail!("{bin} -u {} failed", self.merged.display());
        }
        Ok(())
    }
}

impl std::fmt::Debug for OverlayMount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlayMount")
            .field("merged", &self.merged)
            .field("upper", &self.upper)
            .finish_non_exhaustive()
    }
}

impl Drop for OverlayMount {
    fn drop(&mut self) {
        if let Err(e) = self.unmount() {
            tracing::warn!(%e, path = %self.merged.display(), "fuse-overlayfs unmount failed");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn cow_clone_creates_identical_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), b"hello").unwrap();
        fs::write(src.join("sub/b.txt"), b"world").unwrap();

        let dst = tmp.path().join("dst");
        cow_clone_dir(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert_eq!(fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "world");
    }

    #[test]
    fn cow_clone_is_isolated() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f.txt"), b"original").unwrap();

        let dst = tmp.path().join("dst");
        cow_clone_dir(&src, &dst).unwrap();

        // Mutate dst; src must be unchanged.
        fs::write(dst.join("f.txt"), b"modified").unwrap();
        assert_eq!(fs::read_to_string(src.join("f.txt")).unwrap(), "original");
        assert_eq!(fs::read_to_string(dst.join("f.txt")).unwrap(), "modified");
    }

    #[test]
    fn cow_clone_fails_if_dst_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir(&src).unwrap();
        let dst = tmp.path().join("dst");
        fs::create_dir(&dst).unwrap();

        assert!(cow_clone_dir(&src, &dst).is_err());
    }

    #[test]
    fn detect_strategy_returns_something() {
        // Should always detect at least FullCopy.
        let s = detect_strategy();
        assert!(!matches!(s, CowStrategy::FuseOverlay));
        // Can't assert specific strategy (platform-dependent) but it must not panic.
    }
}
