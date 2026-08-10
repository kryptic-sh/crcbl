//! Frame-scoped bump allocation.
//!
//! A [`FrameArena`] is a fixed block of bytes with a bump pointer. Per-frame
//! transient data — visible-instance lists, draw arguments, scratch index
//! buffers — is carved out of it and thrown away wholesale by [`FrameArena::reset`]
//! at the frame boundary. No per-allocation bookkeeping, no free, no
//! fragmentation.

use core::cell::Cell;
use core::mem;
use core::ptr::NonNull;
use core::slice;

/// Alignment of the arena's backing block, and the largest alignment it can
/// satisfy. 64 bytes covers a cache line and every SIMD type the engine is
/// likely to bump-allocate (including AVX-512 vectors).
const BLOCK_ALIGN: usize = 64;

/// One unit of backing storage. `Vec<u8>` would only guarantee 1-byte
/// alignment for the block itself, which no amount of offset alignment can
/// repair, so the block is a `Vec` of over-aligned chunks instead.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct Block([u8; BLOCK_ALIGN]);

/// Frame budget observability — see [`FrameArena::stats`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ArenaStats {
    /// Bytes handed out since the last [`FrameArena::reset`].
    pub used: usize,
    /// Total bytes the arena can hand out.
    pub capacity: usize,
    /// Largest `used` seen since the arena was created (or since
    /// [`FrameArena::reset_peak`]). This is the number that tells you whether
    /// the frame budget is right.
    pub peak: usize,
}

impl ArenaStats {
    /// Fraction of capacity consumed at the high-water mark, in `0.0..=1.0`.
    #[must_use]
    pub fn peak_fraction(&self) -> f32 {
        if self.capacity == 0 {
            0.0
        } else {
            self.peak as f32 / self.capacity as f32
        }
    }
}

/// A fixed-capacity bump allocator for per-frame data.
///
/// # Growth policy
///
/// The arena **never grows**. Exhaustion is a hard error: the allocating
/// methods panic and the `try_` variants return `None`. Two reasons:
///
/// * A frame arena that grows on demand cannot be distinguished from a leak.
///   The whole point is that per-frame allocation is *bounded*; if the bound is
///   soft, the first system that forgets to clamp a list quietly turns into a
///   memory-growth bug that only shows up as a stall days later.
/// * Growth would invalidate outstanding slices, so a growing arena needs
///   either chunk chaining or `unsafe` aliasing games. Neither is worth it at
///   this layer.
///
/// The peak in [`FrameArena::stats`] is the tool for sizing: run the game, read
/// the high-water mark, set the capacity with headroom. Systems that can
/// legitimately degrade (a debug overlay, a particle burst) should use
/// [`FrameArena::try_alloc_slice`] and drop work when the budget is gone rather
/// than take the process down.
///
/// # Borrowing
///
/// Allocation takes `&self` and returns `&mut [T]` borrowed from the arena, so
/// **any number of allocations can be live at once** — which is the actual
/// usage: a frame system builds a visible-instance list *and* a draw-argument
/// list *and* an index list, and holds all three until the encoder consumes
/// them. [`FrameArena::reset`] takes `&mut self`, so the borrow checker is what
/// proves no slice outlives its frame; the frame boundary needs no discipline
/// beyond calling `reset`.
///
/// ```
/// use crcbl_core::alloc::FrameArena;
///
/// let mut arena = FrameArena::with_capacity(4096);
/// let instances = arena.alloc_slice(3, [0.0f32; 4]);
/// let draw_args = arena.alloc_slice(2, 0u32);
/// instances[0][1] = 1.0;
/// draw_args[1] = 7;
/// // Both live at once; `reset` cannot be called until both are dropped.
/// assert_eq!(instances[0][1] + draw_args[1] as f32, 8.0);
/// arena.reset();
/// ```
///
/// # Aliasing
///
/// This is the `bumpalo` model, and handing `&mut` out of `&self` rests on
/// three facts:
///
/// 1. **Regions are disjoint by construction.** The bump offset only ever
///    advances between resets, so no two allocations can return overlapping
///    byte ranges. Every returned slice is the unique reference to its bytes.
/// 2. **The block never moves and never grows.** Capacity is fixed at
///    construction and the backing `Vec` is *never reborrowed afterwards* —
///    every allocation derives from `base`, a pointer taken once in
///    `with_capacity`. (Calling `Vec::as_mut_ptr` again would create a fresh
///    unique reference to the whole block and invalidate every slice already
///    handed out.)
/// 3. **`reset` and `reset_peak` take `&mut self`.** An exclusive borrow of
///    the arena cannot coexist with any slice borrowed from it, so memory can
///    only be recycled once every reference to it is gone. `T: Copy` also means
///    no allocation has drop glue to skip.
///
/// The arena is neither `Send` nor `Sync`: allocation through `&self` is
/// unsynchronized, and the `Cell` bump offset makes that a compile error rather
/// than a data race. A job system wants one arena per worker anyway.
pub struct FrameArena {
    /// Owns the backing allocation and nothing else. Deliberately never read
    /// after `with_capacity` — see fact 2 above.
    _storage: Vec<Block>,
    /// Start of the block. Every allocation derives from this pointer.
    base: NonNull<u8>,
    capacity: usize,
    /// Bump offset in bytes from `base`.
    offset: Cell<usize>,
    peak: Cell<usize>,
}

// `clippy::mut_from_ref` is exactly what this type does on purpose: allocation
// takes `&self` and returns `&mut [T]`. The soundness argument is the Aliasing
// section on `FrameArena` — disjoint regions, an immovable block, and `reset`
// gated behind `&mut self`. `bumpalo` allows the same lint for the same reason.
#[allow(clippy::mut_from_ref)]
impl FrameArena {
    /// Creates an arena with at least `capacity` bytes, rounded up to a
    /// multiple of the 64-byte block size.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let blocks = capacity.div_ceil(BLOCK_ALIGN);
        let mut storage = vec![Block([0; BLOCK_ALIGN]); blocks];
        // The one and only reborrow of the block. Its provenance covers the
        // whole allocation, and every later allocation is derived from it.
        let base =
            NonNull::new(storage.as_mut_ptr().cast::<u8>()).expect("Vec::as_mut_ptr is never null");
        Self {
            _storage: storage,
            base,
            capacity: blocks * BLOCK_ALIGN,
            offset: Cell::new(0),
            peak: Cell::new(0),
        }
    }

    /// Bytes handed out since the last [`FrameArena::reset`].
    #[inline]
    #[must_use]
    pub fn used(&self) -> usize {
        self.offset.get()
    }

    /// Total bytes available per frame.
    #[inline]
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Bytes still available this frame (before alignment padding).
    #[inline]
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.capacity - self.offset.get()
    }

    /// Used / capacity / peak.
    #[inline]
    #[must_use]
    pub fn stats(&self) -> ArenaStats {
        ArenaStats {
            used: self.offset.get(),
            capacity: self.capacity,
            peak: self.peak.get(),
        }
    }

    /// Frees everything allocated this frame. O(1); the memory is not touched,
    /// so the next frame reads whatever the last one wrote until it
    /// initializes its own data.
    ///
    /// Takes `&mut self`: this is the frame boundary, and the exclusive borrow
    /// is what proves every slice from the last frame is already gone.
    ///
    /// The high-water mark survives — that is the point of tracking it.
    #[inline]
    pub fn reset(&mut self) {
        self.offset.set(0);
    }

    /// Forgets the high-water mark (e.g. when entering a new level and the old
    /// peak stops being informative).
    #[inline]
    pub fn reset_peak(&mut self) {
        self.peak.set(self.offset.get());
    }

    /// Allocates `len` copies of `value`.
    ///
    /// # Panics
    ///
    /// If the arena is exhausted — see the growth policy in the type docs — or
    /// if `T` needs more than 64-byte alignment.
    #[inline]
    pub fn alloc_slice<T: Copy>(&self, len: usize, value: T) -> &mut [T] {
        let (capacity, free) = (self.capacity, self.remaining());
        match self.try_alloc_slice(len, value) {
            Some(slice) => slice,
            None => exhausted::<T>(len, free, capacity),
        }
    }

    /// Allocates `len` copies of `value`, or `None` if the arena is exhausted.
    ///
    /// # Panics
    ///
    /// If `T` needs more than 64-byte alignment — that is a programming error,
    /// not a budget overrun, so it is not reported through the `Option`.
    pub fn try_alloc_slice<T: Copy>(&self, len: usize, value: T) -> Option<&mut [T]> {
        let ptr = self.reserve::<T>(len)?;
        // SAFETY: `reserve` guarantees `len` correctly aligned, exclusively
        // owned, writable elements at `ptr`. Every one of them is initialized
        // here, before the slice exists, so no reference to uninitialized
        // memory is ever created. `T: Copy` means there is nothing to drop.
        unsafe {
            for index in 0..len {
                ptr.add(index).write(value);
            }
            Some(slice::from_raw_parts_mut(ptr, len))
        }
    }

    /// Copies `src` into the arena.
    ///
    /// # Panics
    ///
    /// As [`FrameArena::alloc_slice`].
    #[inline]
    pub fn alloc_slice_copy<T: Copy>(&self, src: &[T]) -> &mut [T] {
        let (capacity, free) = (self.capacity, self.remaining());
        match self.try_alloc_slice_copy(src) {
            Some(slice) => slice,
            None => exhausted::<T>(src.len(), free, capacity),
        }
    }

    /// Copies `src` into the arena, or returns `None` if it is exhausted.
    ///
    /// # Panics
    ///
    /// As [`FrameArena::try_alloc_slice`].
    pub fn try_alloc_slice_copy<T: Copy>(&self, src: &[T]) -> Option<&mut [T]> {
        let ptr = self.reserve::<T>(src.len())?;
        // SAFETY: as `try_alloc_slice`, with a memcpy doing the initializing.
        // The destination cannot overlap `src`: `reserve` only ever returns a
        // range no live reference already covers, and `src` is live here.
        unsafe {
            ptr.copy_from_nonoverlapping(src.as_ptr(), src.len());
            Some(slice::from_raw_parts_mut(ptr, src.len()))
        }
    }

    /// Allocates a single value.
    ///
    /// # Panics
    ///
    /// As [`FrameArena::alloc_slice`].
    #[inline]
    pub fn alloc<T: Copy>(&self, value: T) -> &mut T {
        &mut self.alloc_slice(1, value)[0]
    }

    /// Bumps the offset and returns a pointer to `len` **uninitialized**
    /// elements, or `None` if that does not fit.
    ///
    /// Deliberately returns a raw pointer rather than `&mut [T]`: the memory is
    /// uninitialized at this point, and a reference to uninitialized memory is
    /// not something to hand around even privately.
    fn reserve<T: Copy>(&self, len: usize) -> Option<*mut T> {
        let align = mem::align_of::<T>();
        assert!(
            align <= BLOCK_ALIGN,
            "FrameArena supports alignments up to {BLOCK_ALIGN}, {} needs {align}",
            core::any::type_name::<T>(),
        );
        let size = mem::size_of::<T>().checked_mul(len)?;
        let start = self.offset.get().next_multiple_of(align);
        let end = start.checked_add(size)?;
        if end > self.capacity {
            return None;
        }
        self.offset.set(end);
        self.peak.set(self.peak.get().max(end));

        if size == 0 {
            // Zero-sized (or empty) allocations never touch the block; a
            // dangling-but-aligned pointer is the correct answer for them.
            return Some(NonNull::<T>::dangling().as_ptr());
        }

        // SAFETY: `start .. start + size` lies inside the block — `end` was
        // checked against `capacity`, which is exactly the block's byte length,
        // and `base`'s provenance covers the whole block. `start` is a multiple
        // of `align_of::<T>()` and the block's base is 64-byte aligned (`Block`
        // is `repr(align(64))`, and `BLOCK_ALIGN` is asserted above to be the
        // maximum supported alignment), so the result is aligned for `T`.
        //
        // The region is unaliased even though several allocations can be live
        // at once: the bump offset only advances between resets, so this range
        // has never been handed out before, and it cannot be handed out again
        // until `reset` — which needs `&mut self`, i.e. proof that every slice
        // derived from this arena is already gone. Deriving from `base` rather
        // than from `_storage` is what keeps the earlier slices valid; see the
        // Aliasing section on `FrameArena`.
        Some(unsafe { self.base.as_ptr().add(start).cast::<T>() })
    }
}

/// Shared panic path for the infallible allocators — cold, and out of line so
/// the happy path stays small.
#[cold]
#[inline(never)]
fn exhausted<T>(len: usize, free: usize, capacity: usize) -> ! {
    panic!(
        "FrameArena exhausted: wanted {} bytes for [{}; {len}], {free} of {capacity} free",
        mem::size_of::<T>().saturating_mul(len),
        core::any::type_name::<T>(),
    )
}

impl core::fmt::Debug for FrameArena {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrameArena")
            .field("used", &self.offset.get())
            .field("capacity", &self.capacity)
            .field("peak", &self.peak.get())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_rounds_up_to_a_whole_block() {
        let arena = FrameArena::with_capacity(1);
        assert_eq!(arena.capacity(), BLOCK_ALIGN);
        let arena = FrameArena::with_capacity(BLOCK_ALIGN + 1);
        assert_eq!(arena.capacity(), BLOCK_ALIGN * 2);
    }

    #[test]
    fn allocations_are_aligned_for_their_type() {
        #[derive(Clone, Copy, Debug, PartialEq)]
        #[repr(align(32))]
        struct Wide(u64);

        let arena = FrameArena::with_capacity(4096);
        // Deliberately misalign the bump pointer first.
        let bytes = arena.alloc_slice(3, 0u8);
        assert_eq!(bytes.len(), 3);
        let wide = arena.alloc_slice(2, Wide(7));
        assert_eq!(wide.as_ptr() as usize % 32, 0);
        assert_eq!(wide, [Wide(7), Wide(7)]);

        let floats = arena.alloc_slice(5, 1.5f64);
        assert_eq!(floats.as_ptr() as usize % mem::align_of::<f64>(), 0);
        assert!(floats.iter().all(|f| *f == 1.5));
    }

    #[test]
    fn allocations_do_not_overlap() {
        let arena = FrameArena::with_capacity(4096);
        let first = arena.alloc_slice(8, 1u32).as_ptr() as usize;
        let second = arena.alloc_slice(8, 2u32);
        assert!(second.iter().all(|v| *v == 2));
        assert!(second.as_ptr() as usize >= first + 8 * 4);
    }

    /// The case the `&mut self` API could not express: several allocations
    /// live at once, written through independently, none corrupting another.
    /// This is the real frame shape — instance list, draw arguments, indices —
    /// so it is pinned rather than left to Miri to notice by accident.
    #[test]
    fn many_slices_stay_live_and_independent() {
        let mut arena = FrameArena::with_capacity(4096);

        let instances = arena.alloc_slice(4, [0.0f32; 4]);
        let draw_args = arena.alloc_slice(3, 0u32);
        let indices = arena.alloc_slice_copy(&[10u16, 11, 12, 13, 14]);
        let flag = arena.alloc(false);
        // A differently-aligned type wedged between them, to make sure the
        // padding does not let two allocations share a byte.
        let wide = arena.alloc_slice(2, 0u64);

        // Interleave the writes, so a stale or overlapping pointer corrupts a
        // neighbour rather than merely being overwritten in order.
        for round in 0..8u32 {
            for (index, instance) in instances.iter_mut().enumerate() {
                instance[index % 4] = (round * 10 + index as u32) as f32;
            }
            for (index, arg) in draw_args.iter_mut().enumerate() {
                *arg = round * 100 + index as u32;
            }
            for (index, wide) in wide.iter_mut().enumerate() {
                *wide = u64::from(round) << 32 | index as u64;
            }
            *flag = round.is_multiple_of(2);
        }

        assert_eq!(instances[0][0], 70.0);
        assert_eq!(instances[3][3], 73.0);
        assert_eq!(draw_args, [700, 701, 702]);
        assert_eq!(
            indices,
            [10, 11, 12, 13, 14],
            "untouched slice must be intact"
        );
        assert_eq!(wide, [7u64 << 32, 7u64 << 32 | 1]);
        assert!(!*flag);

        // Every region is disjoint from every other.
        let spans = [
            (instances.as_ptr() as usize, size_of_val(&*instances)),
            (draw_args.as_ptr() as usize, size_of_val(&*draw_args)),
            (indices.as_ptr() as usize, size_of_val(&*indices)),
            (wide.as_ptr() as usize, size_of_val(&*wide)),
            (std::ptr::from_ref(&*flag) as usize, size_of_val(&*flag)),
        ];
        for (i, (start, len)) in spans.iter().enumerate() {
            for (other_start, other_len) in spans.iter().skip(i + 1) {
                assert!(
                    start + len <= *other_start || other_start + other_len <= *start,
                    "allocations overlap: {start}+{len} vs {other_start}+{other_len}"
                );
            }
        }

        // Every borrow above ends at its last use, so `reset` — which needs
        // `&mut self` — compiles here and nowhere earlier. The borrow checker
        // is the frame boundary.
        arena.reset();
        assert_eq!(arena.used(), 0);
    }

    /// Allocation needs no `mut` binding at all: `&FrameArena` is enough, which
    /// is what lets a frame arena be shared by reference across systems.
    #[test]
    fn allocation_works_through_a_shared_reference() {
        let arena = FrameArena::with_capacity(256);
        fn fill(arena: &FrameArena, value: u8) -> &mut [u8] {
            arena.alloc_slice(8, value)
        }
        let a = fill(&arena, 1);
        let b = fill(&arena, 2);
        a[0] = 9;
        assert_eq!(a, [9, 1, 1, 1, 1, 1, 1, 1]);
        assert_eq!(b, [2; 8]);
        assert_eq!(arena.used(), 16);
    }

    #[test]
    fn slices_can_be_copied_in() {
        let arena = FrameArena::with_capacity(256);
        let src = [1.0f32, 2.0, 3.0, 4.0];
        let copied = arena.alloc_slice_copy(&src);
        assert_eq!(copied, src);
        copied[0] = 9.0;
        assert_eq!(src[0], 1.0, "the arena must hold a copy, not a view");
        assert_eq!(arena.used(), 16);
        assert!(arena.try_alloc_slice_copy(&[0u8; 512]).is_none());
    }

    #[test]
    fn an_arena_allocation_hands_back_storage_the_caller_can_write_through() {
        let arena = FrameArena::with_capacity(256);
        let value = arena.alloc([1u16, 2, 3]);
        value[1] = 9;
        assert_eq!(*value, [1, 9, 3]);
    }

    #[test]
    fn zero_sized_and_empty_allocations_cost_nothing() {
        let arena = FrameArena::with_capacity(128);
        let empty = arena.alloc_slice(0, 1u64);
        assert!(empty.is_empty());
        let units = arena.alloc_slice(1000, ());
        assert_eq!(units.len(), 1000);
        assert_eq!(arena.used(), 0);
    }

    #[test]
    fn reset_reuses_the_block_and_keeps_the_peak() {
        let mut arena = FrameArena::with_capacity(1024);
        let first = arena.alloc_slice(64, 0u8).as_ptr() as usize;
        assert_eq!(arena.used(), 64);
        arena.reset();
        assert_eq!(arena.used(), 0);
        assert_eq!(arena.stats().peak, 64);

        let second = arena.alloc_slice(16, 0u8).as_ptr() as usize;
        assert_eq!(second, first, "reset must hand back the same memory");
        assert_eq!(
            arena.stats(),
            ArenaStats {
                used: 16,
                capacity: 1024,
                peak: 64
            }
        );

        arena.reset_peak();
        assert_eq!(arena.stats().peak, 16);
    }

    #[test]
    fn peak_tracks_the_worst_frame() {
        let mut arena = FrameArena::with_capacity(1024);
        for len in [10usize, 700, 3, 200] {
            arena.reset();
            let _ = arena.alloc_slice(len, 0u8);
        }
        assert_eq!(arena.stats().peak, 700);
        assert!((arena.stats().peak_fraction() - 700.0 / 1024.0).abs() < 1e-6);
    }

    #[test]
    fn exhaustion_is_reported_not_grown_into() {
        let arena = FrameArena::with_capacity(128);
        assert!(arena.try_alloc_slice(100, 0u8).is_some());
        assert_eq!(arena.remaining(), 28);
        assert!(arena.try_alloc_slice(29, 0u8).is_none());
        assert_eq!(arena.capacity(), 128, "capacity must not have grown");
        // The failed attempt must not have moved the bump pointer.
        assert_eq!(arena.used(), 100);
        assert!(arena.try_alloc_slice(28, 0u8).is_some());
    }

    #[test]
    #[should_panic(expected = "FrameArena exhausted")]
    fn alloc_panics_when_exhausted() {
        let arena = FrameArena::with_capacity(64);
        let _ = arena.alloc_slice(65, 0u8);
    }

    #[test]
    fn a_fresh_arena_reports_default_stats_and_a_zero_peak_fraction() {
        let arena = FrameArena::with_capacity(0);
        assert_eq!(arena.stats(), ArenaStats::default());
        assert_eq!(arena.stats().peak_fraction(), 0.0);
        assert!(format!("{arena:?}").contains("FrameArena"));
    }
}
