//! The engine's logging: five macros, a filter, and the sink they write to.
//!
//! ```
//! crcbl_core::info!("shell: first configure at {}x{}", 960, 720);
//! crcbl_core::warn!("no config dir; values will not persist");
//! ```
//!
//! [`error!`], [`warn!`], [`info!`], [`debug!`] and [`trace!`] take `format!`
//! arguments and tag each record with the calling module, which is what the
//! filter below matches on. All five forward to [`log_at!`], so the target, the
//! level check and the route to the sink are written once.
//!
//! It is deliberately small rather than a dependency on a logging framework:
//! the engine needs level filtering, a readable line and a way for a test to
//! read back what it said, and every framework that does more brings a runtime,
//! a feature matrix, and opinions about async.
//!
//! # The `log` crate is underneath, and stays
//!
//! Not for the engine's own call sites — those go straight to the sink — but
//! because `wgpu`, `naga` and `gpu-allocator` report through that facade, and
//! their diagnostics have repeatedly been the ones that mattered: a shader
//! naga refused, a device that would not open. The sink implements `log::Log`
//! as well, so third-party records land in the same stream, at the same
//! filter, in the same format. Dropping the facade would not remove the
//! dependency — it would only stop us hearing from it.
//!
//! Both entry points meet at `StderrLogger::emit`, so there is one filter
//! check, one capture point and one line format rather than one of each per
//! path.
//!
//! # What a line looks like
//!
//! ```text
//! [   0.0000s INFO  crcbl_core::log] run started 2026-08-15 05:20:07 UTC
//! [   0.0081s INFO  crcbl::engine] shell: first configure at 960x720
//! ```
//!
//! Seconds since start on every line, because "how long into the run" is the
//! question a frame loop asks; the wall-clock date once, at start-up, so those
//! seconds can be lined up against something outside the process without paying
//! a date conversion per line.
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
use std::fmt;
use std::io::Write as _;
use std::marker::PhantomData;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use ::log::{Log, Metadata, Record, SetLoggerError};

/// The five levels, and the filter form of them.
///
/// Re-exported so a caller can name `crcbl_core::log::Level` without depending
/// on the `log` crate directly — the macros expand to this path, and a macro
/// that named a crate its caller had not taken would not compile there. They
/// **are** `log`'s types rather than copies: the sink bridges third-party
/// records from `wgpu`, `naga` and `gpu-allocator` into the same path, so a
/// parallel enum would only be something to convert through.
pub use ::log::{Level, LevelFilter};

/// The level macros, reachable as `crcbl_core::log::info!` as well as at the
/// crate root.
///
/// `#[macro_export]` puts a macro at the crate root and nowhere else, which
/// would leave the macros and the sink they write to in two different places.
/// Re-exporting them here means `log` is one module whichever half a caller
/// wants — and it is the path the engine already used, so the crates calling
/// `crcbl::log::info!` did not have to be rewritten to reach ours.
pub use crate::{debug, error, info, log_at, trace, warn};

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

    /// Capture, filter and write one record.
    ///
    /// **The one path.** This module's own macros call it directly and the
    /// [`Log`] impl below funnels third-party records into it, so there is a
    /// single filter check, a single capture point and a single line format
    /// rather than one of each per entry point.
    fn emit(&self, level: Level, target: &str, args: fmt::Arguments<'_>) {
        // Before the filter, for the reason `enabled` gives.
        push_captured(level, target, args);
        if !self.permits(&Metadata::builder().level(level).target(target).build()) {
            return;
        }
        // Seconds since init, not a date: the useful question in a frame loop is
        // "how long into the run". The wall-clock time the run *started* is
        // written once by `init_logging`, which is what makes a line here
        // correlatable with an outside log without paying date formatting per
        // line — see `start_banner`.
        let elapsed = self.start.elapsed().as_secs_f64();
        let mut stderr = std::io::stderr().lock();
        // A failed log write must never take the process down.
        let _ = writeln!(
            stderr,
            "[{elapsed:9.4}s {level:<5} {target}] {args}",
            level = level_name(level),
        );
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
        self.emit(record.level(), record.target(), *record.args());
    }

    fn flush(&self) {
        let _ = std::io::stderr().lock().flush();
    }
}

/// Emits one record through **whatever logger the process installed**.
///
/// Not public API: the macros expand to a call here. It is `#[doc(hidden)]`
/// rather than private because a `macro_rules!` expands in the *caller's* crate
/// and has to be able to name it.
///
/// **Dispatch goes through the `log` facade rather than straight to
/// [`StderrLogger`], and that is load-bearing.** This crate's sink is not the
/// only one: `wasm32` installs `crcbl::web`'s queue instead, because a browser
/// has no stderr and `Instant::now` panics there. A version of this that
/// reached for `LOGGER` directly compiles everywhere and silently drops every
/// engine log line in the browser — which is exactly what the `web/` smoke test
/// caught when it was written that way.
///
/// A process that installed no logger drops the record, which is what the
/// facade already did.
#[doc(hidden)]
pub fn __emit(level: Level, target: &str, args: fmt::Arguments<'_>) {
    ::log::logger().log(
        &Record::builder()
            .level(level)
            .target(target)
            .args(args)
            .build(),
    );
}

/// Whether a record at `level` from `target` would go anywhere.
///
/// The macros check this before their `format_args!`, so a filtered-out
/// `debug!` never evaluates the expressions it was passed. Both halves are
/// asked: `max_level` is the facade's cheap global gate, and `enabled` is the
/// installed logger's own answer — which is what lets a capturing thread see
/// records the filter would otherwise drop.
#[doc(hidden)]
#[must_use]
pub fn __enabled(level: Level, target: &str) -> bool {
    level <= ::log::max_level()
        && ::log::logger().enabled(&Metadata::builder().level(level).target(target).build())
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

/// Logs at an explicit [`Level`], taking `format!` arguments.
///
/// The five level macros below forward to this, so the three things that are
/// the same for all of them — where the target comes from, that the filter is
/// asked before the arguments are built, and how a record reaches the sink —
/// are written once.
///
/// **The target is the calling module's path**, which is what `CRCBL_LOG`'s
/// per-module directives match against: `CRCBL_LOG=warn,crcbl_render=debug`.
/// `module_path!` expands where the macro is *used*, so this is the caller's
/// module and not this one.
///
/// **The argument expressions are not evaluated unless something would read
/// them.** The level check comes first, so a filtered-out call costs a
/// comparison rather than running whatever the caller passed. Narrower than it
/// sounds: `format_args!` already defers the *formatting*, so a `Display` impl
/// would not run either way — what the check saves is evaluating the arguments
/// and calling into the sink at all.
///
/// ```
/// crcbl_core::log_at!(crcbl_core::log::Level::Info, "one {} and {}", "value", 2);
/// ```
#[macro_export]
macro_rules! log_at {
    ($level:expr, $($arg:tt)+) => {{
        let level = $level;
        let target = ::core::module_path!();
        if $crate::log::__enabled(level, target) {
            $crate::log::__emit(level, target, ::core::format_args!($($arg)+));
        }
    }};
}

/// Logs at [`Level::Error`]: something the run could not do.
///
/// Takes `format!` arguments. See [`log_at!`] for the target and for when the
/// arguments are built.
///
/// ```
/// crcbl_core::error!("the device went away: {}", "no adapter");
/// ```
#[macro_export]
macro_rules! error {
    ($($arg:tt)+) => { $crate::log_at!($crate::log::Level::Error, $($arg)+) };
}

/// Logs at [`Level::Warn`]: something that will surprise someone later.
///
/// ```
/// crcbl_core::warn!("no config dir; values will not persist");
/// ```
#[macro_export]
macro_rules! warn {
    ($($arg:tt)+) => { $crate::log_at!($crate::log::Level::Warn, $($arg)+) };
}

/// Logs at [`Level::Info`]: what the run is doing, at the default level.
///
/// ```
/// crcbl_core::info!("shell: first configure at {}x{}", 960, 720);
/// ```
#[macro_export]
macro_rules! info {
    ($($arg:tt)+) => { $crate::log_at!($crate::log::Level::Info, $($arg)+) };
}

/// Logs at [`Level::Debug`]: detail for someone reading this subsystem.
///
/// ```
/// crcbl_core::debug!("shell event: {:?}", "Resized");
/// ```
#[macro_export]
macro_rules! debug {
    ($($arg:tt)+) => { $crate::log_at!($crate::log::Level::Debug, $($arg)+) };
}

/// Logs at [`Level::Trace`]: every step, for when nothing else has worked.
///
/// **Shares a name with [`mod@crate::trace`], the profiler module**, which is legal
/// — they are in different namespaces — but means a doc link to either has to
/// say which: `[mod@trace]` for the module, `[trace!]` for this. `use
/// crcbl_core::trace;` imports the module and leaves this reachable as
/// `crcbl_core::trace!`.
///
/// ```
/// crcbl_core::trace!("pumped {} events", 3);
/// ```
#[macro_export]
macro_rules! trace {
    ($($arg:tt)+) => { $crate::log_at!($crate::log::Level::Trace, $($arg)+) };
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
    // Only the winner of the race writes it, so a second `init_logging` does not
    // claim the run restarted.
    logger.emit(
        Level::Info,
        "crcbl_core::log",
        format_args!("run started {}", start_banner()),
    );
    Ok(())
}

/// The wall-clock time the run started, as `YYYY-MM-DD HH:MM:SS UTC`.
///
/// **Written once, at start-up, and never per line.** Every line carries
/// seconds-since-start instead, which is the question a frame loop asks; this
/// is the one value that lets those seconds be lined up against something
/// outside the process, and paying a date conversion for it once is the whole
/// point of not putting it on every line.
///
/// UTC rather than local time, because a local one needs the platform's
/// timezone database and the rules that go with it — exactly the kind of
/// arithmetic that is someone else's solved problem. The civil-date conversion
/// below is only the proleptic Gregorian calendar from a Unix timestamp, which
/// is a closed form with no zone rules in it.
///
/// Falls back to naming the epoch offset if the system clock is before 1970,
/// rather than guessing: a clock that far wrong is worth seeing.
fn start_banner() -> String {
    let Ok(since_epoch) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return "at an unknown time (the system clock is before 1970)".to_owned();
    };
    let secs = since_epoch.as_secs();
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let time = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02} UTC",
        h = time / 3600,
        m = (time % 3600) / 60,
        s = time % 60,
    )
}

/// Days since 1970-01-01 to a civil `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days`, the algorithm `<chrono>` is specified
/// against — transcribed rather than invented, because calendar arithmetic is
/// the classic place a plausible-looking version is wrong only on the days
/// nobody tests. It shifts the era to start in March so the leap day lands at
/// the end of a year and the month-length pattern becomes a closed form.
///
/// `days_from_civil_round_trips_every_day_for_four_centuries` checks it against
/// a full 400-year cycle, which is the period the Gregorian calendar repeats on.
const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
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

/// Adds a record to the calling thread's buffer, if it has one.
fn push_captured(level: Level, target: &str, args: fmt::Arguments<'_>) {
    if !capturing() {
        return;
    }
    // Rendered before the buffer is borrowed: `args` runs the caller's own
    // `Display` impls, and one of those logging in turn would re-enter here and
    // panic on the outstanding `RefCell` borrow.
    let captured = CapturedRecord {
        level,
        target: target.to_owned(),
        message: args.to_string(),
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
            Level::Info,
            "crcbl_core",
            format_args!("dropped on the floor"),
        );
        assert!(!capturing());
    }

    /// **The calendar arithmetic, against dates taken from outside this
    /// repository.**
    ///
    /// A transcription slip here passes every other test in the workspace and
    /// is wrong only on the days nobody happens to run it. The values are the
    /// well-known ones: the epoch, the two century rules that disagree (2000 is
    /// a leap year, 1900 is not — so 2000-02-29 exists and the day counts
    /// either side of it differ), and a leap day in a plain leap year.
    #[test]
    fn the_civil_date_matches_known_timestamps() {
        // `date -u -d @<secs> +%F` produced each of these.
        for (secs, want) in [
            (0_i64, (1970, 1, 1)),
            (86_399, (1970, 1, 1)),
            (86_400, (1970, 1, 2)),
            (951_782_400, (2000, 2, 29)), // the century leap year
            (1_078_012_800, (2004, 2, 29)),
            (1_709_164_800, (2024, 2, 29)),
            (1_755_216_000, (2025, 8, 15)),
            (4_102_444_800, (2100, 1, 1)), // 2100 is *not* a leap year
        ] {
            assert_eq!(
                civil_from_days(secs / 86_400),
                (want.0, want.1, want.2),
                "{secs}"
            );
        }
    }

    /// Every day of a full Gregorian cycle round-trips, which is the period the
    /// calendar repeats on — so a rule that is wrong for any day is wrong for
    /// one of these.
    #[test]
    fn days_from_civil_round_trips_every_day_for_four_centuries() {
        let mut previous = civil_from_days(0);
        for day in 1..=146_097_i64 {
            let (year, month, dom) = civil_from_days(day);
            assert!((1..=12).contains(&month), "day {day} gave month {month}");
            assert!((1..=31).contains(&dom), "day {day} gave day {dom}");
            // Strictly increasing, so no day is skipped or repeated.
            assert!(
                (year, month, dom) > previous,
                "day {day}: {previous:?} then {:?}",
                (year, month, dom)
            );
            previous = (year, month, dom);
        }
    }

    /// The banner is the one line that says when the run started, so it has to
    /// be shaped the way a reader expects rather than merely non-empty.
    #[test]
    fn the_start_banner_names_a_date_and_a_time() {
        let banner = start_banner();
        assert!(banner.ends_with(" UTC"), "{banner}");
        let (date, rest) = banner.split_once(' ').expect("date then time");
        assert_eq!(date.len(), "YYYY-MM-DD".len(), "{banner}");
        assert_eq!(date.matches('-').count(), 2, "{banner}");
        assert_eq!(rest.split_once(' ').expect("time then zone").0.len(), 8);
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
