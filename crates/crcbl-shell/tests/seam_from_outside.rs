//! The shell seam, exercised the way a real consumer will exercise it.
//!
//! An *integration* test rather than an in-crate one for the reason
//! `crcbl-hal`'s equivalent gives: an in-crate test can reach private items, so
//! it cannot prove anything about what the crate exposes. This file compiles
//! against `crcbl-shell`'s public API only.
//!
//! It asserts four things:
//!
//! 1. **The seam is usable through a trait object.** The engine half of the
//!    session below takes `&mut dyn Shell`, and several tests hold
//!    `Box<dyn Shell>` outright. If [`Shell`] stopped being object-safe — which
//!    is what runtime Wayland-or-X11 selection needs — this file would not
//!    compile.
//! 2. **No platform type appears in a public signature.** Every value the seam
//!    produces is bound to an explicitly written type here. The only concrete
//!    implementation named anywhere is [`HeadlessShell`], at construction —
//!    deliberately, because asking for determinism is asking for a specific
//!    implementation. A method that returned a *backend* type would force a
//!    platform name into one of these annotations, and there is nowhere to
//!    write one.
//! 3. **A scripted session produces the engine-visible event sequence**,
//!    including resize and scale-factor changes.
//! 4. **The whole thing is deterministic** when driven from a
//!    [`ManualTime`](crcbl_core::time::ManualTime), which is what lets `crcbl
//!    sim` and CI run the identical engine loop.

use std::time::Duration;

use crcbl_core::time::{ManualTime, TimeSource};
use crcbl_core::{EventTime, FrameClock, SurfaceTarget};
use crcbl_shell::{
    AspectRatio, ButtonState, ClipboardContent, ClipboardOffer, ClipboardRequestId, CloseReply,
    CursorIcon, DisplayMode, HeadlessShell, KeyCode, LogicalSize, MimeType, MonitorId, MonitorInfo,
    PhysicalPoint, PhysicalSize, PointerButton, PointerMode, ReceivedMime, ScrollDelta, Shell,
    ShellBackend, ShellCaps, ShellError, ShellEvent, SizeConstraints, WindowDesc, WindowId,
    WindowState,
};

/// The window a sample or the sandbox would ask for.
fn sandbox_window() -> WindowDesc<'static> {
    WindowDesc {
        title: "crcbl sandbox",
        app_id: "sh.kryptic.crcbl.sandbox",
        size: LogicalSize::new(1280.0, 720.0),
        constraints: SizeConstraints::min(LogicalSize::new(320.0, 180.0)),
        mode: DisplayMode::Windowed,
        resizable: true,
        visible: true,
        accept_drops: true,
    }
}

/// Drains a pump into a list of event names — the form a scripted session
/// asserts on.
fn pump_names(shell: &mut dyn Shell) -> Vec<&'static str> {
    let mut names = Vec::new();
    shell.pump(&mut |event: ShellEvent| names.push(event.name()));
    names
}

/// The engine loop, written the way `docs/plan/10-wasm-webgpu.md` requires:
/// `pump` then `tick(dt)`, with the outer loop owned by the caller so a
/// browser's `requestAnimationFrame` can be that caller.
///
/// Nothing here is native-only, and nothing here names a backend.
struct Engine {
    clock: FrameClock,
    /// `None` until the window system has configured the window. A renderer
    /// that made this a plain `PhysicalSize` would have had to invent a value.
    size: Option<PhysicalSize>,
    scale_factor: f64,
    focused: bool,
    ticks: u64,
    swapchain_recreations: u32,
    should_exit: bool,
    keys_down: Vec<KeyCode>,
    aim: (f64, f64),
    typed: String,
}

impl Engine {
    fn new() -> Self {
        Self {
            clock: FrameClock::new(60),
            size: None,
            scale_factor: 1.0,
            focused: false,
            ticks: 0,
            swapchain_recreations: 0,
            should_exit: false,
            keys_down: Vec::new(),
            aim: (0.0, 0.0),
            typed: String::new(),
        }
    }

    /// One frame. `now` comes from the caller's time source, exactly as it
    /// would come from `performance.now()` in a browser.
    fn frame(&mut self, shell: &mut dyn Shell, window: WindowId, now: Duration) {
        let mut close_requested = false;
        shell.pump(&mut |event: ShellEvent| {
            match event {
                ShellEvent::Resized { size, .. } => {
                    if self.size != Some(size) {
                        self.size = Some(size);
                        self.swapchain_recreations += 1;
                    }
                }
                ShellEvent::ScaleFactorChanged {
                    scale_factor, size, ..
                } => {
                    self.scale_factor = scale_factor;
                    if self.size != Some(size) {
                        self.size = Some(size);
                        self.swapchain_recreations += 1;
                    }
                }
                ShellEvent::Focus { focused, .. } => {
                    self.focused = focused;
                    // The rule every consumer owes: focus loss releases
                    // everything. No platform delivers those key-ups.
                    if !focused {
                        self.keys_down.clear();
                    }
                }
                ShellEvent::Key {
                    key_code,
                    state,
                    repeat,
                    ..
                } => {
                    if let Some(key) = key_code
                        && !repeat
                    {
                        match state {
                            ButtonState::Pressed => self.keys_down.push(key),
                            ButtonState::Released => self.keys_down.retain(|held| *held != key),
                        }
                    }
                }
                // A camera reads `raw_delta` and never differences `abs`.
                ShellEvent::PointerMotion {
                    raw_delta: Some((dx, dy)),
                    ..
                } => {
                    self.aim.0 += dx;
                    self.aim.1 += dy;
                }
                ShellEvent::TextCommit { text, .. } => self.typed.push_str(&text),
                ShellEvent::CloseRequested { .. } => close_requested = true,
                _ => {}
            }
        });

        if close_requested {
            // A real game shows a save prompt here; the point is that it *can*,
            // because the window is still open.
            shell
                .reply_close_request(window, CloseReply::Close)
                .expect("a close request was outstanding");
            self.should_exit = true;
        }

        self.clock.update(now);
        while self.clock.consume_tick() {
            self.ticks += 1;
        }
        // render(self.clock.alpha()) would go here.
    }
}

/// The whole point: a full session where the engine half only ever sees
/// `&mut dyn Shell`, and no platform type appears anywhere.
///
/// The test holds the concrete [`HeadlessShell`] because scripting a session
/// means driving a *specific* implementation — that is what determinism is —
/// but [`Engine::frame`] below takes `&mut dyn Shell` and is the code a real
/// game would ship.
#[test]
fn a_scripted_session_runs_the_engine_loop_through_a_trait_object() {
    let mut shell = HeadlessShell::new();
    {
        let seam: &dyn Shell = &shell;
        assert_eq!(seam.backend(), ShellBackend::Headless);
        let monitors: &[MonitorInfo] = seam.monitors();
        assert_eq!(monitors.len(), 1);
        let primary: MonitorId = monitors[0].id;
        assert!(monitors[0].is_primary);
        assert_eq!(monitors[0].size(), PhysicalSize::new(1920, 1080));
        assert!(seam.monitor(primary).is_some());
    }

    let window: WindowId = shell
        .create_window(&sandbox_window())
        .expect("headless window creation never fails");

    // The HAL side of the seam: an opaque target, and the only thing this test
    // is allowed to know about it is that it is the offscreen one. It is
    // available *immediately* — the surface handle exists as soon as the window
    // does — and it carries no size, so there is nothing in it for an eager
    // consumer to mistake for an extent. The swapchain still has to wait, and
    // the rest of this test is that wait.
    let target: SurfaceTarget = shell.surface_target(window).expect("surface target");
    assert_eq!(target, SurfaceTarget::Offscreen);
    assert!(!target.is_windowed());
    assert_eq!(target.platform_name(), "offscreen");

    // A test's own clock drives both the shell's timestamps and the frame
    // clock, so the whole session is deterministic.
    let mut time = ManualTime::new();
    let mut engine = Engine::new();

    // Frame 1: the window is not configured yet, so there is nothing to
    // render into and no swapchain to create. This is the state a consumer
    // written against a too-helpful test double never sees.
    engine.frame(&mut shell, window, time.elapsed());
    assert_eq!(engine.size, None);
    assert_eq!(engine.swapchain_recreations, 0);
    assert_eq!(engine.ticks, 0, "no wall time has passed yet");

    // Frame 2: the first configure lands, and only now is there an extent.
    time.advance(Duration::from_nanos(16_666_666));
    shell.set_time(time.elapsed());
    engine.frame(&mut shell, window, time.elapsed());
    assert_eq!(engine.size, Some(PhysicalSize::new(1280, 720)));
    assert_eq!(engine.swapchain_recreations, 1);

    // One second of steady 60 Hz with a scripted input burst.
    shell.set_focus(window, true).expect("focus");
    for frame in 0..59u32 {
        time.advance(Duration::from_nanos(16_666_666));
        shell.set_time(time.elapsed());
        match frame {
            10 => {
                shell.key_press(window, KeyCode::KeyW).expect("press");
                shell
                    .move_pointer(window, PhysicalPoint::new(640.0, 360.0), (4.0, -2.0))
                    .expect("motion");
            }
            20 => shell.key_release(window, KeyCode::KeyW).expect("release"),
            // A resize storm: the compositor drags the window edge.
            30..=35 => shell
                .resize(window, PhysicalSize::new(1280 + frame * 2, 720))
                .expect("resize"),
            // Dragged onto a HiDPI monitor mid-session.
            40 => shell
                .change_scale_factor(window, 2.0)
                .expect("scale change"),
            50 => shell
                .commit_text(window, "\u{3088}\u{3046}\u{3053}\u{305d}")
                .expect("text"),
            _ => {}
        }
        engine.frame(&mut shell, window, time.elapsed());
    }

    assert_eq!(engine.ticks, 60, "one second at 60 Hz is 60 ticks");
    assert!(engine.focused);
    assert!(engine.keys_down.is_empty(), "W was pressed and released");
    assert_eq!(engine.aim, (4.0, -2.0));
    assert_eq!(engine.typed, "\u{3088}\u{3046}\u{3053}\u{305d}");
    assert_eq!(
        engine.swapchain_recreations, 8,
        "1 configure + 6 drags + 1 dpi"
    );

    // The DPI check `docs/plan/15-windowing.md` asks for: a scale change
    // mid-session must not leave a wrong-size swapchain behind.
    assert_eq!(engine.scale_factor, 2.0);
    let state: WindowState = shell.window_state(window).expect("state");
    assert_eq!(
        state.size(),
        engine.size,
        "no wrong-size swapchain survives"
    );
    assert_eq!(state.scale_factor(), Some(2.0));
    assert!(state.is_configured());
    // The last drag left the window at 1350x720 physical at scale 1.0, i.e.
    // 1350x720 logical; at scale 2.0 that is 2700x1440.
    assert_eq!(engine.size, Some(PhysicalSize::new(2700, 1440)));
    // Through a resize storm, a scale change and a mode switch, the target
    // never moved. A HAL surface created before the first configure is still
    // valid, which is the property that lets a consumer create one up front.
    assert_eq!(
        shell.surface_target(window).expect("target"),
        target,
        "the target is invariant under everything a window does"
    );

    // Closing is a question the engine answers.
    shell.request_close(window).expect("close request");
    engine.frame(&mut shell, window, time.elapsed());
    assert!(engine.should_exit);
    assert!(matches!(
        shell.window_state(window),
        Err(ShellError::InvalidWindow { .. })
    ));
}

/// The seam is also usable generically: the trait is `?Sized`-friendly, so one
/// function accepts `&dyn Shell` and a concrete shell alike.
#[test]
fn the_seam_is_also_usable_generically() {
    fn backend_of<S: Shell + ?Sized>(shell: &S) -> ShellBackend {
        shell.backend()
    }

    let shell: Box<dyn Shell> = Box::new(HeadlessShell::new());
    assert_eq!(backend_of(shell.as_ref()), ShellBackend::Headless);
    assert_eq!(backend_of(&*shell), ShellBackend::Headless);
}

/// A consumer branches on capabilities, never on the platform.
///
/// The same code runs against a full-featured shell and against one shaped like
/// Wayland (no pointer warp, no aspect hint) and does the right thing both
/// times — which is the regression `ShellCaps` exists to prevent.
#[test]
fn consumers_branch_on_capabilities_not_on_platforms() {
    fn set_up_camera(shell: &mut dyn Shell, window: WindowId) -> &'static str {
        let caps: ShellCaps = shell.caps();
        if caps.has_mouselook() {
            shell
                .set_pointer_mode(window, PointerMode::Locked)
                .expect("the capability was checked");
            "locked"
        } else if caps.contains(ShellCaps::POINTER_WARP) {
            shell
                .warp_pointer(window, PhysicalPoint::new(0.0, 0.0))
                .expect("the capability was checked");
            "warped"
        } else {
            "absolute"
        }
    }

    let mut full: Box<dyn Shell> = Box::new(HeadlessShell::new());
    let window = full.create_window(&sandbox_window()).expect("window");
    assert_eq!(set_up_camera(full.as_mut(), window), "locked");

    // A shell with lock but no relative motion: the trap `has_mouselook`
    // exists to catch. It must not pick "locked".
    let mut half: Box<dyn Shell> = Box::new(
        HeadlessShell::new()
            .with_caps(ShellCaps::POINTER_LOCK | ShellCaps::POINTER_WARP | ShellCaps::MULTI_WINDOW),
    );
    let window = half.create_window(&sandbox_window()).expect("window");
    assert_eq!(set_up_camera(half.as_mut(), window), "warped");

    // And a Wayland-shaped one: no warp at all.
    let mut wayland: Box<dyn Shell> = Box::new(
        HeadlessShell::new()
            .with_caps(ShellCaps::HW_UPSCALE | ShellCaps::CLIPBOARD | ShellCaps::MULTI_WINDOW),
    );
    let window = wayland.create_window(&sandbox_window()).expect("window");
    assert_eq!(set_up_camera(wayland.as_mut(), window), "absolute");
    assert!(matches!(
        wayland.warp_pointer(window, PhysicalPoint::ORIGIN),
        Err(ShellError::Unsupported { .. })
    ));
}

/// An aspect-locked window letterboxes correctly whether or not the window
/// system honours the hint — the "letterboxing always works" fallback.
#[test]
fn aspect_locking_works_with_and_without_the_native_hint() {
    let constraints = SizeConstraints::NONE.with_aspect(AspectRatio::WIDESCREEN);

    // A backend that honours the hint: the window is already the right shape.
    let mut shell: Box<dyn Shell> = Box::new(HeadlessShell::new());
    let window = shell.create_window(&sandbox_window()).expect("window");
    shell
        .set_constraints(window, constraints)
        .expect("constraints");
    // A constraint is a request; the size it produces arrives with a configure.
    for _ in 0..3 {
        shell.pump(&mut |_| {});
    }
    let state = shell.window_state(window).expect("state");
    assert_eq!(state.requested_constraints, constraints);
    assert_eq!(state.size(), Some(PhysicalSize::new(1280, 720)));

    // A backend that does not: the compositor hands over any size it likes, and
    // the renderer computes the letterbox itself.
    let forced = PhysicalSize::new(1000, 1000);
    let viewport = AspectRatio::WIDESCREEN.fit(forced);
    assert_eq!(viewport, PhysicalSize::new(1000, 562));
    assert!(viewport.width <= forced.width && viewport.height <= forced.height);
}

/// The clipboard's asynchronous shape, driven the way an editor paste would
/// drive it: request, keep running, handle the answer when it arrives.
#[test]
fn a_clipboard_read_completes_through_the_event_stream() {
    let mut shell: Box<dyn Shell> = Box::new(HeadlessShell::new());
    let window = shell.create_window(&sandbox_window()).expect("window");

    shell
        .clipboard_offer(
            window,
            &[
                ClipboardOffer::text("Node"),
                ClipboardOffer::ron("(name:\"Node\")"),
            ],
        )
        .expect("offer");

    let request: ClipboardRequestId = shell
        .clipboard_request(window, MimeType::CrcblRon)
        .expect("request");

    // The caller keeps running; the answer arrives on a later pump, exactly as
    // it would after an X11 INCR transfer.
    let mut pasted: Option<ClipboardContent> = None;
    for _ in 0..4 {
        shell.pump(&mut |event: ShellEvent| {
            if let ShellEvent::ClipboardData {
                request: answered,
                content,
                ..
            } = event
                && answered == request
            {
                pasted = Some(content);
            }
        });
        if pasted.is_some() {
            break;
        }
    }
    // No retry loop, and none is permitted: one request, one answer. See the
    // `Shell` trait's implementor obligations 4 and 5.
    assert_eq!(
        pasted.as_ref().and_then(ClipboardContent::bytes),
        Some(&b"(name:\"Node\")"[..])
    );
}

/// Runtime backend selection: the factory exists, is honest about what it has,
/// and hands back a trait object.
#[test]
fn the_factory_selects_a_backend_at_runtime() {
    let shell: Box<dyn Shell> =
        crcbl_shell::open_backend(ShellBackend::Headless).expect("headless is registered");
    assert_eq!(shell.backend(), ShellBackend::Headless);

    // AppKit lands later in P5C; asking for it today is an honest error rather
    // than a silent fallback onto something else. (Win32 was this example
    // until P5C registered it, which is the point.)
    assert!(matches!(
        crcbl_shell::open_backend(ShellBackend::AppKit),
        Err(ShellError::UnknownBackend { .. })
    ));

    // Wayland (P0.5a) and X11 (P0.6) *are* registered on Linux, so asking for
    // either by name gets a real shell or a connection error — never
    // `UnknownBackend`. This test runs on machines with a compositor, with an X
    // server, and with neither, and all three are correct answers; what must
    // not happen is a fallback to something else.
    #[cfg(target_os = "windows")]
    {
        let shell = crcbl_shell::open_backend(ShellBackend::Win32)
            .expect("the Win32 backend is registered on Windows");
        assert_eq!(shell.backend(), ShellBackend::Win32);
    }

    #[cfg(target_os = "linux")]
    for backend in [ShellBackend::Wayland, ShellBackend::X11] {
        match crcbl_shell::open_backend(backend) {
            Ok(shell) => assert_eq!(shell.backend(), backend),
            Err(error) => assert!(
                matches!(error, ShellError::Connect { .. }),
                "expected a connection error with no {backend} display server, got {error}"
            ),
        }
    }
}

/// Timestamps survive the trip across the seam intact and on one clock — the
/// property the P2 pattern evaluator and topic 26's prediction both need.
#[test]
fn input_timestamps_are_the_window_systems_not_the_frames() {
    let mut headless = HeadlessShell::new();
    let window = headless.create_window(&sandbox_window()).expect("window");

    let mut time = ManualTime::new();
    time.advance(Duration::from_secs(5));
    headless.set_time(time.elapsed());
    headless.key_press(window, KeyCode::Space).expect("press");

    // 120 ms later — a "tap" by any pattern table — but still inside the same
    // frame, so a consumer that stamped events at pump time would see 0 ms.
    time.advance(Duration::from_millis(120));
    headless.set_time(time.elapsed());
    headless
        .key_release(window, KeyCode::Space)
        .expect("release");

    let shell: &mut dyn Shell = &mut headless;
    let mut times: Vec<EventTime> = Vec::new();
    shell.pump(&mut |event: ShellEvent| {
        if let Some(stamp) = event.time() {
            times.push(stamp);
        }
    });
    assert_eq!(times.len(), 2);
    assert_eq!(
        times[1].saturating_since(times[0]),
        Duration::from_millis(120)
    );
    assert_eq!(times[0], EventTime::from_millis(5_000));
}

/// Every window-taking method rejects a stale handle rather than acting on
/// whatever took the slot.
#[test]
fn stale_handles_never_reach_a_live_window() {
    let mut shell: Box<dyn Shell> = Box::new(HeadlessShell::new());
    let first = shell.create_window(&sandbox_window()).expect("window");
    shell.destroy_window(first).expect("destroy");

    // The pool recycles the slot, so the new window has the same index and a
    // different generation — the exact case a bare integer id would confuse.
    let second = shell.create_window(&sandbox_window()).expect("window");
    assert_ne!(first, second);
    assert_eq!(first.index(), second.index());

    assert!(matches!(
        shell.set_cursor(first, Some(CursorIcon::Text)),
        Err(ShellError::InvalidWindow { .. })
    ));
    assert!(matches!(
        shell.set_mode(first, DisplayMode::Borderless { monitor: None }),
        Err(ShellError::InvalidWindow { .. })
    ));
    assert!(shell.set_cursor(second, Some(CursorIcon::Text)).is_ok());
}

/// A pump with no events is a no-op, and a scripted burst arrives in order.
#[test]
fn the_event_order_is_the_order_it_was_scripted_in() {
    let mut headless = HeadlessShell::new();
    let window = headless.create_window(&sandbox_window()).expect("window");
    // The first pump carries the deferred configure delay, not the configure.
    assert_eq!(pump_names(&mut headless), Vec::<&str>::new());
    assert_eq!(pump_names(&mut headless), ["Resized"]);
    assert_eq!(pump_names(&mut headless), Vec::<&str>::new());

    headless.set_focus(window, true).expect("focus");
    headless.key_press(window, KeyCode::KeyE).expect("press");
    headless
        .button(
            window,
            PointerButton::Left,
            ButtonState::Pressed,
            Some(PhysicalPoint::new(2.0, 3.0)),
        )
        .expect("click");
    headless
        .scroll(window, ScrollDelta::Pixels { x: 0.0, y: -18.0 }, None)
        .expect("scroll");
    headless
        .resize(window, PhysicalSize::new(800, 600))
        .expect("resize");
    headless
        .change_scale_factor(window, 1.5)
        .expect("scale change");

    assert_eq!(
        pump_names(&mut headless),
        [
            "Focus",
            "Key",
            "Button",
            "Wheel",
            "Resized",
            // A scale change is a resize too, and says so with both events —
            // the pair both Linux backends emit.
            "ScaleFactorChanged",
            "Resized",
        ]
    );
}

/// A swapchain cannot be created before the window system has configured the
/// window, and the seam makes that a type-level fact rather than a convention.
///
/// This is the P1 bring-up path, written the way `crcbl-render` will write it.
#[test]
fn a_swapchain_cannot_be_created_before_the_first_configure() {
    /// Stands in for `Device::create_swapchain`, which needs an extent.
    fn create_swapchain(shell: &dyn Shell, window: WindowId) -> Result<PhysicalSize, &'static str> {
        let state: WindowState = shell.window_state(window).map_err(|_| "stale window")?;
        let size = state.size().ok_or("window is not configured yet")?;
        if size.is_empty() {
            return Err("zero-extent swapchain");
        }
        Ok(size)
    }

    let mut shell: Box<dyn Shell> = Box::new(HeadlessShell::new());
    let window = shell.create_window(&sandbox_window()).expect("window");

    // The eager consumer gets an error naming the actual problem, not a
    // plausible-looking wrong extent taken from `WindowDesc::size`.
    assert_eq!(
        create_swapchain(shell.as_ref(), window),
        Err("window is not configured yet")
    );

    // The surface *handle* is available meanwhile — only the extent is pending
    // — so a HAL surface can be created up front, and no size travels in the
    // target for it to be wrong about.
    assert_eq!(
        shell.surface_target(window).expect("the handle exists"),
        SurfaceTarget::Offscreen
    );

    // The mandatory loop. It really loops: the default configure delay is one
    // pump, so a consumer with an off-by-one `if` instead of a `while` fails
    // here.
    let mut pumps = 0;
    let size = loop {
        pumps += 1;
        shell.pump(&mut |_event: ShellEvent| {});
        if let Ok(size) = create_swapchain(shell.as_ref(), window) {
            break size;
        }
        assert!(pumps < 16, "the window never configured");
    };
    assert!(pumps > 1, "a single pump must not have been enough");
    assert_eq!(size, PhysicalSize::new(1280, 720));
}

/// A mode change is a request. A consumer that reads its own request back as
/// fact is wrong, and the seam gives it no way to do so by accident.
#[test]
fn a_mode_request_is_not_a_mode() {
    let mut shell: Box<dyn Shell> = Box::new(HeadlessShell::new());
    let window = shell.create_window(&sandbox_window()).expect("window");
    for _ in 0..4 {
        shell.pump(&mut |_event: ShellEvent| {});
    }

    let borderless = DisplayMode::Borderless { monitor: None };
    shell.set_mode(window, borderless).expect("request");

    // Immediately after asking: the request is recorded, the effect is not.
    let state: WindowState = shell.window_state(window).expect("state");
    assert_eq!(state.requested_mode, borderless);
    assert_eq!(state.effective_mode(), Some(DisplayMode::Windowed));
    assert!(
        !state.mode_request_honoured(),
        "a fullscreen checkbox reading the request would already be lying"
    );

    // After the window system answers.
    for _ in 0..4 {
        shell.pump(&mut |_event: ShellEvent| {});
    }
    let state: WindowState = shell.window_state(window).expect("state");
    assert_eq!(state.effective_mode(), Some(borderless));
    assert!(state.mode_request_honoured());
    assert_eq!(state.size(), Some(PhysicalSize::new(1920, 1080)));
}

/// A paste from a foreign application carries that application's mime spelling
/// all the way across the seam.
///
/// This is the case `MimeType::Other(&'static str)` structurally cannot hold,
/// and the reason [`ReceivedMime`] exists.
#[test]
fn a_foreign_clipboard_mime_survives_the_seam() {
    let mut headless = HeadlessShell::new();
    let window = headless.create_window(&sandbox_window()).expect("window");
    headless.set_foreign_clipboard(vec![
        (
            ReceivedMime::new("text/plain"),
            b"from another app".to_vec(),
        ),
        (
            ReceivedMime::new("application/vnd.some-editor.scene+json"),
            b"{}".to_vec(),
        ),
    ]);

    let shell: &mut dyn Shell = &mut headless;
    let request = shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("request");

    let mut answer: Option<(ReceivedMime, ClipboardContent)> = None;
    shell.pump(&mut |event: ShellEvent| {
        if let ShellEvent::ClipboardData {
            request: answered,
            mime,
            content,
            ..
        } = event
            && answered == request
        {
            answer = Some((mime, content));
        }
    });

    let (mime, content) = answer.expect("the request was answered");
    assert_eq!(content.bytes(), Some(&b"from another app"[..]));
    // Verbatim, not canonicalized — an X11 backend has to echo the exact target
    // atom back.
    assert_eq!(mime.as_str(), "text/plain");
    assert_ne!(mime, ReceivedMime::from(MimeType::TextUtf8));
    // …and still recognizable as the format that was asked for.
    assert!(mime.matches(MimeType::TextUtf8));
    assert_eq!(mime.recognized(), Some(MimeType::TextUtf8));

    // A format the engine has no name for is representable at all, which is the
    // hole this closes.
    let foreign = ReceivedMime::new("application/vnd.some-editor.scene+json");
    assert_eq!(foreign.recognized(), None);
    assert_eq!(
        foreign.to_string(),
        "application/vnd.some-editor.scene+json"
    );
}
