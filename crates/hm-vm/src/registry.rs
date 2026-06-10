//! Snapshot registry backed by `SQLite`.
//!
//! Provides persistent LRU caching of [`SnapshotId`]s across process restarts.
//! The registry evicts the least-recently-accessed entries when the capacity is
//! exceeded, returning the evicted snapshot IDs so the caller can clean up
//! backend resources.

use std::num::NonZeroU64;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use std::sync::Mutex;

use anyhow::Result;
use rusqlite::Connection;

use crate::types::SnapshotId;

/// Persistent LRU cache mapping opaque keys to [`SnapshotId`]s.
///
/// Backed by a single `SQLite` table with WAL journaling. The registry tracks
/// the last-access timestamp for every entry and evicts the oldest entries
/// when the configured capacity is exceeded.
///
/// The inner `Connection` is wrapped in a [`Mutex`] so that the registry
/// (and any struct containing it, e.g. [`crate::vm::HmVm`]) satisfies
/// `Send + Sync` for safe sharing across async tasks.
#[derive(derive_more::Debug)]
pub struct ImageRegistry {
    #[debug(skip)]
    conn: Mutex<Connection>,
    capacity: NonZeroU64,
}

/// Returns the current Unix epoch in seconds.
fn epoch_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

impl ImageRegistry {
    /// Open or create the registry database at `path`.
    ///
    /// The parent directory is created if it does not exist. The database uses
    /// WAL mode and `NORMAL` synchronous for a good balance of durability and
    /// performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or the schema cannot
    /// be applied.
    pub fn open(path: &Path, capacity: NonZeroU64) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snapshots (
                 key         TEXT PRIMARY KEY,
                 snapshot_id TEXT NOT NULL,
                 accessed_at INTEGER NOT NULL
             );",
        )?;

        // Idempotent migration: add workspace_dir column if missing.
        let has_ws_col: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('snapshots') WHERE name='workspace_dir'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !has_ws_col {
            conn.execute_batch("ALTER TABLE snapshots ADD COLUMN workspace_dir TEXT;")?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
            capacity,
        })
    }

    /// Look up a cached snapshot and update its access time.
    ///
    /// Returns `None` if no entry exists for `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<SnapshotId> {
        self.get_with_workspace(key).map(|(snap, _)| snap)
    }

    /// Look up a cached snapshot and its workspace directory, updating the
    /// access time.
    ///
    /// Returns `None` if no entry exists for `key`.
    #[must_use]
    pub fn get_with_workspace(&self, key: &str) -> Option<(SnapshotId, Option<String>)> {
        let now = epoch_secs();
        let conn = self.conn.lock().ok()?;

        let result: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT snapshot_id, workspace_dir FROM snapshots WHERE key = ?1",
                [key],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .ok();

        if result.is_some() {
            let _ = conn.execute(
                "UPDATE snapshots SET accessed_at = ?1 WHERE key = ?2",
                rusqlite::params![now, key],
            );
        }

        drop(conn);
        result.map(|(snap, ws)| (SnapshotId::new(snap), ws))
    }

    /// Insert or update a cache entry.
    ///
    /// The `workspace_dir` column is always written as `NULL`: the registry
    /// stores system-state snapshots only. Workspace state is strictly
    /// run-scoped and never persisted across runs (non-`NULL` values are
    /// legacy rows kept solely so their directories can be reaped).
    ///
    /// Returns the evicted entries (snapshot ID and optional legacy workspace
    /// directory) to keep the registry within its configured capacity. The
    /// caller is responsible for cleaning up backend resources and legacy
    /// workspace directories associated with evicted entries.
    ///
    /// Upsert + eviction run inside a single transaction so concurrent
    /// writers (including other processes sharing the database file) can
    /// never observe a partially-applied put.
    ///
    /// # Errors
    ///
    /// Returns an error if the registry mutex is poisoned or any statement
    /// fails; the caller must treat the snapshot as unregistered.
    #[allow(
        clippy::significant_drop_tightening,
        reason = "the transaction borrows the guarded connection until commit"
    )]
    pub fn put(
        &self,
        key: &str,
        snapshot: &SnapshotId,
    ) -> Result<Vec<(SnapshotId, Option<String>)>> {
        let now = epoch_secs();

        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("registry mutex poisoned"))?;
        let tx = conn.transaction()?;

        let snapshot_id: &str = snapshot.as_ref();
        tx.execute(
            "INSERT OR REPLACE INTO snapshots (key, snapshot_id, accessed_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![key, snapshot_id, now],
        )?;

        let evicted = Self::evict_overflow_tx(&tx, self.capacity)?;
        tx.commit()?;
        Ok(evicted)
    }

    /// Remove a specific entry.
    ///
    /// Returns the removed snapshot's ID and workspace directory so the
    /// caller can clean up backend resources, or `None` if the key was
    /// not present.
    #[must_use]
    pub fn invalidate(&self, key: &str) -> Option<(SnapshotId, Option<String>)> {
        let conn = self.conn.lock().ok()?;

        let row: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT snapshot_id, workspace_dir FROM snapshots WHERE key = ?1",
                [key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if row.is_some() {
            let _ = conn.execute("DELETE FROM snapshots WHERE key = ?1", [key]);
        }

        drop(conn);
        row.map(|(snap, ws)| (SnapshotId::new(snap), ws))
    }

    /// Compare-and-delete: remove the entry for `key` only if it still maps
    /// to `expected`.
    ///
    /// This closes the race where a stale entry is observed, the lock is
    /// released for an async backend check, and a fresh entry is inserted
    /// under the same key before the invalidation lands — a plain
    /// [`Self::invalidate`] would destroy the fresh entry.
    ///
    /// Returns `Some(legacy_workspace_dir)` when the observed row was
    /// deleted, or `None` when the key is absent or now holds a different
    /// snapshot (the concurrently re-inserted entry survives).
    #[must_use]
    #[allow(
        clippy::significant_drop_tightening,
        reason = "the prepared statement borrows the guarded connection"
    )]
    pub fn invalidate_if(&self, key: &str, expected: &SnapshotId) -> Option<Option<String>> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare(
                "DELETE FROM snapshots WHERE key = ?1 AND snapshot_id = ?2
                 RETURNING workspace_dir",
            )
            .ok()?;
        let mut rows = stmt
            .query_map(rusqlite::params![key, expected.as_ref()], |row| {
                row.get::<_, Option<String>>(0)
            })
            .ok()?;
        let legacy_ws = rows.next()?.ok()?;
        Some(legacy_ws)
    }

    /// Returns `true` if any entry currently maps to `snapshot`.
    ///
    /// Used as a pre-removal guard for deferred eviction cleanup: a tag that
    /// was evicted earlier may have been re-registered since (by a later step
    /// in this run or by a concurrent process); Docker re-tagging means the
    /// tag now names the *fresh* image, so removing it would destroy a live
    /// cache entry.
    #[must_use]
    pub fn contains_snapshot(&self, snapshot: &SnapshotId) -> bool {
        let Ok(conn) = self.conn.lock() else {
            // A poisoned lock means we cannot prove the snapshot is unused;
            // report it as referenced so callers err on the side of keeping it.
            return true;
        };
        conn.query_row(
            "SELECT COUNT(*) FROM snapshots WHERE snapshot_id = ?1",
            [snapshot.as_ref()],
            |row| row.get::<_, i64>(0),
        )
        .map_or(true, |n| n > 0)
    }

    /// Returns the number of cached entries.
    #[must_use]
    pub fn len(&self) -> u64 {
        let Ok(conn) = self.conn.lock() else {
            return 0;
        };
        conn.query_row("SELECT COUNT(*) FROM snapshots", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0)
        .try_into()
        .unwrap_or(0)
    }

    /// Returns `true` if the registry contains no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Evict the oldest entries (by `accessed_at`) when the registry exceeds
    /// its capacity. Runs inside the caller's transaction.
    ///
    /// A single `DELETE .. RETURNING` statement selects and removes the same
    /// rows, with `key` as a deterministic tie-break for equal timestamps, so
    /// the returned set can never diverge from the deleted set.
    fn evict_overflow_tx(
        tx: &rusqlite::Transaction<'_>,
        capacity: NonZeroU64,
    ) -> Result<Vec<(SnapshotId, Option<String>)>> {
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))?;
        let count = u64::try_from(count).unwrap_or(0);
        let capacity = capacity.get();
        if count <= capacity {
            return Ok(Vec::new());
        }

        let overflow = count - capacity;

        let mut stmt = tx.prepare(
            "DELETE FROM snapshots WHERE key IN (
                 SELECT key FROM snapshots ORDER BY accessed_at ASC, key ASC LIMIT ?1
             ) RETURNING snapshot_id, workspace_dir",
        )?;

        let evicted = stmt
            .query_map([overflow], |row| {
                Ok((
                    SnapshotId::new(row.get::<_, String>(0)?),
                    row.get::<_, Option<String>>(1)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(evicted)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn open_temp(capacity: u64) -> (ImageRegistry, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("registry.db");
        let capacity = NonZeroU64::new(capacity).expect("capacity must be non-zero");
        let registry = ImageRegistry::open(&db_path, capacity).expect("failed to open registry");
        (registry, dir)
    }

    #[test]
    fn get_returns_none_for_unknown_key() {
        let (reg, _dir) = open_temp(10);
        assert!(reg.get("nonexistent").is_none());
    }

    /// Insert a legacy-style row with a non-NULL `workspace_dir`, as written
    /// by pre-fix versions that persisted cached workspaces.
    fn insert_legacy_row(reg: &ImageRegistry, key: &str, snap: &str, ws: &str) {
        let conn = reg.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO snapshots (key, snapshot_id, accessed_at, workspace_dir)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![key, snap, epoch_secs(), ws],
        )
        .unwrap();
    }

    #[test]
    fn put_then_get_returns_snapshot() {
        let (reg, _dir) = open_temp(10);
        let snap = SnapshotId::new("snap-abc");
        let evicted = reg.put("my-key", &snap).expect("put");
        assert!(evicted.is_empty());

        let got = reg.get("my-key");
        assert_eq!(got, Some(SnapshotId::new("snap-abc")));
    }

    #[test]
    fn get_updates_access_time() {
        let (reg, _dir) = open_temp(2);

        // Insert a, then b. "a" is older by insertion order.
        reg.put("a", &SnapshotId::new("snap-a")).expect("put a");

        // Tiny sleep so timestamps differ.
        std::thread::sleep(std::time::Duration::from_secs(1));

        reg.put("b", &SnapshotId::new("snap-b")).expect("put b");

        // Touch "a" so it becomes the most recently accessed.
        std::thread::sleep(std::time::Duration::from_secs(1));
        let _ = reg.get("a");

        // Now insert "c" -- capacity is 2, so one must be evicted.
        // "b" should be evicted since "a" was touched more recently.
        std::thread::sleep(std::time::Duration::from_secs(1));
        let evicted = reg.put("c", &SnapshotId::new("snap-c")).expect("put c");

        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].0, SnapshotId::new("snap-b"));

        // "a" should still be present.
        assert!(reg.get("a").is_some());
        // "b" should be gone.
        assert!(reg.get("b").is_none());
    }

    #[test]
    fn eviction_returns_overflow_entries() {
        let (reg, _dir) = open_temp(2);

        reg.put("x", &SnapshotId::new("snap-x")).expect("put x");
        std::thread::sleep(std::time::Duration::from_secs(1));
        reg.put("y", &SnapshotId::new("snap-y")).expect("put y");
        std::thread::sleep(std::time::Duration::from_secs(1));

        // This third insert should evict the oldest ("x").
        let evicted = reg.put("z", &SnapshotId::new("snap-z")).expect("put z");

        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].0, SnapshotId::new("snap-x"));
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn survives_reopen() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("registry.db");

        let capacity = NonZeroU64::new(10).expect("capacity must be non-zero");

        {
            let reg = ImageRegistry::open(&db_path, capacity).expect("open");
            reg.put("persistent", &SnapshotId::new("snap-persist"))
                .expect("put");
            assert_eq!(reg.len(), 1);
            // reg is dropped here, closing the connection.
        }

        let reg2 = ImageRegistry::open(&db_path, capacity).expect("reopen");
        assert_eq!(reg2.len(), 1);
        let got = reg2.get("persistent");
        assert_eq!(got, Some(SnapshotId::new("snap-persist")));
    }

    #[test]
    fn invalidate_returns_removed_snapshot() {
        let (reg, _dir) = open_temp(10);
        let snap = SnapshotId::new("snap-rm");
        reg.put("to-remove", &snap).expect("put");

        let removed = reg.invalidate("to-remove");
        assert_eq!(removed, Some((SnapshotId::new("snap-rm"), None)));
        assert!(reg.get("to-remove").is_none());
        assert_eq!(reg.len(), 0);

        // Invalidating a non-existent key returns None.
        let removed2 = reg.invalidate("to-remove");
        assert!(removed2.is_none());
    }

    #[test]
    fn put_writes_null_workspace() {
        let (reg, _dir) = open_temp(10);
        reg.put("plain-key", &SnapshotId::new("snap-plain"))
            .expect("put");

        let (_, got_ws) = reg.get_with_workspace("plain-key").unwrap();
        assert!(got_ws.is_none());
    }

    #[test]
    fn put_overwrites_legacy_workspace_with_null() {
        let (reg, _dir) = open_temp(10);
        insert_legacy_row(&reg, "k", "snap-old", "/ws/legacy");

        reg.put("k", &SnapshotId::new("snap-new")).expect("put");

        let (snap, ws) = reg.get_with_workspace("k").unwrap();
        assert_eq!(snap, SnapshotId::new("snap-new"));
        assert!(ws.is_none());
    }

    #[test]
    fn invalidate_if_matching_snapshot_deletes_row() {
        let (reg, _dir) = open_temp(10);
        let snap = SnapshotId::new("snap-cas");
        reg.put("cas-key", &snap).expect("put");

        let removed = reg.invalidate_if("cas-key", &snap);
        assert_eq!(removed, Some(None));
        assert!(reg.get("cas-key").is_none());
    }

    #[test]
    fn invalidate_if_mismatched_snapshot_keeps_row() {
        let (reg, _dir) = open_temp(10);
        reg.put("cas-key", &SnapshotId::new("snap-fresh"))
            .expect("put");

        // A stale observer tries to invalidate with the snapshot it saw
        // earlier; the fresh row must survive.
        let removed = reg.invalidate_if("cas-key", &SnapshotId::new("snap-stale"));
        assert!(removed.is_none());
        assert_eq!(reg.get("cas-key"), Some(SnapshotId::new("snap-fresh")));

        // Absent key is also a no-op.
        assert!(
            reg.invalidate_if("missing", &SnapshotId::new("whatever"))
                .is_none()
        );
    }

    #[test]
    fn invalidate_if_returns_legacy_workspace_for_cleanup() {
        let (reg, _dir) = open_temp(10);
        insert_legacy_row(&reg, "legacy", "snap-legacy", "/ws/legacy");

        let removed = reg.invalidate_if("legacy", &SnapshotId::new("snap-legacy"));
        assert_eq!(removed, Some(Some("/ws/legacy".into())));
        assert!(reg.get("legacy").is_none());
    }

    #[test]
    fn contains_snapshot_tracks_rows() {
        let (reg, _dir) = open_temp(10);
        let snap = SnapshotId::new("snap-ref");
        assert!(!reg.contains_snapshot(&snap));

        reg.put("k", &snap).expect("put");
        assert!(reg.contains_snapshot(&snap));

        // The same snapshot under a second key still counts.
        reg.put("k2", &snap).expect("put");
        let _ = reg.invalidate("k");
        assert!(reg.contains_snapshot(&snap));

        let _ = reg.invalidate("k2");
        assert!(!reg.contains_snapshot(&snap));
    }

    #[test]
    fn eviction_returns_legacy_workspace_path() {
        let (reg, _dir) = open_temp(1);
        insert_legacy_row(&reg, "a", "snap-a", "/ws/a");
        std::thread::sleep(std::time::Duration::from_secs(1));

        let evicted = reg.put("b", &SnapshotId::new("snap-b")).expect("put b");
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].0, SnapshotId::new("snap-a"));
        assert_eq!(evicted[0].1.as_deref(), Some("/ws/a"));
    }
}
