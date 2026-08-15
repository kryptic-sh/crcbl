//! The device's out-of-band error channel.
//!
//! WebGPU does not report a creation failure to the call that made it. A
//! pipeline whose shader will not compile is still returned as an object, and
//! the reason is delivered later to the device's error handler — so a backend
//! that only reads return values believes it built everything it asked for.
//! That is not a theoretical concern here: the run that found the UI shader's
//! uniformity error submitted 384 invalid command buffers while reporting a
//! healthy status and a page that said "Playing", and the only symptom was a
//! black canvas.
//!
//! [`ErrorSink`] is where those reports land. It does two jobs, and they are
//! deliberately different:
//!
//! * **Attribution.** [`ErrorSink::since`] answers "did anything fail *during*
//!   this call?" by comparing counts, so [`crate::device::WgpuDevice`] can turn
//!   a synchronous failure — which is what `wgpu-core` produces on native — into
//!   a plain `Err` from `create_shader_module`. An error raised before the call
//!   is not attributed to it.
//! * **Reporting.** [`ErrorSink::take`] drains whatever is left for
//!   [`crcbl_hal::Device::take_error`], which is how the asynchronous half — the
//!   browser's `uncapturederror` event, arriving on a later turn of the event
//!   loop — reaches a caller at all.
//!
//! Every message is also logged at `error` the moment it arrives, because the
//! first thing anyone does with a black canvas is read the console.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Errors the driver reported outside the call that caused them.
///
/// Deliberately `std::sync` rather than [`crate::cell`]'s target-polymorphic
/// pair: wgpu's `on_uncaptured_error` takes an `Arc<dyn Fn(..) + Send + Sync>`
/// on every target, including wasm32, so this one type cannot be `Rc<RefCell<_>>`
/// there.
#[derive(Debug, Default)]
pub(crate) struct ErrorSink {
    /// Total ever recorded. Monotonic, and the basis of [`Self::since`] —
    /// a counter answers "did a new one arrive" without the ambiguity of
    /// comparing message text.
    count: AtomicU64,
    /// The most recent message, until somebody takes it.
    last: Mutex<Option<String>>,
}

impl ErrorSink {
    /// Records one error: logs it, counts it, and keeps its text.
    ///
    /// A second error before the first is taken replaces it. The count still
    /// grows, so nothing is silently lost — the *number* is exact even when
    /// only the newest text survives, which is the same trade `crcbl-vk`'s
    /// `ValidationSink` makes.
    pub(crate) fn record(&self, message: String) {
        crcbl_core::log::error!("crcbl-wgpu: the device reported an error: {message}");
        self.count.fetch_add(1, Ordering::Relaxed);
        *self.last.lock().unwrap() = Some(message);
    }

    /// How many errors have ever been recorded.
    pub(crate) fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// The newest error, if the count has grown past `mark`, taking it.
    ///
    /// `mark` is [`Self::count`] read before the call being guarded. An error
    /// that arrived earlier belongs to whatever caused it and is left for
    /// [`Self::take`] rather than blamed on this call.
    pub(crate) fn since(&self, mark: u64) -> Option<String> {
        if self.count() <= mark {
            return None;
        }
        self.take()
    }

    /// The newest error, if any, clearing it.
    pub(crate) fn take(&self) -> Option<String> {
        self.last.lock().unwrap().take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two readers do not tread on each other: what a guarded call claims,
    /// the frame-level drain no longer reports.
    #[test]
    fn an_error_is_reported_once() {
        let sink = ErrorSink::default();
        let mark = sink.count();
        sink.record("shader would not compile".to_string());

        assert_eq!(
            sink.since(mark).as_deref(),
            Some("shader would not compile")
        );
        assert_eq!(sink.take(), None, "taking it twice would double-report it");
    }

    /// An error that arrived before a call is not that call's fault. Without
    /// this, the first failure in a run would be re-reported by every
    /// subsequent pipeline creation and every one of them would look broken.
    #[test]
    fn a_call_is_only_blamed_for_errors_raised_during_it() {
        let sink = ErrorSink::default();
        sink.record("an earlier failure".to_string());

        let mark = sink.count();
        assert_eq!(sink.since(mark), None, "nothing failed during this call");
        assert_eq!(
            sink.take().as_deref(),
            Some("an earlier failure"),
            "and the earlier one is still there to be reported"
        );
    }

    /// The count is exact even when only the newest message survives, so a
    /// storm reports as a storm rather than as one error.
    #[test]
    fn every_error_is_counted_even_though_only_the_newest_is_kept() {
        let sink = ErrorSink::default();
        for i in 0..8 {
            sink.record(format!("failure {i}"));
        }
        assert_eq!(sink.count(), 8);
        assert_eq!(sink.take().as_deref(), Some("failure 7"));
    }
}
