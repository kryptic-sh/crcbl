//! The spawn seam: one trait, and the backends that can and cannot serve it.

use core::fmt;
use core::num::NonZeroUsize;

/// Work a backend may put on a thread of its own.
///
/// Boxed because [`Spawn`] is used as a trait object — a subsystem stores the
/// spawner it was built with and does not care which backend it got, which is
/// the point of the seam.
pub type Work = Box<dyn FnOnce() + Send + 'static>;

/// How a thread gets started, on a runtime that may not have any.
///
/// See the [crate docs](crate) for why this exists rather than
/// `std::thread::spawn`. Implementors are `Send + Sync` because a spawner is
/// shared: the topology hands the same one to every subsystem, and a subsystem
/// on a worker thread spawns its own children through it.
pub trait Spawn: Send + Sync + fmt::Debug {
    /// Whether this runtime can start a thread at all.
    ///
    /// **Ask this once, while building a subsystem, and pick a shape from the
    /// answer** — a long-lived loop on its own thread, or the same work driven
    /// from the caller's tick. A [`spawn`](Self::spawn) that fails afterwards
    /// is a runtime that broke its word, not a platform to degrade around, and
    /// the closure is gone by then in any case.
    fn threaded(&self) -> bool;

    /// How many workers a data-parallel pool should size itself to.
    ///
    /// The machine's answer, not the pool's: subtracting the pinned pipeline
    /// threads is the pool's business. One means run inline — which is both a
    /// single-core machine and a runtime with no threads at all, and a
    /// `par_for` cannot tell those apart nor need to.
    fn parallelism(&self) -> NonZeroUsize;

    /// Starts `work` on a thread of its own.
    ///
    /// `name` is what the platform's debugger and the profiler will show, so it
    /// names the subsystem (`"sim"`, `"io"`) rather than the call site.
    ///
    /// # Errors
    ///
    /// [`SpawnError::NoThreads`] if [`threaded`](Self::threaded) is false — a
    /// caller that asked first never sees it. [`SpawnError::Os`] if the
    /// platform refused; `work` is consumed either way and does not come back,
    /// which is why the degradation decision belongs at
    /// [`threaded`](Self::threaded) and not here.
    fn spawn(&self, name: &'static str, work: Work) -> Result<(), SpawnError>;
}

/// Why a thread did not start.
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    /// This runtime has no threads, so `{0}` has to run on the caller's own.
    ///
    /// Reaching this means [`Spawn::threaded`] was not asked, or was asked and
    /// ignored.
    #[error("this runtime has no threads, so {0} must run on the caller's own")]
    NoThreads(&'static str),
    /// The platform refused a thread it was expected to give.
    #[error("the platform refused a thread for {name}")]
    Os {
        /// The name the thread would have had.
        name: &'static str,
        /// What the platform said.
        #[source]
        source: std::io::Error,
    },
}

/// A spawner with no threads behind it: every [`Spawn::spawn`] is refused.
///
/// **This is the browser today**, and it is what a consumer degrades onto
/// rather than a stub standing in for something missing: the design's rule is
/// that pipeline stages collapse into sequential calls in the tick driver and
/// `par_for` runs inline, so a runtime that answers `false` to
/// [`Spawn::threaded`] is fully served — just serially.
///
/// It is also the honest backend for a test that wants the single-threaded
/// path, which is half of what the `--threads 1` vs `--threads N` determinism
/// gate compares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inline;

impl Spawn for Inline {
    fn threaded(&self) -> bool {
        false
    }

    fn parallelism(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }

    fn spawn(&self, name: &'static str, work: Work) -> Result<(), SpawnError> {
        // Dropped rather than run: `work` is a subsystem's whole loop as often
        // as it is a finite task, and running one here would never return.
        drop(work);
        Err(SpawnError::NoThreads(name))
    }
}

/// A spawner over `std::thread`.
///
/// **Absent on `wasm32` on purpose.** `std::thread::spawn` compiles there and
/// returns `UNSUPPORTED_PLATFORM` at run time (see the [crate docs](crate)), so
/// a browser build that could name this type would compile clean and fail on
/// the first thread. Not being nameable is what turns that into a compile
/// error, and [`default_spawner`] is how a consumer picks a backend without
/// knowing which target it is on.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Threads;

#[cfg(not(target_arch = "wasm32"))]
impl Spawn for Threads {
    fn threaded(&self) -> bool {
        true
    }

    fn parallelism(&self) -> NonZeroUsize {
        // `Err` here is a platform that will not say — a container with no
        // affinity mask it can read, most often. One worker is the answer that
        // is never wrong, only sometimes slow.
        std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
    }

    fn spawn(&self, name: &'static str, work: Work) -> Result<(), SpawnError> {
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(work)
            // The handle is dropped, which detaches: the seam has no join, and
            // the design says so — "one-shot results: poll or continuation, no
            // join-on-main". A subsystem that has to know when its thread
            // finished says so through a mailbox, like every other
            // cross-thread fact.
            .map(drop)
            .map_err(|source| SpawnError::Os { name, source })
    }
}

/// The backend for the target this was compiled for.
///
/// The one place `cfg(target_arch)` is written for the whole threading model:
/// native gets `Threads`, wasm32 gets [`Inline`] until the worker backend
/// lands behind this same seam. A consumer calls this and asks
/// [`Spawn::threaded`]; nothing above here knows which target it is on.
#[must_use]
pub fn default_spawner() -> Box<dyn Spawn> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Box::new(Threads)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Box::new(Inline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// **A spawned closure runs, and it runs somewhere else.**
    ///
    /// The second half is the one worth asserting: a backend that called the
    /// closure inline would satisfy every "did it run" check and be a
    /// different thing entirely, and it is exactly what [`Inline`] would be if
    /// it ran the work instead of refusing it.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_thread_runs_the_work_somewhere_that_is_not_here() {
        let (tx, rx) = mpsc::channel();
        let here = std::thread::current().id();

        Threads
            .spawn(
                "test-worker",
                Box::new(move || {
                    tx.send(std::thread::current().id())
                        .expect("the test waits");
                }),
            )
            .expect("a native thread");

        let there = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the work never ran");
        assert_ne!(there, here, "the work ran on the calling thread");
    }

    /// The name reaches the thread, because it is what a debugger and the
    /// profiler's timeline will show — a lane labelled `<unnamed>` is the whole
    /// value of the argument lost.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_thread_wears_the_name_it_was_given() {
        let (tx, rx) = mpsc::channel();

        Threads
            .spawn(
                "sim",
                Box::new(move || {
                    let named = std::thread::current().name().map(str::to_owned);
                    tx.send(named).expect("the test waits");
                }),
            )
            .expect("a native thread");

        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5))
                .expect("the work never ran"),
            Some("sim".to_owned()),
        );
    }

    /// **`Inline` refuses, and says which thread it refused** — the message is
    /// the only thing a caller that ignored `threaded()` has to go on.
    #[test]
    fn inline_refuses_by_name_and_never_runs_the_work() {
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&ran);

        let error = Inline
            .spawn(
                "sim",
                Box::new(move || flag.store(true, std::sync::atomic::Ordering::SeqCst)),
            )
            .expect_err("Inline has no threads to give");

        assert!(matches!(error, SpawnError::NoThreads("sim")));
        assert!(
            !ran.load(std::sync::atomic::Ordering::SeqCst),
            "a refused spawn ran the work anyway, which is the deadlock this \
             seam exists to prevent for a subsystem loop",
        );
        assert!(error.to_string().contains("sim"), "{error}");
    }

    /// The two questions a consumer asks at startup, answered consistently:
    /// a backend with no threads has no parallelism either, and one with
    /// threads reports at least itself.
    #[test]
    fn the_startup_questions_agree_with_each_other() {
        assert!(!Inline.threaded());
        assert_eq!(Inline.parallelism(), NonZeroUsize::MIN);

        let spawner = default_spawner();
        assert_eq!(
            spawner.threaded(),
            cfg!(not(target_arch = "wasm32")),
            "the default backend must match what the target can actually do",
        );
        if !spawner.threaded() {
            assert_eq!(
                spawner.parallelism(),
                NonZeroUsize::MIN,
                "a runtime with no threads cannot have workers to run on",
            );
        }
    }

    /// The seam is used as a trait object — a subsystem stores whichever
    /// backend it was handed. A signature that stopped being object-safe would
    /// break every consumer, so it fails here first.
    #[test]
    fn the_seam_is_object_safe_and_shareable() {
        fn assert_shared<T: Send + Sync + ?Sized>() {}
        assert_shared::<dyn Spawn>();

        let backends: Vec<Box<dyn Spawn>> = vec![Box::new(Inline), default_spawner()];
        assert_eq!(backends.len(), 2);
    }
}
