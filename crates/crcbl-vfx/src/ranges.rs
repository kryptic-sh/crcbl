//! The pool allocator: contiguous slot ranges, handed out per effect.
//!
//! `docs/plan/20-particles.md` describes one "global particle pool (SSBO,
//! structure-of-arrays …) + per-effect allocation ranges". This is the half
//! that decides *which* slots an effect owns. [`ParticlePool`] is the storage
//! those slots index into.
//!
//! [`ParticlePool`]: crate::ParticlePool
//!
//! # Why contiguous, and why that is not a compromise
//!
//! An effect's particles have to reach the GPU as one draw's worth of
//! consecutive records, so a range is the unit the whole design is built on:
//! one buffer offset and one count per effect, and no indirection table. It is
//! also what makes the CPU step here foldable into a compute dispatch later —
//! a workgroup covers `[start, start + len)` and needs no per-particle owner
//! lookup to know whose parameters to apply.
//!
//! The price is external fragmentation, which this slice pays rather than
//! solves: ranges are fixed for an effect's whole life and freed whole.
//! `docs/plan/20-particles.md`'s "freelist compaction in compute" is where that
//! is answered, and compaction is a GPU pass, not something to prototype here.

/// A half-open run of pool slots, `[start, start + len)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRange {
    /// The first slot.
    pub start: u32,
    /// How many slots follow it.
    pub len: u32,
}

impl SlotRange {
    /// One past the last slot.
    pub fn end(self) -> u32 {
        self.start + self.len
    }

    /// The slots as a `usize` range, which is what indexes the pool's arrays.
    pub fn indices(self) -> std::ops::Range<usize> {
        self.start as usize..self.end() as usize
    }
}

/// First-fit allocation of [`SlotRange`]s out of a fixed pool, with a
/// coalescing free list.
///
/// # The clamp is the point
///
/// [`alloc_clamped`](Self::alloc_clamped) never fails while a single slot is
/// free: an effect that asks for more than is left gets the largest run there
/// is, and the caller records the shortfall. That is
/// `docs/plan/20-particles.md`'s budget rule read literally — "an effect cannot
/// detonate the frame budget; the pool allocator clamps and the profiler shows
/// who asked for what" — and it is the behaviour a hostile effect meets. An
/// allocator that refused instead would turn a budget overrun into a missing
/// effect, which is the worse failure: it is invisible until someone looks for
/// something that was never there.
///
/// # Invariants of the free list
///
/// The spans are sorted by `start`, non-empty, non-overlapping and
/// non-adjacent — a freed range that touches its neighbours merges with them,
/// so a pool that has been emptied is one span again rather than a hundred.
/// `tests/pool.rs` holds those four to a random churn of allocations and
/// frees, which is `docs/plan/20-particles.md`'s "pool allocator property tests
/// (churn, clamp, freelist integrity)".
#[derive(Clone, Debug)]
pub struct RangeAllocator {
    capacity: u32,
    free: Vec<SlotRange>,
}

impl RangeAllocator {
    /// An allocator over `capacity` slots, all of them free.
    pub fn new(capacity: u32) -> Self {
        let free = if capacity == 0 {
            Vec::new()
        } else {
            vec![SlotRange {
                start: 0,
                len: capacity,
            }]
        };
        Self { capacity, free }
    }

    /// How many slots the pool has in total.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// How many slots are not owned by any range.
    pub fn free_slots(&self) -> u32 {
        self.free.iter().map(|span| span.len).sum()
    }

    /// The free list, for a test or a panel that wants to see fragmentation.
    pub fn spans(&self) -> &[SlotRange] {
        &self.free
    }

    /// `len` slots if a run that long is free, otherwise the longest run there
    /// is, or `None` when the pool is full.
    ///
    /// First fit rather than best fit: an effect's range is its whole life, so
    /// the churn this sees is far coarser than a general allocator's, and best
    /// fit would buy a marginally tighter packing for a scan of the whole list
    /// on every spawn.
    pub fn alloc_clamped(&mut self, len: u32) -> Option<SlotRange> {
        if len == 0 {
            return None;
        }
        let index = match self.free.iter().position(|span| span.len >= len) {
            Some(index) => index,
            // Nothing holds the whole request, so take the largest run whole.
            // `max_by_key` returns the *last* maximum; the first keeps
            // allocation walking forwards through the pool, which is what makes
            // a fresh pool hand out ranges in order.
            None => self
                .free
                .iter()
                .enumerate()
                .max_by_key(|(_, span)| span.len)
                .map(|(index, _)| index)?,
        };
        let span = self.free[index];
        let taken = SlotRange {
            start: span.start,
            len: len.min(span.len),
        };
        if taken.len == span.len {
            self.free.remove(index);
        } else {
            self.free[index] = SlotRange {
                start: taken.end(),
                len: span.len - taken.len,
            };
        }
        Some(taken)
    }

    /// Return a range to the pool, merging it with any run it now touches.
    ///
    /// # Panics
    ///
    /// If the range is not inside the pool, or overlaps a run that is already
    /// free. Both mean a range was freed twice or was never this allocator's,
    /// and a silent merge would corrupt every later allocation instead.
    pub fn free(&mut self, range: SlotRange) {
        if range.len == 0 {
            return;
        }
        assert!(
            range.end() <= self.capacity,
            "range {range:?} is outside a pool of {} slots",
            self.capacity
        );
        let at = self.free.partition_point(|span| span.start < range.start);
        if let Some(before) = at.checked_sub(1).and_then(|i| self.free.get(i)) {
            assert!(
                before.end() <= range.start,
                "range {range:?} overlaps free span {before:?}"
            );
        }
        if let Some(after) = self.free.get(at) {
            assert!(
                range.end() <= after.start,
                "range {range:?} overlaps free span {after:?}"
            );
        }
        self.free.insert(at, range);
        // Merge forwards first so the backwards merge sees the widened span.
        if at + 1 < self.free.len() && self.free[at].end() == self.free[at + 1].start {
            self.free[at].len += self.free[at + 1].len;
            self.free.remove(at + 1);
        }
        if at > 0 && self.free[at - 1].end() == self.free[at].start {
            self.free[at - 1].len += self.free[at].len;
            self.free.remove(at);
        }
    }
}
