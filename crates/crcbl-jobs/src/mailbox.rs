//! Latest-wins mailboxes — how a *state* crosses a thread boundary.
//!
//! One producer publishes complete states at its own cadence; one consumer
//! takes the newest complete one and goes. Neither ever waits for the other: a
//! slow producer publishes less often and a slow consumer misses intermediate
//! states, and both of those are the design's intent rather than a shortfall.
//! `docs/plan/21-jobs.md` calls this the first of the three allowed primitives,
//! and reserves it for states — snapshots, UI draw lists, pose palettes — where
//! the newest is the only one anybody wants.
//!
//! **Not for streams.** Input edges, audio commands and net packets must not be
//! droppable, and this drops by construction: a 3 ms tap that arrived between
//! two reads would simply not be there. Those want the accumulate-then-swap
//! ring instead.
//!
//! # Three slots, and why the count is exactly three
//!
//! One belongs to the producer, one to the consumer, and one sits in the
//! handoff between them. That is what lets both sides make progress without
//! ever waiting: the producer always has a slot nobody is reading, the consumer
//! always has one nobody is writing, and publishing is a swap of the middle one
//! rather than a copy of the payload.
//!
//! Two slots cannot do it — the producer would have to wait for the consumer to
//! let go of the one it wants next. Four buys nothing: the extra slot is never
//! the newest and never the one being written.
//!
//! # The invariant the `unsafe` rests on
//!
//! **`{producer, handoff, consumer}` is always a permutation of `{0, 1, 2}`.**
//! Both sides only ever exchange their own index with the handoff's, so the
//! three stay distinct for the life of the mailbox, and therefore the slot the
//! producer writes is never the slot the consumer reads. Neither side can be
//! duplicated — [`Publisher`] and [`Subscriber`] are not `Clone` and not
//! `Sync`, so "one producer, one consumer" is a fact about the types rather
//! than a rule in a comment.

use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// Slots in the mailbox. See the [module docs](self) for why it is three.
const SLOTS: usize = 3;

/// The slot index inside the handoff word.
const INDEX: u8 = 0b011;

/// Set by a publish, cleared by the read that takes it: whether the handoff
/// holds a state the consumer has not seen.
const FRESH: u8 = 0b100;

struct Shared<T> {
    slots: [UnsafeCell<T>; SLOTS],
    /// The slot in the handoff, plus [`FRESH`].
    handoff: AtomicU8,
}

// SAFETY: the permutation invariant in the module docs is what makes this
// sound. `Shared` is only ever reached through a `Publisher` and a `Subscriber`
// that hold distinct slot indices, each writes or reads only its own slot, and
// the pair are neither `Clone` nor `Sync` — so there is never a second writer
// for a slot nor a reader of the one being written. The `T: Send` bound is the
// other half: a state authored on one thread is dropped and read on another.
unsafe impl<T: Send> Send for Shared<T> {}
// SAFETY: as above. `Sync` is needed only because both halves hold an `Arc`,
// which requires it to be shared at all; nothing here is reachable through a
// `&Shared` in a way that could race, because every accessor takes `&mut self`
// on its own half.
unsafe impl<T: Send> Sync for Shared<T> {}

/// The writing half of a mailbox.
pub struct Publisher<T> {
    shared: Arc<Shared<T>>,
    slot: u8,
}

/// The reading half of a mailbox.
pub struct Subscriber<T> {
    shared: Arc<Shared<T>>,
    slot: u8,
}

impl<T> fmt::Debug for Publisher<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Publisher").finish_non_exhaustive()
    }
}

impl<T> fmt::Debug for Subscriber<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscriber").finish_non_exhaustive()
    }
}

/// Builds a mailbox already holding `initial`, so a consumer that reads before
/// anything has been published still gets a whole state rather than an
/// `Option` every call site would have to answer for.
///
/// `initial` is cloned into all three slots. A state type too expensive to
/// clone three times at startup is a reason to add a factory constructor, not
/// to make the steady-state path pay: publishing never copies the payload, it
/// swaps an index.
pub fn mailbox<T: Clone>(initial: T) -> (Publisher<T>, Subscriber<T>) {
    let shared = Arc::new(Shared {
        slots: [
            UnsafeCell::new(initial.clone()),
            UnsafeCell::new(initial.clone()),
            UnsafeCell::new(initial),
        ],
        // The starting permutation: producer 0, consumer 1, handoff 2, and
        // nothing fresh in it yet.
        handoff: AtomicU8::new(2),
    });
    (
        Publisher {
            shared: Arc::clone(&shared),
            slot: 0,
        },
        Subscriber { shared, slot: 1 },
    )
}

impl<T> Publisher<T> {
    /// Publishes `state` as the newest, and never waits.
    ///
    /// The state the consumer had not yet taken is overwritten — that is what
    /// latest-wins means, and the reason this primitive is for states and not
    /// for streams.
    pub fn publish(&mut self, state: T) {
        // SAFETY: `self.slot` is the producer's own index, distinct from the
        // consumer's and the handoff's by the permutation invariant, so no
        // other thread can be reading it. `&mut self` makes this the only
        // writer, and the assignment drops the state that slot held, which is
        // initialized because every slot starts initialized and is only ever
        // replaced.
        unsafe {
            *self.shared.slots[self.slot as usize].get() = state;
        }
        // `AcqRel`: the release half publishes the write above to whichever
        // thread acquires this word next, and the acquire half takes ownership
        // of the slot coming back — which the consumer wrote nothing to, but
        // which it did *read*, so the acquire is what orders our next
        // overwrite after its last read.
        let previous = self
            .shared
            .handoff
            .swap(self.slot | FRESH, Ordering::AcqRel);
        self.slot = previous & INDEX;
    }
}

impl<T> Subscriber<T> {
    /// The newest complete state, taking one from the handoff if the producer
    /// left one there.
    ///
    /// Never waits, and never fails: with nothing new published this returns
    /// the same state again, which is a consumer running ahead of its producer
    /// rather than an error. A frame drawn from a state one tick old is the
    /// outcome this whole design prefers to a frame that waited.
    pub fn read(&mut self) -> &T {
        // The load is a cheap "is there anything to do"; the swap is the
        // handoff itself. They are deliberately not one operation — a publish
        // landing between them only means the state we take is newer still,
        // and the consumer is the only thread that ever clears `FRESH`.
        if self.shared.handoff.load(Ordering::Acquire) & FRESH != 0 {
            let previous = self.shared.handoff.swap(self.slot, Ordering::AcqRel);
            self.slot = previous & INDEX;
        }
        // SAFETY: `self.slot` is the consumer's own index, distinct from the
        // producer's by the permutation invariant, so nothing is writing it.
        // The acquire above orders the producer's write to this slot before
        // this read.
        unsafe { &*self.shared.slots[self.slot as usize].get() }
    }

    /// Whether a state has been published that [`read`](Self::read) has not
    /// taken yet.
    ///
    /// For the staleness the profiler reports — a consumer that is keeping up
    /// answers `false` most frames. Not a precondition for `read`, which is
    /// always safe to call.
    #[must_use]
    pub fn has_new(&self) -> bool {
        self.shared.handoff.load(Ordering::Acquire) & FRESH != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The permutation the `unsafe` rests on, checked directly rather than
    /// argued for: whatever sequence of publishes and reads has run, the three
    /// indices are still distinct.
    fn assert_permutation<T>(publisher: &Publisher<T>, subscriber: &Subscriber<T>) {
        let handoff = subscriber.shared.handoff.load(Ordering::Acquire) & INDEX;
        let mut seen = [publisher.slot, subscriber.slot, handoff];
        seen.sort_unstable();
        assert_eq!(
            seen,
            [0, 1, 2],
            "the three slots stopped being a permutation: producer {}, \
             consumer {}, handoff {handoff}",
            publisher.slot,
            subscriber.slot,
        );
    }

    #[test]
    fn a_reader_starts_from_the_initial_state() {
        let (publisher, mut subscriber) = mailbox(7_u32);
        assert_eq!(*subscriber.read(), 7);
        assert!(!subscriber.has_new());
        assert_permutation(&publisher, &subscriber);
    }

    #[test]
    fn what_was_published_is_what_is_read() {
        let (mut publisher, mut subscriber) = mailbox(0_u32);
        publisher.publish(1);
        assert!(subscriber.has_new());
        assert_eq!(*subscriber.read(), 1);
        assert_permutation(&publisher, &subscriber);
    }

    /// **Latest wins, and the ones in between are gone.** A consumer that
    /// missed two publishes gets the third, not a queue of three — which is the
    /// whole difference between this primitive and the ring.
    #[test]
    fn a_reader_behind_the_producer_gets_the_newest_and_skips_the_rest() {
        let (mut publisher, mut subscriber) = mailbox(0_u32);
        for state in 1..=8 {
            publisher.publish(state);
        }
        assert_eq!(*subscriber.read(), 8);
        assert_permutation(&publisher, &subscriber);
    }

    /// A consumer ahead of its producer repeats the last state rather than
    /// blocking or failing — the "a stale frame beats a stalled one" rule, and
    /// the reason `read` returns `&T` and not `Option<&T>`.
    #[test]
    fn a_reader_ahead_of_the_producer_repeats_rather_than_waits() {
        let (mut publisher, mut subscriber) = mailbox(0_u32);
        publisher.publish(3);
        assert_eq!(*subscriber.read(), 3);
        for _ in 0..4 {
            assert_eq!(*subscriber.read(), 3);
            assert!(!subscriber.has_new());
        }
        assert_permutation(&publisher, &subscriber);
    }

    /// Interleaved publishes and reads, checking the invariant after every one:
    /// a slot-exchange bug that only shows up after a particular sequence would
    /// otherwise wait for a stress run to find it.
    #[test]
    fn interleaving_keeps_the_slots_distinct_and_the_states_in_order() {
        let (mut publisher, mut subscriber) = mailbox(0_u32);
        let mut published = 0;
        let mut last_seen = 0;

        for round in 1..=32_u32 {
            for _ in 0..(round % 3) {
                published += 1;
                publisher.publish(published);
                assert_permutation(&publisher, &subscriber);
            }
            let seen = *subscriber.read();
            assert_permutation(&publisher, &subscriber);
            assert!(
                seen >= last_seen,
                "a read went backwards: {seen} after {last_seen}",
            );
            assert!(seen <= published, "a state arrived before it was published");
            last_seen = seen;
        }
        assert_eq!(last_seen, published);
    }

    /// **A read is never torn.** Both halves of the state are written by the
    /// producer as one value, so a consumer that ever sees them disagree is
    /// reading a slot while it is being written — the exact failure the
    /// permutation invariant exists to rule out, and the one a single-threaded
    /// test cannot reach.
    #[test]
    fn a_concurrent_reader_never_sees_half_of_a_state() {
        const PUBLISHES: u64 = 20_000;

        let (mut publisher, mut subscriber) = mailbox((0_u64, 0_u64));
        let writer = std::thread::spawn(move || {
            for state in 1..=PUBLISHES {
                publisher.publish((state, state));
            }
        });

        // **Bounded, so a broken mailbox fails instead of hanging.** A publish
        // that never marks the handoff fresh leaves the consumer reading the
        // same state forever, and an unbounded loop would turn that into a
        // wedged test run rather than a red one. The deadline is generous: the
        // real run finishes in milliseconds.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut highest = 0;
        while highest < PUBLISHES {
            let (left, right) = *subscriber.read();
            assert_eq!(left, right, "a torn state: {left} against {right}");
            assert!(
                left >= highest,
                "a read went backwards: {left} after {highest}"
            );
            highest = left;
            assert!(
                std::time::Instant::now() < deadline,
                "stuck at {highest} of {PUBLISHES}: the consumer stopped being \
                 handed new states",
            );
        }

        writer.join().expect("the publisher panicked");
        assert_eq!(highest, PUBLISHES);
    }

    /// Both halves have to cross a thread boundary — a `Publisher` that
    /// stopped being `Send` would make the whole primitive unusable for the
    /// thing it exists for, and the failure would surface at some future call
    /// site rather than here.
    ///
    /// **What this does not check**: that neither half is `Clone`. A second
    /// producer is what breaks the permutation invariant, and the guard against
    /// it is structural — there is no `derive` and no manual impl — but a
    /// passing test cannot demonstrate the absence of an impl without a
    /// compile-fail harness this workspace does not have. Recorded as a gap
    /// rather than dressed up as covered.
    #[test]
    fn both_halves_move_between_threads() {
        fn assert_send<T: Send>() {}
        assert_send::<Publisher<u32>>();
        assert_send::<Subscriber<u32>>();
    }
}
