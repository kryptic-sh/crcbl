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
//! [`console`] is the third: a bounded ring every sink pushes into, which is
//! what the debug console draws. It holds records the filter refused, so the
//! panel can show what the terminal did not — `docs/plan/52-debug-console.md`
//! decision 4.
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
//!
//! It is settable while the engine runs, through [`set_filter`] and the `log`
//! console command below, so a directive can be widened at the moment something
//! is going wrong rather than on the next launch.
//!
//! **The filter is this module's, not a sink's.** `crcbl::web`'s queueing
//! logger takes the process's slot in a browser, where there is no stderr to
//! write to, so a filter kept inside `StderrLogger` would have been unreadable
//! and unsettable for a whole tier — which is what `log` reported there until
//! [`register_sink`] and [`sink_permits`] existed. A sink outside this crate
//! declares itself with the first and decides its records with the second, and
//! then `log warn,crcbl_vk=trace` means the same thing in a browser as in a
//! terminal, per-target directives included.

pub mod console;

use std::cell::RefCell;
use std::env;
use std::fmt;
use std::io::Write as _;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError, RwLock, RwLockReadGuard};
use std::time::Instant;

use ::log::{Log, Metadata, Record, SetLoggerError};
use crcbl_console::Fault;

/// The ring's two constants, up a level: they carry `CONSOLE_` in their names
/// already, so `log::CONSOLE_TARGET` reads as well as `log::console::` does.
/// The functions do not — `console::push` and `console::snapshot` say which
/// log they mean and `push` alone would not — so they stay in [`console`].
pub use console::{CONSOLE_RING_LINES, CONSOLE_TARGET};

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
    /// [`DEFAULT_FILTER`], as a `const`.
    ///
    /// [`FILTER`] is a `static` and a `RwLock<Filter>` can only be built in one
    /// without a lazy initialiser if the value inside it is const-constructible,
    /// which `Filter::parse(DEFAULT_FILTER)` is not. Spelling the same filter
    /// twice is the price, and
    /// `the_const_initial_filter_is_the_default_one_parsed` is what stops the
    /// two drifting.
    const INITIAL: Self = Self {
        default: LevelFilter::Info,
        targets: Vec::new(),
    };

    /// Parses a comma-separated directive list.
    ///
    /// Each directive is either a bare level (setting the default) or
    /// `target=level`. Unparseable directives are skipped rather than fatal: a
    /// typo in an environment variable must not stop the engine from starting.
    /// [`try_parse`](Self::try_parse) is the same reading for a caller who can
    /// be told.
    #[must_use]
    pub fn parse(directives: &str) -> Self {
        Self::read(directives).0
    }

    /// Parses, refusing the first directive it cannot read.
    ///
    /// What the `log` console command uses. [`parse`](Self::parse) skips a bad
    /// directive because a typo in `CRCBL_LOG` must not stop the engine from
    /// starting; a person typing at the console is there to be answered, and a
    /// filter that silently ignored half of what they wrote would be read as one
    /// that did not work.
    ///
    /// # Errors
    ///
    /// The first directive that is neither a level nor `target=level`, as it was
    /// written.
    pub fn try_parse(directives: &str) -> Result<Self, BadDirective> {
        let (filter, refused) = Self::read(directives);
        match refused.into_iter().next() {
            Some(directive) => Err(BadDirective(directive)),
            None => Ok(filter),
        }
    }

    /// The one reading of a directive list: the filter, and what it could not
    /// read.
    ///
    /// Both public forms come through here, so the rule for what a directive
    /// means cannot differ between the one that skips and the one that refuses.
    fn read(directives: &str) -> (Self, Vec<String>) {
        // Anything not named by a directive keeps the default level; a filter
        // string of only overrides ("crcbl_vk=trace") must not silence the
        // rest of the engine.
        let mut filter = Self {
            default: parse_level(DEFAULT_FILTER).unwrap_or(LevelFilter::Info),
            targets: Vec::new(),
        };
        let mut refused = Vec::new();
        for directive in directives.split(',') {
            let directive = directive.trim();
            if directive.is_empty() {
                continue;
            }
            match directive.split_once('=') {
                Some((target, level)) => {
                    if let Some(level) = parse_level(level.trim()) {
                        filter.targets.push((target.trim().to_owned(), level));
                    } else {
                        refused.push(directive.to_owned());
                    }
                }
                None => {
                    if let Some(level) = parse_level(directive) {
                        filter.default = level;
                    } else {
                        refused.push(directive.to_owned());
                    }
                }
            }
        }
        // Longest prefix first, so `find` below picks the most specific match.
        filter
            .targets
            .sort_by_key(|(target, _)| core::cmp::Reverse(target.len()));
        (filter, refused)
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

    /// Whether this filter admits a record at `level` from `target`.
    ///
    /// The one reading of "does the filter allow this", so the stderr sink and
    /// a sink outside this crate — [`sink_permits`] — cannot decide a record
    /// differently.
    fn permits(&self, level: Level, target: &str) -> bool {
        level <= self.level_for(target)
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

/// Writes the directives back in the form [`Filter::parse`] accepts.
///
/// So `log` can print a filter that can be typed straight back in, and so the
/// two spellings cannot drift: `the_filter_round_trips_through_its_own_display`
/// re-parses this.
///
/// The default level comes first and the overrides follow it, longest prefix
/// first, which is the order they are matched in and not the order they were
/// written in — a re-parse sorts them the same way, so the round trip is stable
/// rather than merely equivalent.
impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(level_directive(self.default))?;
        for (target, level) in &self.targets {
            write!(f, ",{target}={}", level_directive(*level))?;
        }
        Ok(())
    }
}

/// A directive [`Filter::try_parse`] could not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadDirective(String);

impl BadDirective {
    /// The directive, as it was written.
    #[must_use]
    pub fn directive(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BadDirective {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a filter directive: write a level, or `target=level`",
            self.0
        )
    }
}

impl std::error::Error for BadDirective {}

/// Whether `target` is `prefix` or a module below it.
fn target_matches(target: &str, prefix: &str) -> bool {
    target.starts_with(prefix)
        && (target.len() == prefix.len() || target[prefix.len()..].starts_with("::"))
}

/// How a directive spells a level — which is not how [`LevelFilter`] does:
/// `Display` for that is `OFF`, and a directive is read case-insensitively but
/// written lower case, the way `CRCBL_LOG` is set on a command line.
const fn level_directive(level: LevelFilter) -> &'static str {
    match level {
        LevelFilter::Off => "off",
        LevelFilter::Error => "error",
        LevelFilter::Warn => "warn",
        LevelFilter::Info => "info",
        LevelFilter::Debug => "debug",
        LevelFilter::Trace => "trace",
    }
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
///
/// It holds no filter of its own: [`FILTER`] is the process's one filter and
/// every sink that honours it reads that, which is what lets [`set_filter`]
/// mean the same thing whichever sink took the slot — see [`register_sink`].
#[derive(Debug)]
struct StderrLogger {
    start: Instant,
}

impl StderrLogger {
    /// Whether the filter admits `metadata`. This, and only this, decides what
    /// reaches stderr — [`Log::enabled`] below widens for capture, and a record
    /// let through on that account must still not be printed.
    fn permits(&self, metadata: &Metadata<'_>) -> bool {
        read_filter().permits(metadata.level(), metadata.target())
    }

    /// Capture, ring, filter and write one record.
    ///
    /// **The one path.** This module's own macros call it directly and the
    /// [`Log`] impl below funnels third-party records into it, so there is a
    /// single filter check, a single capture point and a single line format
    /// rather than one of each per entry point.
    fn emit(&self, level: Level, target: &str, args: fmt::Arguments<'_>) {
        // Seconds since init, not a date: the useful question in a frame loop is
        // "how long into the run". The wall-clock time the run *started* is
        // written once by `init_logging`, which is what makes a line here
        // correlatable with an outside log without paying date formatting per
        // line — see `start_banner`.
        let elapsed = self.start.elapsed();
        // Both before the filter: capture for the reason `enabled` gives, and
        // the console ring so the panel can show what the terminal did not.
        push_captured(level, target, args);
        console::push(level, target, elapsed, args);
        if !self.permits(&Metadata::builder().level(level).target(target).build()) {
            return;
        }
        let elapsed = elapsed.as_secs_f64();
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

/// Whether [`LOGGER`] is the logger the `log` crate is actually calling.
///
/// Separate from `LOGGER.get().is_some()`, which answers a different question
/// and was what [`is_installed`] used to read. [`try_init_logging`] has to
/// build the logger *before* offering it to `log::set_logger`, which takes a
/// `&'static dyn Log` — so on a process where a host application already owns
/// the slot, `LOGGER` is initialised and then rejected, and the old answer
/// reported an install that did not happen.
///
/// `Relaxed` is enough in both directions: this flag publishes no data. The
/// logger itself is published by `OnceLock` and by `log::set_logger`'s own
/// synchronisation, so a caller that reads `true` here and then logs is
/// ordered by those, not by this.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Whether a sink outside this module has declared that it honours [`FILTER`].
///
/// [`register_sink`] sets it and nothing clears it. Read beside [`INSTALLED`]
/// by [`honoured`], which is the question [`filter`] and [`set_filter`] both
/// actually ask: "is there a sink that would notice?"
///
/// `Relaxed` for [`INSTALLED`]'s reason — the flag publishes no data, and the
/// filter it speaks for is published by its own `RwLock`.
static SINK_REGISTERED: AtomicBool = AtomicBool::new(false);

/// The one filter the process applies, whichever sink is installed.
///
/// Behind an `RwLock` because [`set_filter`] swaps it while the engine runs:
/// every record takes the read side and they do not contend with each other,
/// which a `Mutex` would not manage, and swapping is rare enough that the write
/// side never queues behind anything. The alternative — an atomically swapped
/// `Arc` — buys a lock-free read at the price of a dependency this crate does
/// not have.
///
/// **A module-level static rather than a field on [`StderrLogger`]**, because
/// that sink is not the only one: `crcbl::web`'s queueing logger takes the slot
/// in a browser and honours this same filter through [`sink_permits`]. Held in
/// one place, `log warn,crcbl_vk=trace` means the same thing on both tiers; held
/// per sink, the web one could only ever have been the facade's global maximum.
static FILTER: RwLock<Filter> = RwLock::new(Filter::INITIAL);

/// The filter in force, borrowed rather than cloned: this is read once per
/// record, and a clone would allocate a `Vec<String>` for every log line.
///
/// A poisoned lock is stepped over for the reason [`console::push`] gives about
/// the ring's: a logger that panicked must not stop the run from saying what
/// happened next.
fn read_filter() -> RwLockReadGuard<'static, Filter> {
    FILTER.read().unwrap_or_else(PoisonError::into_inner)
}

/// Puts `filter` in force and moves the facade's global maximum with it.
///
/// **The maximum first**: between the two writes a sink is asked about records
/// the new filter admits, and the old filter is what decides them. The other
/// order drops records the caller just asked for.
fn store_filter(filter: Filter) {
    ::log::set_max_level(filter.max_level());
    *FILTER.write().unwrap_or_else(PoisonError::into_inner) = filter;
}

/// Whether any installed sink honours [`FILTER`].
///
/// The question [`filter`] and [`set_filter`] ask: a process whose logger is
/// some host application's has a filter here that decides nothing, and saying
/// so is the only honest answer to `log`.
fn honoured() -> bool {
    is_installed() || SINK_REGISTERED.load(Ordering::Relaxed)
}

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
        start: Instant::now(),
    });
    ::log::set_logger(logger)?;
    INSTALLED.store(true, Ordering::Relaxed);
    // After the slot is won, so a second call's filter never displaces the
    // first's — the `?` above is what turns that one back. The window between
    // the two lines is a fresh process's `log::max_level()`, which is `Off`, so
    // in the ordinary case no record is offered to any sink before the filter
    // this caller asked for is in force.
    store_filter(filter);
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
///
/// `false` when a host application, a test harness or anything else won the
/// process's single logger slot first — this module built a logger in that case
/// and `log` never took it, so nothing it emits reaches this one's filter or
/// its stderr.
///
/// Deliberately not "did this module build a logger", which answers a different
/// question: [`try_init_logging`] has to build one before offering it to
/// `log::set_logger`, so on a process where somebody else won, that check
/// reports an install that was rejected.
#[must_use]
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// The live filter
// ---------------------------------------------------------------------------

/// Declares that the process's logger is a sink this module does not own, and
/// puts `filter` in force for it.
///
/// **What `crcbl::web`'s queueing logger calls the moment it wins the slot.**
/// A browser has no stderr, so `StderrLogger` is never installed there and
/// [`is_installed`] is `false` for the whole run — which used to mean
/// [`filter`] answered `None` and [`set_filter`] refused, so the `log` console
/// command could only report that there was no filter to move. The filter is
/// this module's rather than a sink's, so a sink that reads it through
/// [`sink_permits`] gets the same per-target directives the terminal does; the
/// registration is how this module knows somebody is reading.
///
/// Call it **after** `log::set_logger` has succeeded. A sink that lost the slot
/// registering here would put a filter in force for a logger that is not
/// running, which is the dishonest answer [`is_installed`] exists to avoid.
///
/// Nothing unregisters: a process installs one logger and keeps it.
pub fn register_sink(filter: Filter) {
    SINK_REGISTERED.store(true, Ordering::Relaxed);
    store_filter(filter);
}

/// Whether the live filter admits a record at `level` from `target`.
///
/// The predicate a sink outside this crate answers `log::Log::enabled` with —
/// `crcbl::web`'s is the one caller — so that the directives typed at the
/// console decide its records the way `StderrLogger::permits` decides
/// stderr's.
///
/// **Falls back to the facade's global maximum when no sink has registered**,
/// which is what a sink that never called [`register_sink`] did before this
/// existed: an unregistered sink has no filter of this module's to honour, and
/// answering `false` would silence it outright.
#[must_use]
pub fn sink_permits(level: Level, target: &str) -> bool {
    if !SINK_REGISTERED.load(Ordering::Relaxed) {
        return level <= ::log::max_level();
    }
    read_filter().permits(level, target)
}

/// The filter the installed logger is applying.
///
/// `None` when no installed sink honours it — a host application won the
/// process's logger slot, or nothing has installed anything yet. There is no
/// filter to report in that case, and reporting the one this module holds would
/// describe a sink nothing is writing to.
#[must_use]
pub fn filter() -> Option<Filter> {
    honoured().then(|| read_filter().clone())
}

/// Swaps the filter the installed sink applies, from the next record on.
///
/// Returns `false` when no installed sink honours it, in which case nothing
/// changed — see [`filter`], [`is_installed`] and [`register_sink`].
///
/// **The facade's global maximum is set to match, exactly as installing does.**
/// That is what makes a widened filter reach the sink at all: a call site above
/// `log::max_level()` never builds its arguments, so raising the directive
/// without raising the maximum would change nothing. It cuts the other way too —
/// narrowing here stops those records reaching the console ring as well as
/// stderr, and a [`capture`] running on another thread stops seeing them.
pub fn set_filter(filter: Filter) -> bool {
    if !honoured() {
        return false;
    }
    store_filter(filter);
    true
}

crcbl_console::concommand! {
    /// Show the log filter, or install directives: `log warn,crcbl_render=debug`.
    pub fn log(cx, args) {
        if args.is_empty() {
            let Some(current) = filter() else {
                return Err(Fault::new(
                    "the process's logger honours no engine filter, so there is none to read",
                ));
            };
            cx.print(format!("log {current}"));
            return Ok(());
        }
        // Joined with a comma rather than a space: the directive list has no
        // spaces in it, so every way of typing one — `log warn,crcbl_vk=trace`,
        // `log warn, crcbl_vk=trace`, `log warn crcbl_vk=trace` — reads as the
        // list the person meant.
        let directives = args.join(",");
        let wanted = Filter::try_parse(&directives).map_err(|bad| Fault::new(bad.to_string()))?;
        // The line is the filter as it will be matched, not the text as it was
        // typed, so `log warn crcbl_vk=trace` answers with the list it became.
        let installed = wanted.to_string();
        if !set_filter(wanted) {
            return Err(Fault::new(
                "the process's logger honours no engine filter, so it was left alone",
            ));
        }
        cx.print(format!("log {installed}"));
        Ok(())
    }
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

    /// **`log` prints a filter that can be typed straight back in.**
    ///
    /// Two spellings of one thing drift; this is what stops them. A `Display`
    /// that dropped a directive, or wrote a level `parse_level` does not know,
    /// would still print something plausible — so the check is the round trip
    /// and not the text.
    #[test]
    fn the_filter_round_trips_through_its_own_display() {
        for directives in [
            "info",
            "off",
            "warn,crcbl_render=debug",
            "off,crcbl_render=warn,crcbl_render::graph=trace",
            "crcbl_vk=trace",
        ] {
            let filter = Filter::parse(directives);
            let written = filter.to_string();
            let reparsed = Filter::parse(&written);
            assert_eq!(reparsed.to_string(), written, "{directives}");
            for target in [
                "crcbl_core",
                "crcbl_render",
                "crcbl_render::graph",
                "crcbl_vk",
            ] {
                assert_eq!(
                    reparsed.level_for(target),
                    filter.level_for(target),
                    "{directives} at {target}",
                );
            }
        }
    }

    /// The overrides are written in match order, longest prefix first, which is
    /// not the order they were typed in — so the round trip above is stable and
    /// not merely equivalent.
    #[test]
    fn display_writes_the_default_then_the_overrides_in_match_order() {
        let filter = Filter::parse("crcbl_render::graph=trace,warn,crcbl_render=debug");
        assert_eq!(
            filter.to_string(),
            "warn,crcbl_render::graph=trace,crcbl_render=debug",
        );
    }

    /// **`parse` skips what it cannot read and `try_parse` names it**, which is
    /// the difference between a typo in an environment variable and a typo
    /// somebody just typed at a console.
    #[test]
    fn try_parse_refuses_the_directive_parse_would_skip() {
        let refused = Filter::try_parse("info,crcbl_vk=louder").expect_err("`louder` is no level");
        assert_eq!(refused.directive(), "crcbl_vk=louder");
        assert!(refused.to_string().contains("crcbl_vk=louder"), "{refused}");

        assert_eq!(
            Filter::parse("info,crcbl_vk=louder").level_for("crcbl_vk"),
            LevelFilter::Info,
            "the lenient form still skips it",
        );
        assert_eq!(
            Filter::try_parse("bogus")
                .expect_err("a bare word is no level")
                .directive(),
            "bogus",
        );
        assert!(
            Filter::try_parse("warn,,  ,crcbl_vk=trace").is_ok(),
            "empties are not directives"
        );
    }

    /// Serialises the tests that put a filter in force.
    ///
    /// They share [`FILTER`] and each asserts on the one it just wrote, so
    /// concurrently they would be asserting on each other's.
    static FILTER_ORDER: Mutex<()> = Mutex::new(());

    /// Runs `body` with `directives` in force, and puts back what was there.
    ///
    /// **The facade's global maximum is deliberately left alone**, which is the
    /// one thing this does not share with [`set_filter`]: nothing in this binary
    /// installs a logger — `installing_the_logger_is_idempotent` asserts the slot
    /// is still empty — so the maximum decides nothing here, while lowering it
    /// would silence a [`capture`] running on another thread. The filter tests
    /// that need both halves are `tests/console_log.rs`, in a binary of their own.
    fn with_filter<R>(directives: &str, body: impl FnOnce() -> R) -> R {
        let _order = FILTER_ORDER.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = std::mem::replace(
            &mut *FILTER.write().unwrap_or_else(PoisonError::into_inner),
            Filter::parse(directives),
        );
        let result = body();
        *FILTER.write().unwrap_or_else(PoisonError::into_inner) = previous;
        result
    }

    /// **[`Filter::INITIAL`] is [`DEFAULT_FILTER`], parsed.**
    ///
    /// The one spelled twice — a `const` for the `static` and a string for
    /// everyone else — so this is what stops the two drifting the day the
    /// default level changes.
    #[test]
    fn the_const_initial_filter_is_the_default_one_parsed() {
        assert_eq!(
            Filter::INITIAL.to_string(),
            Filter::parse(DEFAULT_FILTER).to_string(),
        );
    }

    /// **The ring is fed before the filter, and the filter still decides
    /// stderr.**
    ///
    /// The two halves of plan 52 decision 4, and each fails silently alone: a
    /// ring fed after the filter shows the panel exactly what the terminal
    /// already printed, and a ring that widened the filter would print every
    /// dropped line to CI.
    ///
    /// `capture` cannot answer the stderr half — it deliberately captures
    /// *before* the filter too, so a capturing thread sees everything — which is
    /// why `permits` is read directly here. It is documented as the one thing
    /// that decides what is written.
    #[test]
    fn the_ring_holds_records_the_filter_refused() {
        let quiet = "crcbl_core::log::tests::refused";
        let loud = "crcbl_core::log::tests::printed";
        let logger = StderrLogger {
            start: Instant::now(),
        };
        with_filter(&format!("info,{quiet}=off"), || {
            assert!(
                !logger.permits(
                    &Metadata::builder()
                        .level(Level::Error)
                        .target(quiet)
                        .build()
                ),
                "the directive silences that target, so none of its records reach stderr \
                 — filter {filter:?}, max level {max_level}, installed {installed}",
                filter = *read_filter(),
                max_level = ::log::max_level(),
                installed = is_installed(),
            );
            assert!(
                logger.permits(&Metadata::builder().level(Level::Info).target(loud).build()),
                "the default level admits the other target",
            );

            logger.emit(Level::Error, quiet, format_args!("under the filter"));
            logger.emit(Level::Info, loud, format_args!("over the filter"));
        });

        let mine: Vec<console::Record> = console::snapshot()
            .into_iter()
            .filter(|record| record.target == quiet || record.target == loud)
            .collect();
        assert_eq!(
            mine.iter()
                .map(|record| (record.target.as_str(), record.message.as_str()))
                .collect::<Vec<_>>(),
            [(quiet, "under the filter"), (loud, "over the filter")],
            "both records are in the ring, whatever stderr got",
        );
        assert_eq!(mine[0].level, Level::Error);
    }

    #[test]
    fn logger_respects_the_filter_without_being_installed() {
        let logger = StderrLogger {
            start: Instant::now(),
        };
        with_filter("warn,noisy=trace", || {
            assert!(
                logger.enabled(
                    &Metadata::builder()
                        .level(Level::Error)
                        .target("any")
                        .build()
                )
            );
            let meta = Metadata::builder().level(Level::Info).target("any").build();
            assert!(
                !logger.enabled(&meta),
                "an uninstalled logger admits nothing at the default level \
                 — filter {filter:?}, max level {max_level}, permits {permits}, \
                 capturing {capturing}, installed {installed}, thread {thread:?}",
                filter = *read_filter(),
                max_level = ::log::max_level(),
                permits = logger.permits(&meta),
                capturing = capturing(),
                installed = is_installed(),
                thread = std::thread::current().name(),
            );
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
        });
        logger.flush();
    }

    /// The environment variable that tells the re-executed test binary it is
    /// the child of [`a_host_owning_the_slot_means_this_module_installed_nothing`].
    const HOST_SLOT_CHILD: &str = "CRCBL_LOG_HOST_SLOT_CHILD";

    /// **A host that owns the logger slot first means this module installed
    /// nothing, and [`is_installed`] has to say so.**
    ///
    /// The answer used to be `LOGGER.get().is_some()`, and `try_init_logging`
    /// builds that logger before offering it to `log::set_logger` — which takes
    /// a `&'static dyn Log`, so there is no other order. On a process where the
    /// host won, the old answer was `true` for a logger `log` had rejected: a
    /// caller checking before deciding whether to route its own diagnostics
    /// here would have been told yes and then dropped them.
    ///
    /// # Why a child process
    ///
    /// The logger slot is per process and settable once, so this test and
    /// `installing_the_logger_is_idempotent` cannot both have it — and under a
    /// plain `cargo test`, which runs a crate's unit tests as threads in one
    /// binary, whichever ran second would fail on the other's install. The
    /// child gets a slot nobody has touched. This is `trace.rs`'
    /// `the_environment_variable_is_what_turns_the_gate_on` pattern, including
    /// the check that the child actually ran a test.
    #[test]
    #[cfg_attr(miri, ignore = "miri cannot spawn the child process this needs")]
    fn a_host_owning_the_slot_means_this_module_installed_nothing() {
        if env::var(HOST_SLOT_CHILD).is_ok() {
            /// Somebody else's logger, minimal and doing nothing: what is being
            /// checked is who owns the slot, not what the owner does with it.
            struct HostLogger;
            impl Log for HostLogger {
                fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
                    false
                }
                fn log(&self, _record: &Record<'_>) {}
                fn flush(&self) {}
            }
            static HOST: HostLogger = HostLogger;

            assert!(!is_installed(), "nothing has installed anything yet");
            ::log::set_logger(&HOST).expect("the host wins the empty slot");

            assert!(
                try_init_logging(Filter::parse("trace")).is_err(),
                "the slot is taken, so this must not claim it"
            );
            assert!(
                !is_installed(),
                "log::set_logger rejected this module's logger, so it is not installed, \
                 whatever LOGGER holds"
            );
            assert!(!init_logging(), "the convenience form agrees");
            assert!(!is_installed());
            return;
        }

        let output = std::process::Command::new(
            std::env::current_exe().expect("a test binary knows its own path"),
        )
        .args([
            "--exact",
            "log::tests::a_host_owning_the_slot_means_this_module_installed_nothing",
        ])
        .env(HOST_SLOT_CHILD, "1")
        .output()
        .expect("re-running this test binary");
        let report = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "the child failed:\n{report}");
        // `--exact` with a name that matches nothing exits zero, so without this
        // the whole test passes vacuously the moment it is renamed.
        assert!(
            report.contains("1 passed"),
            "the child ran no test — has this one been renamed?\n{report}"
        );
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
