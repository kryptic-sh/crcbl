//! The deletion queue: what makes `destroy_*` mean "this handle is dead now"
//! rather than "the GPU is finished".
//!
//! `crcbl-hal`'s [`device`](crcbl_hal::device) module states the contract and
//! leaves the mechanism to the backend, and
//! `docs/plan/02-vulkan-backend.md` §2.2 names it: "deletion queue (resources
//! retire N frames later)". Destroying a `VkImageView` the GPU is still reading
//! is undefined behaviour, so the handle dies immediately — the generational
//! [`Handle`](crcbl_core::Handle) sees to that — while the driver object is
//! parked here until the submission that last touched it has completed.
//!
//! # Keyed on a timeline value, not a frame count
//!
//! "N frames later" is a heuristic; a timeline value is a fact. The device
//! signals a monotonically increasing value on **every** submission, so
//! `retire_at` is simply "the number of submissions issued when this object was
//! destroyed" and an object is safe to free once the timeline has reached it.
//! That also makes the queue correct when a frame issues two submissions or
//! none, which a frame counter is not.
//!
//! # A timeline value is not the whole answer
//!
//! The seam lets a caller record a command buffer, destroy an object it uses,
//! and submit later — so between `finish` and `submit` an object is referenced
//! by work that **no** timeline value describes, because the submission that
//! would signal one has not been issued. Waiting for a key that only covers the
//! submissions already made frees it under that recording. So [`retire`] takes
//! a `held` predicate beside the timeline value, and `crate::device`'s
//! `poll_retire` answers it from the command buffers that are recorded and not
//! yet submitted.
//!
//! [`retire`]: RetireQueue::retire
//!
//! # What the queue cannot cover
//!
//! Swapchains. A present is queue work that no timeline semaphore signals, so
//! "the timeline reached N" says nothing about whether the presents on a
//! swapchain have finished — and `vkDestroySwapchainKHR` on one still in use is
//! undefined behaviour. `crate::device`'s `retire_swapchain` waits for a device
//! idle instead. Everything else — buffers, images, views, semaphores, query
//! pools — belongs here.
//!
//! # The retirement policy is pure, so it is tested without a GPU
//!
//! [`RetireQueue`] is generic over its payload and knows nothing about Vulkan.
//! The device parameterises it with a closure that calls the right
//! `vkDestroy*`; the ordering rules — nothing retires early, everything retires
//! eventually, and each entry frees exactly when its own key is reached — are
//! unit tests.

use core::fmt;
use std::collections::VecDeque;

/// Payloads waiting for the device timeline to reach a value.
///
/// [`push`](Self::push) is only ever called with a non-decreasing `retire_at`,
/// because the value is the submission counter, which only goes up — so the
/// queue is FIFO by construction. [`extend_matching`](Self::extend_matching)
/// can leave a later key in front of an earlier one, so [`retire`](Self::retire)
/// scans the whole queue and frees every entry whose own key has been reached:
/// each entry is freed exactly when its own submission count is passed, never
/// earlier.
pub struct RetireQueue<T> {
    entries: VecDeque<(u64, T)>,
}

impl<T> fmt::Debug for RetireQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetireQueue")
            .field("pending", &self.entries.len())
            .field("oldest", &self.entries.front().map(|(at, _)| *at))
            .finish()
    }
}

impl<T> Default for RetireQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> RetireQueue<T> {
    /// An empty queue.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }

    /// Parks `payload` until the timeline reaches `retire_at`.
    pub fn push(&mut self, retire_at: u64, payload: T) {
        debug_assert!(
            self.entries
                .back()
                .is_none_or(|(back, _)| *back <= retire_at),
            "the submission counter only increases, so the queue is already ordered"
        );
        self.entries.push_back((retire_at, payload));
    }

    /// Extends the retirement key of every parked payload matching `matches` to
    /// at least `new_at`. `submit` uses this: a submission that references a
    /// parked object keeps it parked until that submission completes. Keys may
    /// now leave FIFO order, which [`Self::retire`] tolerates by freeing every
    /// entry whose own key is reached.
    pub fn extend_matching(&mut self, new_at: u64, mut matches: impl FnMut(&T) -> bool) {
        for (retire_at, payload) in &mut self.entries {
            if matches(payload) {
                *retire_at = (*retire_at).max(new_at);
            }
        }
    }

    /// How many payloads are still parked.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.entries.len()
    }

    /// Hands `destroy` every payload whose `retire_at` the timeline has reached
    /// and that `held` does not claim.
    ///
    /// `completed` is the timeline's current value. The whole queue is scanned
    /// rather than stopping at the first entry that is not ready, because
    /// [`extend_matching`](Self::extend_matching) can put a later key in front
    /// of an earlier one — a ready entry behind an extended one must still free
    /// promptly. Each entry is freed exactly when its own submission count is
    /// passed, never earlier.
    ///
    /// `held` is the second half of "safe to free", and the timeline cannot
    /// answer it: a payload something still *references* but that no submission
    /// has ever been issued for has no key that means anything, because no
    /// submission will ever signal one on its behalf. A held payload is skipped
    /// and stays parked at its own key, so it frees on the next pass after the
    /// hold is released.
    pub fn retire(
        &mut self,
        completed: u64,
        mut held: impl FnMut(&T) -> bool,
        mut destroy: impl FnMut(T),
    ) {
        let mut index = 0;
        while index < self.entries.len() {
            if self.entries[index].0 <= completed && !held(&self.entries[index].1) {
                let (_, payload) = self
                    .entries
                    .remove(index)
                    .unwrap_or_else(|| unreachable!("index is in bounds above"));
                destroy(payload);
            } else {
                index += 1;
            }
        }
    }

    /// Hands `destroy` everything, regardless of the timeline.
    ///
    /// Only correct after the device is idle — which is exactly where it is
    /// called: `wait_idle` at shutdown, and `Drop` after a final
    /// `vkDeviceWaitIdle`.
    pub fn drain_all(&mut self, mut destroy: impl FnMut(T)) {
        while let Some((_, payload)) = self.entries.pop_front() {
            destroy(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retires with nothing held, which is the steady state: every command
    /// buffer that referenced these payloads has been submitted.
    fn drained(queue: &mut RetireQueue<u32>, completed: u64) -> Vec<u32> {
        let mut out = Vec::new();
        queue.retire(completed, |_| false, |payload| out.push(payload));
        out
    }

    /// The whole point: nothing is freed before the GPU has passed the value it
    /// was parked at. A queue that retired early would be a use-after-free the
    /// validation layer reports as "object destroyed while in use" — if you are
    /// lucky.
    #[test]
    fn nothing_retires_before_its_value() {
        let mut queue = RetireQueue::new();
        queue.push(3, 30_u32);
        queue.push(5, 50);
        assert_eq!(queue.pending(), 2);

        assert!(drained(&mut queue, 0).is_empty());
        assert!(drained(&mut queue, 2).is_empty());
        assert_eq!(
            queue.pending(),
            2,
            "a poll that frees nothing frees nothing"
        );

        assert_eq!(drained(&mut queue, 3), vec![30]);
        assert_eq!(queue.pending(), 1);
        assert_eq!(drained(&mut queue, 4), Vec::<u32>::new());
        assert_eq!(drained(&mut queue, 9), vec![50]);
        assert_eq!(queue.pending(), 0);
    }

    /// Objects destroyed during the same submission share a value, and all of
    /// them must go at once — the loop's exit condition is `>`, not `>=`.
    #[test]
    fn a_whole_submissions_worth_retires_together() {
        let mut queue = RetireQueue::new();
        queue.push(7, 1_u32);
        queue.push(7, 2);
        queue.push(7, 3);
        queue.push(8, 4);
        assert_eq!(drained(&mut queue, 7), vec![1, 2, 3]);
        assert_eq!(queue.pending(), 1);
    }

    /// Retirement is FIFO. A backend that freed out of order would still be
    /// *sound*, but the order is what makes a leak report readable, and a view
    /// must not outlive the image it views even in the free list.
    #[test]
    fn retirement_is_in_submission_order() {
        let mut queue = RetireQueue::new();
        for value in 1..=5_u64 {
            #[allow(clippy::cast_possible_truncation)]
            queue.push(value, value as u32 * 10);
        }
        assert_eq!(drained(&mut queue, 5), vec![10, 20, 30, 40, 50]);
    }

    /// A submission referencing a parked object extends its key to this
    /// submission's value, so the object stays parked until the *last*
    /// submission using it completes — the whole fix. A payload the predicate
    /// does not match is left on its own schedule.
    #[test]
    fn extend_matching_keeps_a_referenced_object_parked() {
        let mut queue = RetireQueue::new();
        queue.push(5, 30_u32);
        queue.push(7, 50);
        queue.extend_matching(9, |payload| *payload == 30_u32);
        assert_eq!(
            drained(&mut queue, 7),
            vec![50],
            "the non-matching payload frees on its own schedule"
        );
        assert_eq!(queue.pending(), 1);
        assert_eq!(
            drained(&mut queue, 9),
            vec![30],
            "the referenced payload stays parked until its extended key"
        );

        // And a predicate that matches nothing extends nothing.
        queue.push(3, 70_u32);
        queue.extend_matching(9, |_| false);
        assert_eq!(drained(&mut queue, 9), vec![70]);
        assert_eq!(queue.pending(), 0);
    }

    /// Extending a key can put it behind a later-pushed one; `retire` must scan
    /// the whole queue rather than stop at the first not-ready entry, or the
    /// ready successor behind the extended entry frees late.
    #[test]
    fn retire_frees_every_ready_entry_even_out_of_order() {
        let mut queue = RetireQueue::new();
        queue.push(5, 30_u32);
        queue.push(7, 50);
        // Extending X from 5 to 9 leaves it behind Y's 7, out of FIFO order.
        queue.extend_matching(9, |payload| *payload == 30_u32);
        assert_eq!(
            drained(&mut queue, 7),
            vec![50],
            "the ready successor behind an extended entry still frees promptly"
        );
        assert_eq!(queue.pending(), 1);
        assert_eq!(drained(&mut queue, 9), vec![30]);
        assert_eq!(queue.pending(), 0);
    }

    /// A held payload stays parked however far past its key the timeline has
    /// run — the recorded-but-not-yet-submitted command buffer, which
    /// references the payload while no submission exists to signal a value on
    /// its behalf. Its neighbours are unaffected, and releasing the hold frees
    /// it on the next pass without needing a new key or a new submission.
    #[test]
    fn a_held_payload_stays_parked_past_its_key() {
        let mut queue = RetireQueue::new();
        queue.push(1, 30_u32);
        queue.push(1, 50);
        let mut freed = Vec::new();
        queue.retire(
            9,
            |payload| *payload == 30_u32,
            |payload| freed.push(payload),
        );
        assert_eq!(freed, vec![50], "a payload nothing holds frees on its key");
        assert_eq!(
            queue.pending(),
            1,
            "the held payload is still parked at a key the timeline passed"
        );

        assert_eq!(
            drained(&mut queue, 9),
            vec![30],
            "and frees as soon as the hold is released"
        );
        assert_eq!(queue.pending(), 0);
    }

    /// Shutdown frees everything whatever the timeline says, because the caller
    /// has already waited for the device to go idle. The distinction matters:
    /// `retire` alone would leak on a device that was destroyed before its last
    /// submission's value was ever polled.
    #[test]
    fn shutdown_drains_regardless_of_the_timeline() {
        let mut queue = RetireQueue::new();
        queue.push(100, 1_u32);
        queue.push(200, 2);
        let mut freed = Vec::new();
        queue.drain_all(|payload| freed.push(payload));
        assert_eq!(freed, vec![1, 2]);
        assert_eq!(queue.pending(), 0);

        // And draining an empty queue is a no-op, not a panic.
        queue.drain_all(|_| panic!("nothing left"));
    }

    #[test]
    fn debug_output_names_the_backlog() {
        let mut queue: RetireQueue<u32> = RetireQueue::new();
        assert!(format!("{queue:?}").contains("pending: 0"));
        queue.push(4, 1);
        let text = format!("{queue:?}");
        assert!(text.contains("pending: 1"), "{text}");
        assert!(text.contains("Some(4)"), "{text}");
    }
}
