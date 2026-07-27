//! Logging setup.
//!
//! The engine logs through the [`log`] facade. This module supplies the
//! one thing a facade needs and does not provide: a sink. It is deliberately a
//! ~150-line `log::Log` implementation writing to stderr rather than a
//! dependency on a logging framework — the engine needs level filtering and a
//! readable line, and every framework that does more brings a runtime,
//! a feature matrix, and opinions about async.
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

use std::env;
use std::io::Write as _;
use std::sync::OnceLock;
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

impl Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
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
