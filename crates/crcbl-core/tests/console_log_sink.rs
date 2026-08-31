//! The `log` command against a sink this crate does not own.
//!
//! **A binary of its own, and that is the whole point of it.** The claim here
//! is what `crcbl_core::log::is_installed` answers `false` to: a process whose
//! logger is *not* this crate's `StderrLogger` — which is every browser run,
//! where `crcbl::web`'s queueing logger takes the slot because there is no
//! stderr to write to. `tests/console_log.rs` installs this crate's own logger
//! before it touches the filter, so the arm below cannot be reached from there
//! at all; the process's single logger slot is what makes them two binaries
//! rather than two tests.
//!
//! The sink is a fixture rather than `WebLogger` because that type lives in
//! `crcbl` and is private to it. What the two share is the only thing this is
//! about: neither is `StderrLogger`, and both decide their records with
//! [`crcbl_core::log::sink_permits`]. `crcbl::web`'s own
//! `a_filter_set_at_runtime_decides_what_the_browser_sink_queues` drives the
//! real one.

use std::sync::{Mutex, PoisonError};

use crcbl_console::{Context, Registry};
use crcbl_core::log::Filter;

/// A target the fixture's filter names, and one it leaves at the default.
const LOUD: &str = "console_log_sink::loud";
const QUIET: &str = "console_log_sink::quiet";

/// Every record the sink accepted, as `target message`.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A sink outside `crcbl-core`, shaped exactly like `crcbl::web`'s.
///
/// The two lines that matter are `enabled` — the live filter, asked for this
/// record rather than the facade's one global maximum — and `log` asking it
/// before accepting anything, which is what a `log::Log` has to do: the
/// facade's macros never call `enabled` on the caller's behalf.
struct FakeSink;

impl log::Log for FakeSink {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        crcbl_core::log::sink_permits(metadata.level(), metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        SEEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("{} {}", record.target(), record.args()));
    }

    fn flush(&self) {}
}

static SINK: FakeSink = FakeSink;

/// What the sink has accepted so far, drained.
fn taken() -> Vec<String> {
    std::mem::take(&mut *SEEN.lock().unwrap_or_else(PoisonError::into_inner))
}

/// **A filter typed at the console decides what a sink outside this crate
/// writes.**
///
/// `docs/backlog.md`'s "`log` answers with a fault in a browser": the filter
/// used to live inside `StderrLogger`, so on the one tier where that sink is
/// never installed the command could only report that there was nothing to
/// read and nothing to move. Every step below is one of the halves that was
/// missing, in order, and the last is the observable the whole thing is for —
/// a **record**, offered to the sink, admitted or refused by the directive the
/// console line installed.
///
/// One test rather than four, because the three states it walks through are
/// process-wide and it is the transitions between them that are the claim: a
/// second test would be asserting on whichever state this one had reached.
#[test]
fn a_filter_typed_at_the_console_decides_a_foreign_sinks_records() {
    // 1. Nothing honours the filter yet, and both halves of `log` say so.
    assert!(
        crcbl_core::log::filter().is_none(),
        "no sink has registered, so there is no filter anything is applying",
    );
    assert!(
        !crcbl_core::log::set_filter(Filter::parse("trace")),
        "a filter nothing would read must be refused rather than quietly stored",
    );
    let registry =
        Registry::gather(&[crcbl_core::console_table()]).expect("no two entries claim one name");
    let mut host = ();
    let mut cx = Context::new(&registry, &mut host);
    let fault = registry
        .execute(&mut cx, "log")
        .expect_err("nothing is applying a filter, so there is none to print");
    assert!(
        fault.message().contains("honours no engine filter"),
        "the fault has to say which half is missing: {fault}",
    );

    // 2. The sink takes the slot and registers, exactly as
    //    `crcbl::web::install_logger` does.
    log::set_logger(&SINK).expect("this binary installs no other logger");
    crcbl_core::log::register_sink(Filter::parse("info"));
    assert_eq!(
        crcbl_core::log::filter()
            .expect("a sink is registered")
            .to_string(),
        "info",
        "`log` bare reads the registered sink's filter now",
    );

    // 3. Under that filter a debug record reaches nothing — the facade's global
    //    maximum is `Info`, so it is dropped before the sink is even asked.
    log::debug!(target: LOUD, "before the console moved the filter");
    assert!(
        taken().is_empty(),
        "the default level admits no debug record",
    );

    // 4. The console line, and it is the command that installs the directives.
    let mut cx = Context::new(&registry, &mut host);
    registry
        .execute(&mut cx, &format!("log info,{LOUD}=debug"))
        .expect("a readable directive");
    assert_eq!(cx.lines(), [format!("log info,{LOUD}=debug")]);

    // 5. The observable: one target's debug record is written and the other's
    //    is not, and the only thing separating them is the line typed above.
    //    The pair is what makes it a check — a `set_filter` that raised the
    //    facade's maximum and left the directives alone would write both, and
    //    one that moved the directives without the maximum would write neither.
    log::debug!(target: LOUD, "the directive admits this one");
    log::debug!(target: QUIET, "and refuses this one");
    assert_eq!(
        taken(),
        [format!("{LOUD} the directive admits this one")],
        "the widened directive is what let the record through, and only that one",
    );
}
