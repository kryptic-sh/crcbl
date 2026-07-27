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
//! eventually, and the queue drains in submission order — are unit tests.

use core::fmt;
use std::collections::VecDeque;

/// A FIFO of payloads waiting for the device timeline to reach a value.
///
/// The queue is ordered by construction: [`push`](Self::push) is only ever
/// called with a non-decreasing `retire_at`, because the value is the
/// submission counter, which only goes up. [`retire`](Self::retire) therefore
/// stops at the first entry that is not ready instead of scanning the whole
/// queue.
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

    /// How many payloads are still parked.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.entries.len()
    }

    /// Hands `destroy` every payload whose `retire_at` the timeline has reached.
    ///
    /// `completed` is the timeline's current value. Entries are visited in
    /// submission order and the scan stops at the first one that is not ready,
    /// so a long-lived object cannot hold up its successors' *ordering* — only
    /// its own retirement, which is the correct behaviour.
    pub fn retire(&mut self, completed: u64, mut destroy: impl FnMut(T)) {
        while let Some((retire_at, _)) = self.entries.front() {
            if *retire_at > completed {
                break;
            }
            let (_, payload) = self
                .entries
                .pop_front()
                .unwrap_or_else(|| unreachable!("the queue is non-empty above"));
            destroy(payload);
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

    fn drained(queue: &mut RetireQueue<u32>, completed: u64) -> Vec<u32> {
        let mut out = Vec::new();
        queue.retire(completed, |payload| out.push(payload));
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
