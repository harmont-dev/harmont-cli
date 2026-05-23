# Atomic Rename-Over Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a cross-platform `pub async fn atomic_rename_over` that atomically replaces a target file with a source file, using `ReplaceFileW` on Windows and `rename(2)` on Unix.

**Architecture:** A private sync function (`atomic_rename_over_sync`) contains platform-specific logic behind `#[cfg]` gates. The public async wrapper offloads it to `spawn_blocking`. On Windows, `ReplaceFileW` is preferred (preserves ACLs/streams); falls back to `MoveFileExW` when the target doesn't exist yet. On Unix, `std::fs::rename` is already atomic. The existing `write_atomic_restricted_sync` is updated to call this instead of raw `std::fs::rename`. Dead code (`remove_file_if_exists` and its sync helper) is removed.

**Tech Stack:** `windows` crate (0.62, `Win32_Storage_FileSystem` + `Win32_Foundation` features), conditional on `cfg(windows)`. Tokio `spawn_blocking` for async.

---

### Task 1: Add `windows` crate conditional dependency

**Files:**
- Modify: `crates/hm-util/Cargo.toml`

**Step 1: Add the conditional dependency**

Add to `crates/hm-util/Cargo.toml` after the existing `[dependencies]` entries:

```toml
[target.'cfg(windows)'.dependencies.windows]
version = "0.62"
features = [
    "Win32_Foundation",
    "Win32_Storage_FileSystem",
]
```

**Step 2: Verify it compiles**

Run: `cargo check -p hm-util`
Expected: PASS (on macOS/Linux the `windows` dep is ignored; it only activates on Windows targets)

**Step 3: Commit**

```bash
git add crates/hm-util/Cargo.toml Cargo.lock
git commit -m "feat(hm-util): add windows crate for atomic file replacement"
```

---

### Task 2: Implement `atomic_rename_over` with platform backends

**Files:**
- Modify: `crates/hm-util/src/os/fs.rs`

**Step 1: Write the failing test**

Add at the bottom of the existing `#[cfg(all(test, unix))]` test module in `crates/hm-util/src/os/fs.rs`:

```rust
#[tokio::test]
async fn atomic_rename_over_replaces_target() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let dst = tmp.path().join("target");
    std::fs::write(&dst, b"old").unwrap();
    std::fs::write(&src, b"new").unwrap();

    super::atomic_rename_over(&src, &dst).await.unwrap();

    assert_eq!(std::fs::read(&dst).unwrap(), b"new");
    assert!(!src.exists(), "source should be gone after rename");
}

#[tokio::test]
async fn atomic_rename_over_works_when_target_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let dst = tmp.path().join("target");
    std::fs::write(&src, b"new").unwrap();

    super::atomic_rename_over(&src, &dst).await.unwrap();

    assert_eq!(std::fs::read(&dst).unwrap(), b"new");
    assert!(!src.exists());
}
```

**Step 2: Run the tests to verify they fail**

Run: `cargo test --lib -p hm-util -- atomic_rename_over`
Expected: FAIL — `atomic_rename_over` does not exist yet.

**Step 3: Implement the Unix sync backend**

Add the following private sync function in `crates/hm-util/src/os/fs.rs`, after the existing `write_file_with_mode_sync` non-unix variant (around line 89), before the `// Public async API` section:

```rust
// ---------------------------------------------------------------------------
// Cross-platform atomic rename
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn atomic_rename_over_sync(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(windows)]
fn atomic_rename_over_sync(from: &Path, to: &Path) -> io::Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW,
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        REPLACEFILE_IGNORE_MERGE_ERRORS,
    };

    let from_w = HSTRING::from(from.as_os_str());
    let to_w = HSTRING::from(to.as_os_str());

    // ReplaceFileW preserves ACLs and alternate data streams on the
    // target, but requires the target to already exist.
    if to.exists() {
        let result = unsafe {
            ReplaceFileW(
                &to_w,
                &from_w,
                windows::core::PCWSTR::null(),
                REPLACEFILE_IGNORE_MERGE_ERRORS,
                None,
                None,
            )
        };
        return result.map_err(|e| io::Error::new(io::ErrorKind::Other, e));
    }

    // Target doesn't exist yet — fall back to MoveFileExW which handles
    // both cases but doesn't preserve target metadata (irrelevant here
    // since there is no target).
    let result = unsafe {
        MoveFileExW(
            &from_w,
            &to_w,
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    result.map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}
```

**Step 4: Add the public async wrapper**

Add this in the "Public async API" section of `crates/hm-util/src/os/fs.rs`, after the existing `write_atomic_restricted` async fn:

```rust
/// Atomically replace `to` with `from`.
///
/// On Unix this is a single `rename(2)` call — atomic by POSIX
/// guarantee. On Windows this uses `ReplaceFileW` (preserves ACLs
/// and alternate data streams) when the target exists, falling back
/// to `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` for first-write.
///
/// # Errors
///
/// Returns an error if the rename fails (permission denied, cross-device,
/// source missing, etc.).
pub async fn atomic_rename_over(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
) -> io::Result<()> {
    let from = from.as_ref().to_owned();
    let to = to.as_ref().to_owned();
    tokio::task::spawn_blocking(move || atomic_rename_over_sync(&from, &to))
        .await
        .map_err(io::Error::other)?
}
```

**Step 5: Run the tests to verify they pass**

Run: `cargo test --lib -p hm-util -- atomic_rename_over`
Expected: PASS — both tests should succeed.

**Step 6: Run clippy**

Run: `cargo clippy -p hm-util -- -D warnings`
Expected: PASS

**Step 7: Commit**

```bash
git add crates/hm-util/src/os/fs.rs
git commit -m "feat(hm-util): add atomic_rename_over with ReplaceFileW on Windows"
```

---

### Task 3: Wire `atomic_rename_over_sync` into `write_atomic_restricted_sync`

**Files:**
- Modify: `crates/hm-util/src/os/fs.rs`

**Step 1: Replace `std::fs::rename` with `atomic_rename_over_sync`**

In `write_atomic_restricted_sync` (around line 42-46), replace:

```rust
    let persist_result = std::fs::rename(&tmp_path, path);
    if persist_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    persist_result
```

with:

```rust
    let persist_result = atomic_rename_over_sync(&tmp_path, path);
    if persist_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    persist_result
```

**Step 2: Run all tests**

Run: `cargo test --lib -p hm-util -p harmont-cli`
Expected: All tests pass (the existing `write_atomic_restricted` tests exercise this path).

**Step 3: Run clippy**

Run: `cargo clippy -p hm-util -p harmont-cli -- -D warnings`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/hm-util/src/os/fs.rs
git commit -m "refactor(hm-util): use atomic_rename_over_sync in write_atomic_restricted"
```

---

### Task 4: Remove dead code

**Files:**
- Modify: `crates/hm-util/src/os/fs.rs`

**Step 1: Delete `remove_file_if_exists` (async) and its sync helper**

Remove the `remove_file_if_exists` async function (around lines 118-133 in the current file). There is no sync helper to remove — it uses `tokio::fs::remove_file` directly. But check: there may be a `remove_if_exists_sync` private fn — if it exists and has no callers, remove it too.

**Step 2: Run tests**

Run: `cargo test --lib -p hm-util -p harmont-cli`
Expected: All tests pass. If any test references `remove_file_if_exists`, delete that test too (it's testing dead code).

**Step 3: Run clippy**

Run: `cargo clippy -p hm-util -p harmont-cli -- -D warnings`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/hm-util/src/os/fs.rs
git commit -m "chore(hm-util): remove unused remove_file_if_exists"
```

---

### Notes

- **Windows testing**: CI runs on Linux. The `#[cfg(windows)]` code path cannot be tested there. If Windows CI is added later, the existing `atomic_rename_over_replaces_target` and `atomic_rename_over_works_when_target_missing` tests will exercise the Windows path automatically — they are not gated behind `#[cfg(unix)]`.
- **`host_fns.rs:725`** also uses `std::fs::rename` for plugin KV state persistence. That's inside a sync `host_fn` callback, so it can't call the async version. A follow-up could add a `blocking::atomic_rename_over` wrapper, but that's out of scope for this plan (YAGNI — plugin state is low-stakes compared to credentials).
- **`ReplaceFileW` requires same volume**: both `from` and `to` must be on the same filesystem. This is guaranteed for our use case (temp file is created in the same parent directory as the target).
