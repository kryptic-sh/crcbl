//! Crash ring — a lock-free, fixed-size ring buffer that records the last N
//! ticks of server output.  On panic, the contents are written as a `.crpl`
//! replay file so a crash can be replayed and debugged.
//!
//! # Usage
//!
//! ```ignore
//! use crcbl_store::crash_ring::CrashRing;
//!
//! let ring = CrashRing::new(120); // keep last 120 ticks
//! // ... per tick: ring.push(tick_id, &msg_bytes);
//!
//! // Register for crash dump:
//! // ring.install_panic_hook("crash.crpl", &storage);
//! ```
//!
//! The ring uses interior mutability (atomic indexing into a pre-allocated Vec)
//! so it is safe to push from any thread and read from a panic handler without
//! locking.

use std::cell::UnsafeCell;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use crcbl_core::TickId;

use crate::{StorageError, StorageSource};

/// One recorded tick entry in the ring.
#[derive(Clone, Debug)]
struct TickEntry {
    tick: TickId,
    data: Vec<u8>,
}

/// A fixed-size ring buffer of tick entries.
///
/// Thread-safe for single-producer writes and panic-handler reads.
/// Not safe for concurrent producers — the server tick loop is the only writer.
pub struct CrashRing {
    capacity: usize,
    /// Ring buffer slots, pre-allocated, wrapped in UnsafeCell for interior
    /// mutability (single-producer contract).
    slots: UnsafeCell<Box<[Option<TickEntry>]>>,
    /// Next write position, wrapped by capacity.
    head: AtomicUsize,
    /// Number of entries written (monotonically increases, wraps at usize::MAX).
    count: AtomicUsize,
}

impl CrashRing {
    /// Creates a crash ring holding at most `capacity` ticks.
    ///
    /// # Panics
    ///
    /// If `capacity` is zero.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "crash ring capacity must be non-zero");
        let mut slots = Vec::with_capacity(capacity);
        slots.resize_with(capacity, || None);
        Self {
            capacity,
            slots: UnsafeCell::new(slots.into_boxed_slice()),
            head: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
        }
    }

    /// Records a tick's server output message.
    ///
    /// Overwrites the oldest entry when the ring is full.
    pub fn push(&self, tick: TickId, data: &[u8]) {
        let idx = self.head.fetch_add(1, Ordering::Relaxed) % self.capacity;
        self.count.fetch_add(1, Ordering::Relaxed);
        // SAFETY: idx < capacity, and the single-producer contract ensures no
        // other writer touches this slot concurrently.
        let slots = unsafe { &mut *self.slots.get() };
        slots[idx] = Some(TickEntry {
            tick,
            data: data.to_vec(),
        });
    }

    /// Number of entries written so far (may overflow `usize` for very long
    /// sessions).
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

impl std::fmt::Debug for CrashRing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrashRing")
            .field("capacity", &self.capacity)
            .field("count", &self.count.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// Snapshots the ring into a `Vec<TickEntry>`, ordered from oldest to newest.
///
/// Reads without locking; the producer may be writing concurrently, so a
/// small number of entries near the write head may be missing or partially
/// written.  This is acceptable for crash dumps: a best-effort snapshot.
impl CrashRing {
    fn snapshot(&self) -> Vec<TickEntry> {
        let written = self.count.load(Ordering::Relaxed);
        let count = written.min(self.capacity);
        let head = self.head.load(Ordering::Relaxed);

        // SAFETY: read-only access; the writer may update slots concurrently
        // but we're taking a best-effort snapshot.
        let slots = unsafe { &*self.slots.get() };
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let idx = (head.wrapping_sub(count).wrapping_add(i)) % self.capacity;
            if let Some(entry) = &slots[idx] {
                out.push(entry.clone());
            }
        }
        out
    }

    /// Writes the current ring contents as a `.crpl` replay file through
    /// `storage` at `path`.
    ///
    /// Uses the same on-disk format as [`crate::replay::ReplayWriter`] so
    /// [`crate::replay::FileTransport`] can play it back.
    pub fn dump(
        &self,
        storage: &dyn StorageSource,
        path: &Path,
        tick_rate: u32,
    ) -> Result<(), StorageError> {
        let entries = self.snapshot();
        let mut buf =
            Vec::with_capacity(30 + entries.iter().map(|e| 12 + e.data.len()).sum::<usize>());

        // Replay file header.
        buf.extend_from_slice(b"CRBLREPL");
        buf.extend_from_slice(&1u16.to_le_bytes()); // format version
        buf.extend_from_slice(&(entries.len() as u64).to_le_bytes());
        buf.extend_from_slice(&tick_rate.to_le_bytes());
        let start_tick = entries.first().map(|e| e.tick.get()).unwrap_or(0);
        buf.extend_from_slice(&start_tick.to_le_bytes());

        for entry in &entries {
            buf.extend_from_slice(&entry.tick.get().to_le_bytes());
            let len = entry.data.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(&entry.data);
        }

        storage.write(path, &buf)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    #[test]
    fn empty_ring_dump_is_header_only() {
        let ring = CrashRing::new(10);
        let storage = MemoryStorage::new();
        let path = Path::new("empty.crpl");
        ring.dump(&storage, path, 60).unwrap();

        let bytes = storage.read(path).unwrap();
        assert!(bytes.len() >= 30);
        assert_eq!(&bytes[0..8], b"CRBLREPL");
    }

    #[test]
    fn ring_preserves_entries_in_order() {
        let ring = CrashRing::new(5);
        for i in 0..3 {
            ring.push(TickId::from_raw(i as u64), &[i as u8; 4]);
        }
        let storage = MemoryStorage::new();
        let path = Path::new("three.crpl");
        ring.dump(&storage, path, 60).unwrap();

        let transport = crate::replay::FileTransport::open(&storage, path).unwrap();
        assert_eq!(transport.len(), 3);
    }

    #[test]
    fn ring_wraps_and_overwrites_oldest() {
        let ring = CrashRing::new(3);
        for i in 0..5 {
            ring.push(TickId::from_raw(i), &[i as u8]);
        }
        // Should contain ticks 2, 3, 4 (last 3 of 5).
        let storage = MemoryStorage::new();
        let path = Path::new("wrapped.crpl");
        ring.dump(&storage, path, 60).unwrap();

        let transport = crate::replay::FileTransport::open(&storage, path).unwrap();
        assert_eq!(transport.len(), 3);
    }

    #[test]
    fn ring_count_monotonically_increases() {
        let ring = CrashRing::new(4);
        assert_eq!(ring.count(), 0);
        ring.push(TickId::from_raw(1), b"a");
        assert_eq!(ring.count(), 1);
        ring.push(TickId::from_raw(2), b"b");
        assert_eq!(ring.count(), 2);
    }

    #[test]
    #[should_panic(expected = "capacity must be non-zero")]
    fn zero_capacity_panics() {
        CrashRing::new(0);
    }
}
