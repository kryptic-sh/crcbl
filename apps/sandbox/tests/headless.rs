//! The sandbox as CI runs it: the real binary, no display, deterministic.
//!
//! `apps/sandbox/src/app.rs` unit-tests the loop directly. This file tests the
//! *deliverable* — the compiled binary, its arguments and its exit codes —
//! because that is what `.github/workflows/ci.yml` and a developer both
//! actually invoke, and because `docs/plan/01-foundations.md`'s exit criterion
//! is about the sandbox, not about a function inside it.
//!
//! Everything here runs on Linux, macOS and Windows with no window system
//! present, which is what makes the cross-platform CI leg meaningful rather
//! than a compile check.

use std::process::{Command, Output};

fn sandbox(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sandbox"))
        .args(args)
        .output()
        .expect("the sandbox binary runs")
}

/// The sandbox headless, with the null GPU backend pinned.
///
/// These tests are about the *loop* — frame and tick counts, exit codes,
/// determinism — none of which involve a GPU. Auto-selection deliberately
/// refuses to fall through to `null` (`crcbl::backend`: a game that silently
/// rendered nothing is worse than one that failed), so a machine without a
/// usable Vulkan driver has no auto-selectable backend at all. That is every
/// macOS and Windows runner, and the Linux `test` job, which installs no ICD.
/// Pinning the backend is what makes these assertions mean the same thing on
/// all three platforms; real Vulkan is covered by `run-vk-e2e.sh` and by the
/// two shell harnesses, which run against an ICD on purpose.
///
/// `--headless` is pinned here rather than restated at every call site because
/// omitting it does not fail a test, it makes the test do something else
/// entirely: the binary opens a real Wayland or X11 window on whatever display
/// the developer is sitting in front of. Worse for the run that passes no
/// `--frames`, which relies on headless mode for its budget: windowed it has
/// none, so the test binary waits forever for a window nobody is there to close
/// rather than reporting a failure.
fn sandbox_null(args: &[&str]) -> Output {
    let mut argv = vec!["--backend", "null", "--headless"];
    argv.extend_from_slice(args);
    sandbox(&argv)
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("not killed by a signal")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The CI job's whole contract: it terminates, it exits 0, and it says what it
/// did.
#[test]
fn a_headless_run_exits_zero_with_a_summary() {
    let output = sandbox_null(&["--frames", "24"]);
    assert_eq!(
        code(&output),
        0,
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary = stdout(&output);
    assert!(summary.contains("24 frames"), "{summary}");
    assert!(summary.contains("headless shell"), "{summary}");
    assert!(summary.contains("1280x720"), "{summary}");
}

/// Determinism is the property that makes this assertable in CI at all. Two
/// runs of the same binary with the same arguments must not merely both
/// succeed — they must agree on the tick count, which is what would drift if
/// the loop ever started reading the wall clock in headless mode.
#[test]
fn two_headless_runs_produce_identical_output() {
    let first = sandbox_null(&["--frames", "24"]);
    let second = sandbox_null(&["--frames", "24"]);
    assert_eq!(stdout(&first), stdout(&second));
    // 24 frames of 1/60 s, the first of which only establishes the clock's
    // baseline: 23 ticks at the default 60 Hz.
    assert!(stdout(&first).contains("23 ticks"), "{}", stdout(&first));
}

/// A frame budget is not optional headless: without one the loop would never
/// end, because nothing will ever close a window that does not exist.
#[test]
fn headless_terminates_without_being_given_a_budget() {
    let output = sandbox_null(&[]);
    assert_eq!(code(&output), 0);
    assert!(
        stdout(&output).contains("120 frames"),
        "{}",
        stdout(&output)
    );
}

/// The same exit-code contract the `crcbl` CLI states, because CI scripts read
/// both the same way.
#[test]
fn a_bad_invocation_exits_two_and_help_exits_zero() {
    assert_eq!(code(&sandbox(&["--frames"])), 2);
    assert_eq!(code(&sandbox(&["--nonsense"])), 2);
    assert_eq!(code(&sandbox(&["--tick-hz", "0"])), 2);

    let help = sandbox(&["--help"]);
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).contains("--headless"));
}

/// The tick rate is a knob, and it changes the simulation rather than the
/// frame rate — the property a fixed-timestep accumulator exists to have.
#[test]
fn the_tick_rate_changes_ticks_and_not_frames() {
    let output = sandbox_null(&["--frames", "62", "--tick-hz", "30"]);
    assert_eq!(code(&output), 0);
    let summary = stdout(&output);
    assert!(summary.contains("62 frames"), "{summary}");
    assert!(summary.contains("30 ticks"), "{summary}");
}
