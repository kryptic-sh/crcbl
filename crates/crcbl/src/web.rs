//! The half of a browser entry point that is not about any one game.
//!
//! A sample's `web.rs` is what the JS shim in `web/` calls: a dozen
//! `#[unsafe(no_mangle)] extern "C"` exports named after the demo, a status
//! code the page polls, and a log queue the page drains once a frame. The
//! exports have to stay per-game — the shim looks them up by name — and so does
//! the state machine that holds a game's own `Loop`. Everything else was
//! identical in all four samples, and it is here.
//!
//! What this module owns:
//!
//! * **The status codes.** They are a wire format: the shim in `web/` switches
//!   on the numbers, so a sample that renumbered them would break the page
//!   rather than fail to compile. One definition is the only way that stays
//!   true.
//! * **The log queue.** `crcbl::log` has no `console.log` on this target — an
//!   import would be the only one in the module — so lines are queued in wasm
//!   memory and the shim pulls them across one at a time. Bounded, because a
//!   page that stops draining must not grow the heap without limit.
//!
//! Deliberately **not** here: anything that touches a game's `Loop`. See each
//! sample's `web.rs` for the state machine, which is the part that genuinely
//! differs.

use core::cell::RefCell;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Nothing has been prepared; the demo's `prepare` export has not run.
pub const STATUS_IDLE: u32 = 0;
/// Storage is installed and the shim may pre-load; no shell yet.
pub const STATUS_PREPARED: u32 = 1;
/// Waiting for the canvas's first size, or for the device promise.
pub const STATUS_BOOTING: u32 = 2;
/// Playing. Every `frame` export draws.
pub const STATUS_RUNNING: u32 = 3;
/// The loop ended on its own terms — the page asked it to close, or the window
/// went away. Not an error.
pub const STATUS_STOPPED: u32 = 4;
/// Something failed; the demo's `error_ptr` export says what.
pub const STATUS_FAILED: u32 = 5;
/// Running, but the simulation is stopped: the player pressed Escape, or the
/// canvas lost focus.
///
/// A separate code rather than a flag beside [`STATUS_RUNNING`], because the
/// page's status line is a *status* — it said "Playing." for as long as the demo
/// was alive, including while the canvas sat unfocused behind something else.
/// Numbered after [`STATUS_FAILED`] so the codes already published to the shim
/// keep their values.
pub const STATUS_PAUSED: u32 = 6;

/// The base URL a demo's `FetchSource` resolves asset keys against.
///
/// Relative to the *document*, so a demo served from `/crcbl/demos/breakout/`
/// fetches `/crcbl/demos/breakout/assets/<key>`. The trailing slash is required
/// by `FetchSource::new`, which refuses a base without one so that a key can
/// never be read as a scheme.
pub const ASSET_BASE: &str = "assets/";

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// The most log lines held for the shim before the oldest is dropped.
///
/// A page that never drains must not grow wasm memory without bound; a page that
/// drains once per frame will never see this.
const MAX_LOG_LINES: usize = 512;

/// The longest log line handed to the shim, in bytes.
const MAX_LOG_LINE: usize = 1024;

/// The queue [`log_take`] drains.
#[derive(Default)]
struct LogQueue {
    lines: std::collections::VecDeque<String>,
    /// The line the shim is currently reading. Kept at a fixed capacity so its
    /// address does not move between a `take` and the [`log_ptr`] that follows.
    current: String,
    /// Lines dropped because the shim was not draining. Reported once, on the
    /// next line that fits, rather than silently.
    dropped: u64,
}

thread_local! {
    static LOG: RefCell<LogQueue> = RefCell::new(LogQueue::default());
}

/// A [`log::Log`] that queues lines for the shim instead of writing them.
///
/// There is no `console.log` import and no timestamp: an import would be the
/// only one in the module, and a timestamp would need [`std::time::Instant`],
/// which this target does not have. The browser's console stamps the line when
/// the shim prints it, which is within a frame of when it was written.
struct WebLogger;

impl log::Log for WebLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        LOG.with(|slot| {
            // `try_borrow_mut` because a `Drop` running inside `log_take` could
            // in principle log; dropping the line is better than a panic in a
            // logger.
            let Ok(mut queue) = slot.try_borrow_mut() else {
                return;
            };

            if queue.lines.len() >= MAX_LOG_LINES {
                queue.lines.pop_front();
                queue.dropped = queue.dropped.saturating_add(1);
            }
            let mut line = format!(
                "[{}] {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
            // Byte truncation would split a UTF-8 sequence; `char_indices` finds
            // the last boundary at or before the limit.
            if line.len() > MAX_LOG_LINE {
                let end = line
                    .char_indices()
                    .map(|(i, _)| i)
                    .take_while(|i| *i <= MAX_LOG_LINE)
                    .last()
                    .unwrap_or(0);
                line.truncate(end);
            }
            queue.lines.push_back(line);
        });
    }

    fn flush(&self) {}
}

static LOGGER: WebLogger = WebLogger;

/// Installs the queueing logger, unless a logger is already installed.
pub fn install_logger() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// Sets the log filter: `0` off, `1` error, `2` warn, `3` info, `4` debug,
/// `5` trace.
///
/// Returns `1`, or `0` for a level outside that range — which leaves the filter
/// **unchanged**. Refusing rather than clamping is the point: a shim that sent
/// nonsense would otherwise get a quiet default and a log at a level nobody
/// asked for.
#[must_use]
pub fn set_log_level(level: u32) -> u32 {
    let filter = match level {
        0 => log::LevelFilter::Off,
        1 => log::LevelFilter::Error,
        2 => log::LevelFilter::Warn,
        3 => log::LevelFilter::Info,
        4 => log::LevelFilter::Debug,
        5 => log::LevelFilter::Trace,
        _ => return 0,
    };
    log::set_max_level(filter);
    1
}

/// Moves the next queued line into the scratch buffer and returns its length.
///
/// `0` means there was nothing to take. The line itself is read through
/// [`log_ptr`], which stays valid until the next call to this.
#[must_use]
pub fn log_take() -> u32 {
    LOG.with(|slot| {
        let Ok(mut queue) = slot.try_borrow_mut() else {
            return 0;
        };
        queue.current.clear();
        match queue.lines.pop_front() {
            Some(line) => queue.current.push_str(&line),
            // The overflow notice is emitted only once the queue has actually
            // drained — synthesising it while lines are still queued would
            // itself be a line, and the counter is cleared as it is reported so
            // a page that keeps up never sees it twice.
            None => {
                let dropped = core::mem::take(&mut queue.dropped);
                if dropped == 0 {
                    return 0;
                }
                use core::fmt::Write as _;
                let _ = write!(
                    queue.current,
                    "[WARN] crcbl::web: {dropped} log lines dropped; the shim is not draining"
                );
            }
        }
        u32::try_from(queue.current.len()).unwrap_or(u32::MAX)
    })
}

/// Address of the log scratch buffer, or null when nothing has been taken.
#[must_use]
pub fn log_ptr() -> *const u8 {
    LOG.with(|slot| match slot.try_borrow() {
        Ok(queue) if !queue.current.is_empty() => queue.current.as_ptr(),
        _ => core::ptr::null(),
    })
}

#[cfg(test)]
mod tests {
    use log::Log as _;

    use super::*;

    /// Drains everything the queue is holding, so one test cannot see another's
    /// lines — `LOG` is a thread-local and the test harness reuses threads.
    fn drain() -> Vec<String> {
        let mut taken = Vec::new();
        while log_take() > 0 {
            LOG.with(|slot| taken.push(slot.borrow().current.clone()));
        }
        taken
    }

    fn write_line(target: &str, message: &str) {
        LOGGER.log(
            &log::Record::builder()
                .level(log::Level::Info)
                .target(target)
                .args(format_args!("{message}"))
                .build(),
        );
    }

    /// **A page that stops draining bounds the queue and says how much it lost.**
    ///
    /// Both halves matter and each fails silently on its own: an unbounded queue
    /// grows wasm memory until the tab dies, and a bounded one that drops
    /// quietly turns "the log is missing the interesting part" into a mystery.
    #[test]
    fn a_queue_nobody_drains_is_bounded_and_reports_what_it_dropped() {
        log::set_max_level(log::LevelFilter::Info);
        drain();

        let overflow = 10;
        for i in 0..MAX_LOG_LINES + overflow {
            write_line("test", &format!("line {i}"));
        }

        let taken = drain();
        assert_eq!(
            taken.len(),
            MAX_LOG_LINES + 1,
            "the cap did not hold, or the overflow notice is missing",
        );
        // The oldest went, not the newest: a log that dropped the most recent
        // lines would lose whatever just went wrong.
        assert!(
            taken[0].contains(&format!("line {overflow}")),
            "{}",
            taken[0]
        );
        let notice = taken.last().expect("the queue was not empty");
        assert!(
            notice.contains(&overflow.to_string()) && notice.contains("not draining"),
            "the drop count must be reported, not swallowed: {notice}",
        );

        // Reported once. A counter that never cleared would append the notice to
        // every later drain of a page that had long since caught up.
        write_line("test", "after");
        let taken = drain();
        assert_eq!(taken.len(), 1, "the notice was repeated: {taken:?}");
    }

    /// **A line longer than the cap is cut on a character boundary.**
    ///
    /// `String::truncate` panics on a byte index inside a UTF-8 sequence, and a
    /// logger that panics takes the frame with it. Asserted with a multi-byte
    /// character straddling the limit, which is the only case that can fail.
    #[test]
    fn an_over_long_line_is_cut_without_splitting_a_character() {
        log::set_max_level(log::LevelFilter::Info);
        drain();

        // 'é' is two bytes, and the prefix is chosen so that MAX_LOG_LINE lands
        // *between* the two — the only arrangement a byte truncate panics on.
        // With an even-length prefix the limit falls on a boundary and a wrong
        // implementation passes, which is what the first version of this test
        // did.
        let target = "tt";
        let prefix = format!("[{}] {target}: ", log::Level::Info).len();
        assert!(
            (MAX_LOG_LINE - prefix) % 2 == 1,
            "the fixture must put the limit inside a character, not on a boundary",
        );
        let message = "é".repeat(MAX_LOG_LINE);
        write_line(target, &message);

        let taken = drain();
        let line = taken.first().expect("the line was queued");
        assert!(line.len() <= MAX_LOG_LINE, "not truncated: {}", line.len());
        assert!(
            line.chars().last().is_some_and(|c| c == 'é'),
            "the cut landed inside a character",
        );
    }
}
