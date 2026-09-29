//! The command-pool free list: what lets a command encoder reset a
//! `VkCommandPool` it is handed rather than create one.
//!
//! # Why a pool is kept at all
//!
//! Creating a pool per encoder — a pool per frame — was the largest CPU cost
//! measured on the recording path: `vkCreateCommandPool` itself, and a driver
//! cost the fresh pool deferred into the frame's first large
//! `vkCmdBeginRendering`. `docs/backlog.md`'s performance review has the
//! figures. A pool whose command buffer has been retired is therefore kept
//! here, and the next encoder of the same queue family resets it with
//! `vkResetCommandPool` instead.
//!
//! # Reset only once the GPU has finished with it
//!
//! A pool comes back with the retire-timeline value of the last submission
//! that used it, or zero if it never reached a queue, and
//! [`take`](PoolFreeList::take) hands out only a pool whose value the timeline
//! has reached. The seam already says `destroy_command_buffer` arrives after
//! the submission completed, but this does not lean on that: a caller that
//! breaks the rule gets a freshly created pool rather than a
//! `vkResetCommandPool` under a submission still executing, which is undefined
//! behaviour (`VUID-vkResetCommandPool-commandPool-00040`).
//!
//! # Bounded
//!
//! At most [`CAPACITY`] pools wait here. [`keep`](PoolFreeList::keep) hands a
//! pool back when the list is full, and the device parks it in the deletion
//! queue, which frees it once the timeline says it may.
//!
//! # Pure, so the rules are tested without a GPU
//!
//! The list is generic over its payload and knows nothing about Vulkan — the
//! same split as [`RetireQueue`](crate::deletion::RetireQueue).

/// The most pools the free list keeps.
///
/// The backend cannot see how many frames its caller keeps in flight or how
/// many encoders a frame opens, so this is sized for a frame loop's worth
/// several times over: a steady loop only ever holds the pools of the frames
/// it has retired and not yet begun again. What does not fit is freed, not
/// lost — see [`PoolFreeList::keep`].
pub(crate) const CAPACITY: usize = 8;

/// Kept pools, each with the queue family it was created on and the timeline
/// value it may be reset at.
#[derive(Debug)]
pub(crate) struct PoolFreeList<T> {
    /// Oldest first, so [`take`](Self::take) prefers the pool most likely to
    /// be finished with.
    kept: Vec<Kept<T>>,
}

#[derive(Debug)]
struct Kept<T> {
    family: u32,
    /// The retire-timeline value of the last submission that used the pool;
    /// zero for one that never reached a queue.
    ready_at: u64,
    payload: T,
}

impl<T> Default for PoolFreeList<T> {
    fn default() -> Self {
        Self { kept: Vec::new() }
    }
}

impl<T> PoolFreeList<T> {
    /// How many pools are waiting.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.kept.len()
    }

    /// Keeps `payload`, a pool of `family` that may be reset once the timeline
    /// reaches `ready_at`.
    ///
    /// # Errors
    ///
    /// Hands `payload` back when [`CAPACITY`] pools are already kept, for the
    /// caller to free once the timeline allows.
    pub(crate) fn keep(&mut self, family: u32, ready_at: u64, payload: T) -> Result<(), T> {
        if self.kept.len() >= CAPACITY {
            return Err(payload);
        }
        self.kept.push(Kept {
            family,
            ready_at,
            payload,
        });
        Ok(())
    }

    /// Takes the oldest kept pool of `family` the GPU has finished with.
    ///
    /// `completed` reads the retire timeline. It is called at most once, and
    /// only when a candidate was ever submitted — a pool that never reached a
    /// queue needs no clock — so a caller whose read fails can answer zero
    /// and still reuse those.
    pub(crate) fn take(&mut self, family: u32, completed: impl FnOnce() -> u64) -> Option<T> {
        let mut clock = Some(completed);
        let mut known = 0;
        let index = self.kept.iter().position(|kept| {
            if kept.family != family {
                return false;
            }
            if kept.ready_at > known
                && let Some(read) = clock.take()
            {
                known = read();
            }
            kept.ready_at <= known
        })?;
        Some(self.kept.remove(index).payload)
    }

    /// Hands back every kept pool, regardless of the timeline.
    ///
    /// Only correct once the device is idle, which is where it is called: the
    /// device's teardown, after its final `vkDeviceWaitIdle`.
    pub(crate) fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.kept.drain(..).map(|kept| kept.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAPHICS: u32 = 0;
    const COMPUTE: u32 = 1;

    /// The whole point: a pool is not handed out before the submission that
    /// last used it has completed.
    #[test]
    fn a_pool_is_not_reused_before_its_submission_completes() {
        let mut list = PoolFreeList::default();
        list.keep(GRAPHICS, 5, 'a').expect("room");
        assert_eq!(list.take(GRAPHICS, || 4), None, "submission 5 is running");
        assert_eq!(list.len(), 1, "a refused take keeps the pool");
        assert_eq!(list.take(GRAPHICS, || 5), Some('a'));
        assert_eq!(list.len(), 0);
    }

    /// A pool that never reached a queue — an encoder dropped unfinished, or a
    /// command buffer destroyed unsubmitted — is ready at once, without asking
    /// the timeline.
    #[test]
    fn a_never_submitted_pool_needs_no_clock() {
        let mut list = PoolFreeList::default();
        list.keep(GRAPHICS, 0, 'a').expect("room");
        assert_eq!(
            list.take(GRAPHICS, || unreachable!("no candidate was submitted")),
            Some('a')
        );
    }

    /// The clock is read once per take however many candidates it covers, and
    /// the oldest ready pool is the one handed out.
    #[test]
    fn the_clock_is_read_once_and_the_oldest_ready_pool_wins() {
        let mut list = PoolFreeList::default();
        list.keep(GRAPHICS, 9, 'a').expect("room");
        list.keep(GRAPHICS, 3, 'b').expect("room");
        list.keep(GRAPHICS, 2, 'c').expect("room");
        let mut reads = 0;
        let taken = list.take(GRAPHICS, || {
            reads += 1;
            3
        });
        assert_eq!(
            taken,
            Some('b'),
            "'a' is still running and 'b' is older than 'c'"
        );
        assert_eq!(reads, 1);
    }

    /// Vulkan requires a pool's family to match the queue its buffers are
    /// submitted to, so a pool never crosses families.
    #[test]
    fn a_pool_stays_in_its_queue_family() {
        let mut list = PoolFreeList::default();
        list.keep(COMPUTE, 0, 'a').expect("room");
        assert_eq!(list.take(GRAPHICS, || u64::MAX), None);
        assert_eq!(list.take(COMPUTE, || u64::MAX), Some('a'));
    }

    /// Past [`CAPACITY`] a pool is handed back for the caller to free, rather
    /// than kept or dropped.
    #[test]
    fn the_list_is_bounded_and_hands_back_what_does_not_fit() {
        let mut list = PoolFreeList::default();
        for index in 0..CAPACITY {
            list.keep(GRAPHICS, 0, index).expect("room");
        }
        assert_eq!(list.keep(GRAPHICS, 0, CAPACITY), Err(CAPACITY));
        assert_eq!(list.len(), CAPACITY);
        let mut drained: Vec<usize> = list.drain().collect();
        drained.sort_unstable();
        assert_eq!(drained, (0..CAPACITY).collect::<Vec<_>>());
        assert_eq!(list.len(), 0);
    }

    /// A frame loop with frames in flight, the GPU lagging the CPU: each frame
    /// takes a pool or creates one and submits at the next timeline value. The
    /// pool goes back the moment it is submitted — earlier than the seam
    /// allows, so the timeline alone decides when it may be reset. No pool is
    /// handed out while a submission that used it still runs, and the pools in
    /// circulation stay bounded by the frames in flight.
    #[test]
    fn a_frame_loop_recycles_a_bounded_set_of_pools() {
        const IN_FLIGHT: u64 = 2;
        const FRAMES: u64 = 200;

        let mut list = PoolFreeList::default();
        // Each pool's last submission, indexed by pool.
        let mut last_used: Vec<u64> = Vec::new();
        for value in 1..=FRAMES {
            // Submissions after `completed` are still running.
            let completed = value.saturating_sub(1 + IN_FLIGHT);
            let pool = list.take(GRAPHICS, || completed).unwrap_or_else(|| {
                last_used.push(0);
                last_used.len() - 1
            });
            assert!(
                last_used[pool] <= completed,
                "pool {pool} was handed out at frame {value} while its submission {} runs",
                last_used[pool]
            );
            last_used[pool] = value;
            list.keep(GRAPHICS, value, pool).expect("room");
        }
        assert!(
            last_used.len() as u64 <= IN_FLIGHT + 1,
            "{} pools created for {IN_FLIGHT} frames in flight",
            last_used.len()
        );
    }
}
