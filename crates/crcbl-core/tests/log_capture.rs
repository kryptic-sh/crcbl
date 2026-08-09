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
