//! Per-run source archive store.
//!
//! On build start the orchestrator tar.gzs the user's working
//! directory once (via [`crate::local::build_archive_bytes`])
//! and registers the bytes under an opaque `ArchiveId`. Step-executor
//! plugins receive that ID in their `ExecutorInput` and pull bytes
//! via `hm_archive_read`. The host caches archives in memory keyed
//! by ID for the duration of a single `local::run` invocation.

use std::collections::HashMap;
use std::sync::Mutex;

use hm_plugin_protocol::ArchiveId;
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct ArchiveStore {
    archives: Mutex<HashMap<ArchiveId, Vec<u8>>>,
}

impl ArchiveStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new archive. Returns the freshly-minted ID.
    pub fn register(&self, bytes: Vec<u8>) -> ArchiveId {
        let id = ArchiveId::from(Uuid::new_v4());
        let _ = self.archives.lock().map(|mut m| m.insert(id, bytes));
        id
    }

    /// Total size of the archive identified by `id`, or `0` if no
    /// such archive is registered.
    #[must_use]
    pub fn total_size(&self, id: ArchiveId) -> u64 {
        self.archives
            .lock()
            .ok()
            .and_then(|m| m.get(&id).map(|b| b.len() as u64))
            .unwrap_or(0)
    }

    /// Return a clone of the full archive bytes, or `None` if unknown.
    #[must_use]
    pub fn get_bytes(&self, id: ArchiveId) -> Option<Vec<u8>> {
        self.archives.lock().ok()?.get(&id).cloned()
    }

    /// Read up to `max` bytes from offset `offset`. Returns empty
    /// when offset is beyond end, or when the archive is unknown.
    #[must_use]
    pub fn read(&self, id: ArchiveId, offset: u64, max: u64) -> Vec<u8> {
        // We must hold the lock guard across the read so the bytes
        // slice we copy out is consistent. `let...else` here would
        // require holding `bytes` across the early-return branch.
        let Ok(g) = self.archives.lock() else {
            return Vec::new();
        };
        let Some(bytes) = g.get(&id) else {
            return Vec::new();
        };
        // Archive sizes fit in `usize` on 64-bit hosts (the only
        // supported targets); local-mode archives are at most a few
        // hundred MB. The cast cannot truncate in practice.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "archive sizes fit in usize on supported 64-bit hosts"
        )]
        let start = (offset as usize).min(bytes.len());
        #[allow(
            clippy::cast_possible_truncation,
            reason = "archive sizes fit in usize on supported 64-bit hosts"
        )]
        let max_us = max as usize;
        let end = start.saturating_add(max_us).min(bytes.len());
        bytes[start..end].to_vec()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn register_then_read_round_trip() {
        let s = ArchiveStore::new();
        let id = s.register(b"hello world".to_vec());
        assert_eq!(s.total_size(id), 11);
        assert_eq!(s.read(id, 0, 5), b"hello");
        assert_eq!(s.read(id, 6, 5), b"world");
        assert_eq!(s.read(id, 100, 5), Vec::<u8>::new());
    }

    #[test]
    fn unknown_id_returns_empty() {
        let s = ArchiveStore::new();
        let bogus = ArchiveId(Uuid::new_v4());
        assert_eq!(s.total_size(bogus), 0);
        assert_eq!(s.read(bogus, 0, 100), Vec::<u8>::new());
    }
}
