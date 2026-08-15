//! Breakout as CI runs it: the real binary, no display, deterministic.
//!
//! `apps/breakout/src/app.rs` and `src/game.rs` unit-test the loop and the
//! simulation directly. This file tests the *deliverable* — the compiled
//! binary, its arguments and its exit codes — for the same reason
//! `apps/sandbox/tests/headless.rs` exists: that is what
//! `.github/workflows/ci.yml` and a developer both actually invoke.
//!
//! Breakout had neither this file nor a CI job of its own, so nothing outside
//! the crate's own unit tests ever ran the first playable sample.
//!
//! Everything here runs on Linux, macOS and Windows with no window system
//! present.

use std::process::{Command, Output};

fn breakout(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_breakout"))
        .args(args)
        .output()
        .expect("the breakout binary runs")
}

/// Breakout headless, with the null GPU backend pinned.
///
/// Same argument as the sandbox's: auto-selection deliberately refuses to fall
/// through to `null`, so a runner with no usable Vulkan driver has no
/// auto-selectable backend at all. Pinning it is what makes these assertions
/// mean the same thing on every platform.
///
/// `--headless` is pinned here rather than restated at every call site because
/// omitting it does not fail a test, it makes the test do something else
/// entirely: the binary opens a real Wayland or X11 window, opens a real audio
/// output device, and creates this platform's config directory for `breakout`
/// on the developer's disk.
/// Worse for the run that passes no `--frames`, which relies on headless mode
/// for its budget: windowed it has none, so the test binary waits forever for a
/// window nobody is there to close rather than reporting a failure.
fn breakout_null(args: &[&str]) -> Output {
    let mut argv = vec!["--backend", "null", "--headless"];
    argv.extend_from_slice(args);
    breakout(&argv)
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
    let output = breakout_null(&["--frames", "24"]);
    assert_eq!(
        code(&output),
        0,
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary = stdout(&output);
    assert!(summary.contains("24 frames"), "{summary}");
    assert!(summary.contains("headless shell"), "{summary}");
    assert!(summary.contains("960x720"), "{summary}");
    assert!(summary.contains("score"), "{summary}");
}

/// Determinism is what makes this assertable in CI at all: two runs of the same
/// binary must agree on the tick count and on the game state they reached, not
/// merely both succeed.
#[test]
fn two_headless_runs_produce_identical_output() {
    let first = breakout_null(&["--frames", "24"]);
    let second = breakout_null(&["--frames", "24"]);
    assert_eq!(stdout(&first), stdout(&second));
    // 24 frames of 1/60 s, the first of which only establishes the clock's
    // baseline: 23 ticks at the default 60 Hz.
    assert!(stdout(&first).contains("23 ticks"), "{}", stdout(&first));
}

/// A frame budget is not optional headless: without one the loop would never
/// end, because nothing will ever close a window that does not exist.
#[test]
fn headless_terminates_without_being_given_a_budget() {
    let output = breakout_null(&[]);
    assert_eq!(code(&output), 0);
    assert!(
        stdout(&output).contains("120 frames"),
        "{}",
        stdout(&output)
    );
}

/// The same exit-code contract the `crcbl` CLI and the sandbox state, because
/// CI scripts read all three the same way.
#[test]
fn a_bad_invocation_exits_two_and_help_exits_zero() {
    assert_eq!(code(&breakout(&["--frames"])), 2);
    assert_eq!(code(&breakout(&["--nonsense"])), 2);
    assert_eq!(code(&breakout(&["--tick-hz", "0"])), 2);
    assert_eq!(code(&breakout(&["--frames", "0"])), 2);
    assert_eq!(code(&breakout(&["--backend", "opengl"])), 2);

    let help = breakout(&["--help"]);
    assert_eq!(code(&help), 0);
    assert!(stdout(&help).contains("--headless"));
}

/// `--backend vk` is the spelling every `run-*-e2e.sh` harness passes and the
/// sandbox accepts. Breakout used to reject it with exit 2.
#[test]
fn the_backend_flag_accepts_the_same_names_the_sandbox_does() {
    // `vk` need not *open* here (a runner may have no driver) — it must merely
    // parse. Anything but exit 2 proves the argument was understood.
    assert_ne!(
        code(&breakout(&[
            "--headless",
            "--frames",
            "1",
            "--backend",
            "vk"
        ])),
        2
    );
    assert_eq!(code(&breakout_null(&["--frames", "1"])), 0,);
}

/// The tick rate is a knob, and it changes the simulation rather than the
/// frame rate — the property a fixed-timestep accumulator exists to have. It
/// used to reach only a counter in the summary.
#[test]
fn the_tick_rate_changes_ticks_and_not_frames() {
    let output = breakout_null(&["--frames", "62", "--tick-hz", "30"]);
    assert_eq!(code(&output), 0);
    let summary = stdout(&output);
    assert!(summary.contains("62 frames"), "{summary}");
    assert!(summary.contains("30 ticks"), "{summary}");
}

/// A long enough headless run actually plays: the ball launches only on input,
/// so a run with none stays in `WaitingForLaunch` at score zero — and that is
/// the state the summary must report rather than a fabricated one.
#[test]
fn a_run_with_no_input_waits_for_a_launch() {
    let output = breakout_null(&["--frames", "180"]);
    assert_eq!(code(&output), 0);
    let summary = stdout(&output);
    assert!(summary.contains("score 0"), "{summary}");
    assert!(summary.contains("WaitingForLaunch"), "{summary}");
}
