//! Zero-copy shared buffer pool for inter-component data transfer.
//!
//! Inspired by AlloyStack's `faas_buffer` (`common_service/mm/src/faas_buffer/mod.rs`)
//! which uses a shared register of named buffer slots for zero-copy data passing
//! between serverless functions.
//!
//! This module adapts the concept to OpenFang's async runtime using `DashMap` for
//! concurrent access and `bytes::Bytes` for O(1) cloning via reference counting.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use bytes::Bytes;
use dashmap::DashMap;

/// A handle to a named shared buffer slot.
#[derive(Debug, Clone)]
pub struct SharedBuffer {
    /// Slot name (identifier for this buffer).
    pub slot: String,
    /// The actual data, reference-counted for zero-copy cloning.
    pub data: Bytes,
    /// Fingerprint for cache validation / versioning.
    pub fingerprint: u64,
    /// When this buffer was created.
    pub created_at: Instant,
}

impl SharedBuffer {
    /// Data length in bytes.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Cumulative statistics for the buffer pool.
#[derive(Debug, Default)]
pub struct BufferStats {
    pub allocations: AtomicU64,
    pub zero_copy_reads: AtomicU64,
    pub takes: AtomicU64,
    pub bytes_stored_total: AtomicU64,
    pub evictions: AtomicU64,
}

impl BufferStats {
    /// Snapshot current stats as a serializable struct.
    pub fn snapshot(&self) -> BufferStatsSnapshot {
        BufferStatsSnapshot {
            allocations: self.allocations.load(Ordering::Relaxed),
            zero_copy_reads: self.zero_copy_reads.load(Ordering::Relaxed),
            takes: self.takes.load(Ordering::Relaxed),
            bytes_stored_total: self.bytes_stored_total.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }
}

/// Serializable snapshot of buffer statistics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BufferStatsSnapshot {
    pub allocations: u64,
    pub zero_copy_reads: u64,
    pub takes: u64,
    pub bytes_stored_total: u64,
    pub evictions: u64,
}

/// Error type for buffer operations.
#[derive(Debug, thiserror::Error)]
pub enum BufferError {
    #[error("buffer pool capacity exceeded: need {needed} bytes, limit is {limit}, used {used}")]
    CapacityExceeded {
        needed: usize,
        limit: usize,
        used: usize,
    },
    #[error("slot '{0}' not found")]
    SlotNotFound(String),
}

/// Pool of named shared buffers for zero-copy data passing.
///
/// Mirrors AlloyStack's `BUFFER_REGISTER` / `BUFFER_ALLOCATOR` pattern:
/// - `put()` corresponds to `buffer_alloc()` — store data in a named slot
/// - `take()` corresponds to `access_buffer()` — consume and remove a slot (single-consumer)
/// - `get()` is an extension for multi-consumer reads (Bytes clone is O(1))
pub struct SharedBufferPool {
    buffers: DashMap<String, SharedBuffer>,
    max_total_bytes: usize,
    current_bytes: AtomicUsize,
    stats: BufferStats,
}

impl SharedBufferPool {
    /// Create a new pool with a maximum total byte limit.
    pub fn new(max_total_bytes: usize) -> Self {
        Self {
            buffers: DashMap::new(),
            max_total_bytes,
            current_bytes: AtomicUsize::new(0),
            stats: BufferStats::default(),
        }
    }

    /// Store data in a named slot.
    ///
    /// If the slot already exists, the old data is replaced and its size reclaimed.
    pub fn put(&self, slot: &str, data: Bytes, fingerprint: u64) -> Result<(), BufferError> {
        let new_size = data.len();

        // Reclaim space from existing slot if present.
        if let Some(old) = self.buffers.remove(slot) {
            self.current_bytes
                .fetch_sub(old.1.data.len(), Ordering::Relaxed);
        }

        let current = self.current_bytes.load(Ordering::Relaxed);
        if current + new_size > self.max_total_bytes {
            // Try to evict oldest entries to make room.
            self.evict_until_available(new_size);

            let current = self.current_bytes.load(Ordering::Relaxed);
            if current + new_size > self.max_total_bytes {
                return Err(BufferError::CapacityExceeded {
                    needed: new_size,
                    limit: self.max_total_bytes,
                    used: current,
                });
            }
        }

        self.current_bytes.fetch_add(new_size, Ordering::Relaxed);
        self.stats.allocations.fetch_add(1, Ordering::Relaxed);
        self.stats
            .bytes_stored_total
            .fetch_add(new_size as u64, Ordering::Relaxed);

        self.buffers.insert(
            slot.to_owned(),
            SharedBuffer {
                slot: slot.to_owned(),
                data,
                fingerprint,
                created_at: Instant::now(),
            },
        );

        Ok(())
    }

    /// Read data from a slot without removing it (zero-copy via Bytes refcount).
    ///
    /// This is an extension beyond AlloyStack's single-consumer model.
    pub fn get(&self, slot: &str) -> Option<SharedBuffer> {
        self.buffers.get(slot).map(|entry| {
            self.stats.zero_copy_reads.fetch_add(1, Ordering::Relaxed);
            entry.value().clone()
        })
    }

    /// Take data from a slot, removing it (single-consumer pattern).
    ///
    /// Mirrors AlloyStack's `access_buffer()` which removes the slot.
    pub fn take(&self, slot: &str) -> Option<SharedBuffer> {
        self.buffers.remove(slot).map(|(_, buf)| {
            self.current_bytes
                .fetch_sub(buf.data.len(), Ordering::Relaxed);
            self.stats.takes.fetch_add(1, Ordering::Relaxed);
            buf
        })
    }

    /// Check if a slot exists.
    pub fn contains(&self, slot: &str) -> bool {
        self.buffers.contains_key(slot)
    }

    /// Number of active slots.
    pub fn slot_count(&self) -> usize {
        self.buffers.len()
    }

    /// Current total bytes used.
    pub fn current_usage_bytes(&self) -> usize {
        self.current_bytes.load(Ordering::Relaxed)
    }

    /// Maximum capacity in bytes.
    pub fn max_bytes(&self) -> usize {
        self.max_total_bytes
    }

    /// Get stats snapshot.
    pub fn stats(&self) -> BufferStatsSnapshot {
        self.stats.snapshot()
    }

    /// Clear all slots.
    pub fn clear(&self) {
        self.buffers.clear();
        self.current_bytes.store(0, Ordering::Relaxed);
    }

    /// Evict oldest entries until `needed_bytes` can be accommodated.
    fn evict_until_available(&self, needed_bytes: usize) {
        let mut to_remove = Vec::new();

        // Find oldest entries.
        let mut entries: Vec<_> = self
            .buffers
            .iter()
            .map(|e| (e.key().clone(), e.value().created_at, e.value().data.len()))
            .collect();
        entries.sort_by_key(|(_, created, _)| *created);

        let mut freed = 0usize;
        let current = self.current_bytes.load(Ordering::Relaxed);
        for (key, _, size) in &entries {
            if current - freed + needed_bytes <= self.max_total_bytes {
                break;
            }
            to_remove.push(key.clone());
            freed += size;
        }

        for key in to_remove {
            if let Some((_, buf)) = self.buffers.remove(&key) {
                self.current_bytes
                    .fetch_sub(buf.data.len(), Ordering::Relaxed);
                self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_and_get() {
        let pool = SharedBufferPool::new(1024);
        let data = Bytes::from_static(b"hello world");
        pool.put("slot_a", data.clone(), 42).unwrap();

        let buf = pool.get("slot_a").unwrap();
        assert_eq!(buf.data, data);
        assert_eq!(buf.fingerprint, 42);
        assert_eq!(buf.slot, "slot_a");
        // Slot still exists after get.
        assert!(pool.contains("slot_a"));
    }

    #[test]
    fn test_take_removes_slot() {
        let pool = SharedBufferPool::new(1024);
        pool.put("slot_a", Bytes::from_static(b"data"), 0).unwrap();

        let buf = pool.take("slot_a").unwrap();
        assert_eq!(buf.data.as_ref(), b"data");
        assert!(!pool.contains("slot_a"));
        assert_eq!(pool.slot_count(), 0);
    }

    #[test]
    fn test_capacity_limit() {
        let pool = SharedBufferPool::new(10); // 10 bytes max
        pool.put("a", Bytes::from(vec![0u8; 6]), 0).unwrap();
        pool.put("b", Bytes::from(vec![0u8; 6]), 0).unwrap(); // Evicts "a"

        assert!(pool.contains("b"));
        assert_eq!(pool.current_usage_bytes(), 6);
    }

    #[test]
    fn test_replace_existing_slot() {
        let pool = SharedBufferPool::new(1024);
        pool.put("x", Bytes::from(vec![0u8; 100]), 1).unwrap();
        assert_eq!(pool.current_usage_bytes(), 100);

        pool.put("x", Bytes::from(vec![0u8; 50]), 2).unwrap();
        assert_eq!(pool.current_usage_bytes(), 50);
        assert_eq!(pool.get("x").unwrap().fingerprint, 2);
    }

    #[test]
    fn test_stats() {
        let pool = SharedBufferPool::new(1024);
        pool.put("a", Bytes::from_static(b"hello"), 0).unwrap();
        let _ = pool.get("a");
        let _ = pool.take("a");

        let stats = pool.stats();
        assert_eq!(stats.allocations, 1);
        assert_eq!(stats.zero_copy_reads, 1);
        assert_eq!(stats.takes, 1);
    }
}
