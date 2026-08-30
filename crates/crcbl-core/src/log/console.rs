//! The console's view of the log: the ring every sink feeds, and the target the
//! console's own output carries.
//!
//! `docs/plan/52-debug-console.md` decision 4 is the design. The panel shows
//! **the** log rather than a second one, so there is one bounded ring here and
//! every sink pushes into it — [`crate::log`]'s own `StderrLogger` natively and
//! `crcbl::web`'s `WebLogger` in a browser, through the one [`push`] both call.
//!
//! Two things follow from where the push sits, and both are deliberate:
//!
//! * **It is before the sink's level filter**, so the ring holds a record the
//!   terminal decided not to print and the panel can be asked to show it. The
//!   facade's global maximum still applies — a call site whose level is above
//!   `log::max_level()` never reaches a sink at all — so what the ring adds is
//!   every record a *per-target* directive silenced.
//! * **It renders the message on every record**, where [`crate::log::capture`]
//!   renders only when a thread asked. That is the cost decision 10 prices: one
//!   `String` on a path that was already going to format one, plus a lock and a
//!   `VecDeque` insert.
//!
//! There is no `clear_ring`. `clear` is the panel emptying its own view —
//! Source's `clear` empties the console, not the file — which is why the ask
//! lives on `crcbl_console::Context::request_clear` and not here.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::Level;

/// The target every line the console itself prints carries.
///
/// The console's own output — an echoed command, a variable's value, `help` —
/// goes through [`print()`], so it is on stderr, in the browser console and in
/// the ring at once, and the terminal and the panel cannot show different text.
pub const CONSOLE_TARGET: &str = "console";

/// How many records the ring holds before the oldest is dropped.
///
/// Deep enough that the panel scrolls back past the boot sequence of a demo,
/// which is the run nobody is watching when it goes wrong, and shallow enough
/// that the ring's memory stays a rounding error beside one frame's textures.
/// It is a bound rather than a number to tune — a run logs without limit, and
/// something has to say where the memory stops. Never measured on any tier;
/// the backlog says so.
pub const CONSOLE_RING_LINES: usize = 1024;

/// One record, as the console reads it.
///
/// Distinct from [`crate::log::CapturedRecord`], which is a *test's* view and
/// carries no time: a panel draws the elapsed seconds and needs a cursor into
/// the ring, and a capture assertion wants neither.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// Where this record sits in the run's order, counted from the first record
    /// pushed and never reused.
    ///
    /// What [`snapshot_since`] takes, so a reader that draws every frame copies
    /// the lines that arrived since it last looked instead of the whole ring.
    pub sequence: u64,
    /// The level the record was logged at.
    pub level: Level,
    /// The record's target — the module path, unless the call overrode it, as
    /// [`print()`] does.
    pub target: String,
    /// The formatted message.
    pub message: String,
    /// How long into the run the record was logged, as the sink measured it.
    pub elapsed: Duration,
}

/// The bounded ring itself.
///
/// A type of its own rather than two globals, so the bound and the sequence can
/// be tested on an instance: a boundedness assertion written against the process
/// ring would be answered by whatever else the test binary logged.
#[derive(Debug)]
struct Ring {
    /// Oldest first, at most [`CONSOLE_RING_LINES`] of them.
    records: VecDeque<Record>,
    /// The sequence the next record gets.
    next_sequence: u64,
}

impl Ring {
    const fn new() -> Self {
        Self {
            records: VecDeque::new(),
            next_sequence: 0,
        }
    }

    /// Adds one record, dropping the oldest if the ring is full.
    fn push(&mut self, level: Level, target: &str, message: String, elapsed: Duration) {
        if self.records.len() >= CONSOLE_RING_LINES {
            self.records.pop_front();
        }
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.records.push_back(Record {
            sequence,
            level,
            target: target.to_owned(),
            message,
            elapsed,
        });
    }

    /// Every record from `from` onwards.
    ///
    /// Sequences increase along the ring, so the start is a binary search rather
    /// than a scan — the whole point of the cursor is that a reader drawing at
    /// frame rate does not walk every line it has already seen.
    fn since(&self, from: u64) -> Vec<Record> {
        let at = self
            .records
            .partition_point(|record| record.sequence < from);
        self.records.iter().skip(at).cloned().collect()
    }
}

/// The process's ring.
static RING: Mutex<Ring> = Mutex::new(Ring::new());

/// The ring, with a poisoned lock stepped over rather than propagated.
///
/// A thread that panicked while holding this left a `VecDeque` that is still
/// structurally sound — the push is one insert — and taking every later log
/// record down with it would mean logging could end a run, which is the opposite
/// of what a log is for. The same argument `crate::log::capture` makes about its
/// own setup lock.
fn ring() -> MutexGuard<'static, Ring> {
    RING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Pushes one record into the ring.
///
/// **What a sink calls, before its own level filter**, and the one push both of
/// the engine's sinks make: `StderrLogger::emit` here and `WebLogger::log` in
/// `crates/crcbl/src/web.rs`. `elapsed` is the sink's own clock, because the two
/// do not share one — the native side measures from an `Instant` taken at
/// install, the browser from `performance.now()` at the first frame.
pub fn push(level: Level, target: &str, elapsed: Duration, args: fmt::Arguments<'_>) {
    // Rendered before the lock is taken: `args` runs the caller's own `Display`
    // impls, and one of those logging in turn would deadlock on the ring.
    let message = args.to_string();
    ring().push(level, target, message, elapsed);
}

/// Logs one line the console produced, under [`CONSOLE_TARGET`].
///
/// Everything the console prints goes through here, so a line is on stderr, in
/// the browser console and in the ring at once. It goes to whichever sink the
/// process installed rather than to the ring directly, for the reason
/// [`crate::log::__emit`] gives: on `wasm32` that sink is not this crate's.
///
/// At [`Level::Info`], and past the macros' `__enabled` fast path, so the line
/// reaches the ring whatever the facade's global maximum happens to be. The
/// sink's own filter still decides the terminal: under `CRCBL_LOG=off` the
/// answer to a typed command is in the panel and not on stderr.
pub fn print(line: &str) {
    super::__emit(Level::Info, CONSOLE_TARGET, format_args!("{line}"));
}

/// A copy of the whole ring, oldest first.
///
/// What a panel reads when it opens. Copies rather than drains: the ring is the
/// run's, and a second reader must see the same lines.
#[must_use]
pub fn snapshot() -> Vec<Record> {
    // The oldest record the ring still holds is the oldest sequence it holds, so
    // "everything" and "everything since the beginning" are one copy and not two.
    snapshot_since(0)
}

/// A copy of every record from sequence `from` onwards, oldest first.
///
/// What a panel reads on every later frame: keep `last.sequence + 1` from the
/// previous call and pass it here, and the copy is the new lines only. A cursor
/// older than the ring still holds yields whatever survived, which is the
/// honest answer — the records between were dropped, not hidden.
#[must_use]
pub fn snapshot_since(from: u64) -> Vec<Record> {
    ring().since(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pushes `count` records into `ring`, numbered in their message.
    fn fill(ring: &mut Ring, count: usize) {
        for i in 0..count {
            ring.push(
                Level::Info,
                "crcbl_core::log::console",
                format!("line {i}"),
                Duration::from_millis(i as u64),
            );
        }
    }

    /// **The ring is bounded, and it drops the oldest.**
    ///
    /// A ring that dropped the newest would lose whatever just went wrong, and
    /// an unbounded one would grow a long run's memory without limit — which is
    /// the whole reason the depth is a constant and not a `Vec`.
    #[test]
    fn past_its_depth_the_oldest_records_are_the_ones_that_go() {
        let overflow = 3;
        let mut ring = Ring::new();
        fill(&mut ring, CONSOLE_RING_LINES + overflow);

        let held = ring.since(0);
        assert_eq!(held.len(), CONSOLE_RING_LINES, "the bound did not hold");
        assert_eq!(
            held[0].message,
            format!("line {overflow}"),
            "the oldest three should have gone, not the newest",
        );
        assert_eq!(
            held.last().expect("the ring is not empty").message,
            format!("line {}", CONSOLE_RING_LINES + overflow - 1),
        );
    }

    /// **A sequence is never reused and never skipped**, because that is what
    /// makes it a cursor: a reader that saw `n` asks for `n + 1` and gets
    /// exactly what arrived since.
    #[test]
    fn sequences_stay_contiguous_across_the_drop() {
        let overflow = 3;
        let mut ring = Ring::new();
        fill(&mut ring, CONSOLE_RING_LINES + overflow);

        let held = ring.since(0);
        let first = held[0].sequence;
        assert_eq!(
            first, overflow as u64,
            "the sequence must count every record pushed, dropped ones included",
        );
        for (step, record) in held.iter().enumerate() {
            assert_eq!(
                record.sequence,
                first + step as u64,
                "a gap or a repeat at {step}",
            );
        }
    }

    /// The cursor is what a per-frame reader costs nothing on.
    #[test]
    fn a_cursor_takes_the_newer_records_and_nothing_else() {
        let mut ring = Ring::new();
        fill(&mut ring, 5);

        let all = ring.since(0);
        assert_eq!(all.len(), 5);

        let newer = ring.since(3);
        assert_eq!(
            newer.iter().map(|r| r.sequence).collect::<Vec<_>>(),
            [3, 4],
            "only the records at or after the cursor",
        );
        assert_eq!(newer[0].message, "line 3");
        assert!(
            ring.since(5).is_empty(),
            "a cursor past the end has nothing to hand back",
        );
    }

    /// The record carries what a panel draws, not merely the text.
    #[test]
    fn a_record_keeps_its_level_target_and_elapsed() {
        let mut ring = Ring::new();
        ring.push(
            Level::Warn,
            CONSOLE_TARGET,
            "something to say".to_owned(),
            Duration::from_millis(1250),
        );

        let held = ring.since(0);
        let record = held.first().expect("the record was pushed");
        assert_eq!(record.level, Level::Warn);
        assert_eq!(record.target, "console");
        assert_eq!(record.message, "something to say");
        assert_eq!(record.elapsed, Duration::from_millis(1250));
    }

    /// The process ring is fed by the same code the instance tests exercise, so
    /// this only has to prove the wiring — that [`push`] reaches it at all.
    #[test]
    fn the_process_ring_takes_what_push_is_given() {
        let target = "crcbl_core::log::console::the_process_ring_takes_what_push_is_given";
        push(
            Level::Error,
            target,
            Duration::from_secs(2),
            format_args!("a record from {}", "the shared push"),
        );

        let mine: Vec<Record> = snapshot()
            .into_iter()
            .filter(|record| record.target == target)
            .collect();
        assert_eq!(mine.len(), 1, "{mine:?}");
        assert_eq!(mine[0].message, "a record from the shared push");
        assert_eq!(mine[0].level, Level::Error);
        assert_eq!(mine[0].elapsed, Duration::from_secs(2));
    }
}
