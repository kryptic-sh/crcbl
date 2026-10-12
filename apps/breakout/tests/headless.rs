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

use std::process::Output;

fn breakout(args: &[&str]) -> Output {
    crcbl_spawn::command(env!("CARGO_BIN_EXE_breakout"))
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

/// The one line a refused run explains itself on.
///
/// Not the whole of stderr: `crcbl::args::run_front_end` prints the usage text
/// under the message, and the usage text names paths of its own — an assertion
/// over the lot would be reading `--scene`'s help as though it were the answer.
fn refusal(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find(|line| line.starts_with("breakout: "))
        .expect("a refused run says why")
        .to_string()
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
    assert!(
        stdout(&first).contains("23 ticks (23 simulated)"),
        "{}",
        stdout(&first)
    );
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

/// `--screenshot` names a file and the run leaves one there — checked on the
/// null backend, so it runs on every platform in the plain suite.
///
/// What this is *not* is a check on the picture: the null backend records
/// commands and draws nothing, so the PNG is uniform and
/// `tests/golden.rs` is what looks at pixels. What it pins is the wiring —
/// the flag reaches `crcbl::engine::GpuContext::set_screenshot`, the readback
/// lands, and the file is written — on a runner with no GPU at all, which is
/// every runner the golden suite skips.
///
/// The stale file is removed first and its absence is the whole assertion: a
/// `--screenshot` that quietly did nothing would otherwise pass on the previous
/// run's file forever.
#[test]
fn a_screenshot_run_leaves_a_file_behind() {
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("headless-shot.png");
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("could not clear {}: {error}", path.display()),
    }

    let output = crcbl_spawn::command(env!("CARGO_BIN_EXE_breakout"))
        .args(["--backend", "null", "--frames", "4", "--screenshot"])
        .arg(&path)
        .output()
        .expect("the breakout binary runs");
    assert_eq!(
        code(&output),
        0,
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        path.exists(),
        "breakout exited 0 and wrote no {}",
        path.display()
    );

    // **`--headless` was never passed.** The flag turns it on, and the summary
    // saying so is what proves the offscreen path was taken rather than a
    // window having been opened on the developer's display.
    assert!(
        stdout(&output).contains("headless shell"),
        "{}",
        stdout(&output)
    );
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

/// **A `--scene` the binary cannot read is exit 2, by key, line and column.**
///
/// The refusal is the half worth asserting through the real binary: a run that
/// fell back to the committed board when the directory it was pointed at did
/// not parse would play the game it always played and report nothing, which is
/// a level nobody could tell had failed to load.
#[test]
fn a_scene_directory_that_does_not_parse_is_refused_by_line_and_column() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("unparsable.scn");
    std::fs::create_dir_all(&dir).expect("the target tmp dir is writable");
    // A header whose field is misspelled: `deny_unknown_fields` is what turns
    // that into a position rather than a silently defaulted scene.
    std::fs::write(
        dir.join("scene.ron"),
        "Scene(\n    format: 0,\n    nome: \"board\",\n)\n",
    )
    .expect("the target tmp dir is writable");

    let output = breakout_null(&["--frames", "1", "--scene", dir.to_str().expect("utf-8")]);
    assert_eq!(code(&output), 2, "{}", stdout(&output));
    let message = refusal(&output);
    assert!(message.contains("scene.ron"), "{message}");
    assert!(message.contains("line 3"), "{message}");
    assert!(message.contains("column"), "{message}");
    assert!(message.contains("nome"), "{message}");
}

/// **A `--scene` with no `scene.ron` names the key that is missing**, and names
/// it the way the caller spelled it: the directory the flag points at *is* the
/// scene, so the header is `scene.ron` and not `board.scn/scene.ron`.
///
/// An empty board is the failure this refusal prevents. A loader that answered
/// "no bricks" for a directory with no header would start a run that is
/// instantly won.
#[test]
fn a_scene_directory_with_no_header_is_refused_by_the_missing_key() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("headerless.scn");
    std::fs::create_dir_all(&dir).expect("the target tmp dir is writable");

    let output = breakout_null(&["--frames", "1", "--scene", dir.to_str().expect("utf-8")]);
    assert_eq!(code(&output), 2, "{}", stdout(&output));
    let message = refusal(&output);
    assert!(message.contains("scene.ron"), "{message}");
    assert!(message.contains("path not found"), "{message}");
    assert!(
        !message.contains("board.scn"),
        "the key must be the caller's, not the built-in board's: {message}"
    );
}
