//! The live filter, the `log` command and the `console` target, from outside
//! the crate.
//!
//! A separate binary, for the reason `crates/crcbl-core/tests/log_capture.rs`
//! gives about itself and one more of its own: everything here **writes** the
//! process-wide filter, and `log.rs`'s `installing_the_logger_is_idempotent`
//! asserts the logger slot is still empty when it runs. Within this binary the
//! tests take [`ORDER`] before touching the filter, because they would
//! otherwise be reading each other's.

use std::sync::{Mutex, MutexGuard, PoisonError};

use crcbl_console::{Context, Registry};
use crcbl_core::log::{Filter, console};

/// Serialises the tests that write the process filter.
///
/// They share one global and each asserts on what it just installed, so
/// concurrently they would be asserting on each other's. A poisoned lock is
/// stepped over rather than propagated: one failing test must not turn every
/// other one in the file red and hide what actually broke.
static ORDER: Mutex<()> = Mutex::new(());

fn in_order() -> MutexGuard<'static, ()> {
    ORDER.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Installs this module's logger if nothing has, and puts a known filter in
/// force. Returns the guard the caller holds for the rest of its test.
fn with_filter(directives: &str) -> MutexGuard<'static, ()> {
    let order = in_order();
    let _ = crcbl_core::log::try_init_logging(Filter::parse(directives));
    assert!(
        crcbl_core::log::set_filter(Filter::parse(directives)),
        "this binary's logger has to be the process's for any of this to be observable",
    );
    order
}

/// Every ring record carrying `target`, in order.
fn ringed(target: &str) -> Vec<console::Record> {
    console::snapshot()
        .into_iter()
        .filter(|record| record.target == target)
        .collect()
}

/// **A filter installed at runtime decides the very next record.**
///
/// Two observables, and the pair is what makes it a check rather than a
/// tautology. `__enabled` is the sink's own answer to "would this be written",
/// which is what `StderrLogger::permits` decides and the only thing that does.
/// The ring is the record itself arriving: the facade's global maximum is what
/// stops a `debug!` before any sink is asked, so a `set_filter` that swapped the
/// directives and left the maximum alone would pass the first assertion and fail
/// the second.
#[test]
fn a_filter_installed_at_runtime_decides_the_next_record() {
    let target = "crcbl_core_console_log::live_filter";
    let _order = with_filter("info");

    assert!(
        !crcbl_core::log::__enabled(log::Level::Debug, target),
        "the default level does not admit a debug record",
    );
    log::debug!(target: target, "before the filter moved");
    assert!(
        ringed(target).is_empty(),
        "the facade dropped it before any sink saw it",
    );

    assert!(crcbl_core::log::set_filter(Filter::parse(&format!(
        "info,{target}=debug"
    ))));

    assert!(
        crcbl_core::log::__enabled(log::Level::Debug, target),
        "the directive admits it now",
    );
    log::debug!(target: target, "after the filter moved");
    assert_eq!(
        ringed(target)
            .iter()
            .map(|record| record.message.clone())
            .collect::<Vec<_>>(),
        ["after the filter moved"],
        "the widened filter is what let the record through",
    );
}

/// The registry a console would gather from this crate alone.
fn registry() -> Registry {
    Registry::gather(&[crcbl_core::console_table()]).expect("no two entries claim one name")
}

/// **`log <directives>` installs exactly those directives and says so.**
#[test]
fn the_log_command_installs_the_filter_it_names() {
    let _order = with_filter("info");
    let registry = registry();
    let mut host = ();
    let mut cx = Context::new(&registry, &mut host);

    registry
        .execute(&mut cx, "log crcbl_render=debug")
        .expect("a readable directive");

    assert_eq!(cx.lines(), ["log info,crcbl_render=debug"]);
    let installed = crcbl_core::log::filter().expect("the logger is installed");
    assert_eq!(installed.to_string(), "info,crcbl_render=debug");
    assert_eq!(installed.level_for("crcbl_render"), log::LevelFilter::Debug);
    assert_eq!(installed.level_for("crcbl_core"), log::LevelFilter::Info);
}

/// Bare, it reads rather than writes — Source's shape for a variable, and the
/// only way to find out what is in force without changing it.
#[test]
fn the_log_command_bare_prints_the_filter_in_force() {
    let _order = with_filter("warn,crcbl_vk=trace");
    let registry = registry();
    let mut host = ();
    let mut cx = Context::new(&registry, &mut host);

    registry.execute(&mut cx, "log").expect("`log` reads");

    assert_eq!(cx.lines(), ["log warn,crcbl_vk=trace"]);
    assert_eq!(
        crcbl_core::log::filter().expect("installed").to_string(),
        "warn,crcbl_vk=trace",
        "reading it must not have changed it",
    );
}

/// **A directive nothing can read is a fault that names it, and the filter is
/// left alone.**
///
/// `Filter::parse` skips what it cannot read, which is right for `CRCBL_LOG` and
/// wrong here: a person who typed `louder` would otherwise get a cheerful line
/// back reporting a filter that ignored half of it.
#[test]
fn a_bad_directive_is_refused_and_changes_nothing() {
    let _order = with_filter("info");
    let registry = registry();
    let mut host = ();
    let mut cx = Context::new(&registry, &mut host);

    let fault = registry
        .execute(&mut cx, "log crcbl_vk=louder")
        .expect_err("`louder` is not a level");

    assert!(
        fault.message().contains("crcbl_vk=louder"),
        "the fault has to name the directive it refused: {fault}",
    );
    assert!(cx.lines().is_empty(), "{:?}", cx.lines());
    assert_eq!(
        crcbl_core::log::filter().expect("installed").to_string(),
        "info",
        "the filter is what it was",
    );
}

/// **What the console prints is on stderr, in the ring, and under the `console`
/// target.**
///
/// The engine drains a `Context`'s lines through this in slice 5; what this
/// slice owes is that the one funnel exists and that a line taking it lands in
/// both places at once, so the panel and the terminal cannot show different
/// text.
#[test]
fn a_console_line_reaches_the_capture_and_the_ring_under_one_target() {
    let _order = with_filter("info");
    let message = "antialiasing = smaa";

    let logs = crcbl_core::log::capture();
    console::print(message);
    let records = logs.records();

    let mine: Vec<_> = records
        .iter()
        .filter(|record| record.message == message)
        .collect();
    assert_eq!(mine.len(), 1, "{records:?}");
    assert_eq!(mine[0].target, console::CONSOLE_TARGET);
    assert_eq!(mine[0].level, log::Level::Info);

    let ringed: Vec<_> = ringed(console::CONSOLE_TARGET)
        .into_iter()
        .filter(|record| record.message == message)
        .collect();
    assert_eq!(ringed.len(), 1, "the same line is in the ring: {ringed:?}");
    assert_eq!(ringed[0].level, log::Level::Info);
}
