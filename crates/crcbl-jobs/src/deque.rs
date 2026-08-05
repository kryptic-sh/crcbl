//! The work-stealing deque the pool runs on: bounded Chase-Lev.
//!
//! One **owner** pushes and pops at the bottom, so its own work is LIFO and
//! cache-hot; any number of **thieves** take from the top, so what they steal is
//! the oldest item, which in a fork-join shape is the biggest. The two ends
//! meet only over the last item, and that race is resolved by a compare-exchange
//! on `top` that exactly one side can win.
//!
//! The algorithm is Chase and Lev's — *Dynamic Circular Work-Stealing Deque*,
//! SPAA 2005 — with the memory orderings from Lê, Pop, Cohen and Zappa
//! Nardelli's *Correct and Efficient Work-Stealing for Weak Memory Models*,
//! PPoPP 2013, whose figure 1 is the C11 formulation this file transcribes. The
//! two `SeqCst` fences are that paper's result rather than caution: `pop`'s
//! store to `bottom` and `steal`'s load of `top` must not be reordered against
//! each other, or both ends take the same last item.
//!
//! # Why this is written rather than taken
//!
//! `crossbeam-deque` is the ecosystem's answer and would be the right one. It
//! is **not in this workspace's `Cargo.lock`** — neither it, nor any other
//! `crossbeam-*`, nor `rayon` — and taking on a new dependency is not a
//! decision this crate makes for the project. So the algorithm is transcribed
//! from the papers above and checked against the properties they state, which
//! is what the code-style rule asks for when there genuinely is no dependency
//! already in the tree.
//!
//! # Bounded rather than growable
//!
//! Chase and Lev's deque grows, and growing is where the difficulty lives: the
//! old buffer cannot be freed while a thief might still be reading it, which is
//! why `crossbeam` carries epoch-based reclamation behind it. A bounded deque
//! has no such question — [`Worker::push`] refuses when full and hands the
//! decision back, exactly as [`ring`](crate::ring) does, and the pool runs a
//! refused item on the spot rather than queueing it. That costs parallelism at
//! the margin and never costs correctness.
//!
//! # The slots hold pointers, and that is a soundness decision
//!
//! A thief reads its slot **speculatively** — before the compare-exchange that
//! decides whether the item is really its. If that read loses, the owner may
//! already be overwriting the slot: the read and the write are concurrent, and
//! nothing orders them. Read as a plain value that is a data race, which is
//! undefined behaviour rather than a tolerable one; `crossbeam-deque` does read
//! it that way and says so in a comment on its own `Buffer::read`.
//!
//! Holding only **pointers**, in [`AtomicPtr`], removes the question instead of
//! arguing about it: the speculative read is an atomic load, so there is
//! nothing to race. The cost is that the caller owns the storage the pointers
//! point at and keeps it alive until the item has been run — which the pool
//! does anyway, because a chunk of a `par_for` is described by a slot in a
//! buffer the call itself owns.
//!
//! Nothing here drops anything, for the same reason: the deque moves
//! references to work, never the work.

use core::fmt;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicIsize, AtomicPtr, Ordering, fence};
use std::sync::Arc;

struct Inner<T> {
    /// `mask + 1` slots, of which those in `top..bottom` hold an item.
    slots: Box<[AtomicPtr<T>]>,
    /// `capacity - 1`, and `capacity` is a power of two so this is a mask.
    mask: isize,
    /// One past the newest item. Written only by the owner.
    bottom: AtomicIsize,
    /// The oldest item. Advanced by whoever takes it — a thief, or the owner
    /// racing a thief for the last one.
    top: AtomicIsize,
}

/// The owning end of a deque: the only thing that may push or pop.
///
/// Not `Clone`, and every method takes `&mut self`, so "one owner" is a fact
/// about the type rather than a rule in a comment — the same guarantee
/// [`ring`](crate::ring)'s halves give.
pub(crate) struct Worker<T> {
    inner: Arc<Inner<T>>,
}

/// A thieving end of a deque. Cloneable and shareable: every worker thread
/// holds one, and stealing is what they contend on.
pub(crate) struct Stealer<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Clone for Stealer<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> fmt::Debug for Worker<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Worker")
            .field("capacity", &self.inner.slots.len())
            .finish_non_exhaustive()
    }
}

impl<T> fmt::Debug for Stealer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stealer")
            .field("capacity", &self.inner.slots.len())
            .finish_non_exhaustive()
    }
}

/// What a thief got, which is three answers rather than two.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Steal<T> {
    /// An item, now this thief's alone.
    Task(NonNull<T>),
    /// Nothing was there.
    Empty,
    /// Something was there and somebody else took it first. **Not** empty: a
    /// thief that treated this as empty would go to sleep beside a full deque.
    Retry,
}

/// A push a full deque would not take.
///
/// The item comes back to the caller by construction — a [`NonNull`] is `Copy`,
/// so the pusher still has it — and what it must not do is drop it on the
/// floor. The pool runs it inline.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Full;

/// Builds a deque holding at least `capacity` items, rounded up to a power of
/// two so the index wrap is a mask. A zero request becomes one, as in
/// [`ring`](crate::ring): a deque nothing fits in would refuse every push.
pub(crate) fn deque<T>(capacity: usize) -> (Worker<T>, Stealer<T>) {
    let capacity = capacity.max(1).next_power_of_two();
    let inner = Arc::new(Inner {
        slots: (0..capacity)
            .map(|_| AtomicPtr::new(core::ptr::null_mut()))
            .collect(),
        mask: capacity as isize - 1,
        bottom: AtomicIsize::new(0),
        top: AtomicIsize::new(0),
    });
    (
        Worker {
            inner: Arc::clone(&inner),
        },
        Stealer { inner },
    )
}

impl<T> Worker<T> {
    /// Appends an item at the bottom, or refuses when the deque is full.
    ///
    /// # Errors
    ///
    /// [`Full`] when `bottom - top` is already the capacity. The caller keeps
    /// the item — the pool runs it on the calling thread — and nothing is
    /// dropped.
    pub(crate) fn push(&mut self, task: NonNull<T>) -> Result<(), Full> {
        let inner = &*self.inner;
        // Relaxed: this thread is the only writer of `bottom`.
        let bottom = inner.bottom.load(Ordering::Relaxed);
        // Acquire, and it is what makes reusing a slot safe: a thief publishes
        // the slot it finished with by releasing `top` past it, so observing
        // the new `top` here orders that thief's read of the slot before the
        // overwrite below.
        let top = inner.top.load(Ordering::Acquire);
        if bottom - top >= inner.slots.len() as isize {
            return Err(Full);
        }
        inner.slots[(bottom & inner.mask) as usize].store(task.as_ptr(), Ordering::Relaxed);
        // Release, as a fence rather than on the store below, because it has to
        // publish *two* things a thief will read without holding a lock: this
        // slot, and whatever the item itself points at. A thief acquires
        // `bottom`, so everything sequenced before this fence happens-before
        // everything it does after that load.
        fence(Ordering::Release);
        inner.bottom.store(bottom + 1, Ordering::Relaxed);
        Ok(())
    }

    /// Takes the newest item, or `None` when there is none left to take.
    ///
    /// LIFO on purpose: the item the owner pushed last is the one still in its
    /// cache. The final item is the one a thief may also be going for, and the
    /// compare-exchange below is who decides.
    pub(crate) fn pop(&mut self) -> Option<NonNull<T>> {
        let inner = &*self.inner;
        // Claim the slot *before* looking at `top`, which is what makes the
        // race with a thief decidable at all: a thief that sees the lowered
        // `bottom` will not touch this item.
        let bottom = inner.bottom.load(Ordering::Relaxed) - 1;
        inner.bottom.store(bottom, Ordering::Relaxed);
        // SeqCst, and the paper's whole point: this store to `bottom` and a
        // thief's load of `top` must not be reordered past each other. Without
        // a total order over the two fences, the owner can read a stale `top`
        // while the thief reads a stale `bottom`, and both take the same item.
        fence(Ordering::SeqCst);
        let top = inner.top.load(Ordering::Relaxed);
        if top > bottom {
            // Empty. `bottom` goes back where it was, or the deque would count
            // down one slot on every failed pop.
            inner.bottom.store(bottom + 1, Ordering::Relaxed);
            return None;
        }
        // Relaxed: this thread wrote this slot itself.
        let task = inner.slots[(bottom & inner.mask) as usize].load(Ordering::Relaxed);
        if top == bottom {
            // The last item, and a thief may be reaching for it. Winning the
            // exchange is what makes it ours; losing means the thief has it and
            // the deque is now genuinely empty.
            let won = inner
                .top
                .compare_exchange(top, top + 1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok();
            inner.bottom.store(bottom + 1, Ordering::Relaxed);
            if !won {
                return None;
            }
        }
        NonNull::new(task)
    }
}

impl<T> Stealer<T> {
    /// Takes the oldest item, if the owner has not taken it first.
    pub(crate) fn steal(&self) -> Steal<T> {
        let inner = &*self.inner;
        // Acquire: whatever the thief that advanced `top` last did to that slot
        // must be visible before this one reads around it.
        let top = inner.top.load(Ordering::Acquire);
        // SeqCst: the other half of the pair in `pop`. See there.
        fence(Ordering::SeqCst);
        // Acquire, pairing with the release fence in `push`: it is what makes
        // the slot's contents — and the item behind the pointer — visible here.
        let bottom = inner.bottom.load(Ordering::Acquire);
        if top >= bottom {
            return Steal::Empty;
        }
        // Speculative, and the reason the slots are atomic: if the exchange
        // below fails, the owner may already be overwriting this slot, and a
        // non-atomic read would be racing that write. See the module docs.
        let task = inner.slots[(top & inner.mask) as usize].load(Ordering::Relaxed);
        if inner
            .top
            .compare_exchange(top, top + 1, Ordering::SeqCst, Ordering::Relaxed)
            .is_err()
        {
            return Steal::Retry;
        }
        Steal::Task(
            NonNull::new(task).expect(
                "a slot inside `top..bottom` was written before `bottom` was released past it",
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for the pool's chunk descriptors: the deque moves pointers to
    /// items the caller owns, so a test owns an array and pushes pointers into
    /// it.
    fn items(count: usize) -> Vec<usize> {
        (0..count).collect()
    }

    fn at(items: &[usize], index: usize) -> NonNull<usize> {
        NonNull::from(&items[index])
    }

    #[test]
    fn capacity_rounds_up_to_a_power_of_two_and_zero_still_holds_something() {
        let (worker, _) = deque::<usize>(5);
        assert_eq!(worker.inner.slots.len(), 8);

        let (worker, _) = deque::<usize>(0);
        assert_eq!(
            worker.inner.slots.len(),
            1,
            "a deque nothing fits in is useless",
        );
    }

    /// **The owner's end is LIFO and a thief's end is FIFO** — the property the
    /// whole shape exists for, and the one that makes a stolen item the oldest
    /// and therefore, in a fork-join, the largest.
    #[test]
    fn the_owner_takes_the_newest_and_a_thief_takes_the_oldest() {
        let items = items(4);
        let (mut worker, stealer) = deque(8);
        for index in 0..4 {
            worker.push(at(&items, index)).expect("within capacity");
        }

        assert_eq!(stealer.steal(), Steal::Task(at(&items, 0)));
        assert_eq!(worker.pop(), Some(at(&items, 3)));
        assert_eq!(stealer.steal(), Steal::Task(at(&items, 1)));
        assert_eq!(worker.pop(), Some(at(&items, 2)));
        assert_eq!(stealer.steal(), Steal::Empty);
        assert_eq!(worker.pop(), None);
    }

    /// A full deque refuses rather than overwriting the item at the wrap, which
    /// is the failure a masked ring makes silent.
    #[test]
    fn a_full_deque_refuses_the_push() {
        let items = items(4);
        let (mut worker, _stealer) = deque(2);
        worker.push(at(&items, 0)).expect("first");
        worker.push(at(&items, 1)).expect("second");
        assert_eq!(worker.push(at(&items, 2)), Err(Full));

        assert_eq!(worker.pop(), Some(at(&items, 1)), "the deque still works");
        worker.push(at(&items, 3)).expect("a slot came free");
    }

    /// **A failed pop must leave `bottom` where it found it.** `pop` lowers it
    /// before it knows whether there is anything to take, so an empty deque
    /// that forgot to put it back would count down one slot per call and start
    /// handing out items that were never pushed.
    #[test]
    fn a_pop_from_an_empty_deque_puts_bottom_back() {
        let items = items(1);
        let (mut worker, _stealer) = deque(4);
        for _ in 0..8 {
            assert_eq!(worker.pop(), None);
        }
        assert_eq!(
            worker.inner.bottom.load(Ordering::Relaxed),
            0,
            "eight failed pops moved the deque's own cursor",
        );

        worker.push(at(&items, 0)).expect("empty");
        assert_eq!(worker.pop(), Some(at(&items, 0)));
        assert_eq!(worker.pop(), None);
    }

    /// The same for the last item taken by a thief: the owner's `pop` lowers
    /// `bottom`, loses the exchange, and must still leave the cursor consistent
    /// — otherwise the *next* push writes over an occupied slot.
    #[test]
    fn losing_the_race_for_the_last_item_leaves_the_deque_usable() {
        let items = items(2);
        let (mut worker, stealer) = deque(4);
        worker.push(at(&items, 0)).expect("empty");

        assert_eq!(stealer.steal(), Steal::Task(at(&items, 0)));
        assert_eq!(worker.pop(), None, "the thief took the only item");

        worker.push(at(&items, 1)).expect("empty again");
        assert_eq!(worker.pop(), Some(at(&items, 1)));
    }

    /// Indices wrap many times without confusing full for empty — the classic
    /// off-by-one in a masked ring, which only shows up after more items than
    /// slots have been through it.
    #[test]
    fn a_deque_wraps_many_times_without_confusing_full_for_empty() {
        let items = items(4);
        let (mut worker, stealer) = deque(4);
        for round in 0..64 {
            for index in 0..4 {
                worker.push(at(&items, index)).expect("emptied last round");
            }
            assert_eq!(worker.push(at(&items, 0)), Err(Full), "round {round}");
            assert_eq!(stealer.steal(), Steal::Task(at(&items, 0)));
            for index in (1..4).rev() {
                assert_eq!(worker.pop(), Some(at(&items, index)));
            }
            assert_eq!(worker.pop(), None);
        }
    }

    /// **Every item is taken exactly once, by exactly one of them**, whatever
    /// the interleaving — which is the property the paper proves and the only
    /// one worth asserting across a real thread boundary. A lost item hangs the
    /// pool that waits for it; a duplicated one runs a `par_for` chunk twice
    /// over the same `&mut [T]`.
    ///
    /// Run under miri too, where the fences are the only thing standing between
    /// this and a race the hardware here cannot exhibit.
    #[test]
    fn every_item_is_taken_exactly_once_across_threads() {
        const ITEMS: usize = if cfg!(miri) { 200 } else { 20_000 };
        const THIEVES: usize = 3;

        let items = items(ITEMS);
        let taken: Vec<_> = (0..ITEMS)
            .map(|_| std::sync::atomic::AtomicUsize::new(0))
            .collect();
        let taken = &taken;
        let count = |item: NonNull<usize>| {
            // SAFETY: every pointer in the deque came from `items`, which
            // outlives this scope and which nothing ever writes.
            let index = unsafe { *item.as_ref() };
            taken[index].fetch_add(1, Ordering::Relaxed);
        };
        let (mut worker, stealer) = deque(64);
        let done = std::sync::atomic::AtomicBool::new(false);
        let done = &done;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        std::thread::scope(|scope| {
            for _ in 0..THIEVES {
                let stealer = stealer.clone();
                scope.spawn(move || {
                    while !done.load(Ordering::Acquire) {
                        match stealer.steal() {
                            Steal::Task(item) => count(item),
                            Steal::Empty | Steal::Retry => std::thread::yield_now(),
                        }
                    }
                });
            }

            let mut pushed = 0;
            while pushed < ITEMS && std::time::Instant::now() < deadline {
                if worker.push(at(&items, pushed)).is_ok() {
                    pushed += 1;
                } else if let Some(item) = worker.pop() {
                    count(item);
                }
            }
            while let Some(item) = worker.pop() {
                count(item);
            }
            // Whatever a thief has already claimed still has to be counted, so
            // the owner waits for the deque to drain rather than stopping the
            // thieves the moment it runs out of pushes.
            while taken.iter().any(|c| c.load(Ordering::Relaxed) == 0)
                && std::time::Instant::now() < deadline
            {
                std::thread::yield_now();
            }
            // **Before any assertion, and never inside one.** `scope` joins the
            // thieves before it propagates a panic, and the thieves run until
            // this flag is set — so an assertion that fired in here would hang
            // the run rather than fail it. Found by a mutation that made the
            // deque lose items: the red test wedged instead.
            done.store(true, Ordering::Release);
        });

        for (index, count) in taken.iter().enumerate() {
            assert_eq!(
                count.load(Ordering::Relaxed),
                1,
                "item {index} was taken {} times",
                count.load(Ordering::Relaxed),
            );
        }
    }

    /// **The last item goes to exactly one taker.** The owner and a thief reach
    /// for the same item whenever the deque holds one, and the compare-exchange
    /// on `top` is the only thing that decides between them — so this keeps the
    /// deque one item deep, which is the state the whole race lives in and the
    /// state the test above almost never reaches.
    ///
    /// Measured rather than assumed: replacing that exchange with an
    /// unconditional take leaves the test above green and turns this one red.
    #[test]
    fn the_last_item_goes_to_exactly_one_taker() {
        const ROUNDS: usize = if cfg!(miri) { 100 } else { 20_000 };
        const THIEVES: usize = 3;

        let items = items(ROUNDS);
        let taken: Vec<_> = (0..ROUNDS)
            .map(|_| std::sync::atomic::AtomicUsize::new(0))
            .collect();
        let taken = &taken;
        let count = |item: NonNull<usize>| {
            // SAFETY: as in the test above — every pointer came from `items`.
            let index = unsafe { *item.as_ref() };
            taken[index].fetch_add(1, Ordering::Relaxed);
        };
        let (mut worker, stealer) = deque(8);
        let done = std::sync::atomic::AtomicBool::new(false);
        let done = &done;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        std::thread::scope(|scope| {
            for _ in 0..THIEVES {
                let stealer = stealer.clone();
                scope.spawn(move || {
                    while !done.load(Ordering::Acquire) {
                        if let Steal::Task(item) = stealer.steal() {
                            count(item);
                        }
                    }
                });
            }

            for round in 0..ROUNDS {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                worker.push(at(&items, round)).expect("one item at a time");
                // Immediately, so the owner is reaching for the same item the
                // thieves are. Either it gets it or a thief did.
                if let Some(item) = worker.pop() {
                    count(item);
                }
            }
            while taken.iter().any(|c| c.load(Ordering::Relaxed) == 0)
                && std::time::Instant::now() < deadline
            {
                std::thread::yield_now();
            }
            // Before any assertion; see the test above for why.
            done.store(true, Ordering::Release);
        });

        for (index, count) in taken.iter().enumerate() {
            assert_eq!(
                count.load(Ordering::Relaxed),
                1,
                "item {index} was taken {} times",
                count.load(Ordering::Relaxed),
            );
        }
    }

    /// The thieving end has to reach a worker thread, and the owning end has to
    /// reach whichever thread drives the pool.
    #[test]
    fn both_ends_move_between_threads() {
        fn assert_send<T: Send>() {}
        fn assert_shared<T: Send + Sync>() {}
        assert_send::<Worker<usize>>();
        assert_shared::<Stealer<usize>>();
    }
}
