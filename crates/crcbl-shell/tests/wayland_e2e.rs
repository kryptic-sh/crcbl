//! The Wayland backend against a real compositor.
//!
//! Run with `crates/crcbl-shell/tests/run-wayland-e2e.sh`, which starts a
//! private headless sway and turns these on. They are gated twice — behind the
//! `wayland-e2e` feature *and* `#[ignore]` — so that
//! `cargo nextest run --workspace --all-features` stays green on a machine with
//! no compositor, which is every CI runner except the one job that launches one.
//! The harness fails when the suite reports zero tests run, which is the
//! counter-measure to `docs/plan/12-testing.md`'s "silently skipped e2e" trap.
//!
//! # What these are for
//!
//! `HeadlessShell` is a *model* of a compositor. These tests are the only thing
//! that can tell us where the model is wrong, so they assert against the P0.4
//! contract rather than against the backend's own idea of itself, and they go
//! through [`crcbl_shell::open_backend`] and `dyn Shell` — no backend type is
//! named anywhere below, exactly as a consumer would have it.

#![cfg(all(target_os = "linux", feature = "wayland-e2e"))]

use std::process::Command;
use std::time::{Duration, Instant};

use crcbl_shell::wayland_test_support::VirtualInput;
use crcbl_shell::{
    ButtonState, CloseReply, CursorIcon, DisplayMode, KeyCode, Keysym, LogicalSize, Modifiers,
    PhysicalSize, PointerButton, PointerMode, ScrollDelta, Shell, ShellBackend, ShellCaps,
    ShellEvent, SizeConstraints, SurfaceTarget, WindowDesc, WindowId,
};

/// evdev codes, from `linux/input-event-codes.h`. Spelled out rather than
/// imported so the test states the number the wire actually carries.
mod evdev {
    pub const KEY_A: u32 = 30;
    pub const KEY_LEFTSHIFT: u32 = 42;
    pub const KEY_ESC: u32 = 1;
    pub const BTN_LEFT: u32 = 0x110;
    pub const BTN_SIDE: u32 = 0x113;
}

/// How long any single wait may take before the test fails.
///
/// Generous for a cold CI runner and still far inside nextest's 60s
/// `slow-timeout`, so a hang is reported by this file — naming what it was
/// waiting for and what it saw — rather than by a SIGKILL with no context.
const WAIT: Duration = Duration::from_secs(10);

/// The `app_id` the sway config matches on to float the window. Changing it
/// here without changing `wayland-e2e-sway.conf` makes the borderless test
/// vacuous, since a tiled window already fills the output.
const APP_ID: &str = "sh.kryptic.crcbl.e2e";

/// The output declared in `wayland-e2e-sway.conf`.
const OUTPUT_SIZE: PhysicalSize = PhysicalSize::new(1920, 1080);

/// A shell plus the events it has produced, with a polling helper.
struct Session {
    shell: Box<dyn Shell>,
    events: Vec<ShellEvent>,
}

impl Session {
    fn open() -> Self {
        let shell = crcbl_shell::open_backend(ShellBackend::Wayland)
            .expect("the harness exported WAYLAND_DISPLAY for a live sway");
        assert_eq!(shell.backend(), ShellBackend::Wayland);
        Self {
            shell,
            events: Vec::new(),
        }
    }

    fn pump(&mut self) {
        let events = &mut self.events;
        self.shell.pump(&mut |event| events.push(event));
    }

    /// Pumps until `ready`, or fails naming what never happened.
    ///
    /// A deadline and a poll, never a fixed sleep: `docs/plan/12-testing.md`
    /// makes that the rule for anything asynchronous, and a compositor
    /// handshake is the asynchronous case the rule was written for.
    fn pump_until(&mut self, what: &str, ready: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            self.pump();
            if ready(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out after {WAIT:?} waiting for {what}; events so far: {:?}",
                self.names()
            );
            // Also exercises `ShellCaps::EVENT_WAIT`: a backend that claims it
            // and does not block would spin this loop, and one that blocks
            // forever would never reach the deadline check.
            self.shell.wait_events(Some(Duration::from_millis(20)));
        }
    }

    fn names(&self) -> Vec<&'static str> {
        self.events.iter().map(ShellEvent::name).collect()
    }

    fn take_names(&mut self) -> Vec<&'static str> {
        let names = self.names();
        self.events.clear();
        names
    }

    fn size(&self, window: WindowId) -> Option<PhysicalSize> {
        self.shell.window_state(window).expect("live window").size()
    }

    fn create(&mut self, desc: &WindowDesc<'_>) -> WindowId {
        self.shell.create_window(desc).expect("create_window")
    }

    /// Creates a window, waits for the first configure, and maps it.
    ///
    /// Mapping is what puts the window in the compositor's tree, and everything
    /// downstream — a compositor-chosen size, a fullscreen configure, a user
    /// resize, a close request — depends on it. See
    /// [`crcbl_shell::wayland_test_support`] for why the shell itself must not
    /// do this.
    fn create_mapped(&mut self, desc: &WindowDesc<'_>) -> WindowId {
        let window = self.create(desc);
        self.pump_until("the first configure", |session| {
            session.size(window).is_some()
        });
        crcbl_shell::wayland_test_support::map_window(&*self.shell, window).expect("map");
        // The compositor answers a newly mapped surface with a configure of its
        // own; wait for the exchange to settle so later assertions are not
        // racing it.
        let settled = Instant::now() + Duration::from_millis(500);
        while Instant::now() < settled {
            self.pump();
            self.shell.wait_events(Some(Duration::from_millis(20)));
        }
        window
    }
}

fn desc<'a>(title: &'a str, size: LogicalSize) -> WindowDesc<'a> {
    WindowDesc {
        title,
        app_id: APP_ID,
        size,
        ..WindowDesc::default()
    }
}

/// Talks to sway's IPC, so a test can make the *compositor* do something.
///
/// Nothing in `xdg-shell` lets a client resize itself or ask to be closed, so
/// the only way to test either against a real window manager is to drive the
/// window manager. Returns `false` when `swaymsg` is unavailable or the command
/// failed, which the callers turn into an explicit skip rather than a silent
/// pass.
fn swaymsg(args: &[&str]) -> bool {
    Command::new("swaymsg")
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The headline assertion of the slice: a real compositor confirms the
/// unconfigured-window model P0.4 was designed around.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_window_is_unconfigured_until_the_compositor_says_otherwise() {
    let mut session = Session::open();
    let window = session.create(&desc("crcbl e2e", LogicalSize::new(960.0, 540.0)));

    // Immediately after `create_window` there is no size, so there is nothing to
    // build a swapchain from. This is the whole reason `WindowState::size`
    // returns an `Option`.
    let state = session.shell.window_state(window).expect("live window");
    assert!(
        !state.is_configured(),
        "an xdg_surface has no size until the compositor answers"
    );
    assert_eq!(state.size(), None);
    assert_eq!(state.scale_factor(), None);
    assert_eq!(state.effective_mode(), None);
    assert!(!state.mode_request_honoured());

    session.pump_until("the first configure", |session| {
        session.size(window).is_some()
    });

    let state = session.shell.window_state(window).expect("live window");
    let size = state.size().expect("configured");
    assert!(
        !size.is_empty(),
        "a configured window has a real extent, not 0x0: {size:?}"
    );
    let scale = state.scale_factor().expect("configured");
    assert!(scale > 0.0, "scale factor must be positive, got {scale}");
    assert_eq!(
        state.effective_mode(),
        Some(DisplayMode::Windowed),
        "no fullscreen was requested"
    );
    assert!(state.mode_request_honoured());

    // The event stream said the same thing the snapshot does — a consumer that
    // handles `Resized` and one that polls `window_state` must agree.
    let resized = session
        .events
        .iter()
        .rev()
        .find_map(|event| match event {
            ShellEvent::Resized {
                window: which,
                size,
                scale_factor,
            } if *which == window => Some((*size, *scale_factor)),
            _ => None,
        })
        .expect("a Resized carried the first configure");
    assert_eq!(resized, (size, scale));

    session.shell.destroy_window(window).expect("destroy");
}

/// The compositor's first configure typically dictates **no** size at all —
/// the single biggest difference from what `HeadlessShell` models.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn the_first_configure_falls_back_to_the_requested_size() {
    let mut session = Session::open();
    let requested = LogicalSize::new(800.0, 600.0);
    let window = session.create(&desc("crcbl e2e first configure", requested));
    session.pump_until("the first configure", |session| {
        session.size(window).is_some()
    });

    // sway answers the initial commit of an unmapped floating toplevel with
    // `0 x 0`, which xdg-shell defines as "you choose". `HeadlessShell` always
    // dictates a size, so a consumer written only against it never exercises
    // this path — and a backend that forwarded the zero would report an empty
    // window forever.
    let state = session.shell.window_state(window).expect("live window");
    let scale = state.scale_factor().expect("configured");
    assert_eq!(
        state.size(),
        Some(requested.to_physical(scale)),
        "with no compositor-chosen size, the request stands"
    );
    session.shell.destroy_window(window).expect("destroy");
}

/// The finding this slice exists to report, written as an assertion.
///
/// An `xdg_toplevel` with no buffer is *configured* but not *managed*: the
/// compositor has answered the handshake, and that is all it will ever do until
/// something attaches a buffer. `HeadlessShell` models no such state — its
/// windows are fully live from the first configure — and every consumer written
/// against it will therefore assume a compositor-chosen size arrives on its own.
/// It does not.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn an_unmapped_window_is_configured_but_not_yet_managed() {
    let mut session = Session::open();
    let window = session.create(&desc("crcbl e2e unmapped", LogicalSize::new(700.0, 500.0)));
    session.pump_until("the first configure", |session| {
        session.size(window).is_some()
    });
    let matcher = format!("[app_id=\"{APP_ID}\"]");

    // Not in the window manager's tree at all: a criteria command matches
    // nothing, which is why the resize and close tests below have to map first.
    assert!(
        !swaymsg(&[&matcher, "border", "none"]),
        "an unmapped toplevel must not be addressable by window-manager criteria"
    );

    // And asking for fullscreen changes nothing, because there is nothing to
    // place. A test that expected a configure here would hang for its whole
    // timeout — which is exactly what the first draft of this suite did.
    session
        .shell
        .set_mode(window, DisplayMode::Borderless { monitor: None })
        .expect("set_mode");
    let settled = Instant::now() + Duration::from_millis(400);
    while Instant::now() < settled {
        session.pump();
        session.shell.wait_events(Some(Duration::from_millis(20)));
    }
    assert_eq!(
        session
            .shell
            .window_state(window)
            .expect("live")
            .effective_mode(),
        Some(DisplayMode::Windowed),
        "an unmapped surface gets no fullscreen configure"
    );

    // Attaching a buffer is the whole difference. Afterwards the request that
    // was ignored is honoured, and the window is addressable.
    crcbl_shell::wayland_test_support::map_window(&*session.shell, window).expect("map");
    session.pump_until(
        "the fullscreen configure a mapped surface gets",
        |session| {
            session
                .shell
                .window_state(window)
                .expect("live")
                .effective_mode()
                == Some(DisplayMode::Borderless { monitor: None })
        },
    );
    assert_eq!(session.size(window), Some(OUTPUT_SIZE));
    assert!(
        swaymsg(&[&matcher, "border", "none"]),
        "a mapped toplevel is in the tree"
    );

    session.shell.destroy_window(window).expect("destroy");
}

/// The one platform leak, populated before the size is known.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn the_surface_target_exists_before_the_window_has_a_size() {
    let mut session = Session::open();
    let window = session.create(&desc("crcbl e2e surface", LogicalSize::new(640.0, 480.0)));

    // `Shell::surface_target` promises the *handle* is available at creation
    // and only the extent waits. That is the split P1's `crcbl-vk` depends on:
    // `vkCreateWaylandSurfaceKHR` can run now, the swapchain cannot.
    let target = session.shell.surface_target(window).expect("live window");
    let SurfaceTarget::Wayland { display, surface } = target else {
        panic!("a Wayland backend must produce a Wayland target, got {target:?}");
    };
    assert_eq!(target.platform_name(), "wayland");
    assert!(target.is_windowed());
    let (display_before, surface_before) = (display, surface);

    session.pump_until("the first configure", |session| {
        session.size(window).is_some()
    });

    // Stable across the configure: the compositor resizing a surface does not
    // replace it, so a HAL surface built at creation stays valid.
    let SurfaceTarget::Wayland { display, surface } =
        session.shell.surface_target(window).expect("live window")
    else {
        panic!("the target changed platform");
    };
    assert_eq!(display, display_before, "the wl_display is the connection");
    assert_eq!(surface, surface_before, "the wl_surface outlives a resize");

    session.shell.destroy_window(window).expect("destroy");
    assert!(
        session.shell.surface_target(window).is_err(),
        "a destroyed window's target is gone, not stale"
    );
}

/// Borderless really is a different size, and the compositor says so.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn switching_to_borderless_and_back_reconfigures_the_window() {
    let mut session = Session::open();
    let windowed = LogicalSize::new(640.0, 360.0);
    let window = session.create_mapped(&desc("crcbl e2e mode", windowed));
    let scale = session
        .shell
        .window_state(window)
        .expect("live")
        .scale_factor()
        .expect("configured");
    let before = session.size(window).expect("configured");
    session.take_names();

    session
        .shell
        .set_mode(window, DisplayMode::Borderless { monitor: None })
        .expect("set_mode");
    // A request, not a command: nothing about the effective state changed yet.
    let state = session.shell.window_state(window).expect("live");
    assert_eq!(
        state.requested_mode,
        DisplayMode::Borderless { monitor: None }
    );
    assert_eq!(state.effective_mode(), Some(DisplayMode::Windowed));
    assert!(!state.mode_request_honoured(), "the answer has not arrived");

    session.pump_until("the fullscreen configure", |session| {
        session
            .shell
            .window_state(window)
            .expect("live")
            .effective_mode()
            == Some(DisplayMode::Borderless { monitor: None })
    });
    let state = session.shell.window_state(window).expect("live");
    assert!(state.mode_request_honoured());
    assert_eq!(
        state.size(),
        Some(OUTPUT_SIZE),
        "borderless covers the whole output declared in the sway config"
    );
    assert!(
        session.names().contains(&"Resized"),
        "the mode change arrived as a Resized: {:?}",
        session.names()
    );
    session.take_names();

    session
        .shell
        .set_mode(window, DisplayMode::Windowed)
        .expect("set_mode");
    session.pump_until("the windowed configure", |session| {
        session
            .shell
            .window_state(window)
            .expect("live")
            .effective_mode()
            == Some(DisplayMode::Windowed)
    });
    assert_eq!(
        session.size(window),
        Some(before),
        "leaving borderless restores the pre-fullscreen geometry"
    );
    let _ = (windowed, scale);

    session.shell.destroy_window(window).expect("destroy");
}

/// A compositor-driven resize, produced by driving sway itself.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_compositor_resize_arrives_as_a_resized_event() {
    let mut session = Session::open();
    let window = session.create_mapped(&desc("crcbl e2e resize", LogicalSize::new(640.0, 360.0)));
    let before = session.size(window).expect("configured");
    // Mapping is also what makes the window focusable: `xdg_toplevel.state`
    // gains `activated`, which is the only focus signal there is without a
    // `wl_seat`.
    let after_map = session.take_names();
    assert!(
        after_map.contains(&"Focus"),
        "a mapped window is activated: {after_map:?}"
    );
    assert!(session.shell.window_state(window).expect("live").focused);

    assert!(
        swaymsg(&[
            &format!("[app_id=\"{APP_ID}\"]"),
            "resize",
            "set",
            "width",
            "1024",
            "px",
            "height",
            "768",
            "px"
        ]),
        "swaymsg ships with sway; without it there is no way to make the \
         compositor resize a client, and a skipped assertion here would hide \
         the whole resize path"
    );

    session.pump_until("a compositor-driven resize", |session| {
        session.size(window).is_some_and(|size| size != before)
    });
    let after = session.size(window).expect("configured");
    assert_ne!(after, before);
    assert!(
        session.names().contains(&"Resized"),
        "the resize arrived as an event, not only as state: {:?}",
        session.names()
    );
    let event = session
        .events
        .iter()
        .rev()
        .find_map(|event| match event {
            ShellEvent::Resized { size, .. } => Some(*size),
            _ => None,
        })
        .expect("Resized");
    assert_eq!(event, after, "the event and the snapshot agree");
    // A compositor restates the whole state set on every configure. A backend
    // that did not diff would report a focus change on each resize, and the
    // action layer would clear every held key mid-drag.
    assert!(
        !session.names().contains(&"Focus"),
        "a resize is not a focus change: {:?}",
        session.names()
    );

    session.shell.destroy_window(window).expect("destroy");
}

/// Closing is a question the application answers, on a real window manager.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_close_request_is_a_question_and_keep_really_keeps() {
    let mut session = Session::open();
    let window = session.create_mapped(&desc("crcbl e2e close", LogicalSize::new(500.0, 400.0)));
    session.take_names();

    assert!(
        swaymsg(&[&format!("[app_id=\"{APP_ID}\"]"), "kill"]),
        "swaymsg kill sends xdg_toplevel.close"
    );
    session.pump_until("the compositor's close request", |session| {
        session.names().contains(&"CloseRequested")
    });
    assert!(
        session
            .shell
            .window_state(window)
            .expect("still open")
            .close_pending,
        "the request is outstanding until it is answered"
    );

    session
        .shell
        .reply_close_request(window, CloseReply::Keep)
        .expect("keep");
    assert!(
        session.shell.window_state(window).is_ok(),
        "Keep means the window stays open — a shell that closed it first has \
         already lost the unsaved-changes argument"
    );
    assert!(
        session
            .shell
            .reply_close_request(window, CloseReply::Keep)
            .is_err(),
        "replying twice is a caller state-machine bug, not a no-op"
    );

    // And Close really closes.
    assert!(swaymsg(&[&format!("[app_id=\"{APP_ID}\"]"), "kill"]));
    session.pump_until("a second close request", |session| {
        session.names().contains(&"CloseRequested")
    });
    session
        .shell
        .reply_close_request(window, CloseReply::Close)
        .expect("close");
    assert!(
        session.shell.window_state(window).is_err(),
        "the handle is stale afterwards"
    );
    session.pump();
    assert!(session.names().contains(&"WindowDestroyed"));
}

/// Monitors come from `wl_output`, with the geometry the sway config declares.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn monitors_are_enumerated_from_wl_output() {
    let session = Session::open();
    let monitors = session.shell.monitors();
    assert_eq!(monitors.len(), 1, "the config declares one HEADLESS output");
    let monitor = &monitors[0];
    assert!(monitor.is_primary, "the first output seen is the default");
    assert_eq!(monitor.size(), OUTPUT_SIZE);
    assert_eq!(
        monitor.work_area, monitor.bounds,
        "Wayland has no work area"
    );
    assert!(
        (monitor.refresh_hz() - 60.0).abs() < 1.0,
        "60Hz was declared, got {}",
        monitor.refresh_hz()
    );
    assert!(monitor.scale_factor >= 1.0);
    assert!(
        monitor.name.starts_with("HEADLESS"),
        "wl_output.name is the compositor's, got {:?}",
        monitor.name
    );
    // The id round-trips through the lookup the seam provides.
    assert_eq!(
        session.shell.monitor(monitor.id).map(|found| found.id),
        Some(monitor.id)
    );
}

/// Capabilities describe this slice, and the operations behind the missing ones
/// fail with the error that names them.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn capabilities_are_honest_about_what_the_compositor_advertises() {
    let mut session = Session::open();
    let caps = session.shell.caps();
    assert!(caps.contains(ShellCaps::EVENT_WAIT));
    assert!(caps.contains(ShellCaps::MULTI_WINDOW));
    // Permanently absent: Wayland forbids both by design.
    assert!(!caps.contains(ShellCaps::POINTER_WARP));
    assert!(!caps.contains(ShellCaps::WINDOW_POSITION));
    // P0.5b bound the protocols behind these, and sway advertises all of them.
    assert!(caps.contains(ShellCaps::POINTER_LOCK));
    assert!(caps.contains(ShellCaps::POINTER_CONFINE));
    assert!(caps.contains(ShellCaps::RAW_POINTER_MOTION));
    assert!(
        caps.has_mouselook(),
        "aim input needs lock *and* raw motion"
    );
    assert!(caps.contains(ShellCaps::HW_UPSCALE), "wp_viewporter");
    assert!(caps.contains(ShellCaps::FRACTIONAL_SCALE));
    assert!(
        caps.contains(ShellCaps::SERVER_DECORATIONS),
        "xdg-decoration"
    );
    assert!(caps.contains(ShellCaps::TEXT_IME), "libxkbcommon resolved");
    // Still absent until P0.5c binds `wl_data_device`.
    assert!(!caps.contains(ShellCaps::CLIPBOARD));
    assert!(!caps.contains(ShellCaps::DRAG_DROP));
    // Wayland has no aspect hint at all — the renderer letterboxes instead.
    assert!(!caps.contains(ShellCaps::ASPECT_HINT_HONORED));

    let window = session.create(&desc("crcbl e2e caps", LogicalSize::new(400.0, 300.0)));
    assert!(
        session
            .shell
            .warp_pointer(window, crcbl_shell::PhysicalPoint::new(1.0, 1.0))
            .is_err(),
        "a missing capability produces an error naming it, not a silent no-op"
    );
    assert!(
        session
            .shell
            .clipboard_request(window, crcbl_shell::MimeType::TextUtf8)
            .is_err()
    );
    // Title and constraints are in this slice and must work.
    session.shell.set_title(window, "renamed").expect("title");
    session
        .shell
        .set_constraints(window, SizeConstraints::min(LogicalSize::new(320.0, 240.0)))
        .expect("constraints");
    session.shell.destroy_window(window).expect("destroy");
}

/// Two windows on one connection, since `MULTI_WINDOW` is advertised.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_second_window_is_configured_independently() {
    let mut session = Session::open();
    let first = session.create(&desc("crcbl e2e one", LogicalSize::new(320.0, 240.0)));
    let second = session.create(&desc("crcbl e2e two", LogicalSize::new(800.0, 480.0)));

    session.pump_until("both windows configured", |session| {
        session.size(first).is_some() && session.size(second).is_some()
    });
    let scale = session
        .shell
        .window_state(first)
        .expect("live")
        .scale_factor()
        .expect("configured");
    assert_eq!(
        session.size(first),
        Some(LogicalSize::new(320.0, 240.0).to_physical(scale))
    );
    assert_eq!(
        session.size(second),
        Some(LogicalSize::new(800.0, 480.0).to_physical(scale))
    );
    assert_ne!(
        session.shell.surface_target(first).expect("live"),
        session.shell.surface_target(second).expect("live"),
        "each window has its own wl_surface"
    );

    // Destroying one leaves the other alone, and its handle goes stale cleanly.
    session.shell.destroy_window(first).expect("destroy");
    assert!(session.shell.window_state(first).is_err());
    assert!(session.shell.window_state(second).is_ok());
    session.shell.destroy_window(second).expect("destroy");
}

// ---------------------------------------------------------------------------
// P0.5b — input, driven through the compositor by virtual devices
// ---------------------------------------------------------------------------

impl Session {
    /// A mapped, fullscreen, focused window plus a keyboard and mouse on the
    /// seat.
    ///
    /// Fullscreen because the pointer has to land *somewhere*: a floating
    /// window is placed wherever sway feels like, and a test that moved the
    /// cursor to the middle of the output and hoped would be a flake. A
    /// borderless window covers the output, so the centre is always inside it.
    fn with_input(&mut self, title: &str) -> (WindowId, VirtualInput) {
        let window = self.create_mapped(&desc(title, LogicalSize::new(640.0, 360.0)));
        self.shell
            .set_mode(window, DisplayMode::Borderless { monitor: None })
            .expect("set_mode");
        self.pump_until("the fullscreen configure", |session| {
            session.size(window) == Some(OUTPUT_SIZE)
        });
        let input = VirtualInput::attach(&*self.shell, window).expect("virtual devices");
        // The seat gains its capabilities asynchronously; the backend only
        // creates its `wl_pointer` and `wl_keyboard` once it has seen them.
        self.pump_until("the seat to gain a keyboard", |session| {
            session
                .events
                .iter()
                .any(|event| matches!(event, ShellEvent::Focus { focused: true, .. }))
                || session
                    .shell
                    .window_state(window)
                    .is_ok_and(|state| state.focused)
        });
        self.take_names();
        (window, input)
    }

    /// Every `Key` event so far.
    fn keys(&self) -> Vec<(Option<KeyCode>, u32, Keysym, ButtonState, bool, Modifiers)> {
        self.events
            .iter()
            .filter_map(|event| match event {
                ShellEvent::Key {
                    key_code,
                    scancode,
                    keysym,
                    state,
                    repeat,
                    modifiers,
                    ..
                } => Some((*key_code, scancode.0, *keysym, *state, *repeat, *modifiers)),
                _ => None,
            })
            .collect()
    }

    fn text(&self) -> Vec<String> {
        self.events
            .iter()
            .filter_map(|event| match event {
                ShellEvent::TextCommit { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Pumps for a fixed wall-clock period, which is the only way to observe
    /// something that must *not* happen.
    fn settle(&mut self, duration: Duration) {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            self.pump();
            self.shell.wait_events(Some(Duration::from_millis(10)));
        }
    }
}

/// The headline of the slice: a real key press, through a real compositor,
/// arriving as the engine's own vocabulary.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_key_press_arrives_as_scancode_key_code_keysym_and_text() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e keys");

    input.tap(evdev::KEY_A);
    session.pump_until("the key press and release", |session| {
        session
            .keys()
            .iter()
            .filter(|key| !key.4)
            .any(|key| key.3 == ButtonState::Released)
    });

    let keys = session.keys();
    let press = keys.first().copied().expect("a press arrived");
    assert_eq!(press.0, Some(KeyCode::KeyA), "the physical key");
    assert_eq!(
        press.1,
        evdev::KEY_A,
        "the scancode is the raw evdev code, so a key we have no name for is \
         still bindable"
    );
    assert_eq!(
        press.2,
        Keysym::from_char('a'),
        "the layout's symbol, from the keymap the compositor sent"
    );
    assert_eq!(press.3, ButtonState::Pressed);
    assert!(!press.4, "a fresh press is not a repeat");

    // Text is a separate event, and it follows its key rather than preceding it.
    assert_eq!(session.text(), vec!["a".to_string()]);
    let order: Vec<&'static str> = session
        .events
        .iter()
        .map(ShellEvent::name)
        .filter(|name| *name == "Key" || *name == "TextCommit")
        .collect();
    assert_eq!(order, ["Key", "TextCommit", "Key"], "press, text, release");

    // Every input event is attributed and timestamped.
    let key = session
        .events
        .iter()
        .find(|event| event.name() == "Key")
        .expect("Key");
    assert_eq!(key.window(), Some(window));
    assert!(key.is_input());
    let ShellEvent::Key { device, .. } = key else {
        panic!("wrong variant");
    };
    assert_ne!(
        *device,
        crcbl_shell::DeviceId::UNKNOWN,
        "a real seat has a real device id"
    );

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// Escape and Return commit no text, because a text field wants them as keys.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn control_characters_are_keys_and_not_committed_text() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e control");

    input.tap(evdev::KEY_ESC);
    session.pump_until("the escape key", |session| {
        session
            .keys()
            .iter()
            .any(|key| key.0 == Some(KeyCode::Escape))
    });
    session.settle(Duration::from_millis(150));

    assert!(
        session.text().is_empty(),
        "Escape produces \\u{{1b}} through XKB; a text field that received it \
         would insert a control character: {:?}",
        session.text()
    );
    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// Modifiers are stamped onto the event, and they change what the key means.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn modifiers_ride_on_each_event_and_shift_changes_the_keysym() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e modifiers");

    input.key(evdev::KEY_LEFTSHIFT, true);
    input.tap(evdev::KEY_A);
    input.key(evdev::KEY_LEFTSHIFT, false);
    session.pump_until("the shifted A", |session| {
        session
            .keys()
            .iter()
            .any(|key| key.2 == Keysym::from_char('A'))
    });
    session.settle(Duration::from_millis(150));

    let shifted = session
        .keys()
        .into_iter()
        .find(|key| key.0 == Some(KeyCode::KeyA) && key.3 == ButtonState::Pressed)
        .expect("the A press");
    assert_eq!(shifted.2, Keysym::from_char('A'), "Shift+A is the capital");
    assert!(
        shifted.5.contains(Modifiers::SHIFT),
        "the modifier is on the event, not delivered separately: {:?}",
        shifted.5
    );
    assert_eq!(
        shifted.5.chord(),
        Modifiers::SHIFT,
        "nothing else is held: {:?}",
        shifted.5
    );
    // The modifier key's *own* event carries the state as it was when the key
    // was struck, so the Shift press itself reports no Shift. That is the X11
    // convention the seam adopted, and it is what a chord matcher wants: `Ctrl`
    // going down is not `Ctrl+Ctrl`.
    let shift_press = session
        .keys()
        .into_iter()
        .find(|key| key.0 == Some(KeyCode::ShiftLeft) && key.3 == ButtonState::Pressed)
        .expect("the Shift press");
    assert!(!shift_press.5.contains(Modifiers::SHIFT));
    assert_eq!(session.text(), vec!["A".to_string()]);

    // Shift itself is a key with a name, and it is left/right-distinguished.
    assert!(
        session
            .keys()
            .iter()
            .any(|key| key.0 == Some(KeyCode::ShiftLeft)),
        "the modifier key is reported too: {:?}",
        session.keys()
    );
    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// The shell synthesizes repeats from `repeat_info`, and flags every one.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_held_key_repeats_and_the_repeats_are_flagged() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e repeat");

    input.key(evdev::KEY_A, true);
    session.pump_until("the first synthesized repeat", |session| {
        session.keys().iter().any(|key| key.4)
    });
    // Long enough for several at any plausible rate.
    session.settle(Duration::from_millis(400));
    input.key(evdev::KEY_A, false);
    session.settle(Duration::from_millis(300));

    let keys = session.keys();
    let repeats: Vec<_> = keys.iter().filter(|key| key.4).collect();
    assert!(
        repeats.len() >= 2,
        "a held key repeats more than once: {keys:?}"
    );
    for repeat in &repeats {
        assert_eq!(repeat.0, Some(KeyCode::KeyA));
        assert_eq!(
            repeat.3,
            ButtonState::Pressed,
            "a repeat is a press; it never produces a release edge, which is \
             what keeps hold-pattern detection correct"
        );
    }
    // Exactly one real press and one real release, whatever happened in
    // between: the repeats must not have invented edges.
    let real: Vec<_> = keys.iter().filter(|key| !key.4).collect();
    assert_eq!(real.len(), 2, "one press, one release: {real:?}");
    assert_eq!(real[0].3, ButtonState::Pressed);
    assert_eq!(real[1].3, ButtonState::Released);

    // And repeats stop on release rather than running forever.
    session.events.clear();
    session.settle(Duration::from_millis(300));
    assert!(
        session.keys().is_empty(),
        "the release stopped the repeat: {:?}",
        session.keys()
    );

    // Timestamps rise monotonically and are spaced by the compositor's rate,
    // not quantized to whenever `pump` happened to run.
    let times: Vec<Duration> = session
        .events
        .iter()
        .filter_map(|event| event.time().map(crcbl_shell::EventTime::as_duration))
        .collect();
    assert!(
        times.windows(2).all(|pair| pair[1] >= pair[0]),
        "timestamps never go backwards: {times:?}"
    );

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// Pointer focus, motion, buttons — from a compositor that hit-tested them.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn pointer_focus_motion_and_buttons_come_back_in_window_pixels() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e pointer");

    input.move_to(960, 540, OUTPUT_SIZE);
    session.pump_until("the pointer to enter the surface", |session| {
        session.names().contains(&"PointerFocus")
    });
    let entered = session
        .events
        .iter()
        .find_map(|event| match event {
            ShellEvent::PointerFocus {
                entered: true,
                position,
                ..
            } => Some(*position),
            _ => None,
        })
        .expect("an enter with a position");
    let entered = entered.expect("the compositor said where");
    assert!(
        (entered.x - 960.0).abs() < 2.0 && (entered.y - 540.0).abs() < 2.0,
        "the pointer entered where it was put, in window pixels: {entered:?}"
    );

    session.take_names();
    input.move_to(200, 300, OUTPUT_SIZE);
    // Wait for the motion that carries the *new* position, not merely for any
    // motion: the enter above can still have one in flight, and a probe that
    // accepted it would pass on a stale value.
    session.pump_until("a motion event at the new position", |session| {
        session.events.iter().any(|event| match event {
            ShellEvent::PointerMotion { abs: Some(abs), .. } => {
                (abs.x - 200.0).abs() < 2.0 && (abs.y - 300.0).abs() < 2.0
            }
            _ => false,
        })
    });

    session.take_names();
    input.button(evdev::BTN_LEFT, true);
    input.button(evdev::BTN_LEFT, false);
    input.button(evdev::BTN_SIDE, true);
    input.button(evdev::BTN_SIDE, false);
    session.pump_until("four button events", |session| {
        session
            .events
            .iter()
            .filter(|event| event.name() == "Button")
            .count()
            >= 4
    });
    let buttons: Vec<(PointerButton, ButtonState)> = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Button { button, state, .. } => Some((*button, *state)),
            _ => None,
        })
        .collect();
    assert_eq!(
        buttons,
        [
            (PointerButton::Left, ButtonState::Pressed),
            (PointerButton::Left, ButtonState::Released),
            (PointerButton::Back, ButtonState::Pressed),
            (PointerButton::Back, ButtonState::Released),
        ],
        "BTN_SIDE is the thumb 'back' button, not an anonymous index"
    );
    let position = session
        .events
        .iter()
        .find_map(|event| match event {
            ShellEvent::Button { position, .. } => Some(*position),
            _ => None,
        })
        .expect("Button");
    assert!(
        position.is_some(),
        "a click carries where it happened; that is the whole event"
    );

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// A wheel detent is detents, not the pixel count the compositor made up.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_wheel_notch_is_reported_as_a_detent_not_as_pixels() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e wheel");

    input.move_to(960, 540, OUTPUT_SIZE);
    session.pump_until("the pointer to enter", |session| {
        session.names().contains(&"PointerFocus")
    });
    session.take_names();

    input.wheel(1);
    session.pump_until("a wheel event", |session| {
        session.names().contains(&"Wheel")
    });
    // Everything else the compositor had queued has been delivered by now, so
    // the "exactly one Wheel" count below is counting this notch alone.
    session.settle(Duration::from_millis(150));
    let delta = session
        .events
        .iter()
        .find_map(|event| match event {
            ShellEvent::Wheel { delta, .. } => Some(*delta),
            _ => None,
        })
        .expect("Wheel");
    match delta {
        ScrollDelta::Lines { x, y } => {
            assert!((x).abs() < f32::EPSILON, "a vertical notch has no x: {x}");
            assert!(
                (y.abs() - 1.0).abs() < 0.01,
                "one notch is one detent, not {y}"
            );
        }
        ScrollDelta::Pixels { .. } => panic!(
            "a notched wheel reported pixels; `ScrollDelta` exists precisely so \
             this cannot be collapsed: {delta:?}"
        ),
    }
    // Exactly one wheel event for one notch, even though the compositor sends
    // an `axis` and an `axis_value120` for it.
    assert_eq!(
        session
            .events
            .iter()
            .filter(|event| event.name() == "Wheel")
            .count(),
        1,
        "one notch is one event: {:?}",
        session.names()
    );

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// The other half of [`ScrollDelta`]: a touchpad reports pixels.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_touchpad_scroll_is_reported_as_pixels_not_as_detents() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e touchpad");

    input.move_to(960, 540, OUTPUT_SIZE);
    session.pump_until("the pointer to enter", |session| {
        session.names().contains(&"PointerFocus")
    });
    session.take_names();

    input.touchpad_scroll(13.0);
    session.pump_until("a wheel event", |session| {
        session.names().contains(&"Wheel")
    });
    let delta = session
        .events
        .iter()
        .find_map(|event| match event {
            ShellEvent::Wheel { delta, .. } => Some(*delta),
            _ => None,
        })
        .expect("Wheel");
    match delta {
        ScrollDelta::Pixels { x, y } => {
            assert!(x.abs() < 1e-6, "a vertical scroll has no x: {x}");
            assert!(
                (y + 13.0).abs() < 0.5,
                "13 px down is -13 in the engine's away-from-the-user \
                 convention, got {y}"
            );
        }
        ScrollDelta::Lines { .. } => panic!(
            "a continuous scroll was rounded into detents; a touchpad has no \
             detents to round to: {delta:?}"
        ),
    }

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// Pointer lock plus relative motion — the pair a first-person camera needs.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_locked_pointer_reports_raw_motion_and_no_absolute_position() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e lock");

    input.move_to(960, 540, OUTPUT_SIZE);
    session.pump_until("the pointer to enter", |session| {
        session.names().contains(&"PointerFocus")
    });

    session
        .shell
        .set_pointer_mode(window, PointerMode::Locked)
        .expect("pointer-constraints is advertised");
    assert_eq!(
        session
            .shell
            .window_state(window)
            .expect("live")
            .pointer_mode,
        PointerMode::Locked
    );
    session.settle(Duration::from_millis(200));
    session.take_names();

    input.move_by(12.0, -7.0);
    // Again, the *specific* delta rather than any relative motion: an absolute
    // move still in flight from before the lock also carries one.
    session.pump_until("the relative motion we sent", |session| {
        session.events.iter().any(|event| match event {
            ShellEvent::PointerMotion {
                raw_delta: Some((dx, dy)),
                ..
            } => (dx - 12.0).abs() < 1.0 && (dy + 7.0).abs() < 1.0,
            _ => false,
        })
    });
    for event in &session.events {
        if let ShellEvent::PointerMotion { abs, .. } = event {
            assert_eq!(
                *abs, None,
                "a locked pointer has no meaningful absolute position, and \
                 reporting the frozen one would make a camera appear to work"
            );
        }
    }

    // Unlocking restores absolute reporting.
    session
        .shell
        .set_pointer_mode(window, PointerMode::Free)
        .expect("free");
    session.settle(Duration::from_millis(200));
    session.take_names();
    input.move_to(400, 400, OUTPUT_SIZE);
    session.pump_until("absolute motion again", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { abs: Some(_), .. }))
    });

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// Confinement is a separate capability and a separate constraint object.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn confining_the_pointer_is_accepted_and_reported_in_window_state() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e confine");

    session
        .shell
        .set_pointer_mode(window, PointerMode::Confined)
        .expect("pointer-constraints advertises confine too");
    assert_eq!(
        session
            .shell
            .window_state(window)
            .expect("live")
            .pointer_mode,
        PointerMode::Confined
    );
    // Switching straight from one constraint to the other must destroy the
    // first: `zwp_pointer_constraints_v1` raises `already_constrained` and
    // disconnects the client otherwise, which would show up as every later
    // assertion failing at once.
    session
        .shell
        .set_pointer_mode(window, PointerMode::Locked)
        .expect("relock");
    session
        .shell
        .set_pointer_mode(window, PointerMode::Free)
        .expect("free");
    session.settle(Duration::from_millis(200));
    assert!(
        session.shell.window_state(window).is_ok(),
        "the connection survived three constraint changes"
    );

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// Hiding the cursor needs no buffer; naming a shape is recorded and inert.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn hiding_the_cursor_works_and_naming_a_shape_is_accepted() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e cursor");
    input.move_to(960, 540, OUTPUT_SIZE);
    session.pump_until("the pointer to enter", |session| {
        session.names().contains(&"PointerFocus")
    });

    // Both directions are accepted; only the hide is expressible without a
    // buffer, which the backend documents rather than hiding.
    session.shell.set_cursor(window, None).expect("hide");
    session
        .shell
        .set_cursor(window, Some(CursorIcon::Crosshair))
        .expect("a shape is recorded, not refused");
    session.settle(Duration::from_millis(100));
    assert!(
        session.shell.window_state(window).is_ok(),
        "a null-surface set_cursor is valid protocol, not a disconnect"
    );

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// Unplugging the seat's devices mid-session is survivable.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_seat_that_loses_its_devices_drops_focus_and_keeps_running() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e hotplug");
    input.move_to(960, 540, OUTPUT_SIZE);
    session.pump_until("the pointer to enter", |session| {
        session.names().contains(&"PointerFocus")
    });
    session.take_names();

    // Unplug both. `wl_seat.capabilities` drops to zero, and the backend has to
    // release its `wl_pointer` and `wl_keyboard` without leaving the window
    // focused by a device that no longer exists.
    drop(input);
    session.pump_until("the pointer to leave with its device", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerFocus { entered: false, .. }))
    });
    session.settle(Duration::from_millis(200));
    assert!(
        session.shell.window_state(window).is_ok(),
        "losing every input device is not a disconnect"
    );

    // And plugging them back in works, which is the half that a shell that
    // only handled removal would fail.
    let input = VirtualInput::attach(&*session.shell, window).expect("replug");
    session.take_names();
    input.move_to(500, 500, OUTPUT_SIZE);
    session.pump_until("the pointer to come back", |session| {
        session.names().contains(&"PointerFocus")
    });

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// `xdg_output` gives a monitor layout the mode alone cannot.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn monitor_bounds_come_from_xdg_output() {
    let session = Session::open();
    let monitor = &session.shell.monitors()[0];
    assert_eq!(
        monitor.bounds.x, 0,
        "the config puts the output at the origin"
    );
    assert_eq!(monitor.bounds.y, 0);
    assert_eq!(monitor.size(), OUTPUT_SIZE);
    assert!(
        (monitor.scale_factor - 1.0).abs() < 1e-9,
        "scale 1: mode and logical size agree, got {}",
        monitor.scale_factor
    );
    // Still not a window position, and the capability still says so.
    assert!(
        !session.shell.caps().contains(ShellCaps::WINDOW_POSITION),
        "xdg_output places monitors, not windows"
    );
}

/// Restores sway's output scale when the fractional-scale test is done, however
/// it ends.
struct OutputScale;

impl Drop for OutputScale {
    fn drop(&mut self) {
        swaymsg(&["output", "HEADLESS-1", "scale", "1"]);
    }
}

/// The finding this slice was asked for: what a real compositor does about
/// fractional scale, and whether the seam's model survives it.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_fractional_output_scale_reaches_the_window_as_a_non_integer_factor() {
    let mut session = Session::open();
    let window = session.create_mapped(&desc("crcbl e2e scale", LogicalSize::new(640.0, 360.0)));
    let before = session
        .shell
        .window_state(window)
        .expect("live")
        .scale_factor()
        .expect("configured");
    assert!(
        (before - 1.0).abs() < 1e-9,
        "the output starts at scale 1, got {before}"
    );

    let _restore = OutputScale;
    assert!(
        swaymsg(&["output", "HEADLESS-1", "scale", "1.5"]),
        "sway scales an output through IPC"
    );
    session.pump_until("the compositor's preferred scale", |session| {
        session
            .shell
            .window_state(window)
            .expect("live")
            .scale_factor()
            .is_some_and(|scale| (scale - 1.0).abs() > 1e-9)
    });

    let state = session.shell.window_state(window).expect("live");
    let scale = state.scale_factor().expect("configured");
    assert!(
        (scale - 1.5).abs() < 1e-9,
        "fractional-scale-v1 reports 180/120 = 1.5, not the integer 2 that \
         `wl_output.scale` would have said; got {scale}"
    );
    assert!(
        session.names().contains(&"ScaleFactorChanged"),
        "the change arrived as an event, not only as state: {:?}",
        session.names()
    );
    // The size that comes with it is a real buffer size, and it is the one the
    // seam's rounding produces from the logical size the compositor asked for.
    let size = state.size().expect("configured");
    assert!(
        !size.is_empty() && size.width > 0,
        "a scaled window still has an extent: {size:?}"
    );

    // The monitor's own scale is fractional too, which `wl_output.scale` alone
    // could never say.
    session.pump_until("the monitor list to catch up", |session| {
        session
            .shell
            .monitors()
            .first()
            .is_some_and(|monitor| (monitor.scale_factor - 1.5).abs() < 1e-9)
    });
    let monitor = &session.shell.monitors()[0];
    assert_eq!(
        monitor.size(),
        OUTPUT_SIZE,
        "the monitor's *size* is still the mode — 1920x1080 device pixels — \
         while its scale came from xdg_output's 1280x720 logical size. Taking \
         the size from xdg_output too would shrink every monitor by the scale."
    );
    assert_eq!(monitor.bounds.x, 0, "one output, still at the origin");

    // And a shell opened *now* sees the fractional scale from its very first
    // `monitors()` call, with no pumping at all. That is the startup path
    // rather than the hotplug one: `open` binds `wl_output` and
    // `zxdg_output_manager_v1` in one round trip and can only create the
    // `zxdg_output_v1` afterwards, so the logical geometry lands a round trip
    // behind the mode. A backend that published monitors once and never
    // reconciled would report the integer 2 here for the whole session.
    let fresh = Session::open();
    let fresh_monitor = &fresh.shell.monitors()[0];
    assert!(
        (fresh_monitor.scale_factor - 1.5).abs() < 1e-9,
        "a freshly opened shell already knows the output is at 1.5, got {}",
        fresh_monitor.scale_factor
    );
    assert_eq!(fresh_monitor.size(), OUTPUT_SIZE);
    drop(fresh);

    session.shell.destroy_window(window).expect("destroy");
}

/// The decoration negotiation happened, and sway is drawing them.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn server_side_decorations_are_negotiated_rather_than_assumed() {
    let mut session = Session::open();
    assert!(
        session.shell.caps().contains(ShellCaps::SERVER_DECORATIONS),
        "sway advertises zxdg_decoration_manager_v1"
    );
    let window = session.create_mapped(&desc(
        "crcbl e2e decoration",
        LogicalSize::new(500.0, 400.0),
    ));

    // Asked externally rather than by reading our own field back: sway reports
    // `"border": "csd"` for a client that decorates itself, and the configured
    // border otherwise. If the negotiation had silently failed, this would say
    // `csd` and the engine would have shipped an undecorated window.
    let tree = Command::new("swaymsg")
        .args(["-t", "get_tree", "-r"])
        .output()
        .expect("swaymsg ships with sway");
    let tree = String::from_utf8_lossy(&tree.stdout);
    let window_entry = tree
        .split(&format!("\"app_id\": \"{APP_ID}\""))
        .nth(1)
        .map(|tail| tail.to_string())
        .or_else(|| {
            tree.split(APP_ID)
                .nth(1)
                .map(std::string::ToString::to_string)
        })
        .expect("the mapped window is in sway's tree");
    let _ = window_entry;
    assert!(
        !tree.contains("\"border\": \"csd\""),
        "the compositor accepted server-side decorations; a `csd` border would \
         mean it refused and the window has no title bar at all"
    );

    session.shell.destroy_window(window).expect("destroy");
}

/// The epoch contract, exercised through the seam rather than asserted about.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn aligning_the_event_clock_moves_input_timestamps_onto_the_engine_epoch() {
    let mut session = Session::open();
    let (window, input) = session.with_input("crcbl e2e clock");

    // Pretend the engine clock has been running for a minute before the shell
    // was created. Every subsequent timestamp must be past that mark.
    session.shell.align_event_clock(Duration::from_secs(60));
    session.take_names();

    input.tap(evdev::KEY_A);
    session.pump_until("a timestamped input event", |session| {
        session.events.iter().any(ShellEvent::is_input)
    });
    let time = session
        .events
        .iter()
        .find_map(ShellEvent::time)
        .expect("an input event")
        .as_duration();
    assert!(
        time >= Duration::from_secs(60),
        "after alignment the compositor's clock reads as engine time, got {time:?}"
    );
    assert!(
        time < Duration::from_secs(600),
        "and it is not the raw CLOCK_MONOTONIC value, which is uptime: {time:?}"
    );

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}
