//! What a submission still needs alive, and the fence value that says when it
//! may go.
//!
//! # Why D3D12 needs this and `crcbl-mtl` does not
//!
//! An `MTLCommandBuffer` retains every resource it references, so Metal's
//! `destroy_*` releases its own reference and is done. **A D3D12 command list
//! retains nothing.** `ExecuteCommandLists` reads the resources the recorded
//! commands name through addresses the list captured at record time, so
//! releasing the last reference to one while that list is in flight is a
//! use-after-free — reported by the debug layer, and by nothing at all in a
//! release build. `crcbl-vk` reached the same conclusion about `VkBuffer`,
//! which is why it has `crcbl_vk::deletion`.
//!
//! # What is parked here, and why `destroy_buffer` still frees immediately
//!
//! The encoder takes its own reference to every resource it records against,
//! and [`Device::submit`](crcbl_hal::Device::submit) parks a clone of that set
//! here keyed on the fence value the submission signals. So a resource survives
//! because **the submission using it holds a reference**, not because
//! `destroy_buffer` deferred anything — which is why `crate::device`'s
//! `destroy_*` calls are unchanged and drop the pool's reference on the spot.
//!
//! That is the one place COM is genuinely easier than Vulkan.
//! `crcbl_vk::deletion` cannot work this way: a `VkBuffer` is a bare handle
//! with no refcount, so its queue keeps every destroyed object and re-keys it
//! against each submission's recorded raw handles. Here the refcount already
//! *is* the bookkeeping, so this queue holds one thing — the set a submission
//! needs — and holds it once.
//!
//! Alongside the resources it holds the `ID3D12GraphicsCommandList` and
//! `ID3D12CommandAllocator` the commands were recorded into.
//! `ExecuteCommandLists` does not retain those either, and
//! [`Device::destroy_command_buffer`](crcbl_hal::Device::destroy_command_buffer)
//! may be called while the list is still executing.
//!
//! # Keyed on a fence value, not a frame count
//!
//! "N frames later" is a heuristic; a fence value is a fact. Every submission
//! signals a monotonically increasing value on the device's one `ID3D12Fence`,
//! so a batch is safe to release once `GetCompletedValue` has reached the value
//! it was parked at. That stays correct when a frame issues two submissions or
//! none, which a frame counter does not.
//!
//! # The queue is FIFO by construction, so [`retire`](RetireQueue::retire) can
//! stop at the first entry that is not ready
//!
//! [`park`](RetireQueue::park) is only ever called with the value a submission
//! just reserved, and that value only increases. `crcbl_vk::deletion` has to
//! scan its whole queue because it re-keys existing entries to later values;
//! nothing here re-keys anything, so the front is always the oldest.
//!
//! # The policy is pure, so it is tested without a device
//!
//! [`RetireQueue`] is generic over its payload and names no D3D12 type. The
//! device parameterises it with an enum of interfaces whose release *is* their
//! `Drop`; the ordering rules — nothing goes before its value, everything goes
//! eventually, and a batch goes together — are unit tests over a payload that
//! records its own release.

use std::collections::VecDeque;

/// Payloads waiting for a fence value, oldest first.
#[derive(Debug)]
pub(crate) struct RetireQueue<T> {
    entries: VecDeque<(u64, T)>,
}

impl<T> RetireQueue<T> {
    /// An empty queue.
    pub(crate) const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }

    /// Holds `item` until the fence has reached `at`.
    pub(crate) fn park(&mut self, at: u64, item: T) {
        debug_assert!(
            self.entries.back().is_none_or(|&(back, _)| back <= at),
            "a submission's fence value only increases, so the queue is already in order"
        );
        self.entries.push_back((at, item));
    }

    /// How many payloads are still held. The device logs it when it leaks.
    pub(crate) fn pending(&self) -> usize {
        self.entries.len()
    }

    /// Releases everything the fence has passed.
    ///
    /// `completed` is `ID3D12Fence::GetCompletedValue`, so a payload parked at
    /// exactly that value is released: the fence reaching a value means the
    /// submission that signalled it has finished.
    pub(crate) fn retire(&mut self, completed: u64) {
        while let Some(&(at, _)) = self.entries.front() {
            if at > completed {
                break;
            }
            self.entries.pop_front();
        }
    }

    /// Releases everything, whatever the fence says.
    ///
    /// Correct only once the device is idle, which is the one place it is
    /// called from: `DeviceInner`'s `Drop`, after it has waited for the last
    /// value it handed out.
    pub(crate) fn drain_all(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A payload that records its own release.
    ///
    /// The release is the property under test and it is otherwise invisible: a
    /// queue that never freed anything and one that freed on time both leave
    /// `pending()` at zero once the run ends, so counting entries would pass
    /// either way.
    struct Probe {
        id: u32,
        released: Rc<RefCell<Vec<u32>>>,
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            self.released.borrow_mut().push(self.id);
        }
    }

    /// Hands out probes and reports what has been released, in order.
    struct Ledger {
        released: Rc<RefCell<Vec<u32>>>,
    }

    impl Ledger {
        fn new() -> Self {
            Self {
                released: Rc::new(RefCell::new(Vec::new())),
            }
        }

        fn probe(&self, id: u32) -> Probe {
            Probe {
                id,
                released: Rc::clone(&self.released),
            }
        }

        fn released(&self) -> Vec<u32> {
            self.released.borrow().clone()
        }
    }

    /// **The whole point.** Nothing is released before the fence has reached
    /// the value it was parked at — a queue that released early would be the
    /// use-after-free this module exists to prevent, and the symptom is a
    /// corrupted read on a machine nobody is watching.
    ///
    /// The value's own poll is asserted too, because that is the off-by-one
    /// that would leave every batch parked one submission too long: a leak
    /// rather than a crash, and therefore never noticed.
    #[test]
    fn nothing_is_released_before_the_fence_reaches_its_value() {
        let ledger = Ledger::new();
        let mut queue = RetireQueue::new();
        queue.park(3, ledger.probe(30));
        queue.park(5, ledger.probe(50));

        for completed in [0, 1, 2] {
            queue.retire(completed);
            assert!(
                ledger.released().is_empty(),
                "a payload was released at {completed}, before its fence value"
            );
            assert_eq!(queue.pending(), 2);
        }
        queue.retire(3);
        assert_eq!(ledger.released(), vec![30], "the value's own poll releases");
        assert_eq!(queue.pending(), 1);
        queue.retire(4);
        assert_eq!(ledger.released(), vec![30], "the later payload went early");
        queue.retire(9);
        assert_eq!(ledger.released(), vec![30, 50]);
        assert_eq!(queue.pending(), 0);
    }

    /// One submission's whole batch goes at once, and in the order it was
    /// parked.
    ///
    /// Order matters even though every payload here is only dropped: a command
    /// list is parked after the resources it names, so releasing back to front
    /// would drop the resources while the list still references them — the
    /// exact hazard the queue exists for, one step smaller.
    #[test]
    fn one_submissions_batch_is_released_together_and_in_park_order() {
        let ledger = Ledger::new();
        let mut queue = RetireQueue::new();
        for id in [1, 2, 3] {
            queue.park(7, ledger.probe(id));
        }
        queue.park(8, ledger.probe(4));

        queue.retire(7);
        assert_eq!(ledger.released(), vec![1, 2, 3]);
        assert_eq!(
            queue.pending(),
            1,
            "the next submission's batch went with this one"
        );
        queue.retire(8);
        assert_eq!(ledger.released(), vec![1, 2, 3, 4]);
    }

    /// Shutdown releases everything, because by then the device has been waited
    /// on and the fence will never advance again.
    #[test]
    fn shutdown_releases_everything_whatever_the_fence_says() {
        let ledger = Ledger::new();
        let mut queue = RetireQueue::new();
        queue.park(100, ledger.probe(1));
        queue.park(200, ledger.probe(2));
        queue.retire(0);
        assert!(ledger.released().is_empty(), "nothing is due yet");

        queue.drain_all();
        assert_eq!(ledger.released(), vec![1, 2]);
        assert_eq!(queue.pending(), 0);
        // And draining an empty queue is a no-op rather than a second release.
        queue.drain_all();
        assert_eq!(ledger.released(), vec![1, 2]);
    }
}
