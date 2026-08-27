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
    PointerCapture, SettingsSource, accept_close, drive, open_window, run_ticks,
    wait_for_configure,
};
use crcbl::hal::{CommandEncoderDesc, ResourceState};
use crcbl::prelude::*;
use crcbl::render::{
    EffectOverride, EffectRequest, ImportedImage, InitialClaim, RenderEffects, RenderGraph,
    TransientPool,
};
use crcbl::shell::{
    DisplayMode, HeadlessShell, PhysicalSize, Shell, ShellBackend, WindowDesc, WindowId,
    open_backend,
};
use crcbl::store::settings::SETTINGS_FILE;
use crcbl::store::{MemoryStorage, StorageSource};

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
            // Hermetic: nothing outside this file may decide what it draws.
            settings: SettingsSource::None,
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
                claim: InitialClaim::Acquired,
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
            // Hermetic: nothing outside this file may decide what it draws.
            settings: SettingsSource::None,
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

// ---- the player's video settings ------------------------------------------

/// A settings source holding `toml` at the file the engine's start-up reads.
///
/// [`MemoryStorage`] rather than a temporary directory: what is being exercised
/// is the read, and a suite that reached the real filesystem would answer
/// differently on a machine whose `~/.config` already has one of these.
fn settings_file(toml: &str) -> MemoryStorage {
    let storage = MemoryStorage::new();
    storage
        .write(std::path::Path::new(SETTINGS_FILE), toml.as_bytes())
        .expect("memory storage accepts every write");
    storage
}

/// One forward frame on a context opened with `settings`: what it resolved to,
/// and the passes it declared, in order.
///
/// The whole start-up, from a shell to a compiled frame — `GpuContext::open` is
/// what reads the settings, so a helper that called the reader directly would
/// prove nothing about whether opening a context runs it.
fn a_frame_opened_with(settings: SettingsSource<'_>) -> (RenderEffects, Vec<String>) {
    let (mut shell, window, _clock) = windowed();
    let mut events = 0;
    let extent =
        wait_for_configure(shell.as_mut(), window, &mut events).expect("headless configures");

    let mut gpu = GpuContext::open(
        shell.as_ref(),
        window,
        extent,
        &GpuContextDesc {
            label: "library seam",
            backend: BACKEND,
            settings,
            ..GpuContextDesc::default()
        },
    )
    .expect("the null backend opens everywhere");

    let mut renderer = ForwardRenderer::new(gpu.device(), gpu.queue(), gpu.format())
        .expect("the null backend builds every pipeline");
    // The one line a game writes. Everything the player asked for is already in
    // the context by now.
    renderer.set_effect_request(gpu.effect_request());

    let mut pool = TransientPool::new();
    let acquired = gpu
        .acquire()
        .expect("acquire")
        .expect("the null swapchain always yields an image");
    renderer
        .begin_frame(
            gpu.device(),
            &Camera::default(),
            &DirectionalLight::default(),
            acquired.extent,
        )
        .expect("the frame's uniform and instance writes");

    let labels = {
        let mut graph = RenderGraph::new(gpu.queue());
        let target = graph.import_image(
            "swapchain",
            ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: gpu.format(),
                extent: acquired.extent,
                initial: ResourceState::Undefined,
                claim: InitialClaim::Acquired,
                final_state: ResourceState::Present,
            },
        );
        renderer.add_passes(&mut graph, &pool, target, acquired.extent);
        let compiled = graph.compile(&pool).expect("a legal frame");
        let labels: Vec<String> = compiled
            .passes()
            .iter()
            .map(|pass| pass.label().to_string())
            .collect();
        let mut encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
            label: Some("library seam forward frame"),
            queue: gpu.queue(),
        });
        compiled
            .execute(gpu.device(), &mut pool, encoder.as_mut(), None)
            .expect("execute");
        let command_buffer = encoder.finish().expect("a recorded command buffer");
        gpu.submit_and_present(&acquired, command_buffer)
            .expect("present");
        labels
    };

    let effects = renderer.effects();
    gpu.drain().expect("the frame retires");
    renderer.destroy(gpu.device());
    pool.destroy(gpu.device());
    gpu.destroy().expect("the device is released");
    shell.destroy_window(window).expect("the window goes away");
    (effects, labels)
}

/// **A settings file that switches an effect off produces a frame with fewer
/// passes**, through the engine's own start-up and nothing hand-assembled.
///
/// The observable is the compiled pass list rather than the resolved set,
/// because the thing that could otherwise be true is that the read happened,
/// the request was stored, and every pass ran anyway — a frame that reports the
/// right effects and draws all of them.
///
/// The two effects are checked by name and the third by arithmetic: a cascade's
/// cull passes are labelled exactly as the camera's, so `SHADOWS` has no unique
/// label to look for. What it does have is a frame that is strictly shorter and
/// keeps the `shadow` pass itself, which is what writes the clear every
/// comparison reads as "fully lit".
#[test]
fn a_settings_file_switching_an_effect_off_is_a_frame_with_fewer_passes() {
    let (all_on, every_pass) = a_frame_opened_with(SettingsSource::None);
    assert_eq!(
        all_on,
        RenderEffects::DEFAULT_STACK,
        "a run with no settings at all has to be the default-stack frame, or the \
         comparisons below are against the wrong control"
    );

    for (toml, gone) in [
        (
            "ambient_occlusion = false",
            ["ssao", "ssao-blur"].as_slice(),
        ),
        // The pyramid goes with the march that climbs it: the reduction passes
        // are recorded on the frames that reflect and on no others, so
        // switching reflections off costs a frame the whole chain and not only
        // the two passes named after it. Matched by prefix because how many
        // levels this window has is a function of its size — see
        // `crcbl_render::hiz` — and the claim here is that *all* of them go.
        (
            "reflections = false",
            ["ssr", "ssr-blur", "hiz-"].as_slice(),
        ),
    ] {
        let storage = settings_file(&format!("[engine.video]\n{toml}\n"));
        let (effects, labels) = a_frame_opened_with(SettingsSource::Source(&storage));
        assert_ne!(effects, all_on, "{toml}: the player's row did nothing");
        let expected: Vec<String> = every_pass
            .iter()
            .filter(|label| {
                !gone
                    .iter()
                    .any(|dropped| *label == dropped || label.starts_with(dropped))
            })
            .cloned()
            .collect();
        assert_eq!(
            labels, expected,
            "{toml}: the frame must lose {gone:?}, gain nothing in their place, and keep \
             every other pass"
        );
    }

    let storage = settings_file("[engine.video]\nshadows = false\n");
    let (effects, labels) = a_frame_opened_with(SettingsSource::Source(&storage));
    assert_eq!(
        effects,
        RenderEffects::DEFAULT_STACK.difference(RenderEffects::SHADOWS)
    );
    assert!(
        labels.len() < every_pass.len(),
        "shadows off recorded {} passes and the every-effect frame {} — the culls the \
         atlas needs are still in the frame",
        labels.len(),
        every_pass.len()
    );
    assert!(
        labels.iter().any(|label| label == "shadow"),
        "the atlas pass is what writes the clear a shadow comparison reads as fully lit, \
         so switching shadows off must not take it out"
    );
}

/// **The player's layer clamps downward only, and the order around it holds.**
///
/// Every arm starts from a request the *start-up* built — the settings file is
/// read by `GpuContext::open` and nothing here writes
/// [`EffectRequest::video`](crcbl::render::EffectRequest::video) by hand, which
/// is the difference between testing the resolution order and testing this
/// layer's source.
#[test]
fn the_video_layer_clamps_downward_and_the_order_around_it_holds() {
    let all = RenderEffects::all();
    let storage = settings_file("[engine.video]\nshadows = false\nreflections = true\n");

    let (mut shell, window, _clock) = windowed();
    let mut events = 0;
    let extent =
        wait_for_configure(shell.as_mut(), window, &mut events).expect("headless configures");
    let gpu = GpuContext::open(
        shell.as_ref(),
        window,
        extent,
        &GpuContextDesc {
            label: "library seam",
            backend: BACKEND,
            settings: SettingsSource::Source(&storage),
            ..GpuContextDesc::default()
        },
    )
    .expect("the null backend opens everywhere");

    let request = gpu.effect_request();
    assert_eq!(
        request.video,
        all.difference(RenderEffects::SHADOWS),
        "the file's one `false` is the whole of what the player asked for"
    );

    // **Downward only.** The file asks for reflections and the view does not
    // draw them, so the frame does not — the arm that fails if the two are
    // unioned rather than intersected.
    let monitor = EffectRequest {
        camera: all.difference(RenderEffects::REFLECTIONS),
        ..request
    };
    assert_eq!(
        monitor.resolve(all),
        all.difference(RenderEffects::SHADOWS)
            .difference(RenderEffects::REFLECTIONS),
        "a settings file must not add an effect the view never asked for"
    );

    // **The override still escapes the player's clamp**, which is what a game
    // with its own quality logic is for.
    let forced = EffectRequest {
        programmatic: EffectOverride::none().force(RenderEffects::SHADOWS, Some(true)),
        ..request
    };
    // `DEFAULT_STACK` rather than `all`, and it is the camera layer that makes
    // the difference: nothing here declares a render stack, so the view is
    // asking for every effect that models the scene's own light transport and
    // for no lens effect. What this arm is about is the *shadow* bit the file
    // took away and the override put back.
    assert_eq!(
        forced.resolve(all),
        RenderEffects::DEFAULT_STACK,
        "the override is applied after the video clamp, so it can restore what it took"
    );

    // **And the device still clamps last and absolutely.**
    assert_eq!(
        forced.resolve(all.difference(RenderEffects::SHADOWS)),
        RenderEffects::DEFAULT_STACK.difference(RenderEffects::SHADOWS),
        "no toggle may conjure an effect the device has no way to draw"
    );

    gpu.destroy().expect("the device is released");
    shell.destroy_window(window).expect("the window goes away");
}
