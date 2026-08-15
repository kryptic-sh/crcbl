//! [`crcbl_core::log::capture`], exercised the way another crate's tests use
//! it: through the public API, from outside the crate.
//!
//! A separate binary rather than unit tests beside the code, and that is
//! load-bearing. `capture` installs the process logger, and `log.rs`'s
//! `installing_the_logger_is_idempotent` asserts the slot is still *empty* when
//! it runs. Those two cannot share a binary under a thread-per-test runner:
//! whichever loses the race fails. They share nothing across processes, so they
//! stop fighting the moment they stop being the same executable.

use crcbl_core::log::capture;

/// Capture must not turn on `CRCBL_LOG`: the `debug!` here is below the default
/// filter and never reaches stderr, and a test asserting on it must still see
/// it. Otherwise every assertion written on this mechanism passes or fails on
/// whatever the developer happened to export.
#[test]
fn capture_sees_this_thread_s_records_below_the_filter() {
    let logs = capture();
    log::info!("engine: a thing happened");
    log::debug!("engine: quietly, too");

    let records = logs.records();
    assert_eq!(records.len(), 2, "{records:?}");
    assert_eq!(records[0].level, log::Level::Info);
    assert_eq!(records[0].message, "engine: a thing happened");
    assert_eq!(
        records[0].target, "log_capture",
        "the target is the logging module's, not the sink's"
    );
    assert_eq!(records[1].level, log::Level::Debug);
    assert_eq!(records[1].message, "engine: quietly, too");
}

/// The property that makes concurrent tests sound: a buffer shared across
/// threads would have the worker's two records in it as well.
#[test]
fn capture_is_scoped_to_the_thread_that_asked_for_it() {
    let logs = capture();
    std::thread::spawn(|| {
        log::info!("from somewhere else");
        log::warn!("and again");
    })
    .join()
    .expect("the worker logged and returned");
    log::info!("from here");

    let records = logs.records();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].message, "from here");
}

#[test]
fn dropping_the_guard_stops_the_capture() {
    drop(capture());
    log::info!("nobody is listening");

    let logs = capture();
    assert!(
        logs.records().is_empty(),
        "a second capture starts empty, and did not pick up the line above"
    );
}

#[test]
#[should_panic(expected = "already capturing")]
fn capturing_twice_on_one_thread_is_a_bug() {
    let _outer = capture();
    let _inner = capture();
}

/// **The level macros reach the sink, at their own levels, with the calling
/// module as the target.**
///
/// Here rather than beside the code for the reason this file's own header
/// gives: asserting on them needs an installed logger, and `log.rs`'s
/// idempotency test needs one that has not been installed yet.
#[test]
fn the_level_macros_arrive_at_their_own_levels() {
    let logs = capture();
    crcbl_core::error!("an error {}", 1);
    crcbl_core::warn!("a warning");
    crcbl_core::info!("some info");
    crcbl_core::debug!("some detail");
    crcbl_core::trace!("every step");

    let records = logs.records();
    assert_eq!(
        records.iter().map(|r| r.level).collect::<Vec<_>>(),
        [
            log::Level::Error,
            log::Level::Warn,
            log::Level::Info,
            log::Level::Debug,
            log::Level::Trace,
        ],
    );
    assert_eq!(records[0].message, "an error 1", "arguments are applied");
    assert_eq!(
        records[0].target, "log_capture",
        "the target is the calling module's, not the sink's"
    );
}

/// **A call the filter rejects never evaluates its argument expressions.**
///
/// This is what putting the level check in the macro buys, and it is narrower
/// than it first looks. `format_args!` already defers the *formatting* — a
/// `Display` impl that panicked would not run either way — so the thing the
/// guard actually saves is evaluating the arguments at all. `explode()` below
/// is called by `format_args!` before anything is written, so a macro without
/// the check panics here and one with it does not.
///
/// Run on a thread that is *not* capturing, because capture deliberately widens
/// the filter, and the claim is about a plain run.
#[test]
fn a_filtered_call_never_evaluates_its_arguments() {
    fn explode() -> u32 {
        panic!("an argument was evaluated for a call nothing would read");
    }

    // Installs the logger if nothing else has, so `__enabled` has a filter to
    // answer from. Dropped before the worker starts, so the worker's thread is
    // certainly not capturing.
    drop(capture());

    std::thread::spawn(|| {
        // Guarded rather than assumed: with `CRCBL_LOG=trace` exported this
        // call *would* run, and the test would be asserting nothing. Fail
        // saying so instead.
        assert!(
            !crcbl_core::log::__enabled(log::Level::Trace, module_path!()),
            "CRCBL_LOG has trace enabled, so this test cannot check the guard"
        );
        crcbl_core::trace!("{}", explode());
    })
    .join()
    .expect("the worker must not panic");
}
