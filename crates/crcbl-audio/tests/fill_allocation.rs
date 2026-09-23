//! The audio thread's path, `Mixer::fill`, allocates nothing — including
//! under a voice budget that is stealing.
//!
//! # Why it is a separate target
//!
//! It installs a counting global allocator, and a global allocator is one per
//! binary. As its own test target it counts this file's allocations and no
//! other test's.
//!
//! What it counts is **allocation** — `alloc`, `alloc_zeroed` and `realloc` —
//! on the thread that asks. Frees are not counted: `fill` drops the voices
//! that finish or end their release block, and that free on the audio thread
//! predates the budget.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use crcbl_audio::AudioSource;
use crcbl_audio::CHANNELS;
use crcbl_audio::mixer::{Mixer, PlayOutcome, Voice};

/// Forwards to [`System`], counting the allocations made on a thread that has
/// switched counting on.
struct Counting;

thread_local! {
    /// Whether this thread's allocations are being counted.
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    /// How many allocations this thread made while counting.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn note_allocation() {
    // `try_with`: a thread being torn down has no locals left, and it is not
    // one this file measures.
    let _ = COUNTING.try_with(|counting| {
        if counting.get() {
            ALLOCATIONS.with(|n| n.set(n.get() + 1));
        }
    });
}

// SAFETY: every method forwards to `System` with the arguments it was given,
// so the allocator contract is `System`'s; the counting touches only a
// `const`-initialised thread-local `Cell`, which never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        // SAFETY: as for `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_allocation();
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// How many allocations `f` made on this thread.
fn allocations_in(f: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    ALLOCATIONS.with(Cell::get)
}

/// The counter is wired to something: a `Vec` with capacity is one
/// allocation. Without this, a counter that never ran would pass the test
/// below.
#[test]
fn the_counter_sees_an_allocation() {
    let made = allocations_in(|| {
        std::hint::black_box(Vec::<u8>::with_capacity(64));
    });
    assert_eq!(made, 1);
}

/// **`fill` allocates nothing**, with every list it walks non-empty: voices
/// sounding under a full budget, voices stolen and fading out on the release
/// list, and one-shots that finish inside the measured blocks.
#[test]
fn fill_allocates_nothing_while_the_budget_steals() {
    let mixer = Mixer::new();
    mixer.set_voice_budget(Some(8));
    for _ in 0..8 {
        mixer.play(Voice::new(vec![0.1f32; 64 * CHANNELS]));
    }
    let mut stole = 0;
    for _ in 0..4 {
        let long = Voice::new(vec![0.1f32; 4096 * CHANNELS]).with_priority(1);
        if matches!(mixer.try_play(long), PlayOutcome::Stole { .. }) {
            stole += 1;
        }
    }
    assert_eq!(stole, 4, "the fixture must have voices on the release list");

    let mut buf = vec![0.0f32; 128 * CHANNELS];
    let made = allocations_in(|| {
        for _ in 0..4 {
            mixer.fill(&mut buf, 48_000);
        }
    });
    assert_eq!(made, 0, "the audio thread's path allocated");
    // The blocks did run: the short voices finished and only the long ones are
    // left.
    assert_eq!(mixer.voice_count(), 4);
}
