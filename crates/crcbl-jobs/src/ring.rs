//! Bounded SPSC rings — how a *stream* crosses a thread boundary.
//!
//! The second of the design's three primitives, and the opposite discipline to
//! [`mailbox`](crate::mailbox): every item is delivered, in order, and a
//! producer that outruns its consumer is refused rather than allowed to
//! overwrite. Input edges, audio commands and net packets go through here,
//! because **a 3 ms tap must survive a 16 ms tick** — a stream that quietly
//! dropped what arrived between two reads would lose exactly the events the
//! game is played with.
//!
//! Latest-wins is the right answer for a snapshot and the wrong one here; that
//! is the whole reason there are two primitives rather than one configurable
//! one.
//!
//! # Bounded, and what happens at the bound
//!
//! [`Producer::push`] hands the item **back** when the ring is full, and counts
//! the refusal. Nothing is dropped on the floor and nothing is decided on the
//! caller's behalf: an input thread can escalate, a net reader can shed load,
//! and a test can assert the count is zero. [`Consumer::overflows`] is what the
//! profiler's ring-overflow counter reads.
//!
//! **Drop-oldest is not implemented**, though the design lists it as a policy.
//! It cannot be done from this side: the read cursor belongs to the consumer,
//! and a producer that advanced it to make room would be a second writer to it
//! — which is precisely what makes an SPSC ring cheap. A ring wanting that
//! behaviour wants the consumer to drain more eagerly, or a mailbox instead.
//! Recorded in `docs/backlog.md` rather than left as a surprise.

use core::cell::UnsafeCell;
use core::fmt;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

struct Shared<T> {
    /// `capacity` slots, live exactly between `head` and `tail`.
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    /// `capacity - 1`, and `capacity` is a power of two so this is a mask.
    mask: usize,
    /// The next index to read. Written only by the consumer.
    head: AtomicUsize,
    /// The next index to write. Written only by the producer.
    tail: AtomicUsize,
    /// Pushes refused because the ring was full. Written only by the producer.
    overflows: AtomicU64,
}

// SAFETY: the producer only ever writes the slot at `tail` and only ever
// advances `tail`; the consumer only ever reads the slot at `head` and only
// ever advances `head`. A slot is written before `tail` is released and read
// after `tail` is acquired, so the write happens-before the read; a slot is
// only overwritten once `head` has been released past it, so the read
// happens-before the next write. The two halves cannot be duplicated —
// `Producer` and `Consumer` are neither `Clone` nor `Sync` — so "one of each"
// is a fact about the types. `T: Send` because an item is authored on one
// thread and read and dropped on another.
unsafe impl<T: Send> Send for Shared<T> {}
// SAFETY: as above; `Sync` is required only so the two halves may each hold an
// `Arc`, and every accessor takes `&mut self` on its own half.
unsafe impl<T: Send> Sync for Shared<T> {}

impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        // Whatever was pushed and never popped is still initialized and still
        // owned here. Missing this leaks every item left in a ring that outlived
        // its consumer, which for an input ring is every event of the last tick.
        let mut index = *self.head.get_mut();
        let tail = *self.tail.get_mut();
        while index != tail {
            // SAFETY: every index in `head..tail` names a slot the producer
            // wrote and the consumer never took, so it is initialized, and
            // `&mut self` means nothing else can reach it.
            unsafe {
                (*self.slots[index & self.mask].get()).assume_init_drop();
            }
            index = index.wrapping_add(1);
        }
    }
}

/// The writing half of a ring.
pub struct Producer<T> {
    shared: Arc<Shared<T>>,
    /// The producer's own copy of `tail`, so the steady-state push reads one
    /// atomic rather than two.
    tail: usize,
}

/// The reading half of a ring.
pub struct Consumer<T> {
    shared: Arc<Shared<T>>,
    /// The consumer's own copy of `head`; see [`Producer`].
    head: usize,
}

impl<T> fmt::Debug for Producer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Producer")
            .field("capacity", &self.shared.slots.len())
            .finish_non_exhaustive()
    }
}

impl<T> fmt::Debug for Consumer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Consumer")
            .field("capacity", &self.shared.slots.len())
            .finish_non_exhaustive()
    }
}

/// An item a full ring would not take, handed back rather than dropped.
///
/// The caller owns the decision: retry after draining, shed it deliberately, or
/// treat the refusal as the fault it usually is. What this type prevents is the
/// third option happening silently.
#[derive(Debug, PartialEq, Eq)]
pub struct Full<T>(pub T);

impl<T> fmt::Display for Full<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the ring is full")
    }
}

impl<T: fmt::Debug> std::error::Error for Full<T> {}

/// Builds a ring holding at least `capacity` items.
///
/// The real capacity is `capacity` rounded up to a power of two — the index
/// wrap is then a mask rather than a division — and [`Producer::capacity`]
/// reports what it actually became. A zero request becomes one: a ring nothing
/// fits in would refuse every push forever, which is a configuration mistake
/// worth surviving rather than a state worth having.
///
/// # Panics
///
/// If `capacity` rounded up overflows `usize`.
#[must_use]
pub fn ring<T>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    let capacity = capacity.max(1).next_power_of_two();
    let shared = Arc::new(Shared {
        slots: (0..capacity)
            .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
            .collect(),
        mask: capacity - 1,
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
        overflows: AtomicU64::new(0),
    });
    (
        Producer {
            shared: Arc::clone(&shared),
            tail: 0,
        },
        Consumer { shared, head: 0 },
    )
}

impl<T> Producer<T> {
    /// How many items the ring holds when full.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.shared.slots.len()
    }

    /// Appends `item`, or hands it back if the ring is full.
    ///
    /// Never waits.
    ///
    /// # Errors
    ///
    /// [`Full`] carries `item` back untouched, and the refusal is added to
    /// [`Consumer::overflows`].
    pub fn push(&mut self, item: T) -> Result<(), Full<T>> {
        // Acquire, because a slot may only be overwritten once the consumer's
        // read of it has happened-before this write.
        let head = self.shared.head.load(Ordering::Acquire);
        if self.tail.wrapping_sub(head) == self.shared.slots.len() {
            // Relaxed: a counter nothing orders anything against. The consumer
            // may read it late, which for a diagnostic is not a defect.
            self.shared.overflows.fetch_add(1, Ordering::Relaxed);
            return Err(Full(item));
        }
        // SAFETY: `tail` is not within `head..tail`, so this slot holds no live
        // item — either it was never written, or the consumer has taken it and
        // released `head` past it, which the acquire above observed. The
        // producer is the only writer of this slot.
        unsafe {
            (*self.shared.slots[self.tail & self.shared.mask].get()).write(item);
        }
        self.tail = self.tail.wrapping_add(1);
        // Release: the write above must be visible to the consumer that
        // acquires this.
        self.shared.tail.store(self.tail, Ordering::Release);
        Ok(())
    }
}

impl<T> Consumer<T> {
    /// How many items the ring holds when full.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.shared.slots.len()
    }

    /// Takes the oldest item, or `None` when there is nothing to take.
    ///
    /// Never waits.
    pub fn pop(&mut self) -> Option<T> {
        // Acquire: pairs with the producer's release, so the item's bytes are
        // visible before it is read.
        let tail = self.shared.tail.load(Ordering::Acquire);
        if self.head == tail {
            return None;
        }
        // SAFETY: `head` is within `head..tail`, so the producer wrote this
        // slot and released `tail` past it, which the acquire above observed.
        // Reading it out moves ownership here, and `head` is released past it
        // below so the producer will not read it again.
        let item =
            unsafe { (*self.shared.slots[self.head & self.shared.mask].get()).assume_init_read() };
        self.head = self.head.wrapping_add(1);
        // Release: the producer may reuse this slot only after this read.
        self.shared.head.store(self.head, Ordering::Release);
        Some(item)
    }

    /// Pushes the producer has been refused because the ring was full.
    ///
    /// The counter the profiler surfaces. **For an input ring this should be
    /// zero**: an edge that was refused is an edge the game never saw, so a
    /// non-zero count here is a ring sized wrong or a consumer that stopped
    /// draining, not a load-shedding success.
    #[must_use]
    pub fn overflows(&self) -> u64 {
        self.shared.overflows.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc as StdArc;
    use std::sync::atomic::AtomicUsize as Counter;

    #[test]
    fn capacity_rounds_up_to_a_power_of_two_and_zero_still_holds_something() {
        let (producer, consumer) = ring::<u32>(5);
        assert_eq!(producer.capacity(), 8);
        assert_eq!(consumer.capacity(), 8);

        let (producer, _consumer) = ring::<u32>(0);
        assert_eq!(producer.capacity(), 1, "a ring nothing fits in is useless");
    }

    /// **Order is the whole contract.** A stream that arrived out of order
    /// would turn a press-then-release into a release-then-press, which is a
    /// key stuck down.
    #[test]
    fn items_come_out_in_the_order_they_went_in() {
        let (mut producer, mut consumer) = ring(8);
        for item in 0..8_u32 {
            producer.push(item).expect("within capacity");
        }
        let drained: Vec<_> = std::iter::from_fn(|| consumer.pop()).collect();
        assert_eq!(drained, (0..8).collect::<Vec<u32>>());
        assert_eq!(consumer.pop(), None);
    }

    /// A full ring hands the item back rather than dropping it, and says so.
    #[test]
    fn a_full_ring_refuses_and_returns_what_it_would_not_take() {
        let (mut producer, consumer) = ring(2);
        producer.push(1_u32).expect("first");
        producer.push(2).expect("second");

        assert_eq!(producer.push(3), Err(Full(3)), "the item must come back");
        assert_eq!(consumer.overflows(), 1);
        assert_eq!(producer.push(4), Err(Full(4)));
        assert_eq!(consumer.overflows(), 2);
    }

    /// Draining makes room, and the indices wrap without confusing full for
    /// empty — the classic off-by-one in a masked ring, which shows up only
    /// after more items than slots have been through it.
    #[test]
    fn a_ring_wraps_many_times_without_confusing_full_for_empty() {
        let (mut producer, mut consumer) = ring(4);
        let mut next_in = 0_u32;
        let mut next_out = 0_u32;

        for _ in 0..64 {
            while producer.push(next_in).is_ok() {
                next_in += 1;
            }
            assert_eq!(
                next_in - next_out,
                4,
                "a full ring must hold exactly its capacity",
            );
            for _ in 0..3 {
                assert_eq!(consumer.pop(), Some(next_out));
                next_out += 1;
            }
        }
        assert!(next_in > 64, "the ring never wrapped: {next_in} items");
    }

    /// Counts drops, so an item that is never dropped or dropped twice is
    /// visible. `MaybeUninit` makes both possible and neither loud.
    struct Tracked(StdArc<Counter>);

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// **Everything pushed is dropped exactly once**, whether it was popped or
    /// still in the ring when it went away. A `MaybeUninit` slot that is not
    /// dropped leaks, and one dropped twice is undefined behaviour; nothing but
    /// a count can tell you which happened.
    #[test]
    fn every_item_is_dropped_exactly_once() {
        let drops = StdArc::new(Counter::new(0));

        let (mut producer, mut consumer) = ring(8);
        for _ in 0..5 {
            producer
                .push(Tracked(StdArc::clone(&drops)))
                .map_err(|_| "within capacity")
                .expect("within capacity");
        }
        // Two taken out and dropped by the caller...
        drop(consumer.pop().expect("an item"));
        drop(consumer.pop().expect("an item"));
        assert_eq!(drops.load(Ordering::SeqCst), 2);

        // ...and three still inside when the ring goes away.
        drop(producer);
        drop(consumer);
        assert_eq!(
            drops.load(Ordering::SeqCst),
            5,
            "items left in the ring were leaked rather than dropped",
        );
    }

    /// **Nothing is lost, duplicated or reordered across a real thread
    /// boundary**, which is the property the whole primitive exists for and the
    /// one no single-threaded test can reach. Run under miri too, where a
    /// missing fence is reported as a data race.
    #[test]
    fn a_concurrent_producer_and_consumer_move_every_item_in_order() {
        const ITEMS: u32 = 20_000;

        let (mut producer, mut consumer) = ring(64);
        let writer = std::thread::spawn(move || {
            let mut item = 0;
            while item < ITEMS {
                if producer.push(item).is_ok() {
                    item += 1;
                } else {
                    std::thread::yield_now();
                }
            }
        });

        // Bounded so a ring that stops delivering fails red rather than
        // hanging the run; the real thing finishes in milliseconds.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut expected = 0;
        while expected < ITEMS {
            match consumer.pop() {
                Some(item) => {
                    assert_eq!(item, expected, "an item arrived out of order");
                    expected += 1;
                }
                None => std::thread::yield_now(),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "stuck at {expected} of {ITEMS}",
            );
        }

        writer.join().expect("the producer panicked");
        assert_eq!(consumer.pop(), None, "an item arrived that was never sent");
    }

    /// Both halves have to cross a thread boundary. As with the mailbox, that
    /// neither is `Clone` is structural and not demonstrable in a passing test.
    #[test]
    fn both_halves_move_between_threads() {
        fn assert_send<T: Send>() {}
        assert_send::<Producer<u32>>();
        assert_send::<Consumer<u32>>();
    }
}
