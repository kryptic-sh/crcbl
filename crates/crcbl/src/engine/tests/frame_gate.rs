//! The loop presenting only when the window system asks: a frame request
//! before every present, the answer fed back from the pump, and a frame the
//! answer has not reached held without stopping the simulation or the run.

use super::*;

use crcbl_shell::ShellCaps;

/// Frames a held stretch runs, past the reconfigure limit so a held frame
/// counted as a failed present would have ended the run inside it.
const HELD: u32 = MAX_CONSECUTIVE_RECONFIGURES + 10;

/// A loop hosting [`FakeGame`] on a headless shell reporting `caps`, its
/// frames presented through a null context that records into the returned
/// recorder.
fn paced(
    caps: ShellCaps,
    clock_source: Clock,
    config: LoopConfig,
) -> (Hosted, crcbl_hal::null::Recorder) {
    let (shell, window, recorder, context) = null_context("frame gate", Pacing::Vsync);
    let engine = Loop::new(
        Booted {
            shell: Box::new(shell.with_caps(caps)),
            window,
            gpu: FakeGpu::presenting(context),
            clock_source,
            events: 0,
        },
        FakeGame::default(),
        config,
    );
    (engine, recorder)
}

/// [`paced`] on the headless clock, with no idling: the frames the tests
/// below count are the frames they step.
fn headless(caps: ShellCaps) -> (Hosted, crcbl_hal::null::Recorder) {
    paced(caps, Clock::new(true), hosted_config(None))
}

/// One frame through the whole loop, budget check and idle included.
fn frame(engine: &mut Hosted) {
    assert_eq!(
        engine.frame().expect("a held frame is not a failed one"),
        Flow::Continue
    );
}

/// Whether the loop's window has a frame request the shell has not answered.
fn requested(engine: &Hosted) -> bool {
    engine
        .shell
        .frame_requested(engine.window)
        .expect("the window is live")
}

/// **A held frame is neither a failed present nor a presented one.** A
/// window on an output that stays off is held for as long as it stays off,
/// and a run that counted those frames toward
/// [`MAX_CONSECUTIVE_RECONFIGURES`] would end with
/// [`LoopError::NeverPresented`] over a surface nothing is wrong with.
#[test]
fn a_held_frame_counts_toward_neither_the_reconfigure_limit_nor_the_budget() {
    let (mut engine, recorder) = headless(ShellCaps::DESKTOP);
    frame(&mut engine);
    assert_eq!(presents(&recorder), 1, "the first frame is never held");
    assert!(requested(&engine), "and it asked for the next one");

    engine.shell_mut().set_outputs_powered(false);
    for _ in 0..HELD {
        frame(&mut engine);
    }
    assert_eq!(presents(&recorder), 1, "nothing presented while held");
    assert_eq!(engine.gpu.frames, 1, "and the bundle was never asked to");
    assert_eq!(
        engine.budget.presented(),
        1,
        "a held frame is not presented"
    );
    assert_eq!(
        engine.budget.reconfigures_in_a_row, 0,
        "nor is it a frame that failed to present"
    );
    assert_eq!(reconfigures(&recorder), 0);
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// The outcome on its own: a held frame leaves the streak where it was —
/// neither growing it nor resetting it — and the presented count too.
#[test]
fn a_recorded_held_frame_leaves_the_streak_and_the_count_alone() {
    let mut budget = FrameBudget::new(None);
    for _ in 0..HELD {
        budget
            .record::<core::convert::Infallible>(FrameOutcome::Held)
            .expect("holding is never an error");
    }
    assert_eq!(budget.presented(), 0);
    assert_eq!(budget.reconfigures_in_a_row, 0);

    for _ in 1..MAX_CONSECUTIVE_RECONFIGURES {
        budget
            .record::<core::convert::Infallible>(FrameOutcome::Reconfigured)
            .expect("under the limit");
    }
    budget
        .record::<core::convert::Infallible>(FrameOutcome::Held)
        .expect("holding is never an error");
    let error = budget
        .record::<core::convert::Infallible>(FrameOutcome::Reconfigured)
        .expect_err("the held frame did not reset the streak");
    assert!(matches!(error, LoopError::NeverPresented), "{error}");
}

/// **The simulation does not wait for the display.** Every held frame still
/// runs the ticks its time owes: replays, multiplayer and determinism all
/// assume the simulation's schedule is its own.
#[test]
fn ticks_keep_their_schedule_while_frames_are_held() {
    /// Held frames stepped; each is one tick on the headless clock.
    const FRAMES: u64 = 30;

    let (mut engine, recorder) = headless(ShellCaps::DESKTOP);
    frame(&mut engine);
    engine.shell_mut().set_outputs_powered(false);
    let (loop_ticks, game_ticks) = (engine.ticks, engine.game.ticks);
    for _ in 0..FRAMES {
        frame(&mut engine);
    }
    assert_eq!(presents(&recorder), 1, "every one of them was held");
    assert_eq!(
        engine.ticks - loop_ticks,
        FRAMES,
        "one tick a frame, held or not"
    );
    assert_eq!(engine.game.ticks - game_ticks, FRAMES);
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The answer is what lets the next frame present**, and that frame asks
/// again: the loop feeds the context what the pump delivers.
#[test]
fn the_frame_after_the_answer_presents_and_asks_again() {
    let (mut engine, recorder) = headless(ShellCaps::DESKTOP);
    frame(&mut engine);
    engine.shell_mut().set_outputs_powered(false);
    for _ in 0..3 {
        frame(&mut engine);
    }
    assert_eq!(presents(&recorder), 1);

    engine.shell_mut().set_outputs_powered(true);
    frame(&mut engine);
    assert_eq!(presents(&recorder), 2, "the answer reopened the gate");
    assert_eq!(engine.budget.presented(), 2);
    assert!(
        requested(&engine),
        "and the frame it let through asked again"
    );

    // Answered at every pump while the output is on: a frame a pump.
    for _ in 0..3 {
        frame(&mut engine);
    }
    assert_eq!(presents(&recorder), 5);
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Only a compositor that answers is asked.** Without
/// [`ShellCaps::FRAME_CALLBACK`] the answer would come at the next pump
/// whatever the window's state, and a pending one ends every
/// [`Shell::wait_events`] at once — so the loop does not ask, and presents
/// every frame as it did before frame requests existed.
#[test]
fn a_shell_whose_compositor_does_not_pace_frames_is_not_asked() {
    /// Frames stepped.
    const FRAMES: usize = 5;

    let (mut engine, recorder) = headless(ShellCaps::DESKTOP.difference(ShellCaps::FRAME_CALLBACK));
    // Off, so an answer would be held and a request would show.
    engine.shell_mut().set_outputs_powered(false);
    for _ in 0..FRAMES {
        frame(&mut engine);
    }
    assert!(
        !requested(&engine),
        "the loop asked a shell that does not pace"
    );
    assert_eq!(presents(&recorder), FRAMES, "and so nothing was held");
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A held frame sleeps until the next tick, not until never and not at
/// all.** On a real clock with no frame limit the limiter offers no idle —
/// [`a_windowed_loop_idles_only_until_its_frame_limiters_deadline`] — so a
/// held loop that took the limiter's answer would spin, and one that waited
/// unbounded would stop the simulation with the display. Observed through the
/// headless shell's count of waits, and the bound read off the loop itself.
#[test]
fn a_held_frame_sleeps_until_the_next_tick() {
    /// Held frames stepped, each a real sleep of up to a tick.
    const FRAMES: u32 = 3;
    /// A slow tick, so no frame's own work outlasts one. At the loop's usual
    /// rate a slow runner's frame can take longer than a tick, and then the
    /// next tick is already due when the frame would wait — the loop rightly
    /// does not sleep, and a count of one sleep a frame came up short on
    /// macOS (2026-10-22, run 39791350330).
    const TICK_HZ: u32 = 4;

    let mut clock_source = Clock::new(false);
    clock_source.set_limit(FrameLimit::unlimited());
    let (mut engine, recorder) = paced(
        ShellCaps::DESKTOP,
        clock_source,
        LoopConfig {
            tick_hz: TICK_HZ,
            windowed: true,
            limit: FrameLimit::unlimited(),
            ..hosted_config(None)
        },
    );
    // Counted from here: waiting for the configure waited too.
    let waits = engine.shell.wait_count();
    frame(&mut engine);
    assert_eq!(
        engine.shell.wait_count(),
        waits,
        "an unlimited frame does not idle"
    );

    engine.shell_mut().set_outputs_powered(false);
    let idle = engine
        .held_idle()
        .expect("a tick is always ahead of a held frame");
    assert!(
        idle <= engine.frame_clock.tick_dt(),
        "offered {idle:?}, past the next tick"
    );
    let ticks = engine.ticks;
    for _ in 0..FRAMES {
        frame(&mut engine);
    }
    assert_eq!(
        engine.shell.wait_count() - waits,
        FRAMES,
        "one sleep a held frame"
    );
    assert_eq!(presents(&recorder), 1);
    assert!(
        engine.ticks - ticks >= u64::from(FRAMES),
        "each sleep ends where a tick is due, so each frame ran one; ran {}",
        engine.ticks - ticks
    );
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **The shared page bundle tells a held frame from a reconfigure**, though
/// its context's `acquire` answers `None` for both: a bundle driven outside
/// [`Loop`], which does not ask before each frame, records the right one.
#[test]
fn the_page_bundle_reports_a_held_frame_as_held() {
    let (mut shell, window, recorder, context) = null_context("frame gate page", Pacing::Vsync);
    let mut page = PageBundle::new(context, "frame gate page", [0.0; 4]).expect("a page bundle");
    page.context_mut()
        .request_frame(&mut shell)
        .expect("the window is live");
    assert_eq!(page.frame().expect("a frame"), FrameOutcome::Presented);
    assert_eq!(page.frame().expect("a frame"), FrameOutcome::Held);
    assert_eq!(presents(&recorder), 1);
    assert_eq!(reconfigures(&recorder), 0, "nothing was rebuilt");

    shell.pump(&mut |event| page.context_mut().observe(&event));
    assert_eq!(page.frame().expect("a frame"), FrameOutcome::Presented);
    page.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
}
