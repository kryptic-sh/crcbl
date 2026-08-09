//! Logging setup.
//!
//! The engine logs through the [`log`] facade. This module supplies the
//! one thing a facade needs and does not provide: a sink. It is deliberately a
//! small `log::Log` implementation writing to stderr rather than a
//! dependency on a logging framework — the engine needs level filtering and a
//! readable line, and every framework that does more brings a runtime,
//! a feature matrix, and opinions about async.
//!
//! [`capture`] is the second thing that sink does: it hands a test the records
//! the engine emitted, so a log line that is the only evidence of a decision
//! can be asserted on instead of trusted. See its docs for the concurrency
//! argument.
//!
//! Filtering is `env_logger`-style, read from `CRCBL_LOG`:
//!
//! ```text
//! CRCBL_LOG=info                          # global level
//! CRCBL_LOG=warn,crcbl_render=debug       # global level plus per-module overrides
//! CRCBL_LOG=off,crcbl_vk=trace            # silence everything but one module
//! ```
//!
//! A directive's target matches a module path prefix at `::` boundaries, and
//! the **longest** matching directive wins, so `crcbl_render=debug` and
//! `crcbl_render::graph=trace` compose the way you would expect.

use std::cell::RefCell;
use std::env;
use std::io::Write as _;
use std::marker::PhantomData;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use ::log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};

/// Environment variable holding the filter directives.
pub const ENV_VAR: &str = "CRCBL_LOG";

/// Filter used when `CRCBL_LOG` is unset or empty.
pub const DEFAULT_FILTER: &str = "info";

/// A parsed set of filter directives.
#[derive(Clone, Debug)]
pub struct Filter {
    default: LevelFilter,
    /// Module-path prefix → level, longest prefix first.
    targets: Vec<(String, LevelFilter)>,
}

impl Filter {
    /// Parses a comma-separated directive list.
    ///
    /// Each directive is either a bare level (setting the default) or
    /// `target=level`. Unparseable directives are skipped rather than fatal: a
    /// typo in an environment variable must not stop the engine from starting.
    #[must_use]
    pub fn parse(directives: &str) -> Self {
        // Anything not named by a directive keeps the default level; a filter
        // string of only overrides ("crcbl_vk=trace") must not silence the
        // rest of the engine.
        let mut filter = Self {
            default: parse_level(DEFAULT_FILTER).unwrap_or(LevelFilter::Info),
            targets: Vec::new(),
        };
        for directive in directives.split(',') {
            let directive = directive.trim();
            if directive.is_empty() {
                continue;
            }
            match directive.split_once('=') {
                Some((target, level)) => {
                    if let Some(level) = parse_level(level.trim()) {
                        filter.targets.push((target.trim().to_owned(), level));
                    }
                }
                None => {
                    if let Some(level) = parse_level(directive) {
                        filter.default = level;
                    }
                }
            }
        }
        // Longest prefix first, so `find` below picks the most specific match.
        filter
            .targets
            .sort_by_key(|(target, _)| core::cmp::Reverse(target.len()));
        filter
    }

    /// Reads the filter from `CRCBL_LOG`, falling back to [`DEFAULT_FILTER`].
    #[must_use]
    pub fn from_env() -> Self {
        match env::var(ENV_VAR) {
            Ok(value) if !value.trim().is_empty() => Self::parse(&value),
            _ => Self::parse(DEFAULT_FILTER),
        }
    }

    /// The level in force for `target`.
    #[must_use]
    pub fn level_for(&self, target: &str) -> LevelFilter {
        self.targets
            .iter()
            .find(|(prefix, _)| target_matches(target, prefix))
            .map_or(self.default, |(_, level)| *level)
    }

    /// The loosest level any target can produce — what the `log` facade needs
    /// for its global fast path.
    #[must_use]
    pub fn max_level(&self) -> LevelFilter {
        self.targets
            .iter()
            .map(|(_, level)| *level)
            .chain([self.default])
            .max()
            .unwrap_or(LevelFilter::Off)
    }
}

impl Default for Filter {
    fn default() -> Self {
        Self::parse(DEFAULT_FILTER)
    }
}

/// Whether `target` is `prefix` or a module below it.
fn target_matches(target: &str, prefix: &str) -> bool {
    target.starts_with(prefix)
        && (target.len() == prefix.len() || target[prefix.len()..].starts_with("::"))
}

fn parse_level(text: &str) -> Option<LevelFilter> {
    match text.to_ascii_lowercase().as_str() {
        "off" | "none" => Some(LevelFilter::Off),
        "error" => Some(LevelFilter::Error),
        "warn" | "warning" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

/// The stderr logger installed by [`init_logging`].
#[derive(Debug)]
struct StderrLogger {
    filter: Filter,
    start: Instant,
}

impl StderrLogger {
    /// Whether the filter admits `metadata`. This, and only this, decides what
    /// reaches stderr — [`Log::enabled`] below widens for capture, and a record
    /// let through on that account must still not be printed.
    fn permits(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }
}

impl Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        // A capturing thread sees everything, whatever `CRCBL_LOG` says: a test
        // asserting the engine said something must not turn on the developer's
        // environment. Off this thread — which is every thread in a normal run
        // — the answer is the filter's, unchanged.
        self.permits(metadata) || capturing()
    }

    fn log(&self, record: &Record<'_>) {
        // Before the filter, for the reason `enabled` gives.
        push_captured(record);
        if !self.permits(record.metadata()) {
            return;
        }
        // Seconds since init rather than a wall-clock date: the useful question
        // in a frame loop is "how long into the run", and it needs no time
        // formatting dependency.
        let elapsed = self.start.elapsed().as_secs_f64();
        let mut stderr = std::io::stderr().lock();
        // A failed log write must never take the process down.
        let _ = writeln!(
            stderr,
            "[{elapsed:9.4}s {level:<5} {target}] {args}",
            level = level_name(record.level()),
            target = record.target(),
            args = record.args(),
        );
    }

    fn flush(&self) {
        let _ = std::io::stderr().lock().flush();
    }
}

const fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "ERROR",
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    }
}

/// The process-wide logger.
///
/// A `static` rather than a boxed logger so this module works against `log`'s
/// default features (`set_boxed_logger` needs the crate's `std` feature) and
/// leaks nothing.
static LOGGER: OnceLock<StderrLogger> = OnceLock::new();

/// Installs the stderr logger using the filter from `CRCBL_LOG`.
///
/// Idempotent and never fatal: if a logger is already installed (a host
/// application, a test harness, a second call) this is a no-op returning
/// `false`. Call it once at startup; anything that *needs* to know whether it
/// won the race can use [`try_init_logging`].
pub fn init_logging() -> bool {
    try_init_logging(Filter::from_env()).is_ok()
}

/// Installs the stderr logger with an explicit filter.
///
/// # Errors
///
/// If a logger is already installed for the process — including by an earlier
/// call to this function, whose filter stays in force.
pub fn try_init_logging(filter: Filter) -> Result<(), SetLoggerError> {
    let logger = LOGGER.get_or_init(|| StderrLogger {
        filter,
        start: Instant::now(),
    });
    ::log::set_logger(logger)?;
    ::log::set_max_level(logger.filter.max_level());
    Ok(())
}

/// Whether this module installed the process logger.
#[must_use]
pub fn is_installed() -> bool {
    LOGGER.get().is_some()
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

/// One record the installed logger saw, reduced to the three things an
/// assertion is written against.
///
/// The message is rendered here rather than kept as `Arguments`, which borrows
/// from the stack frame of the `log!` that built it and cannot outlive the call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedRecord {
    /// The level the record was logged at.
    pub level: Level,
    /// The record's target — the module path, unless the call overrode it.
    pub target: String,
    /// The formatted message.
    pub message: String,
}

/// Target of the record [`capture`] logs to prove the capture is wired up.
const PROBE_TARGET: &str = "crcbl_core::log::capture";

/// Serialises the *start* of a capture, and nothing else.
///
/// Starting one installs the logger, raises the facade's global maximum and
/// then probes it, and those three steps are not atomic. Two threads starting a
/// capture at the same moment both call [`init_logging`], and the one that
/// loses the `set_logger` race can still be inside [`try_init_logging`] — whose
/// last act is to set the maximum to *its* filter, putting it back under the
/// other thread's probe. The probe then never arrives and the assertion below
/// fires on code that is perfectly fine. Seen as a 1-in-50 failure of this
/// crate's own capture tests under a thread-per-test runner, which is exactly
/// the flake this mechanism exists to avoid producing.
///
/// Held across the setup only. Captures themselves run concurrently: their
/// buffers are thread-local and share nothing.
static CAPTURE_SETUP: Mutex<()> = Mutex::new(());

thread_local! {
    /// The calling thread's capture buffer, `None` when it is not capturing.
    ///
    /// **Thread-local, not global, and that is the whole design.** Tests in one
    /// binary run concurrently on many threads by default, so a single shared
    /// buffer would interleave two tests' records and make both assertions
    /// depend on the scheduler. The alternatives were a global buffer behind a
    /// mutex the API forces every capturing test to hold — which serialises the
    /// suite and deadlocks the moment a test panics with it held — or making
    /// capture a separate `log::Log` registration, which cannot work at all:
    /// `log::set_logger` takes the process slot once and [`init_logging`] has
    /// already taken it. Scoping to the thread costs nothing, needs no lock,
    /// and is exactly right for a record logged synchronously by the code under
    /// test. Its one limit is stated on [`capture`]: work the test hands to
    /// another thread logs somewhere this cannot see.
    static CAPTURED: RefCell<Option<Vec<CapturedRecord>>> = const { RefCell::new(None) };
}

/// Whether the calling thread is capturing.
fn capturing() -> bool {
    CAPTURED.with(|slot| slot.borrow().is_some())
}

/// Adds `record` to the calling thread's buffer, if it has one.
fn push_captured(record: &Record<'_>) {
    if !capturing() {
        return;
    }
    // Rendered before the buffer is borrowed: `record.args()` runs the caller's
    // own `Display` impls, and one of those logging in turn would re-enter here
    // and panic on the outstanding `RefCell` borrow.
    let captured = CapturedRecord {
        level: record.level(),
        target: record.target().to_owned(),
        message: record.args().to_string(),
    };
    CAPTURED.with(|slot| {
        if let Some(buffer) = slot.borrow_mut().as_mut() {
            buffer.push(captured);
        }
    });
}

/// Records logged by the calling thread, for as long as this value lives.
///
/// From [`capture`]. Neither `Send` nor `Sync`: the buffer is the *thread's*,
/// and dropping this on another thread would stop that thread's capture while
/// leaving this one's running for the rest of the process.
#[derive(Debug)]
pub struct Capture {
    _not_send: PhantomData<*const ()>,
}

impl Capture {
    /// A copy of everything captured so far, oldest first.
    ///
    /// Copies rather than drains, so an assertion that fails can be followed by
    /// one that prints the whole buffer.
    #[must_use]
    pub fn records(&self) -> Vec<CapturedRecord> {
        CAPTURED.with(|slot| slot.borrow().clone().unwrap_or_default())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        CAPTURED.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Starts capturing the records **this thread** logs.
///
/// For a log line that is the only evidence a decision was taken — a capability
/// downgrade, a pacing resolution — so that deleting the line turns a test red
/// instead of nothing at all.
///
/// Capture is additive: stderr still gets whatever `CRCBL_LOG` admits, and a
/// process that never calls this behaves exactly as before.
///
/// # What it does and does not see
///
/// Everything the calling thread logs, at every level, regardless of
/// `CRCBL_LOG` — a test must not pass or fail on the developer's environment.
/// It sees nothing logged by any *other* thread, so a test that hands work to a
/// worker and asserts on what the worker logged needs a different mechanism
/// than this one.
///
/// # Panics
///
/// If this thread is already capturing — nesting would leave it ambiguous which
/// buffer a record belongs to — or if the capture cannot be proven to work,
/// which this checks by logging a record of its own and looking for it. That
/// second one means the process slot belongs to a logger that is not this
/// module's, or that `log`'s `release_max_level_*` features compiled the record
/// away before it was ever offered to a sink.
///
/// ```
/// let logs = crcbl_core::log::capture();
/// log::info!("the device does not have wings");
///
/// let records = logs.records();
/// assert_eq!(records.len(), 1);
/// assert_eq!(records[0].level, log::Level::Info);
/// assert_eq!(records[0].message, "the device does not have wings");
/// ```
#[must_use]
pub fn capture() -> Capture {
    // See `CAPTURE_SETUP` for what this excludes. The poison is stepped over
    // rather than propagated: this function panics on purpose below, and a test
    // that hit one of those must not take every later capture down with it.
    let _setup = CAPTURE_SETUP.lock().unwrap_or_else(PoisonError::into_inner);

    // Nothing reaches a sink until there is one. Idempotent, and the return
    // value is deliberately unused: another crate's harness may have installed
    // this module's logger already, and the probe below is what decides whether
    // capture actually works — not who won this race.
    let _ = init_logging();

    CAPTURED.with(|slot| {
        let mut slot = slot.borrow_mut();
        assert!(
            slot.is_none(),
            "this thread is already capturing log records"
        );
        *slot = Some(Vec::new());
    });

    // The facade's global maximum drops a record before any logger is asked, so
    // capture has to lift it. Raised and never lowered: a `Drop` that put it
    // back would race a live capture on another thread, and raising it changes
    // no output, because the write to stderr is gated on the filter and not on
    // this.
    if ::log::max_level() < LevelFilter::Trace {
        ::log::set_max_level(LevelFilter::Trace);
    }

    let capture = Capture {
        _not_send: PhantomData,
    };

    // Prove the wiring instead of assuming it. If some other logger owns the
    // process slot this capture is silently empty forever, and every "the
    // engine said nothing" assertion written against it passes vacuously.
    ::log::trace!(target: PROBE_TARGET, "capture probe");
    let wired = CAPTURED.with(|slot| {
        let mut slot = slot.borrow_mut();
        let buffer = slot.as_mut().expect("this thread's capture was just set");
        let wired = buffer.iter().any(|record| record.target == PROBE_TARGET);
        // The probe is plumbing, not evidence; the caller must not have to
        // filter it out of every assertion.
        buffer.clear();
        wired
    });
    assert!(
        wired,
        "log capture is not wired up: the process logger is not crcbl-core's, \
         or this level was compiled out"
    );

    capture
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_level_sets_the_default() {
        let filter = Filter::parse("debug");
        assert_eq!(filter.level_for("anything"), LevelFilter::Debug);
        assert_eq!(filter.max_level(), LevelFilter::Debug);
    }

    #[test]
    fn empty_directives_fall_back_to_the_default_filter() {
        assert_eq!(Filter::parse("").level_for("crcbl_core"), LevelFilter::Info);
        assert_eq!(Filter::default().level_for("crcbl_core"), LevelFilter::Info);
    }

    #[test]
    fn per_target_overrides_apply_to_submodules_only() {
        let filter = Filter::parse("warn,crcbl_render=debug");
        assert_eq!(filter.level_for("crcbl_core"), LevelFilter::Warn);
        assert_eq!(filter.level_for("crcbl_render"), LevelFilter::Debug);
        assert_eq!(filter.level_for("crcbl_render::graph"), LevelFilter::Debug);
        // Prefix match must respect module boundaries.
        assert_eq!(filter.level_for("crcbl_renderer"), LevelFilter::Warn);
        assert_eq!(filter.max_level(), LevelFilter::Debug);
    }

    #[test]
    fn the_longest_matching_directive_wins() {
        let filter = Filter::parse("off,crcbl_render=warn,crcbl_render::graph=trace");
        assert_eq!(filter.level_for("crcbl_vk"), LevelFilter::Off);
        assert_eq!(filter.level_for("crcbl_render"), LevelFilter::Warn);
        assert_eq!(filter.level_for("crcbl_render::graph"), LevelFilter::Trace);
        assert_eq!(
            filter.level_for("crcbl_render::graph::pass"),
            LevelFilter::Trace
        );
        assert_eq!(filter.max_level(), LevelFilter::Trace);
    }

    #[test]
    fn overrides_without_a_bare_level_keep_the_default() {
        let filter = Filter::parse("crcbl_vk=trace");
        assert_eq!(filter.level_for("crcbl_core"), LevelFilter::Info);
        assert_eq!(filter.level_for("crcbl_vk"), LevelFilter::Trace);
    }

    #[test]
    fn nonsense_directives_are_ignored_not_fatal() {
        let filter = Filter::parse("info,,  ,crcbl_vk=louder,=warn,bogus");
        assert_eq!(filter.level_for("crcbl_core"), LevelFilter::Info);
        assert_eq!(filter.level_for("crcbl_vk"), LevelFilter::Info);
        assert_eq!(
            filter.level_for(""),
            LevelFilter::Warn,
            "an empty target matches everything"
        );
    }

    #[test]
    fn level_aliases_and_case_are_accepted() {
        let filter = Filter::parse("WARNING,crcbl_net=NONE,crcbl_ui=Error");
        assert_eq!(filter.level_for("x"), LevelFilter::Warn);
        assert_eq!(filter.level_for("crcbl_net"), LevelFilter::Off);
        assert_eq!(filter.level_for("crcbl_ui"), LevelFilter::Error);
    }

    #[test]
    fn logger_respects_the_filter_without_being_installed() {
        let logger = StderrLogger {
            filter: Filter::parse("warn,noisy=trace"),
            start: Instant::now(),
        };
        assert!(
            logger.enabled(
                &Metadata::builder()
                    .level(Level::Error)
                    .target("any")
                    .build()
            )
        );
        assert!(!logger.enabled(&Metadata::builder().level(Level::Info).target("any").build()));
        assert!(
            logger.enabled(
                &Metadata::builder()
                    .level(Level::Trace)
                    .target("noisy")
                    .build()
            )
        );
        logger.log(
            &Record::builder()
                .args(format_args!("hello"))
                .target("noisy")
                .build(),
        );
        logger.flush();
    }

    /// Installs the process logger. Each nextest test runs in its own process,
    /// so this does not fight the other tests over the global slot.
    #[test]
    fn installing_the_logger_is_idempotent() {
        assert!(!is_installed());
        assert!(try_init_logging(Filter::parse("trace")).is_ok());
        assert!(is_installed());
        assert_eq!(::log::max_level(), LevelFilter::Trace);

        ::log::info!("crcbl-core log smoke test");
        ::log::logger().flush();

        // A second install loses the race but must not panic.
        assert!(try_init_logging(Filter::parse("off")).is_err());
        assert!(!init_logging());
        assert_eq!(
            ::log::max_level(),
            LevelFilter::Trace,
            "the first filter stays in force"
        );
    }

    /// Nothing is captured until someone asks, and the sink must not pay for
    /// the feature on the way past.
    #[test]
    fn a_thread_that_never_asked_captures_nothing() {
        assert!(!capturing());
        push_captured(
            &Record::builder()
                .args(format_args!("dropped on the floor"))
                .target("crcbl_core")
                .build(),
        );
        assert!(!capturing());
    }

    #[test]
    fn level_names_are_padded_consistently() {
        for level in [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ] {
            assert!(level_name(level).len() <= 5);
        }
    }
}
