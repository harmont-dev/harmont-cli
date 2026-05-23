# Extract `hm-util` Crate — OS & FS Utilities

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extract low-level OS utilities from `crates/hm` into a shared `crates/hm-util` crate with async-first filesystem operations, then modernize the cancellation infrastructure.

**Architecture:** New `hm-util` crate exposes `os::fs` (async + blocking atomic file I/O with permission control), `os::dirs` (platform directory resolution with proper error handling). Async functions use `spawn_blocking` over proven sync cores — same strategy as `tokio::fs`. The custom `CancellationToken` in `orchestrator/cancel.rs` is replaced by `tokio_util::sync::CancellationToken` (already used elsewhere in the codebase at `plugin/host_fns.rs:850`).

**Tech Stack:** Rust 2024, tokio (spawn_blocking), anyhow, dirs, tokio-util (sync feature)

**Key Design Decisions:**
- `os::fs` has two APIs: `pub async fn` primary + `pub mod blocking` for sync callers (host_fns extism callbacks are sync)
- `os::dirs` wraps the `dirs` crate with `anyhow::Result` instead of `Option` — standardizes error handling
- Application-specific paths (`~/.harmont/`, `~/.config/harmont/plugins/`) stay in `hm` — only generic OS primitives move to `hm-util`
- `creds_store` and `Config` stay sync using `blocking` API — their only callers from host_fns are sync extism callbacks
- Signal handler stays in `hm` (application-specific two-stage Ctrl-C with exit code 130)

---

## Opportunity Inventory

Before implementation, here's what was identified and the disposition:

| Module | Location | Disposition | Rationale |
|--------|----------|-------------|-----------|
| `fs_util.rs` | `hm/src/fs_util.rs` | **Extract → `hm-util::os::fs`** | Pure OS utility, no domain logic, reusable |
| `user_config_dir()` | `hm/src/config.rs:14` | **Thin wrapper stays in `hm`; generic `home_dir`/`config_dir` → `hm-util::os::dirs`** | Product path (`~/.harmont/`) is app-specific; underlying dir resolution is generic |
| `user_plugins_dir()` etc. | `hm/src/plugin/paths.rs` | **Stay in `hm`; use `hm-util::os::dirs` underneath** | Product paths; thin wrappers over generic helpers |
| `CancellationToken` | `hm/src/orchestrator/cancel.rs` | **Replace with `tokio_util::sync::CancellationToken`** | 55-line Arc\<AtomicBool\> wrapper; tokio_util has same API + `.cancelled()` future + tree cancellation |
| `wait_cancel` polling loop | `hm/src/orchestrator/docker_host_fns.rs:163` | **Delete; replace with `token.cancelled().await`** | 50ms polling loop → zero-cost wakeup |
| `install_ctrlc` | `hm/src/plugin/signal.rs` | **Stay in `hm`** | Application-specific (two-stage Ctrl-C, exit 130, specific messages) |
| `EventBus` | `hm/src/orchestrator/events.rs` | **Stay in `hm`** | Domain-specific (BuildEvent broadcast) |
| `ArchiveStore` | `hm/src/orchestrator/archive.rs` | **Stay in `hm`** | Domain-specific (source archives for build runs) |
| `output/` module | `hm/src/output/` | **Stay in `hm`** | Tightly coupled to CLI output preferences |

---

## Task 1: Create `hm-util` crate skeleton

**Files:**
- Create: `crates/hm-util/Cargo.toml`
- Create: `crates/hm-util/src/lib.rs`
- Create: `crates/hm-util/src/os/mod.rs`
- Modify: `Cargo.toml` (workspace root)

**Step 1: Create directory structure**

```bash
mkdir -p crates/hm-util/src/os
```

**Step 2: Write `Cargo.toml`**

Create `crates/hm-util/Cargo.toml`:

```toml
[package]
name = "hm-util"
version = "0.0.0-dev"
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "Shared OS and filesystem utilities for Harmont crates."

[dependencies]
anyhow = { workspace = true }
dirs = "6"
tokio = { version = "1", features = ["rt"] }

[dev-dependencies]
tempfile = "3"
tokio = { version = "1", features = ["full", "test-util"] }

[lints]
workspace = true
```

**Step 3: Write `src/lib.rs`**

```rust
pub mod os;
```

**Step 4: Write `src/os/mod.rs`**

```rust
pub mod dirs;
pub mod fs;
```

**Step 5: Create placeholder files so it compiles**

Create `crates/hm-util/src/os/dirs.rs`:
```rust
// Populated in Task 4.
```

Create `crates/hm-util/src/os/fs.rs`:
```rust
// Populated in Task 2.
```

**Step 6: Add to workspace**

In root `Cargo.toml`, add `"crates/hm-util"` to `[workspace.members]` and `[workspace.default-members]`:

```toml
members = [
    "crates/hm",
    "crates/hm-plugin-protocol",
    "crates/hm-plugin-sdk",
    "crates/hm-plugin-docker",
    "crates/hm-plugin-output-human",
    "crates/hm-plugin-output-json",
    "crates/hm-plugin-cloud",
    "crates/hm-fixtures",
    "crates/hm-util",
]
default-members = [
    "crates/hm",
    "crates/hm-plugin-protocol",
    "crates/hm-plugin-sdk",
    "crates/hm-util",
]
```

Also add to `[workspace.dependencies]`:
```toml
hm-util = { path = "crates/hm-util", version = "0.0.0-dev" }
```

**Step 7: Verify compilation**

```bash
cargo check -p hm-util
```

Expected: success (empty modules compile fine).

**Step 8: Commit**

```bash
git add crates/hm-util/ Cargo.toml
git commit -m "feat: add hm-util crate skeleton with os module structure"
```

---

## Task 2: Implement `os::fs` — blocking core + async wrappers

**Files:**
- Create: `crates/hm-util/src/os/fs.rs`

The sync implementation is the proven code from `hm/src/fs_util.rs`. The async API wraps it in `spawn_blocking`.

**Step 1: Write tests for blocking API**

Write tests first in `crates/hm-util/src/os/fs.rs` — these mirror the existing tests from `hm/src/fs_util.rs:131-192`:

```rust
#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::blocking;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn writes_file_and_dir_with_requested_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("sub").join("creds");
        blocking::write_atomic_restricted(&target, b"hello", 0o600, 0o700).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
        let file_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(target.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600, "file mode must be 0o600, got {file_mode:o}");
        assert_eq!(dir_mode, 0o700, "dir mode must be 0o700, got {dir_mode:o}");
    }

    #[test]
    fn overwrites_existing_file_preserving_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("creds");
        blocking::write_atomic_restricted(&target, b"v1", 0o600, 0o700).unwrap();
        blocking::write_atomic_restricted(&target, b"v2", 0o600, 0o700).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"v2");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn tightens_existing_dir_with_looser_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("loose");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        let target = dir.join("creds");
        blocking::write_atomic_restricted(&target, b"x", 0o600, 0o700).unwrap();

        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
    }

    #[test]
    fn remove_if_exists_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nothing");
        blocking::remove_if_exists(&target).unwrap();
        std::fs::write(&target, "x").unwrap();
        blocking::remove_if_exists(&target).unwrap();
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn async_write_atomic_restricted() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("sub").join("async_creds");
        super::write_atomic_restricted(&target, b"async hello", 0o600, 0o700)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"async hello");
        let file_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
    }

    #[tokio::test]
    async fn async_remove_if_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nothing");
        super::remove_if_exists(&target).await.unwrap();
        std::fs::write(&target, "x").unwrap();
        super::remove_if_exists(&target).await.unwrap();
        assert!(!target.exists());
    }
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test -p hm-util
```

Expected: FAIL — `blocking` module and functions don't exist yet.

**Step 3: Implement the full `os::fs` module**

Write `crates/hm-util/src/os/fs.rs`:

```rust
use std::path::Path;

use anyhow::{Context, Result};

// ---------------------------------------------------------------------------
// Private sync core — shared by async wrappers and blocking module
// ---------------------------------------------------------------------------

fn write_atomic_restricted_sync(
    path: &Path,
    contents: &[u8],
    file_mode: u32,
    dir_mode: u32,
) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;

    create_dir_with_mode_sync(parent, dir_mode)
        .with_context(|| format!("creating {}", parent.display()))?;

    let file_name = path
        .file_name()
        .with_context(|| format!("{} has no file name", path.display()))?
        .to_os_string();
    let mut tmp_name = file_name;
    tmp_name.push(format!(".tmp.{}", std::process::id()));
    let tmp_path = parent.join(&tmp_name);

    write_file_with_mode_sync(&tmp_path, contents, file_mode)
        .with_context(|| format!("writing {}", tmp_path.display()))?;

    let persist_result = std::fs::rename(&tmp_path, path)
        .with_context(|| format!("renaming {} -> {}", tmp_path.display(), path.display()));

    if persist_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    persist_result?;

    Ok(())
}

fn remove_if_exists_sync(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

#[cfg(unix)]
fn create_dir_with_mode_sync(dir: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if dir.exists() {
        let current = std::fs::metadata(dir)?.permissions().mode() & 0o777;
        if current != mode {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))?;
        }
    } else {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(mode)
            .create(dir)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_dir_with_mode_sync(dir: &Path, _mode: u32) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn write_file_with_mode_sync(path: &Path, contents: &[u8], mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)?;
    f.write_all(contents)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_file_with_mode_sync(path: &Path, contents: &[u8], _mode: u32) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

// ---------------------------------------------------------------------------
// Public async API
// ---------------------------------------------------------------------------

/// Write `contents` to `path` atomically with `file_mode`, ensuring the
/// parent directory exists and is set to `dir_mode`.
///
/// On Unix the target file is created with the requested mode before any
/// bytes are written, closing the TOCTOU window. Contents are written to
/// a sibling tempfile and renamed over `path`.
///
/// Offloads to the blocking thread pool via `spawn_blocking`.
pub async fn write_atomic_restricted(
    path: impl AsRef<Path>,
    contents: impl AsRef<[u8]>,
    file_mode: u32,
    dir_mode: u32,
) -> Result<()> {
    let path = path.as_ref().to_owned();
    let contents = contents.as_ref().to_vec();
    tokio::task::spawn_blocking(move || {
        write_atomic_restricted_sync(&path, &contents, file_mode, dir_mode)
    })
    .await
    .context("write_atomic_restricted task panicked")?
}

/// Remove a file if it exists; silently return `Ok(())` if not found.
///
/// Offloads to the blocking thread pool via `spawn_blocking`.
pub async fn remove_if_exists(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref().to_owned();
    tokio::task::spawn_blocking(move || remove_if_exists_sync(&path))
        .await
        .context("remove_if_exists task panicked")?
}

// ---------------------------------------------------------------------------
// Blocking (synchronous) API
// ---------------------------------------------------------------------------

/// Synchronous variants for use in contexts that cannot await
/// (e.g. extism host function callbacks).
pub mod blocking {
    use std::path::Path;

    use anyhow::Result;

    /// Synchronous version of [`super::write_atomic_restricted`].
    pub fn write_atomic_restricted(
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
        file_mode: u32,
        dir_mode: u32,
    ) -> Result<()> {
        super::write_atomic_restricted_sync(
            path.as_ref(),
            contents.as_ref(),
            file_mode,
            dir_mode,
        )
    }

    /// Synchronous version of [`super::remove_if_exists`].
    pub fn remove_if_exists(path: impl AsRef<Path>) -> Result<()> {
        super::remove_if_exists_sync(path.as_ref())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::blocking;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn writes_file_and_dir_with_requested_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("sub").join("creds");
        blocking::write_atomic_restricted(&target, b"hello", 0o600, 0o700).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
        let file_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(target.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600, "file mode must be 0o600, got {file_mode:o}");
        assert_eq!(dir_mode, 0o700, "dir mode must be 0o700, got {dir_mode:o}");
    }

    #[test]
    fn overwrites_existing_file_preserving_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("creds");
        blocking::write_atomic_restricted(&target, b"v1", 0o600, 0o700).unwrap();
        blocking::write_atomic_restricted(&target, b"v2", 0o600, 0o700).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"v2");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn tightens_existing_dir_with_looser_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("loose");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        let target = dir.join("creds");
        blocking::write_atomic_restricted(&target, b"x", 0o600, 0o700).unwrap();

        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
    }

    #[test]
    fn remove_if_exists_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nothing");
        blocking::remove_if_exists(&target).unwrap();
        std::fs::write(&target, "x").unwrap();
        blocking::remove_if_exists(&target).unwrap();
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn async_write_atomic_restricted() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("sub").join("async_creds");
        super::write_atomic_restricted(&target, b"async hello", 0o600, 0o700)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"async hello");
        let file_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
    }

    #[tokio::test]
    async fn async_remove_if_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nothing");
        super::remove_if_exists(&target).await.unwrap();
        std::fs::write(&target, "x").unwrap();
        super::remove_if_exists(&target).await.unwrap();
        assert!(!target.exists());
    }
}
```

**Step 4: Run tests to verify they pass**

```bash
cargo test -p hm-util
```

Expected: all 6 tests pass.

**Step 5: Commit**

```bash
git add crates/hm-util/src/os/fs.rs
git commit -m "feat(hm-util): implement os::fs with async + blocking atomic file I/O"
```

---

## Task 3: Implement `os::dirs` — platform directory resolution

**Files:**
- Modify: `crates/hm-util/src/os/dirs.rs`

**Step 1: Write tests**

```rust
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_resolves() {
        let p = home_dir().unwrap();
        assert!(p.exists(), "home dir should exist: {}", p.display());
    }

    #[test]
    fn config_dir_resolves() {
        let p = config_dir().unwrap();
        assert!(
            p.to_string_lossy().len() > 1,
            "config dir should be a real path"
        );
    }
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test -p hm-util -- dirs
```

Expected: FAIL — functions don't exist.

**Step 3: Implement `os::dirs`**

Write `crates/hm-util/src/os/dirs.rs`:

```rust
use std::path::PathBuf;

use anyhow::{Context, Result};

/// Platform home directory (`~/` on Unix, `C:\Users\<user>` on Windows).
pub fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("could not determine home directory")
}

/// Platform config directory (`~/.config` on Linux,
/// `~/Library/Application Support` on macOS, `%APPDATA%` on Windows).
pub fn config_dir() -> Result<PathBuf> {
    dirs::config_dir().context("could not determine config directory")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_resolves() {
        let p = home_dir().unwrap();
        assert!(p.exists(), "home dir should exist: {}", p.display());
    }

    #[test]
    fn config_dir_resolves() {
        let p = config_dir().unwrap();
        assert!(
            p.to_string_lossy().len() > 1,
            "config dir should be a real path"
        );
    }
}
```

**Step 4: Run tests**

```bash
cargo test -p hm-util
```

Expected: all tests pass (6 fs + 2 dirs).

**Step 5: Commit**

```bash
git add crates/hm-util/src/os/dirs.rs
git commit -m "feat(hm-util): add os::dirs for platform directory resolution"
```

---

## Task 4: Migrate `hm` to use `hm-util::os::fs`

**Files:**
- Modify: `crates/hm/Cargo.toml` — add `hm-util` dependency
- Modify: `crates/hm/src/config.rs:84` — use `hm_util::os::fs::blocking`
- Modify: `crates/hm/src/creds_store.rs:35` — use `hm_util::os::fs::blocking`
- Modify: `crates/hm/src/lib.rs:14` — remove `pub mod fs_util;`
- Delete: `crates/hm/src/fs_util.rs`

**Step 1: Add `hm-util` dependency to `hm`**

In `crates/hm/Cargo.toml`, add to `[dependencies]`:

```toml
hm-util = { workspace = true }
```

**Step 2: Update `config.rs` — replace `crate::fs_util` with `hm_util::os::fs::blocking`**

In `crates/hm/src/config.rs`, line 84, change:

```rust
// Before:
crate::fs_util::write_atomic_restricted(&path, serialized.as_bytes(), 0o644, 0o700)
// After:
hm_util::os::fs::blocking::write_atomic_restricted(&path, serialized.as_bytes(), 0o644, 0o700)
```

**Step 3: Update `creds_store.rs` — same replacement**

In `crates/hm/src/creds_store.rs`, line 35, change:

```rust
// Before:
crate::fs_util::write_atomic_restricted(&p, serialized.as_bytes(), 0o600, 0o700)
// After:
hm_util::os::fs::blocking::write_atomic_restricted(&p, serialized.as_bytes(), 0o600, 0o700)
```

Also update the module doc comment at line 4:

```rust
// Before:
//! mode 0o600 (parent dir 0o700) via [`crate::fs_util::write_atomic_restricted`].
// After:
//! mode 0o600 (parent dir 0o700) via [`hm_util::os::fs::blocking::write_atomic_restricted`].
```

**Step 4: Remove `fs_util` module from `lib.rs`**

In `crates/hm/src/lib.rs`, remove line 14:

```rust
pub mod fs_util;
```

**Step 5: Delete `fs_util.rs`**

```bash
rm crates/hm/src/fs_util.rs
```

**Step 6: Update doc comment in `fs_util.rs` references**

The doc header in the now-deleted file referenced `crate::creds_store` and `config::user_config_dir` — these lived in `fs_util.rs` which is now gone. No action needed since the file is deleted.

**Step 7: Verify compilation and tests**

```bash
cargo check -p harmont-cli && cargo test -p harmont-cli
```

Expected: all existing tests pass. The `fs_util::tests` that were in the deleted file are now covered by identical tests in `hm-util`.

**Step 8: Commit**

```bash
git add crates/hm/Cargo.toml crates/hm/src/config.rs crates/hm/src/creds_store.rs crates/hm/src/lib.rs
git rm crates/hm/src/fs_util.rs
git commit -m "refactor: migrate fs_util callers to hm-util::os::fs::blocking"
```

---

## Task 5: Migrate `hm` directory functions to use `hm-util::os::dirs`

**Files:**
- Modify: `crates/hm/src/config.rs:14-16` — use `hm_util::os::dirs::home_dir`
- Modify: `crates/hm/src/plugin/paths.rs:16` — use `hm_util::os::dirs::config_dir`

**Step 1: Update `config.rs::user_config_dir()`**

In `crates/hm/src/config.rs`, change the `user_config_dir` function (lines 14-17):

```rust
// Before:
pub fn user_config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".harmont"))
}

// After:
pub fn user_config_dir() -> Result<PathBuf> {
    Ok(hm_util::os::dirs::home_dir()?.join(".harmont"))
}
```

**Step 2: Update `plugin/paths.rs::user_plugins_dir()`**

In `crates/hm/src/plugin/paths.rs`, change line 16:

```rust
// Before:
pub fn user_plugins_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("harmont").join("plugins"))
}

// After:
pub fn user_plugins_dir() -> Option<PathBuf> {
    hm_util::os::dirs::config_dir()
        .ok()
        .map(|p| p.join("harmont").join("plugins"))
}
```

**Step 3: Remove `dirs` direct dependency from `hm`**

In `crates/hm/Cargo.toml`, remove:

```toml
dirs = "6"
```

**Step 4: Verify no other `dirs::` usage in `hm`**

```bash
grep -rn 'dirs::' crates/hm/src/ --include='*.rs'
```

Expected: no matches (all usage now goes through `hm_util::os::dirs`).

**Step 5: Run tests**

```bash
cargo test -p harmont-cli
```

Expected: all tests pass.

**Step 6: Commit**

```bash
git add crates/hm/Cargo.toml crates/hm/src/config.rs crates/hm/src/plugin/paths.rs
git commit -m "refactor: delegate directory resolution to hm-util::os::dirs"
```

---

## Task 6: Replace custom `CancellationToken` with `tokio_util::sync::CancellationToken`

**Context:** The custom `CancellationToken` in `orchestrator/cancel.rs` is a 55-line `Arc<AtomicBool>` wrapper. `tokio_util::sync::CancellationToken` provides the same API (`new()`, `cancel()`, `is_cancelled()`) plus a zero-cost `.cancelled()` future — eliminating the 50ms polling loop in `docker_host_fns.rs:163-171`.

Note: `tokio_util::sync::CancellationToken` is already used in `plugin/host_fns.rs:850` for the OAuth loopback server, so the dependency and feature flag are already available.

**Files:**
- Delete: `crates/hm/src/orchestrator/cancel.rs`
- Modify: `crates/hm/src/orchestrator/mod.rs` — remove `pub mod cancel;`
- Modify: `crates/hm/src/orchestrator/state.rs:27` — update import
- Modify: `crates/hm/src/orchestrator/scheduler.rs:47,74` — update import + construction
- Modify: `crates/hm/src/orchestrator/docker_host_fns.rs:163-171` — replace polling loop
- Modify: `crates/hm/src/plugin/signal.rs:18` — update import
- Modify: `crates/hm/src/plugin/host_fns.rs:925` — update path

**Step 1: Update `orchestrator/mod.rs` — remove cancel module**

In `crates/hm/src/orchestrator/mod.rs`, remove line 11:

```rust
pub mod cancel;
```

**Step 2: Update `orchestrator/state.rs` — change import**

In `crates/hm/src/orchestrator/state.rs`, replace line 27:

```rust
// Before:
use super::cancel::CancellationToken;
// After:
use tokio_util::sync::CancellationToken;
```

**Step 3: Update `orchestrator/scheduler.rs` — change import**

In `crates/hm/src/orchestrator/scheduler.rs`, replace line 47:

```rust
// Before:
use super::cancel::CancellationToken;
// After:
use tokio_util::sync::CancellationToken;
```

**Step 4: Update `plugin/signal.rs` — change import**

In `crates/hm/src/plugin/signal.rs`, replace line 18:

```rust
// Before:
use crate::orchestrator::cancel::CancellationToken;
// After:
use tokio_util::sync::CancellationToken;
```

**Step 5: Update `orchestrator/docker_host_fns.rs` — replace polling loop with `.cancelled()`**

Replace lines 163-171:

```rust
// Before:
async fn wait_cancel(cancel: &crate::orchestrator::cancel::CancellationToken) {
    // Poll the atomic every 50ms. Cheap; never wakes a thread early.
    loop {
        if cancel.is_cancelled() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

// After:
async fn wait_cancel(cancel: &tokio_util::sync::CancellationToken) {
    cancel.cancelled().await;
}
```

**Step 6: Update `plugin/host_fns.rs:925` — fix `is_cancelled` path**

In `crates/hm/src/plugin/host_fns.rs`, line 925 references `s.cancel.is_cancelled()`. The `CancellationToken` field type changes but the method name is the same — verify this line still compiles (it should, as `tokio_util::sync::CancellationToken` also has `is_cancelled()`).

**Step 7: Delete `orchestrator/cancel.rs`**

```bash
rm crates/hm/src/orchestrator/cancel.rs
```

**Step 8: Verify compilation and tests**

```bash
cargo check -p harmont-cli && cargo test -p harmont-cli
```

Expected: compiles and all tests pass. The three tests that were in `cancel.rs` (default_is_not_cancelled, cancel_persists, cancel_is_clone_shared) are trivially true for `tokio_util::sync::CancellationToken` — the upstream crate tests them.

**Step 9: Commit**

```bash
git rm crates/hm/src/orchestrator/cancel.rs
git add crates/hm/src/orchestrator/mod.rs crates/hm/src/orchestrator/state.rs crates/hm/src/orchestrator/scheduler.rs crates/hm/src/orchestrator/docker_host_fns.rs crates/hm/src/plugin/signal.rs crates/hm/src/plugin/host_fns.rs
git commit -m "refactor: replace custom CancellationToken with tokio_util::sync::CancellationToken

Eliminates 55-line Arc<AtomicBool> wrapper. The 50ms polling loop in
wait_cancel is replaced by the zero-cost .cancelled() future."
```

---

## Task 7: Final verification and cleanup

**Step 1: Full workspace build**

```bash
cargo build --workspace
```

Expected: clean build, no warnings.

**Step 2: Full workspace test suite**

```bash
cargo test --workspace
```

Expected: all tests pass.

**Step 3: Clippy**

```bash
cargo clippy --workspace -- -D warnings
```

Expected: no warnings.

**Step 4: Verify module structure matches intent**

```bash
find crates/hm-util/src -name '*.rs' | sort
```

Expected:
```
crates/hm-util/src/lib.rs
crates/hm-util/src/os/dirs.rs
crates/hm-util/src/os/fs.rs
crates/hm-util/src/os/mod.rs
```

**Step 5: Verify deleted files are gone**

```bash
test ! -f crates/hm/src/fs_util.rs && echo "fs_util.rs removed"
test ! -f crates/hm/src/orchestrator/cancel.rs && echo "cancel.rs removed"
```

**Step 6: Final commit if any cleanup was needed**

```bash
git status
# If clean: done. If changes: commit cleanup.
```

---

## Future Opportunities (Not In Scope)

These were identified during analysis but deferred:

1. **Async propagation in `Config` and `RunContext`** — `Config::load()` and `Config::save()` could become async, using `hm_util::os::fs::write_atomic_restricted` (async variant) directly. `RunContext::from_cli()` would become `async fn from_cli()`. Benefit: avoids blocking tokio worker thread during config I/O. Cost: minor — `from_cli` is only called from `async fn run()` in `main.rs`. Deferred because config files are tiny and the perf impact is negligible.

2. **`creds_store` async variant** — Blocked by extism host_fn callbacks being sync. Would require `block_in_place` bridge in host_fns. No benefit until extism supports async host functions.

3. **Signal handler extraction** — `plugin/signal.rs::install_ctrlc` is application-specific (two-stage Ctrl-C, exit code 130, stderr messages). Not a reusable utility. Could move from `plugin/` to a top-level `signal.rs` module if the `plugin/` location feels wrong, but extraction to `hm-util` is over-engineering.

4. **`output/format.rs` time utilities** — `rel_time()`, `duration_human()`, `elapsed_between()` are generic but small (< 30 lines total). Not worth extracting until a second consumer exists.

5. **`os::process` module** — Future home for process-related utilities if patterns emerge (e.g., a generic `spawn_and_stream` helper for the Docker client).
