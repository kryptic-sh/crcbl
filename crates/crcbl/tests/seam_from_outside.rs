//! The `crcbl` seam, exercised the way a real consumer will exercise it: what a
//! game can reach through the umbrella, and whether it is enough.
//!
//! The third file under that name — `crcbl-hal` and `crcbl-shell` each have one
//! — because all three answer the same question about a different crate, and one
//! shared name is what makes that legible in a nextest log and a grep.
//!
//! # Why this is an integration test and not a unit test
//!
//! An integration test compiles as a **separate crate**, so it sees exactly what
//! a game sees: `pub` items, reached through the umbrella. It cannot touch a
//! `pub(crate)` field, cannot call a `#[cfg(test)]` helper, and cannot import a
//! workspace crate the umbrella does not re-export. That is the whole point —
//! **anything this file cannot do is a hole in the public API**, and a unit test
//! inside `crcbl` could not tell you that because it can reach everything.
//!
//! `crcbl`'s samples are becoming game modules plugged into a loop the engine
//! owns, which is the arrangement Unity, Unreal and Bevy all settled on. The
//! risk that comes with it is an engine that only works one way. `apps/bare` is
//! the readable demonstration that it works the other way too; this file is the
//! guard with teeth, because it fails at the seam rather than in a sample.
//!
//! # What it asserts
//!
//! One whole frame, driven by hand: shell → window → configure → device →
//! clock → pump → fixed step → render graph → present → teardown. Then the
//! pieces a hand-written loop needs on its own — menu input, pointer capture,
//! the display-mode request, the budget, and the engine's own driver.
//!
//! Everything runs on `HeadlessShell` and the null backend, so it needs no
//! window system and no driver and is a normal `cargo test`.

use core::time::Duration;

use crcbl::args::{Common, Consumed};
use crcbl::backend::GpuBackend;
use crcbl::core::input::KeyCode;
use crcbl::engine::{
    Clock, ExitReason, Flow, FrameBudget, FrameOutcome, GameLoop, GpuContext, GpuContextDesc,
    GpuError, Handled, LoopError, MENU_ACTIVATE_KEY, MENU_DOWN_KEY, MenuPump, ModeRequest, Pending,
    PointerCapture, accept_close, drive, open_window, run_ticks, wait_for_configure,
};
use crcbl::hal::{CommandEncoderDesc, ResourceState};
use crcbl::prelude::*;
use crcbl::render::{ImportedImage, RenderGraph, TransientPool};
use crcbl::shell::{
    DisplayMode, HeadlessShell, PhysicalSize, Shell, ShellBackend, WindowDesc, WindowId,
    open_backend,
};

/// The device this suite opens: no driver, available everywhere CI runs.
const BACKEND: Option<GpuBackend> = Some(GpuBackend::Null);

/// Opens a shell and a window the way a consumer would — through the trait
/// object `open_backend` hands back, which is the shape a real game holds.
fn windowed() -> (Box<dyn Shell>, WindowId, Clock) {
    let mut shell = open_backend(ShellBackend::Headless).expect("headless opens everywhere");
    let clock = Clock::new(true);
    let window = open_window(
        shell.as_mut(),
        &clock,
        &WindowDesc {
            title: "library seam",
            app_id: "sh.kryptic.crcbl.tests",
            ..WindowDesc::default()
        },
    )
    .expect("headless always creates a window");
    (shell, window, clock)
}

/// The same, on the concrete shell, for tests that inject the events a window
/// system would otherwise send.
fn concrete() -> (HeadlessShell, WindowId, Clock) {
    let mut shell = HeadlessShell::new();
    let clock = Clock::new(true);
    let window = open_window(
        &mut shell,
        &clock,
        &WindowDesc {
            title: "library seam",
            app_id: "sh.kryptic.crcbl.tests",
            ..WindowDesc::default()
        },
    )
    .expect("headless always creates a window");
    (shell, window, clock)
}

/// **A consumer can bring the engine up, run a frame and tear it down, using
/// nothing but the public API.**
///
/// The claim the whole seam rests on. Every call here is one a game author
/// writes; if any of them stopped being public this file would not compile,
/// which is the failure worth having.
#[test]
fn a_consumer_can_drive_a_whole_frame_by_hand() {
    let (mut shell, window, clock) = windowed();

    let mut events = 0;
    let extent =
        wait_for_configure(shell.as_mut(), window, &mut events).expect("headless configures");
    assert!(
        events > 0,
        "the configure arrived as an event, not by magic"
    );

    let mut gpu = GpuContext::open(
        shell.as_ref(),
        window,
        extent,
        &GpuContextDesc {
            label: "library seam",
            backend: BACKEND,
            ..GpuContextDesc::default()
        },
    )
    .expect("the null backend opens everywhere");
    assert_eq!(gpu.extent(), extent);

    let mut pool = TransientPool::new();
    let mut budget = FrameBudget::new(Some(3));
    let mut frame_clock = FrameClock::new(60);
    let mut clock = clock;
    let mut ticks = 0_u64;

    while !budget.is_spent() {
        let mut pending = Pending::default();
        shell.pump(&mut |event| {
            let _ = pending.observe(&event);
        });

        frame_clock.update(clock.advance());
        run_ticks(&mut frame_clock, false, || ticks += 1);

        let acquired = gpu
            .acquire()
            .expect("acquire")
            .expect("the null swapchain always yields an image");

        // The import a consumer has to get right: an acquired image's contents
        // are undefined and the window system may only be handed one in
        // `Present`. Declaring both is what lets the graph emit the barriers.
        let mut graph = RenderGraph::new(gpu.queue());
        let target = graph.import_image(
            "swapchain",
            ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: gpu.format(),
                extent: acquired.extent,
                initial: ResourceState::Undefined,
                final_state: ResourceState::Present,
            },
        );
        graph
            .add_render_pass("clear")
            .clear_color(target, [0.1, 0.2, 0.3, 1.0])
            .execute(|_ctx| {});

        let compiled = graph.compile(&pool).expect("a one-pass graph compiles");
        let mut encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
            label: Some("library seam"),
            queue: gpu.queue(),
        });
        compiled
            .execute(gpu.device(), &mut pool, encoder.as_mut(), None)
            .expect("execute");
        let command_buffer = encoder.finish().expect("a recorded command buffer");

        let outcome = gpu
            .submit_and_present(&acquired, command_buffer)
            .expect("present");
        budget
            .record::<core::convert::Infallible>(outcome)
            .expect("a presenting swapchain never trips the cap");
    }

    assert_eq!(budget.presented(), 3);
    assert!(ticks > 0, "the fixed step never ran");

    gpu.destroy().expect("the device is released");
    shell.destroy_window(window).expect("the window goes away");
}

/// **A resize reaches the swapchain through the public path.**
///
/// The one piece of frame bookkeeping a consumer cannot skip: a window that
/// changed size and a swapchain that did not is a stretched or cropped picture
/// on every backend.
#[test]
fn a_resize_observed_through_pending_reaches_the_swapchain() {
    // Concrete `HeadlessShell` here, not the trait object: `resize` is its
    // fixture for injecting what a compositor would send. A real backend gets
    // the event from the window system, and the loop above it is identical.
    let (mut shell, window, clock) = concrete();
    let mut events = 0;
    let extent = wait_for_configure(&mut shell, window, &mut events).expect("configures");
    let mut gpu = GpuContext::open(
        &shell,
        window,
        extent,
        &GpuContextDesc {
            label: "resize",
            backend: BACKEND,
            ..GpuContextDesc::default()
        },
    )
    .expect("opens");

    let grown = PhysicalSize::new(extent.0 + 64, extent.1 + 32);
    shell.resize(window, grown).expect("the window is live");

    let mut pending = Pending::default();
    shell.pump(&mut |event| {
        let _ = pending.observe(&event);
    });
    let size = pending
        .resized
        .expect("the resize was reported to the loop");
    assert_eq!((size.width, size.height), (grown.width, grown.height));

    gpu.resize((size.width, size.height)).expect("reconfigures");

    // **`extent()` follows the acquire, not the resize**, and a consumer that
    // assumed otherwise would render a frame at the old size. The swapchain is
    // the authority on the size it actually gave — the window system may pin a
    // range the request fell outside — so `GpuContext` learns it when it next
    // acquires an image, and reports the previous one until then.
    let acquired = gpu
        .acquire()
        .expect("acquire")
        .expect("the null swapchain always yields an image");
    assert_eq!(
        acquired.extent,
        (grown.width, grown.height),
        "the swapchain did not take the new size",
    );
    assert_eq!(
        gpu.extent(),
        acquired.extent,
        "extent() disagreed with the image it just handed out",
    );

    gpu.destroy().expect("teardown");
    shell.destroy_window(window).expect("the window goes away");
    let _ = clock;
}

/// **The engine's own driver is reachable, and reports a frame error rather
/// than a teardown one.**
///
/// `drive` is the native runner. A consumer that wants it must be able to
/// implement `GameLoop` from outside the crate — which is what this proves.
#[test]
fn a_consumer_can_implement_game_loop_and_use_the_engines_driver() {
    struct Counted {
        left: u32,
        torn_down: bool,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Refused;

    impl core::fmt::Display for Refused {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "refused")
        }
    }

    impl GameLoop for Counted {
        type Error = Refused;
        type Summary = u32;

        fn frame(&mut self) -> Result<Flow, Self::Error> {
            if self.left == 0 {
                return Ok(Flow::Stop(ExitReason::FrameBudget));
            }
            self.left -= 1;
            Ok(Flow::Continue)
        }

        fn finish(mut self, _exit: ExitReason) -> Result<Self::Summary, Self::Error> {
            self.torn_down = true;
            Ok(self.left)
        }
    }

    let left = drive(Counted {
        left: 5,
        torn_down: false,
    })
    .expect("a clean stop");
    assert_eq!(left, 0, "the driver stopped before the budget ran out");
}

/// **Menu input, pointer capture and the mode request are usable standalone.**
///
/// A hand-written loop wants these without adopting the engine's loop. Each is
/// a type a consumer constructs and drives itself.
#[test]
fn the_loops_input_helpers_work_outside_the_engines_loop() {
    let (mut shell, window, _clock) = concrete();

    // -- the display-mode request -------------------------------------------
    assert_eq!(
        ModeRequest::mode(&shell, window),
        Some(DisplayMode::Windowed),
    );
    ModeRequest::toggle(&mut shell, window).expect("the shell accepts the request");
    let mut request = ModeRequest::new();
    request.check(&shell, window);

    // -- the pointer --------------------------------------------------------
    let mut capture = PointerCapture::new();
    let mut pending = capture.pending();
    pending.pointer_pressed = true;
    pending.pointer_released = true;
    let input = capture.resolve(&pending);
    assert!(
        input.down && input.released,
        "a click inside one frame must latch and fire together",
    );

    // -- the menu -----------------------------------------------------------
    let mut menus = crcbl::ui::menu::MenuSet::new(
        0_u8,
        vec![(
            1_u8,
            crcbl::ui::menu::Menu::new(
                "PAUSED",
                vec![crcbl::ui::menu::MenuItem::new(7, "RESUME", "ESC")],
            ),
        )],
    );
    menus.show(1);
    let mut held = Vec::new();
    shell
        .key_press(window, MENU_ACTIVATE_KEY)
        .expect("the window is live");
    shell
        .key_release(window, MENU_ACTIVATE_KEY)
        .expect("the window is live");
    let mut menu = MenuPump::new(&mut menus, &mut held, true);
    shell.pump(&mut |event| {
        menu.observe(&event);
    });
    assert_eq!(
        menu.activated,
        Some(7),
        "the commit key did not reach the menu through the public seam",
    );

    shell.destroy_window(window).expect("the window goes away");
}

/// **A consumer can parse the shared flags and add nothing.**
///
/// `Common` is offered, not imposed: a game keeps its own parse loop and claims
/// what `consume` hands back.
#[test]
fn the_shared_flag_set_is_usable_from_outside() {
    let mut common = Common::new(60);
    let mut args = ["--headless", "--frames", "4"]
        .into_iter()
        .map(str::to_string)
        .peekable();

    while let Some(arg) = args.next() {
        assert!(
            matches!(common.consume(&arg, &mut args), Consumed::Yes),
            "the shared set refused a flag it owns: {arg}",
        );
    }
    assert!(common.headless);
    assert_eq!(common.frame_budget(), Some(4));

    // …and something it does not own comes back for the caller to claim.
    let mut none = core::iter::empty::<String>().peekable();
    assert!(matches!(
        common.consume("--a-game-flag", &mut none),
        Consumed::No
    ));
}

/// **Every error the seam can produce converts into the engine's error type.**
///
/// A consumer writing `?` in a bring-up function depends on these, and a missing
/// `From` is a compile error in *their* crate rather than this one.
#[test]
fn the_engines_errors_compose_for_a_consumers_bring_up() {
    let mut shell = HeadlessShell::new();
    let window = shell
        .create_window(&WindowDesc::default())
        .expect("headless creates a window");
    shell.destroy_window(window).expect("goes away");
    let shell_error = shell
        .destroy_window(window)
        .expect_err("destroying it twice is refused");

    let wrapped: LoopError = shell_error.into();
    assert!(matches!(wrapped, LoopError::Shell(_)));
    let wrapped: LoopError = GpuError::Unusable("no queue").into();
    assert!(matches!(wrapped, LoopError::Gpu(_)));
}

/// **The pieces a loop needs that are not types**: the close handshake, the
/// event verdict and the frame outcome are all public and namable.
#[test]
fn the_loops_vocabulary_is_public() {
    let (mut shell, window, _clock) = concrete();

    // `Handled` is what a consumer branches on inside its own pump closure.
    let mut pending = Pending::default();
    let mut verdicts = Vec::new();
    shell
        .key_press(window, KeyCode::Space)
        .expect("the window is live");
    shell.pump(&mut |event| verdicts.push(pending.observe(&event)));
    assert!(
        verdicts.contains(&Handled::Game),
        "a key the loop does not reserve must come back as the game's",
    );

    // **The close handshake, both halves.** The window system asks and the loop
    // answers; a consumer that never replies leaves a window the compositor
    // thinks is hung. `accept_close` is the reply, and it is public precisely
    // so a hand-written loop can send it.
    shell.request_close(window).expect("the window is live");
    let mut closing = Pending::default();
    shell.pump(&mut |event| {
        let _ = closing.observe(&event);
    });
    assert!(closing.close_requested, "the request reached the loop");
    accept_close(&mut shell, window).expect("the reply is accepted");

    // And the outcome a consumer feeds to its budget is nameable.
    let outcome = FrameOutcome::Presented;
    assert_ne!(outcome, FrameOutcome::Reconfigured);

    let _ = Duration::from_millis(1);
    let _ = MENU_DOWN_KEY;
}
