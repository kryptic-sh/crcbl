//! What value a submission's semaphore wait and signal actually carry.
//!
//! # One rule, and D3D12 cannot report it
//!
//! An `ID3D12Fence` is a monotonic counter, and `ID3D12CommandQueue::Signal`
//! will set one **backwards** without complaint: no `HRESULT`, no debug-layer
//! message, nothing in any log. What happens instead is that every waiter past
//! the higher value stops waking, on a queue that is otherwise healthy — which
//! is the shape of bug that survives a green run and is found weeks later on
//! somebody else's machine.
//!
//! So the rule lives here rather than at the call site, and it is checked
//! against the highest value **submitted** onto that semaphore rather than the
//! highest it has reached: a `Signal` sits in the queue until the GPU gets to
//! it, so two submissions in flight would otherwise be free to take the same
//! number, and the second waiter would be satisfied by the first submission's
//! work.
//!
//! [`signal_value`] also carries the intra-submission half, which is the one
//! that is easy to miss: two signals on the same semaphore in one `SubmitInfo`
//! have to be compared with each other, not only with what the semaphore
//! already holds.
//!
//! # Not Windows-only, and that is the point
//!
//! This module holds no `windows` type — it is `u64` arithmetic and one
//! [`HalError`] — so off Windows it exists in the test build alone and
//! `cargo test` on any host checks it, exactly as [`crate::present`] and
//! [`crate::root`] are compiled for. The alternative would be arithmetic
//! reachable only from a machine that reports nothing when it is wrong.

use crcbl_hal::HalError;

/// The value a wait carries.
///
/// A **binary** semaphore's `value` field is ignored per the seam, so the wait
/// is for the value most recently submitted onto it — which is what makes a
/// binary semaphore mean "the thing an earlier submission signalled" rather
/// than a number the caller chose. A **timeline**'s is taken as given,
/// including one nothing has signalled yet: `ID3D12CommandQueue::Wait` is
/// D3D12's way of expressing ordering, and a real fence blocks until somebody
/// signals it.
pub(crate) const fn wait_value(timeline: bool, submitted: u64, requested: u64) -> u64 {
    if timeline { requested } else { submitted }
}

/// The value a signal carries, or why the submission is refused.
///
/// `floor` is the highest value already submitted onto this semaphore,
/// including by an earlier signal in the same `SubmitInfo`.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a timeline signal that does not exceed
/// `floor` — the value is a field the caller can correct, which is why it is
/// not [`HalError::Unsupported`].
pub(crate) fn signal_value(timeline: bool, floor: u64, requested: u64) -> Result<u64, HalError> {
    if !timeline {
        // A binary semaphore's counter is this crate's own bookkeeping, so the
        // next integer is always available and the caller's value is ignored.
        return Ok(floor + 1);
    }
    if requested <= floor {
        return Err(HalError::InvalidDescriptor(format!(
            "a timeline semaphore signalled with {requested} has already been submitted with \
             {floor}; an ID3D12Fence only moves forwards and a waiter on the higher value would \
             never wake"
        )));
    }
    Ok(requested)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A timeline wait is the caller's number and a binary one is this crate's.
    ///
    /// Red if the two arms are swapped, which is the failure that reads
    /// correctly: a binary wait for the caller's `value` — usually zero —
    /// would be satisfied by a fence that has never been signalled at all, so
    /// the submission would run before the work it was ordered behind.
    #[test]
    fn a_binary_wait_ignores_the_callers_value_and_a_timeline_wait_uses_it() {
        assert_eq!(wait_value(true, 3, 9), 9, "a timeline takes the request");
        // Including a value nothing has submitted: that is the wait-before-signal
        // D3D12 permits and `Capability::TimelineWaitBeforeSignal` names.
        assert_eq!(wait_value(true, 3, 100), 100);
        assert_eq!(
            wait_value(false, 3, 9),
            3,
            "a binary wait must be for what was last submitted onto it, not for the value the \
             seam says is ignored"
        );
        // And zero is the case that would look like success: a binary semaphore
        // nothing has signalled yet waits for zero, which every fence already
        // holds — so the ordering is the caller's to arrange, per the seam.
        assert_eq!(wait_value(false, 0, 7), 0);
    }

    /// **A timeline signal must move forwards, and equality is not forwards.**
    ///
    /// The `<=` is the whole check. Written as `<` it would accept a repeat of
    /// the value the semaphore already carries, which is the silent deadlock
    /// this module exists for: the second waiter is woken by the first
    /// submission's work and reads a buffer nothing has written.
    #[test]
    fn a_timeline_signal_must_exceed_everything_already_submitted() {
        assert_eq!(signal_value(true, 4, 5).expect("above the floor"), 5);
        assert_eq!(signal_value(true, 0, 1).expect("above zero"), 1);
        for repeated in [4, 3, 0] {
            let error = signal_value(true, 4, repeated).expect_err("not forwards");
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "signalling {repeated} over 4: {error:?}"
            );
            // The message has to name both numbers, or a caller reading it
            // cannot tell which of its submissions took the value first.
            let text = error.to_string();
            assert!(
                text.contains(&repeated.to_string()) && text.contains('4'),
                "{text}"
            );
        }
    }

    /// A binary signal takes the next integer, whatever the caller asked for.
    ///
    /// Red if the caller's value leaks through: the seam says it is ignored,
    /// and two submissions passing the same `value` would then encode the same
    /// number twice and the second wait would be satisfied by the first
    /// submission.
    #[test]
    fn a_binary_signal_takes_the_next_integer_and_never_the_callers() {
        assert_eq!(signal_value(false, 0, 0).expect("binary"), 1);
        assert_eq!(signal_value(false, 7, 0).expect("binary"), 8);
        assert_eq!(
            signal_value(false, 7, 7).expect("binary"),
            8,
            "a binary signal repeated the value it was given"
        );
        // Two in a row, which is what the intra-submission floor produces.
        let first = signal_value(false, 2, 0).expect("binary");
        assert_eq!(signal_value(false, first, 0).expect("binary"), first + 1);
    }
}
