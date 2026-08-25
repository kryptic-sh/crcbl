//! The pool allocator under churn: does the free list stay a free list.
//!
//! `docs/plan/20-particles.md`'s Testing section asks for "pool allocator
//! property tests (churn, clamp, freelist integrity)". A table of hand-picked
//! sequences would test the cases someone thought of; the four invariants below
//! are what has to hold after *any* sequence, and proptest is what looks for
//! the one that breaks them.

use std::collections::HashSet;

use crcbl_vfx::{RangeAllocator, SlotRange};
use proptest::prelude::*;

/// Small enough that a random walk fills and empties it many times over.
const CAPACITY: u32 = 64;

/// One thing a caller can do to the allocator.
#[derive(Clone, Copy, Debug)]
enum Op {
    /// Ask for this many slots.
    Alloc(u32),
    /// Give back the range at this position in the live list, if it exists.
    Free(usize),
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    let op = prop_oneof![
        (1u32..=CAPACITY + 8).prop_map(Op::Alloc),
        (0usize..24).prop_map(Op::Free),
    ];
    prop::collection::vec(op, 1..300)
}

/// The four things that are true of the free list after every operation.
///
/// Returns how many slots are free, so the caller can check the accounting
/// against what it believes it holds.
fn check_free_list(allocator: &RangeAllocator) -> u32 {
    let spans = allocator.spans();
    let mut total = 0;
    for (at, span) in spans.iter().enumerate() {
        assert!(span.len > 0, "free span {at} is empty: {span:?}");
        assert!(
            span.end() <= allocator.capacity(),
            "free span {at} runs past the pool: {span:?}"
        );
        if at > 0 {
            let before = spans[at - 1];
            assert!(
                before.end() < span.start,
                "free spans {} and {at} overlap or touch: {before:?} then {span:?}",
                at - 1
            );
        }
        total += span.len;
    }
    assert_eq!(
        total,
        allocator.free_slots(),
        "the reported free count disagrees with the spans"
    );
    total
}

/// Every slot an allocated range claims, so overlaps between live ranges show
/// up as a duplicate rather than as a subtle arithmetic mismatch.
fn claimed(live: &[SlotRange]) -> HashSet<u32> {
    let mut slots = HashSet::new();
    for range in live {
        for slot in range.start..range.end() {
            assert!(
                slots.insert(slot),
                "slot {slot} is claimed by two live ranges: {live:?}"
            );
        }
    }
    slots
}

proptest! {
    #[test]
    fn churn_keeps_the_free_list_sound(ops in ops()) {
        let mut allocator = RangeAllocator::new(CAPACITY);
        let mut live: Vec<SlotRange> = Vec::new();

        for op in ops {
            match op {
                Op::Alloc(len) => {
                    let free_before = check_free_list(&allocator);
                    match allocator.alloc_clamped(len) {
                        Some(range) => {
                            prop_assert!(range.len > 0, "a granted range is empty");
                            prop_assert!(
                                range.len <= len,
                                "asked for {len} slots and was given {}",
                                range.len
                            );
                            prop_assert!(
                                range.end() <= CAPACITY,
                                "the granted range {range:?} runs past the pool"
                            );
                            live.push(range);
                        }
                        None => prop_assert_eq!(
                            free_before, 0,
                            "the allocator refused {} slots while {} were free",
                            len, free_before
                        ),
                    }
                }
                Op::Free(at) => {
                    if at < live.len() {
                        allocator.free(live.swap_remove(at));
                    }
                }
            }

            let free = check_free_list(&allocator);
            let held = claimed(&live);
            prop_assert_eq!(
                free as usize + held.len(),
                CAPACITY as usize,
                "{} free plus {} held is not the pool's {} slots",
                free,
                held.len(),
                CAPACITY
            );
        }

        // Emptying it must put it back exactly as it started: one whole span.
        for range in live.drain(..) {
            allocator.free(range);
        }
        prop_assert_eq!(allocator.free_slots(), CAPACITY, "the pool did not empty");
        prop_assert_eq!(
            allocator.spans(),
            &[SlotRange { start: 0, len: CAPACITY }],
            "an emptied pool is more than one span, so freed ranges are not merging"
        );
    }
}

#[test]
fn a_hole_between_two_ranges_merges_with_both() {
    let mut allocator = RangeAllocator::new(30);
    let first = allocator.alloc_clamped(10).expect("the pool is empty");
    let middle = allocator
        .alloc_clamped(10)
        .expect("twenty of thirty are free");
    let last = allocator.alloc_clamped(10).expect("ten of thirty are free");
    assert_eq!(allocator.spans(), &[], "the pool is not full");

    allocator.free(first);
    allocator.free(last);
    assert_eq!(
        allocator.spans(),
        &[
            SlotRange { start: 0, len: 10 },
            SlotRange { start: 20, len: 10 }
        ],
        "the two ends did not come back as separate spans"
    );

    allocator.free(middle);
    assert_eq!(
        allocator.spans(),
        &[SlotRange { start: 0, len: 30 }],
        "the hole between two free spans did not merge with either"
    );
}

#[test]
fn a_request_larger_than_the_largest_span_is_clamped_to_it() {
    let mut allocator = RangeAllocator::new(40);
    let held = allocator.alloc_clamped(25).expect("the pool is empty");
    let clamped = allocator
        .alloc_clamped(100)
        .expect("fifteen slots are still free");
    assert_eq!(
        clamped,
        SlotRange { start: 25, len: 15 },
        "a request past the pool was not clamped to what was left"
    );
    assert_eq!(
        allocator.alloc_clamped(1),
        None,
        "the allocator handed out a slot from a full pool"
    );
    allocator.free(held);
    assert_eq!(
        allocator.alloc_clamped(1),
        Some(SlotRange { start: 0, len: 1 }),
        "a freed range was not available again"
    );
}

#[test]
fn an_empty_pool_grants_nothing() {
    let mut allocator = RangeAllocator::new(0);
    assert_eq!(allocator.capacity(), 0);
    assert_eq!(allocator.spans(), &[], "a pool of no slots has a free span");
    assert_eq!(allocator.alloc_clamped(1), None);
}

#[test]
#[should_panic(expected = "overlaps free span")]
fn freeing_a_range_twice_is_refused() {
    let mut allocator = RangeAllocator::new(16);
    let range = allocator.alloc_clamped(8).expect("the pool is empty");
    allocator.free(range);
    allocator.free(range);
}
