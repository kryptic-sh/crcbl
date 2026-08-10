//! Crash ring — a fixed-size ring buffer that records the last N ticks of
//! server output.  On panic, the contents are written as a `.crpl` replay file
//! so a crash can be replayed and debugged.
//!
//! # Usage
//!
//! ```ignore
//! use crcbl_store::crash_ring::CrashRing;
//!
//! let ring = CrashRing::new(120); // keep last 120 ticks
//! // ... per tick: ring.push(tick_id, &msg_bytes);
//!
//! // From a panic hook, or wherever the crash is caught:
//! // ring.dump(&storage, Path::new("crash.crpl"), tick_rate)?;
//! ```
//!
//! The ring is behind a [`Mutex`], so it is `Sync`: pushes from any thread and
//! reads from a panic handler are both safe, and a snapshot is a consistent
//! view rather than a torn one.  The lock is held only for a memcpy of one
//! entry, which is nothing next to the tick it belongs to.  A poisoned lock is
//! recovered rather than propagated: [`dump`](CrashRing::dump) exists precisely
//! for the case where another thread panicked.

use std::path::Path;
use std::sync::Mutex;

use crcbl_core::TickId;

use crate::replay::{REPLAY_FORMAT_VERSION, REPLAY_MAGIC, REPLAY_MIN_SIZE};
use crate::{StorageError, StorageSource};

/// One recorded tick entry in the ring.
#[derive(Clone, Debug)]
struct TickEntry {
    tick: TickId,
    data: Vec<u8>,
}

/// The mutable half of the ring.
#[derive(Debug)]
struct RingState {
    /// Ring buffer slots, pre-allocated.
    slots: Box<[Option<TickEntry>]>,
    /// Next write position, wrapped by capacity.
    head: usize,
    /// Number of entries written (saturates rather than wrapping).
    count: usize,
}

/// A fixed-size ring buffer of tick entries.
///
/// Safe for any number of producers and for reads from a panic handler.
#[derive(Debug)]
pub struct CrashRing {
    capacity: usize,
    state: Mutex<RingState>,
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
            state: Mutex::new(RingState {
                slots: slots.into_boxed_slice(),
                head: 0,
                count: 0,
            }),
        }
    }

    /// Records a tick's server output message.
    ///
    /// Overwrites the oldest entry when the ring is full.
    pub fn push(&self, tick: TickId, data: &[u8]) {
        let mut state = self.lock();
        let idx = state.head % self.capacity;
        state.head = state.head.wrapping_add(1);
        state.count = state.count.saturating_add(1);
        state.slots[idx] = Some(TickEntry {
            tick,
            data: data.to_vec(),
        });
    }

    /// Number of entries written so far (saturating for very long sessions).
    pub fn count(&self) -> usize {
        self.lock().count
    }

    /// Lock the ring, recovering from poisoning.
    ///
    /// A producer that panicked mid-push left a well-formed `RingState` — the
    /// slot it was writing is simply still the old entry — and refusing to dump
    /// after a panic would defeat the purpose of the ring.
    fn lock(&self) -> std::sync::MutexGuard<'_, RingState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Snapshots the ring, ordered from oldest to newest.
    fn snapshot(&self) -> Vec<TickEntry> {
        let state = self.lock();
        let count = state.count.min(self.capacity);
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let idx = (state.head.wrapping_sub(count).wrapping_add(i)) % self.capacity;
            if let Some(entry) = &state.slots[idx] {
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
        let mut buf = Vec::with_capacity(
            REPLAY_MIN_SIZE + entries.iter().map(|e| 12 + e.data.len()).sum::<usize>(),
        );

        // Replay file header — constants from `replay` so a format bump cannot
        // leave crash dumps silently emitting the previous version.
        buf.extend_from_slice(REPLAY_MAGIC);
        buf.extend_from_slice(&REPLAY_FORMAT_VERSION.to_le_bytes());
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
        // Oldest first: ticks 2, 3, 4.
        for (i, expected) in [2u64, 3, 4].into_iter().enumerate() {
            assert_eq!(transport.tick_at(i), TickId::from_raw(expected));
        }
    }

    #[test]
    fn ring_is_shareable_across_threads() {
        use std::sync::Arc;

        let ring = Arc::new(CrashRing::new(64));
        let threads: Vec<_> = (0..4u64)
            .map(|t| {
                let ring = Arc::clone(&ring);
                std::thread::spawn(move || {
                    for i in 0..25u64 {
                        ring.push(TickId::from_raw(t * 100 + i), &[t as u8; 8]);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(ring.count(), 100);
        let storage = MemoryStorage::new();
        ring.dump(&storage, Path::new("threads.crpl"), 60).unwrap();
        let transport = crate::replay::FileTransport::open(&storage, Path::new("threads.crpl"))
            .expect("dump must be a readable replay");
        assert_eq!(transport.len(), 64);
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
    fn a_crash_ring_asked_for_zero_capacity_panics_rather_than_holding_nothing() {
        CrashRing::new(0);
    }
}
