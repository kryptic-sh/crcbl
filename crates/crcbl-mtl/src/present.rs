//! The count a Metal present wait is answered from.
//!
//! Metal has neither a number for a present nor an object to block on. What it
//! has is `MTLDrawable::addPresentedHandler:`, a block that runs once the
//! drawable has been shown — the third shape
//! [`Features::PRESENT_FEEDBACK`](crcbl_hal::Features::PRESENT_FEEDBACK) names,
//! "one only calls back once a drawable has been shown". So the number in
//! [`PresentInfo::present_id`](crcbl_hal::PresentInfo::present_id) has to be
//! kept on this side of the seam, and matched against what the handler reports.
//!
//! [`PresentLedger`] is that record, and it is deliberately **plain Rust**: two
//! `u64`s under a `Mutex`, a `Condvar` to sleep on, and not one Objective-C
//! type. Every decision the capability makes lives here — which ids may be
//! waited for, which are answered at once, what a lapsed timeout returns, how a
//! blocked wait is woken — so all of it is exercised by `cargo test` on any
//! host. That matters more here than it usually would: the
//! `addPresentedHandler:` half needs a window server and is covered by no
//! automated test on any machine, which `docs/backlog.md` states as a gap. What
//! *can* be checked without a drawable is in this file so that it is.
//!
//! That is also why this module alone is `#[cfg(any(target_os = "macos",
//! test))]` while the rest of the backend is macOS-only: off macOS it exists in
//! the test build and nowhere else, purely so those assertions run.
//!
//! # The ledger is per swapchain, and resets with it
//!
//! One [`PresentLedger`] belongs to one `SwapchainTarget::Layer`, so a
//! reconfigure — which replaces the whole entry with a freshly built one —
//! restarts the numbering by construction, exactly as `crcbl-vk`'s
//! `SwapchainEntry::presented_id` does. A handler still in flight from the old
//! swapchain keeps its own `Arc` to the old ledger and reports into it; nothing
//! consults that ledger again, and nothing dangles.
//!
//! An offscreen ring has no ledger at all, because it has no drawable to
//! observe — `SwapchainTarget::Offscreen` carries no field for one, which is
//! what makes the seam's immediate answer for a ring structural rather than a
//! branch someone could delete.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use crcbl_hal::SurfaceError;

/// What a [`PresentLedger::wait_until_shown`] turned out to be.
///
/// The distinction is not for the caller's control flow — both are the seam's
/// `Ok(())` — but for its **log**. A backend that advertises the capability and
/// then answers every wait immediately is indistinguishable from one that
/// paces, because the frame time is the same either way: `displaySyncEnabled`
/// already holds the loop to the refresh rate on its own. So whether a wait
/// ever genuinely blocked is something only this side can report, and this is
/// what it reports with. `crcbl-vk` keeps the same fact for the same reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PresentWait {
    /// The id named a frame this swapchain was never given, so there was
    /// nothing to block on and nothing was learnt about the display.
    NothingToWaitFor,
    /// The frame is up: the presented handler had run, or ran while this call
    /// was asleep on it.
    Shown,
}

/// The two numbers, under the one lock that keeps them ordered.
#[derive(Debug, Default)]
struct Counts {
    /// The highest id this swapchain has taken for a present it went on to
    /// commit. An id above it names a frame the swapchain was never given.
    committed: u64,
    /// The highest id whose drawable's presented handler has run.
    shown: u64,
}

/// What `Device::wait_until_presented` is answered from on Metal.
///
/// # Sound to touch from the thread the handler runs on
///
/// The presented block is called by Core Animation on a thread this crate does
/// not choose, so an `Arc<PresentLedger>` captured by that block is shared
/// across threads. It is sound because [`Mutex`] and [`Condvar`] are `Send +
/// Sync` and [`Counts`] is two `u64`s — so `PresentLedger: Send + Sync` is
/// **derived by the compiler**, not claimed by an `unsafe impl`, and the block
/// carries nothing else but a `u64`. There is no interior mutability outside
/// the mutex and no Objective-C object in the type at all.
#[derive(Debug, Default)]
pub(crate) struct PresentLedger {
    counts: Mutex<Counts>,
    /// Notified whenever [`PresentLedger::record_shown`] moves `shown` up.
    /// `notify_all`, not `notify_one`: nothing stops two threads waiting on the
    /// same swapchain, and waking the wrong one would leave the other asleep
    /// until its timeout.
    woken: Condvar,
}

impl PresentLedger {
    /// Takes `present_id` as the number for the present about to be committed,
    /// and says whether it was taken.
    ///
    /// `false` means the present goes out unnumbered and nothing will be able
    /// to wait for it, which is the seam's "no record of this id" case and
    /// answers immediately. Two ids get that answer:
    ///
    /// * **Zero**, which numbers nothing. The seam's id is an `Option` so that
    ///   no sentinel is needed, but a caller can still pass `Some(0)`, and
    ///   `committed` starts at 0 — treating it as a real id would make an
    ///   untouched ledger claim to have presented something.
    /// * **One that does not follow the last**, which the seam calls a caller
    ///   bug: ids must strictly increase across a swapchain's presents. The
    ///   answer is to refuse the number rather than to move `committed`
    ///   backwards, because a `committed` that went down would strand every
    ///   wait for an id between the two.
    pub(crate) fn record_present(&self, present_id: u64) -> bool {
        let mut counts = self.counts();
        if present_id == 0 || present_id <= counts.committed {
            return false;
        }
        counts.committed = present_id;
        true
    }

    /// The presented handler's end: `present_id`'s drawable has been shown.
    ///
    /// `max`, so a report that arrives out of order cannot walk the count
    /// backwards and re-block a wait that has already been answered. Presents
    /// on one queue are shown in commit order and this should not arise, which
    /// is the reason to make it cheap rather than the reason to leave it out.
    pub(crate) fn record_shown(&self, present_id: u64) {
        let mut counts = self.counts();
        counts.shown = counts.shown.max(present_id);
        drop(counts);
        self.woken.notify_all();
    }

    /// Blocks until `present_id`'s drawable has been shown, `timeout` lapses,
    /// or the id turns out to be one this swapchain was never given.
    ///
    /// The last of those is the seam's documented immediate `Ok(())`, and it is
    /// not a rare case: a present can be refused after the caller has already
    /// spent an id, and a reconfigure builds a ledger that has seen none of the
    /// old ones. Blocking for the whole timeout on a frame that will never
    /// arrive is the worst of the answers available.
    ///
    /// An id **below** `committed` that was never itself taken — a caller that
    /// skipped a number — waits for the next id that was, since `shown` only
    /// ever passes it. That is later than the truth and never earlier, which is
    /// the safe direction for a wait whose promise is "no longer waiting to
    /// happen".
    ///
    /// The returned [`PresentWait`] says which of the two happened, so a caller
    /// can log that a wait genuinely reached a drawable; both are the seam's
    /// `Ok(())`.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::Timeout`] when `timeout` lapsed with the frame still not
    /// up. The seam calls that expected traffic on a stalled compositor rather
    /// than a reason to abandon the frame.
    pub(crate) fn wait_until_shown(
        &self,
        present_id: u64,
        timeout: Duration,
    ) -> Result<PresentWait, SurfaceError> {
        let counts = self.counts();
        if present_id == 0 || present_id > counts.committed {
            return Ok(PresentWait::NothingToWaitFor);
        }
        let (_counts, outcome) = self
            .woken
            .wait_timeout_while(counts, timeout, |counts| counts.shown < present_id)
            .unwrap_or_else(PoisonError::into_inner);
        if outcome.timed_out() {
            return Err(SurfaceError::Timeout);
        }
        Ok(PresentWait::Shown)
    }

    /// The lock, poisoning tolerated the way the device's own state lock
    /// tolerates it: a panic elsewhere must not turn every later frame into a
    /// second panic, and two counters have no invariant a poisoned guard could
    /// have left half-written.
    fn counts(&self) -> std::sync::MutexGuard<'_, Counts> {
        self.counts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    /// Long enough that a wait which ignored the ledger and returned at once
    /// would be caught by the elapsed-time assertion, short enough not to
    /// lengthen the suite noticeably.
    const HANDLER_DELAY: Duration = Duration::from_millis(40);

    /// Generous, because the tests that use it expect the wait to be *woken*
    /// rather than to lapse — a slow machine must not turn a pass into a
    /// timeout.
    const GENEROUS: Duration = Duration::from_secs(10);

    /// How soon after the handler a woken wait must return.
    ///
    /// Far above [`HANDLER_DELAY`] so a loaded machine cannot trip it, and far
    /// below [`GENEROUS`] because that gap is the assertion: a wait nobody
    /// notified does not fail, it comes back after `GENEROUS` and reports
    /// success anyway.
    const PROMPTLY: Duration = Duration::from_secs(2);

    /// **The seam's immediate answer, which is the case that costs a whole
    /// timeout when it is missing.**
    ///
    /// An id above `committed` names a frame this swapchain was never given: a
    /// present that failed after the caller spent the id, or any id at all
    /// before the first present. Nothing will ever show it, so the wait has to
    /// return rather than sleep.
    ///
    /// Red when `wait_until_shown`'s `present_id > counts.committed` guard is
    /// dropped: every assertion below then blocks for `GENEROUS` and comes back
    /// `Err(Timeout)`.
    #[test]
    fn an_id_this_metal_swapchain_was_never_given_is_answered_at_once() {
        let ledger = PresentLedger::default();
        let started = Instant::now();
        let never = ledger
            .wait_until_shown(1, GENEROUS)
            .expect("nothing has been presented, so there is nothing to wait for");
        assert_eq!(never, PresentWait::NothingToWaitFor);
        let zero = ledger
            .wait_until_shown(0, GENEROUS)
            .expect("zero numbers nothing");
        assert_eq!(zero, PresentWait::NothingToWaitFor);

        assert!(ledger.record_present(1));
        assert!(ledger.record_present(2));
        let ahead = ledger
            .wait_until_shown(3, GENEROUS)
            .expect("the frame after the last present has not been presented");
        assert_eq!(
            ahead,
            PresentWait::NothingToWaitFor,
            "an id past the last present must not be reported as a real wait"
        );

        assert!(
            started.elapsed() < HANDLER_DELAY,
            "none of those may block; they took {:?}",
            started.elapsed()
        );
    }

    /// **A wait genuinely sleeps until the handler reports, and is woken by
    /// it.**
    ///
    /// This is the one assertion that covers the mechanism rather than the
    /// bookkeeping: a `Condvar` that is never notified, or a wait that returns
    /// before the frame is up, both look identical in a type check. The thread
    /// stands in for Core Animation, which is also what makes this the
    /// cross-thread exercise the handler's soundness argument rests on.
    ///
    /// Red when `wait_until_shown` returns without waiting: `reported` is still
    /// false and the elapsed time is under `HANDLER_DELAY`.
    ///
    /// Red, through the *upper* bound, when `record_shown` stops calling
    /// `notify_all` — and that bound is there because the obvious assertions
    /// are not enough. A wait nobody notifies still returns `Ok(Shown)`:
    /// `Condvar::wait_timeout_while` re-tests its condition after the timeout
    /// lapses, finds `shown` has moved, and reports **not** timed out. So a
    /// missing wake-up costs the caller a whole `PRESENT_WAIT_TIMEOUT` per
    /// frame and produces no error anywhere. Measured: with the `notify_all`
    /// removed, every assertion but this one still passed and the suite merely
    /// took ten seconds longer.
    #[test]
    fn a_wait_sleeps_until_the_presented_handler_reports() {
        let ledger = Arc::new(PresentLedger::default());
        assert!(ledger.record_present(7));

        let reported = Arc::new(AtomicBool::new(false));
        // **Before the spawn, not after.** The handler's sleep starts the moment
        // the thread runs, which is somewhere inside `spawn` — so a clock
        // started afterwards is already behind it, and the lower bound below can
        // miss by however long the spawn took. CI caught that at 39.932915ms
        // against a 40ms delay: sixty-seven microseconds of slack, and a flake
        // that says nothing about the mechanism under test.
        let started = Instant::now();
        let handler = {
            let ledger = Arc::clone(&ledger);
            let reported = Arc::clone(&reported);
            std::thread::spawn(move || {
                std::thread::sleep(HANDLER_DELAY);
                reported.store(true, Ordering::SeqCst);
                ledger.record_shown(7);
            })
        };

        let outcome = ledger
            .wait_until_shown(7, GENEROUS)
            .expect("the handler reports well inside the timeout");
        assert_eq!(
            outcome,
            PresentWait::Shown,
            "a wait that reached a drawable must say so, or the log cannot tell \
             a closed loop from an immediate answer"
        );
        assert!(
            reported.load(Ordering::SeqCst),
            "the wait returned before anything was shown"
        );
        assert!(
            started.elapsed() >= HANDLER_DELAY,
            "the wait returned in {:?}, sooner than the handler could have run",
            started.elapsed()
        );
        assert!(
            started.elapsed() < PROMPTLY,
            "the wait took {:?} to notice a report that arrived after {HANDLER_DELAY:?}; \
             it was not woken, it timed out and re-checked",
            started.elapsed()
        );
        handler.join().expect("the handler thread");
    }

    /// A frame that never comes up is a lapsed timeout, not a success.
    ///
    /// The seam makes [`SurfaceError::Timeout`] expected traffic on a stalled
    /// compositor — the caller renders anyway — so the thing that must not
    /// happen is `Ok(())`, which would tell a frame loop the display had caught
    /// up when it had not.
    ///
    /// Red when the `outcome.timed_out()` arm returns `Ok(())`.
    #[test]
    fn a_frame_that_never_comes_up_times_out_rather_than_succeeding() {
        let ledger = PresentLedger::default();
        assert!(ledger.record_present(1));

        let timeout = Duration::from_millis(50);
        let started = Instant::now();
        let error = ledger
            .wait_until_shown(1, timeout)
            .expect_err("nothing ever showed present 1");
        assert!(matches!(error, SurfaceError::Timeout), "{error:?}");
        // Nine tenths rather than the whole timeout: `Condvar::wait_timeout` is
        // documented as blocking for *roughly* the duration asked for, and a
        // test that demanded the exact figure would be asserting on the
        // platform's timer granularity. The point of the bound is only that the
        // call slept rather than returning an instant `Timeout`.
        assert!(
            started.elapsed() >= timeout * 9 / 10,
            "it gave up after {:?}, which is not a timeout of {timeout:?}",
            started.elapsed()
        );
    }

    /// **An id that does not follow the last is refused rather than
    /// renumbering the swapchain.**
    ///
    /// The seam requires ids to strictly increase and calls a repeat or a
    /// going-backwards one a caller bug. Letting `committed` move down would be
    /// worse than ignoring it: every id between the old and new values would
    /// then be reported as never presented, and waits for frames that really
    /// are on their way would return at once.
    ///
    /// Red when `record_present` drops the `present_id <= counts.committed`
    /// guard (id 5 is accepted twice and id 3 walks `committed` back to 3, so
    /// the last assertion finds present 5 unwaitable).
    #[test]
    fn a_metal_present_id_that_does_not_follow_the_last_is_refused() {
        let ledger = PresentLedger::default();
        assert!(!ledger.record_present(0), "zero numbers nothing");
        assert!(ledger.record_present(5));
        assert!(!ledger.record_present(5), "a repeat is a caller bug");
        assert!(!ledger.record_present(3), "so is going backwards");
        assert!(ledger.record_present(6), "and the next real id still works");

        // `committed` is 6 after all that, so 5 is still a frame this ledger
        // was given and a wait for it must block rather than answer.
        ledger.record_shown(6);
        let outcome = ledger
            .wait_until_shown(5, GENEROUS)
            .expect("present 6 being up means present 5 is no longer waiting to happen");
        assert_eq!(outcome, PresentWait::Shown);
    }

    /// A report that arrives out of order must not re-block a wait that the
    /// previous one already answered.
    ///
    /// Red when `record_shown` assigns instead of taking the maximum: `shown`
    /// drops to 1 and the wait for 3 sits out `GENEROUS`.
    #[test]
    fn an_out_of_order_report_never_walks_the_count_backwards() {
        let ledger = PresentLedger::default();
        for id in 1..=3 {
            assert!(ledger.record_present(id));
        }
        ledger.record_shown(3);
        ledger.record_shown(1);

        let started = Instant::now();
        for id in 1..=3 {
            let outcome = ledger
                .wait_until_shown(id, GENEROUS)
                .expect("present 3 is up, so every earlier one is too");
            assert_eq!(outcome, PresentWait::Shown, "present {id}");
        }
        assert!(
            started.elapsed() < HANDLER_DELAY,
            "nothing there should have blocked; it took {:?}",
            started.elapsed()
        );
    }

    /// A rebuilt swapchain remembers none of the old one's presents.
    ///
    /// `reconfigure_swapchain` replaces the whole entry, and with it the
    /// ledger, so this pins the property that makes that correct: the engine's
    /// own counter does **not** restart, so the first id asked of a fresh
    /// ledger is a large one, and it has to be answered rather than waited for.
    ///
    /// Red when `Counts::default` is given a non-zero `committed`, and red for
    /// any future ledger that carried state across a reconfigure.
    #[test]
    fn a_fresh_ledger_answers_every_id_the_old_metal_layer_had_presented() {
        let old = PresentLedger::default();
        for id in 1..=100 {
            assert!(old.record_present(id));
        }
        old.record_shown(100);

        let rebuilt = PresentLedger::default();
        let started = Instant::now();
        assert_eq!(
            rebuilt
                .wait_until_shown(100, GENEROUS)
                .expect("this object never presented 100"),
            PresentWait::NothingToWaitFor
        );
        assert_eq!(
            rebuilt
                .wait_until_shown(101, GENEROUS)
                .expect("nor 101, which is where the engine's counter has got to"),
            PresentWait::NothingToWaitFor
        );
        assert!(
            started.elapsed() < HANDLER_DELAY,
            "a rebuilt ledger must answer at once; it took {:?}",
            started.elapsed()
        );
    }

    /// **The soundness argument for handing this to an Objective-C block,
    /// checked by the compiler rather than asserted in a comment.**
    ///
    /// The presented handler runs on a thread Core Animation chooses, so the
    /// `Arc<PresentLedger>` it captures crosses a thread boundary and is then
    /// touched from both sides at once. That is exactly `Send + Sync`, and it
    /// holds here because every field is — no `unsafe impl` anywhere.
    ///
    /// Red at compile time the moment a field with interior mutability outside
    /// the mutex, a raw pointer, or a `Retained<_>` of an unmarked Objective-C
    /// protocol is added to the ledger — which is the change that would make
    /// the handler unsound while leaving every runtime test above green.
    #[test]
    fn the_ledger_is_sound_to_share_with_a_block_that_runs_on_another_thread() {
        const fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PresentLedger>();
        assert_send_sync::<Arc<PresentLedger>>();
    }
}
