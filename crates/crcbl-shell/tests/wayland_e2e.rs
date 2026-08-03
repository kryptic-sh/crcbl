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

use crcbl_shell::wayland_test_support::{DragSource, VirtualInput};
use crcbl_shell::{
    ButtonState, CloseReply, CursorIcon, DisplayMode, KeyCode, Keysym, LogicalSize, Modifiers,
    MonitorInfo, PhysicalSize, PointerButton, PointerMode, ScrollDelta, Shell, ShellBackend,
    ShellCaps, ShellError, ShellEvent, SizeConstraints, SurfaceTarget, WindowDesc, WindowId,
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

/// The most any single [`Session::pump`] may take before the frame loop is
/// judged to have blocked.
///
/// **Not a performance budget**, and it cannot be one: under contention this
/// process can be descheduled in the middle of a `pump` for as long as the
/// scheduler likes, and that says nothing whatever about the backend. What it
/// still catches is the failure it was written for — a `pump` that *waits on a
/// peer* is not late by a scheduling quantum, it is late by the whole
/// conversation: the stalled-transfer deadline, or every pipe-full of a 512 KiB
/// paste.
///
/// The same constant, for the same reasons, as the X11 suite's `SLOWEST_PUMP`;
/// the two backends must not disagree about what "the loop kept running" means.
const SLOWEST_PUMP: Duration = Duration::from_millis(1_500);

/// The `app_id` the sway config matches on to float the window. Changing it
/// here without changing `wayland-e2e-sway.conf` makes the borderless test
/// vacuous, since a tiled window already fills the output.
const APP_ID: &str = "sh.kryptic.crcbl.e2e";

/// The first output declared in `wayland-e2e-sway.conf`, which is the one the
/// focused workspace is pinned to and therefore the one every window opens on.
const OUTPUT_SIZE: PhysicalSize = PhysicalSize::new(1920, 1080);

/// The name of that output. Positions in `monitors()` are the compositor's
/// order, not the config's, so a test that wants a *particular* display asks
/// for it by name.
const OUTPUT_NAME: &str = "HEADLESS-1";

/// The second output, deliberately a different size — see the config for why.
const SECOND_OUTPUT_SIZE: PhysicalSize = PhysicalSize::new(1280, 720);

/// Its name.
const SECOND_OUTPUT_NAME: &str = "HEADLESS-2";

/// Where the second output starts, which is the first one's width.
const SECOND_OUTPUT_X: i32 = 1920;

/// The two outputs side by side, which is the space an absolute pointer motion
/// is expressed in — **not** one output's size. See
/// [`VirtualInput::move_to`](crcbl_shell::wayland_test_support::VirtualInput).
const LAYOUT_SIZE: PhysicalSize = PhysicalSize::new(
    OUTPUT_SIZE.width + SECOND_OUTPUT_SIZE.width,
    OUTPUT_SIZE.height,
);

/// A shell plus the events it has produced, with a polling helper.
struct Session {
    shell: Box<dyn Shell>,
    events: Vec<ShellEvent>,
    /// The longest any single [`pump`](Self::pump) has taken.
    ///
    /// `Shell::pump` promises to be finite and non-blocking, and the clipboard
    /// is the first thing in this backend that could break that promise — it
    /// reads a pipe another application writes. Recording it here means every
    /// test that pastes is also a test that the frame loop kept running.
    slowest_pump: Duration,
}

impl Session {
    fn open() -> Self {
        let shell = crcbl_shell::open_backend(ShellBackend::Wayland)
            .expect("the harness exported WAYLAND_DISPLAY for a live sway");
        assert_eq!(shell.backend(), ShellBackend::Wayland);
        Self {
            shell,
            events: Vec::new(),
            slowest_pump: Duration::ZERO,
        }
    }

    fn pump(&mut self) {
        let started = Instant::now();
        let events = &mut self.events;
        self.shell.pump(&mut |event| events.push(event));
        self.slowest_pump = self.slowest_pump.max(started.elapsed());
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

    /// The monitor the compositor calls `name`.
    ///
    /// **By name, never by index.** `monitors()` is in the order the compositor
    /// advertised its outputs, which is not the order the config declares them:
    /// with two `HEADLESS` outputs, `monitors()[0]` is the second one. A test
    /// that indexed was reading whichever display sway happened to bind first.
    fn monitor_named(&self, name: &str) -> &MonitorInfo {
        self.shell
            .monitors()
            .iter()
            .find(|monitor| monitor.name == name)
            .unwrap_or_else(|| panic!("the config declares an output called {name}"))
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
                .is_some_and(DisplayMode::is_borderless)
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
            .is_some_and(DisplayMode::is_borderless)
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

/// [`WindowDesc::mode`] is honoured at creation, with no `set_mode` call
/// anywhere.
///
/// The X11 suite covers the same field; Wayland did not, and this is the path a
/// game's `--fullscreen` flag takes.
///
/// # Which configures the flash claim is about
///
/// `WindowDesc::mode`'s doc says setting it at creation "avoids a visible flash
/// of a decorated window". On Wayland that can only ever be a claim about
/// configures **after the surface is mapped**: xdg-shell requires an initial
/// commit with no buffer, the compositor answers that one with `0x0` — "you
/// choose" — and the backend reports the requested size for it. Nothing is on
/// screen for any of that, because nothing has been painted yet. So this walks
/// the configures one at a time rather than going through
/// [`Session::create_mapped`], and asserts the part a user could actually see:
/// once mapped, no configure reports the windowed size.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_window_created_borderless_is_never_mapped_at_its_windowed_size() {
    let mut session = Session::open();
    // Deliberately not the output size, so "it came up borderless" and "it came
    // up at the size we asked for" cannot be the same observation.
    let windowed = LogicalSize::new(640.0, 360.0);
    let window = session.create(&WindowDesc {
        mode: DisplayMode::Borderless { monitor: None },
        ..desc("crcbl e2e born borderless", windowed)
    });
    assert_eq!(
        session
            .shell
            .window_state(window)
            .expect("live")
            .requested_mode,
        DisplayMode::Borderless { monitor: None },
        "the desc's mode is what was asked for",
    );

    session.pump_until("the first configure", |session| {
        session.size(window).is_some()
    });
    let scale = session
        .shell
        .window_state(window)
        .expect("live")
        .scale_factor()
        .expect("configured");

    // Everything after this is a configure about a surface with a buffer on it,
    // which is to say one somebody could see.
    session.take_names();
    crcbl_shell::wayland_test_support::map_window(&*session.shell, window).expect("map");
    session.pump_until("the fullscreen configure", |session| {
        session
            .shell
            .window_state(window)
            .expect("live")
            .effective_mode()
            .is_some_and(DisplayMode::is_borderless)
    });

    // The headline, and asserted first: the flash this field exists to prevent
    // is a `Resized` carrying the size the window would have had if it had been
    // created windowed and switched afterwards.
    let flash = windowed.to_physical(scale);
    for event in &session.events {
        if let ShellEvent::Resized { size, .. } = event {
            assert_ne!(
                *size,
                flash,
                "a mapped window was configured at its windowed size, which is \
                 the flash WindowDesc::mode exists to avoid: {:?}",
                session.names()
            );
        }
    }

    let state = session.shell.window_state(window).expect("live");
    assert!(state.mode_request_honoured());
    assert_eq!(
        state.size(),
        Some(OUTPUT_SIZE),
        "borderless covers the whole output declared in the sway config",
    );

    session.shell.destroy_window(window).expect("destroy");
}

/// A fullscreen the client never asked for.
///
/// Every other mode test here drives [`Shell::set_mode`] and then watches for
/// the answer. A compositor does not need to be asked: sway will fullscreen a
/// window on a keybinding, a rule, or an IPC message, and the client's first
/// news of it is a configure. The seam's requested/effective split is what has
/// to survive that — `requested_mode` must still say what *we* asked for, so
/// `mode_request_honoured` cannot start reporting true for a request nobody
/// made.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_fullscreen_the_client_never_asked_for_arrives_as_a_resize() {
    let mut session = Session::open();
    let window = session.create_mapped(&desc("crcbl e2e imposed", LogicalSize::new(640.0, 360.0)));
    let before = session.size(window).expect("configured");
    assert_ne!(
        before, OUTPUT_SIZE,
        "the sway config floats this app_id; a tiled window already fills the \
         output and this test could not fail",
    );
    session.take_names();

    let matcher = format!("[app_id=\"{APP_ID}\"]");
    assert!(
        swaymsg(&[&matcher, "fullscreen", "enable"]),
        "swaymsg ships with sway; without it there is no way to make the \
         compositor impose a mode",
    );
    session.pump_until("the imposed fullscreen configure", |session| {
        session.size(window) == Some(OUTPUT_SIZE)
    });

    let first = session.monitor_named(OUTPUT_NAME).id;
    let state = session.shell.window_state(window).expect("live");
    assert_eq!(
        state.effective_mode(),
        Some(DisplayMode::Borderless {
            monitor: Some(first)
        }),
        "the compositor's answer is what effective_mode reports, and it names \
         the output the surface is on",
    );
    assert_eq!(
        state.requested_mode,
        DisplayMode::Windowed,
        "nothing asked for this, so the request is still the one we made",
    );
    assert!(
        !state.mode_request_honoured(),
        "an imposed mode is not a request being honoured",
    );
    assert!(
        session.names().contains(&"Resized"),
        "the imposed mode arrived as a Resized: {:?}",
        session.names()
    );

    // And it comes back off the same way, so a game that reconfigured its
    // swapchain for the imposed size gets told to undo it.
    session.take_names();
    assert!(swaymsg(&[&matcher, "fullscreen", "disable"]));
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
        "leaving an imposed fullscreen restores the pre-fullscreen geometry",
    );

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

    // And Close really closes. The first `CloseRequested` is still in the list,
    // and a probe that merely looks for the name would be satisfied by it — so
    // the wait below would not wait, and `reply_close_request` would be
    // answering a request the compositor has not sent yet. Clearing first is
    // what makes the probe demand a *new* one.
    session.take_names();
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
    assert_eq!(
        monitors.len(),
        2,
        "the config declares two HEADLESS outputs"
    );
    assert_eq!(
        monitors.iter().filter(|monitor| monitor.is_primary).count(),
        1,
        "exactly one primary, whichever output the compositor advertised first"
    );

    let monitor = session.monitor_named(OUTPUT_NAME);
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

    // The second one is a *different* size, which is what makes every
    // assertion about "which monitor" below capable of failing.
    let second = session.monitor_named(SECOND_OUTPUT_NAME);
    assert_eq!(second.size(), SECOND_OUTPUT_SIZE);
    assert_ne!(second.id, monitor.id, "two outputs, two ids");

    // The ids round-trip through the lookup the seam provides.
    for wanted in [monitor.id, second.id] {
        assert_eq!(
            session.shell.monitor(wanted).map(|found| found.id),
            Some(wanted)
        );
    }
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
    // P0.5c: both come from `wl_data_device_manager`, which sway advertises.
    assert!(caps.contains(ShellCaps::CLIPBOARD));
    assert!(caps.contains(ShellCaps::DRAG_DROP));
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

    // This window is unmapped, so it has never had keyboard focus, so the
    // compositor has never told this client what is on the clipboard. That is
    // *not* an empty clipboard, and the seam now says so in two ways.
    assert!(
        !session.shell.clipboard_readable(window),
        "a window that cannot receive wl_data_device.selection is not readable"
    );
    let request = session
        .shell
        .clipboard_request(window, crcbl_shell::MimeType::TextUtf8)
        .expect("the capability is present, so the read is accepted");
    // Held rather than answered: obligation 5. It still *ends*, which is
    // obligation 4 — a request with no answer is a UI stuck on "pasting…".
    session.pump_until("the held read to give up", |session| {
        session.clipboard_answer(request).is_some()
    });
    let (mime, content) = session.clipboard_answer(request).expect("answered");
    assert_eq!(
        content,
        crcbl_shell::ClipboardContent::Unavailable,
        "never readable is not the same as empty"
    );
    assert!(
        mime.matches(crcbl_shell::MimeType::TextUtf8),
        "the answer names the format that was asked for"
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
        self.with_input_desc(&desc(title, LogicalSize::new(640.0, 360.0)))
    }

    /// [`with_input`](Self::with_input) for a window that needs a different
    /// descriptor — a drop target, above all.
    fn with_input_desc(&mut self, description: &WindowDesc<'_>) -> (WindowId, VirtualInput) {
        let window = self.create_mapped(description);
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

    // The key timestamps rise monotonically and are spaced by the compositor's
    // rate, not quantized to whenever `pump` happened to run.
    //
    // Collected *before* the clear below, which is the repeat sequence this is
    // about. Reading them after it — as this did — left the list empty and the
    // `windows(2)` check vacuously true, which reads as coverage and is not.
    //
    // Over `Key` events specifically, and that is not a narrowing: it is the
    // claim the comment above has always made. Mixing kinds does not test it,
    // because `wl_keyboard.key` carries a **millisecond** and `wl_pointer.enter`
    // carries no timestamp at all, so the backend stamps the latter from its own
    // clock at full precision. Two events in the same millisecond then order
    // either way — a truncation artefact of the protocol rather than anything
    // the backend chose, and an assertion that failed on it would be reporting
    // Wayland's units as a defect.
    let times: Vec<Duration> = session
        .events
        .iter()
        .filter(|event| matches!(event, ShellEvent::Key { .. }))
        .filter_map(|event| event.time().map(crcbl_shell::EventTime::as_duration))
        .collect();
    assert!(
        times.len() > repeats.len(),
        "every repeat carried a timestamp, and so did the press and release \
         around them: {times:?}"
    );
    assert!(
        times.windows(2).all(|pair| pair[1] >= pair[0]),
        "timestamps never go backwards: {times:?}"
    );

    // And repeats stop on release rather than running forever.
    session.events.clear();
    session.settle(Duration::from_millis(300));
    assert!(
        session.keys().is_empty(),
        "the release stopped the repeat: {:?}",
        session.keys()
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

    input.move_to(960, 540, LAYOUT_SIZE);
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
    input.move_to(200, 300, LAYOUT_SIZE);
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

    input.move_to(960, 540, LAYOUT_SIZE);
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

    input.move_to(960, 540, LAYOUT_SIZE);
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

    input.move_to(960, 540, LAYOUT_SIZE);
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
    input.move_to(400, 400, LAYOUT_SIZE);
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
    input.move_to(960, 540, LAYOUT_SIZE);
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
    input.move_to(960, 540, LAYOUT_SIZE);
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
    input.move_to(500, 500, LAYOUT_SIZE);
    session.pump_until("the pointer to come back", |session| {
        session.names().contains(&"PointerFocus")
    });

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// `xdg_output` gives a monitor layout the mode alone cannot.
///
/// **The layout is the thing `wl_output.mode` cannot tell you.** A mode says
/// how big a display is; only `xdg_output` says where it sits relative to the
/// others, and with one output every wrong answer and the right one are all
/// `0, 0`. Two outputs side by side make the second one's `x` a number that has
/// to have come from somewhere.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn monitor_bounds_come_from_xdg_output() {
    let session = Session::open();
    let monitor = session.monitor_named(OUTPUT_NAME);
    assert_eq!(
        monitor.bounds.x, 0,
        "the config puts the first output at the origin"
    );
    assert_eq!(monitor.bounds.y, 0);
    assert_eq!(monitor.size(), OUTPUT_SIZE);
    assert!(
        (monitor.scale_factor - 1.0).abs() < 1e-9,
        "scale 1: mode and logical size agree, got {}",
        monitor.scale_factor
    );

    let second = session.monitor_named(SECOND_OUTPUT_NAME);
    assert_eq!(
        second.bounds.x, SECOND_OUTPUT_X,
        "the config puts the second output to the right of the first"
    );
    assert_eq!(second.bounds.y, 0);
    assert_eq!(second.size(), SECOND_OUTPUT_SIZE);

    // One coordinate space, so the two do not overlap. A backend that reported
    // each output's own origin would put both at 0,0 and pass everything above
    // except this.
    assert!(
        second.bounds.x
            >= monitor.bounds.x + i32::try_from(monitor.bounds.width).expect("a screen width"),
        "the outputs are side by side in one space: {:?} and {:?}",
        monitor.bounds,
        second.bounds
    );

    // Still not a window position, and the capability still says so.
    assert!(
        !session.shell.caps().contains(ShellCaps::WINDOW_POSITION),
        "xdg_output places monitors, not windows"
    );
}

/// A fullscreen request naming the second monitor lands on the second monitor.
///
/// **The `monitor` field is the only part of `DisplayMode::Borderless` that a
/// single-output test cannot check.** With one display every value of it —
/// `None`, and the one id there is — produces the same window at the same size,
/// so a backend that dropped the field on the floor would pass. Here the two
/// outputs are different sizes, so the extent the compositor configures says
/// which one it used.
///
/// Wayland's `set_fullscreen` takes the output as a *hint* the compositor may
/// ignore, which is why this asserts through `mode_request_honoured` and the
/// size rather than assuming. sway honours it.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn borderless_on_a_named_monitor_uses_that_monitor() {
    // **sway's focused workspace outlives the process that moved it.** Every
    // test here runs against one compositor, and a window fullscreened onto the
    // second output leaves the focus there — so the next test's window opens on
    // a 1280x720 display and waits out its deadline for a 1920x1080 configure.
    // The same shape as the X11 suite's pointer, in a different compositor.
    let _restore = FocusedWorkspace;
    let mut session = Session::open();
    let window = session.create_mapped(&desc("second monitor", LogicalSize::new(640.0, 360.0)));
    let second = session.monitor_named(SECOND_OUTPUT_NAME).id;
    assert_ne!(
        session.size(window),
        Some(SECOND_OUTPUT_SIZE),
        "the windowed size must differ from the answer, or the assertion below          cannot fail"
    );

    session
        .shell
        .set_mode(
            window,
            DisplayMode::Borderless {
                monitor: Some(second),
            },
        )
        .expect("set_mode");
    // Waiting on the *answer*, not on the size: `wl_surface.enter` for the
    // monitor the window landed on arrives after the configure that resized
    // it, so a test that asserted the moment the extent changed would read the
    // monitor the window came from. Which is the ordering the backend's
    // `surface_output_changed` refresh exists for.
    session.pump_until("the compositor's answer", |session| {
        session
            .shell
            .window_state(window)
            .is_ok_and(|state| state.mode_request_honoured())
    });

    let state = session.shell.window_state(window).expect("state");
    assert!(
        state.mode_request_honoured(),
        "the compositor put it on the monitor that was asked for, so the \
         request is honoured: asked for {second:?}, effective {:?}, monitors \
         {:?}",
        state.effective_mode(),
        session
            .shell
            .monitors()
            .iter()
            .map(|monitor| (monitor.id, monitor.name.clone(), monitor.size()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        state.size(),
        Some(SECOND_OUTPUT_SIZE),
        "and the extent is the second output's, not the first's"
    );

    session.shell.destroy_window(window).expect("destroy");
}

/// A fullscreen request naming a monitor that is not there is refused by name.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn borderless_on_a_monitor_that_is_not_there_is_refused() {
    let mut session = Session::open();
    let window = session.create_mapped(&desc("no such monitor", LogicalSize::new(640.0, 360.0)));
    let absent = crcbl_shell::MonitorId(u32::MAX);
    let error = session
        .shell
        .set_mode(
            window,
            DisplayMode::Borderless {
                monitor: Some(absent),
            },
        )
        .expect_err("there is no such output");
    assert!(
        matches!(error, ShellError::NoSuchMonitor(id) if id == absent.0),
        "the error names the monitor: {error}"
    );
    session.shell.destroy_window(window).expect("destroy");
}

/// Puts sway's focus back on the first output's workspace when a test that
/// moved it is done, however it ends.
struct FocusedWorkspace;

impl Drop for FocusedWorkspace {
    fn drop(&mut self) {
        swaymsg(&["workspace", "1"]);
    }
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
        (session.monitor_named(OUTPUT_NAME).scale_factor - 1.5).abs() < 1e-9
    });
    let monitor = session.monitor_named(OUTPUT_NAME);
    assert_eq!(
        monitor.size(),
        OUTPUT_SIZE,
        "the monitor's *size* is still the mode — 1920x1080 device pixels — \
         while its scale came from xdg_output's 1280x720 logical size. Taking \
         the size from xdg_output too would shrink every monitor by the scale."
    );
    assert_eq!(
        monitor.bounds.x, 0,
        "the scaled output is still at the origin"
    );
    assert!(
        (session.monitor_named(SECOND_OUTPUT_NAME).scale_factor - 1.0).abs() < 1e-9,
        "and the other output was not scaled with it"
    );

    // And a shell opened *now* sees the fractional scale from its very first
    // `monitors()` call, with no pumping at all. That is the startup path
    // rather than the hotplug one: `open` binds `wl_output` and
    // `zxdg_output_manager_v1` in one round trip and can only create the
    // `zxdg_output_v1` afterwards, so the logical geometry lands a round trip
    // behind the mode. A backend that published monitors once and never
    // reconciled would report the integer 2 here for the whole session.
    let fresh = Session::open();
    let fresh_monitor = fresh.monitor_named(OUTPUT_NAME);
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

// ---------------------------------------------------------------------------
// P0.5c — clipboard and drag-and-drop, with a real second client
// ---------------------------------------------------------------------------

/// The `app_id` of the *peer* client, so `swaymsg` can tell the two apart when
/// a test has to move keyboard focus between them.
const PEER_APP_ID: &str = "sh.kryptic.crcbl.e2e.peer";

/// A window belonging to the peer client.
fn peer_desc(title: &str) -> WindowDesc<'_> {
    WindowDesc {
        title,
        app_id: PEER_APP_ID,
        size: LogicalSize::new(480.0, 320.0),
        ..WindowDesc::default()
    }
}

/// Moves keyboard focus to the named application, and fails if sway will not.
///
/// Focus is load-bearing rather than cosmetic here: `wl_data_device.selection`
/// is delivered **only** to the client with keyboard focus on that seat, so a
/// clipboard test that does not control focus is testing nothing.
fn focus_app(app_id: &str) {
    // Anchored, and with the dots escaped: sway matches criteria as **regular
    // expressions**, unanchored, so a bare `sh.kryptic.crcbl.e2e` also matches
    // `sh.kryptic.crcbl.e2e.peer` — and focusing "either of the two windows"
    // is not a test, it is a coin flip.
    let pattern = format!("^{}$", app_id.replace('.', "\\."));
    assert!(
        swaymsg(&[&format!("[app_id=\"{pattern}\"]"), "focus"]),
        "swaymsg could not focus {app_id}; the clipboard tests need it and must \
         not quietly pass without it"
    );
}

impl Session {
    /// The answer to one [`Shell::clipboard_request`], if it has arrived.
    fn clipboard_answer(
        &self,
        request: crcbl_shell::ClipboardRequestId,
    ) -> Option<(crcbl_shell::ReceivedMime, crcbl_shell::ClipboardContent)> {
        self.events.iter().find_map(|event| match event {
            ShellEvent::ClipboardData {
                request: which,
                mime,
                content,
                ..
            } if *which == request => Some((mime.clone(), content.clone())),
            _ => None,
        })
    }

    /// Every file dropped so far.
    fn drops(&self) -> Vec<(std::path::PathBuf, Option<crcbl_shell::PhysicalPoint>)> {
        self.events
            .iter()
            .filter_map(|event| match event {
                ShellEvent::DroppedFile { path, position, .. } => Some((path.clone(), *position)),
                _ => None,
            })
            .collect()
    }

    /// A focused, mapped window with a keyboard on its seat, and **not**
    /// fullscreen.
    ///
    /// The clipboard needs focus (that is where `wl_data_device.selection` is
    /// delivered) and a serial (that is what `set_selection` quotes), and
    /// neither needs the pointer. Fullscreen is worse than unnecessary here: a
    /// fullscreen window on sway keeps focus when a second client maps a
    /// window, so a peer would never receive the selection at all.
    fn with_keyboard(&mut self, title: &str) -> (WindowId, VirtualInput) {
        let window = self.create_mapped(&desc(title, LogicalSize::new(640.0, 360.0)));
        let input = VirtualInput::attach(&*self.shell, window).expect("virtual devices");
        // Waited for by *typing*, not by reading `WindowState::focused`. The
        // flag is already true from `xdg_toplevel.state.activated`, which a
        // compositor sets before the seat even has a keyboard — and what a
        // clipboard write needs is not focus, it is a **serial**, which only a
        // real input event carries. A key that comes back is proof of both.
        let deadline = Instant::now() + WAIT;
        loop {
            input.tap(evdev::KEY_A);
            self.pump();
            if self
                .events
                .iter()
                .any(|event| matches!(event, ShellEvent::Key { .. }))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for a key to reach {title}, so the seat has \
                 no serial to claim the selection with"
            );
            self.shell.wait_events(Some(Duration::from_millis(20)));
        }
        self.take_names();
        (window, input)
    }

    /// A focused, mapped, fullscreen window with input devices, which either
    /// does or does not accept drops.
    fn dropping_window(&mut self, title: &str, accept_drops: bool) -> (WindowId, VirtualInput) {
        self.with_input_desc(&WindowDesc {
            accept_drops,
            ..desc(title, LogicalSize::new(640.0, 360.0))
        })
    }

    /// A mapped window on this connection that the compositor has focused.
    fn mapped_peer_window(&mut self, title: &str) -> WindowId {
        let window = self.create_mapped(&peer_desc(title));
        // Explicitly, rather than trusting sway's new-window focus policy,
        // which depends on what else is on the workspace.
        focus_app(PEER_APP_ID);
        self.pump_until("the peer window to be focused", |session| {
            session
                .shell
                .window_state(window)
                .is_ok_and(|state| state.focused)
        });
        window
    }
}

/// Pumps two clients until `ready`, or fails naming what never happened.
///
/// Both have to be pumped: a clipboard transfer is one client writing while the
/// other reads, and a test that only drove the reader would deadlock against
/// its own peer — which is exactly the failure a blocking implementation has
/// against a real one.
fn pump_pair(
    ours: &mut Session,
    peer: &mut Session,
    what: &str,
    ready: impl Fn(&Session, &Session) -> bool,
) {
    let deadline = Instant::now() + WAIT;
    loop {
        ours.pump();
        peer.pump();
        if ready(ours, peer) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out after {WAIT:?} waiting for {what}; ours: {:?}, peer: {:?}",
            ours.names(),
            peer.names()
        );
        ours.shell.wait_events(Some(Duration::from_millis(10)));
        peer.shell.wait_events(Some(Duration::from_millis(10)));
    }
}

/// Pastes once, and returns the bytes.
///
/// **There is no retry loop here, and that is the assertion.** Before P0.5c's
/// seam revision there had to be one: `wl_data_device.selection` is
/// asynchronous and focus-gated, so a request issued a moment early was
/// answered "there is nothing to paste", and only asking again got the bytes —
/// a loop no editor's paste command would ever contain. The backend now holds
/// the request until it can answer it, so one request is enough, and anything
/// other than [`ClipboardContent::Bytes`] here is a regression rather than a
/// reason to ask again.
fn paste_bytes(
    ours: &mut Session,
    mut peer: Option<&mut Session>,
    window: WindowId,
    mime: crcbl_shell::MimeType,
    what: &str,
) -> (crcbl_shell::ReceivedMime, Vec<u8>) {
    let request = ours
        .shell
        .clipboard_request(window, mime)
        .expect("clipboard capability");
    let deadline = Instant::now() + WAIT;
    loop {
        ours.pump();
        if let Some(peer) = peer.as_deref_mut() {
            peer.pump();
        }
        if let Some((spelling, content)) = ours.clipboard_answer(request) {
            let crcbl_shell::ClipboardContent::Bytes(bytes) = content else {
                panic!(
                    "{what} was answered {} on the first and only request; \
                     the seam is back to making consumers retry",
                    content.name()
                );
            };
            return (spelling, bytes);
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}; events: {:?}",
            ours.names()
        );
        ours.shell.wait_events(Some(Duration::from_millis(10)));
        if let Some(peer) = peer.as_deref_mut() {
            peer.shell.wait_events(Some(Duration::from_millis(10)));
        }
    }
}

/// We publish a selection and a genuinely separate Wayland client reads it.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_selection_we_publish_is_read_by_another_client() {
    let mut session = Session::open();
    let (window, input) = session.with_keyboard("crcbl e2e copy");

    // `set_selection` quotes an input serial, so a window that has never been
    // touched cannot claim the clipboard. This one has keyboard focus, which is
    // where its serial comes from.
    session
        .shell
        .clipboard_offer(
            window,
            &[
                crcbl_shell::ClipboardOffer::text("engine text"),
                crcbl_shell::ClipboardOffer::ron("(node:1)"),
            ],
        )
        .expect("claiming the selection");
    session.pump();

    // A second connection is a second client as far as the compositor is
    // concerned: its own `wl_data_device`, its own offers, its own everything.
    let mut peer = Session::open();
    let peer_window = peer.mapped_peer_window("crcbl e2e paste");

    // The peer only receives `wl_data_device.selection` once it has focus,
    // which mapping its window gave it.
    let (mime, data) = paste_bytes(
        &mut peer,
        Some(&mut session),
        peer_window,
        crcbl_shell::MimeType::CrcblRon,
        "the peer's paste",
    );
    assert_eq!(
        data, b"(node:1)",
        "the lossless engine format crossed the compositor intact"
    );
    assert_eq!(mime.as_str(), "application/x-crcbl+ron");

    // The other half of "offer both": a peer that wants text gets text, from
    // the same selection, without a second copy.
    let (_, text) = paste_bytes(
        &mut peer,
        Some(&mut session),
        peer_window,
        crcbl_shell::MimeType::TextUtf8,
        "the peer's text paste",
    );
    assert_eq!(text, b"engine text");

    drop(input);
    peer.shell.destroy_window(peer_window).expect("destroy");
    session.shell.destroy_window(window).expect("destroy");
}

/// Another client publishes, we read — including a format this engine cannot
/// name and a spelling it would never produce.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_peers_selection_arrives_spelled_the_way_the_peer_spelled_it() {
    let mut session = Session::open();
    let (window, input) = session.with_keyboard("crcbl e2e paste");

    let mut peer = Session::open();
    let peer_window = peer.mapped_peer_window("crcbl e2e copy");
    // Bare `text/plain` — the spelling GTK and every X11 client use, and one
    // `MimeType::TextUtf8` never produces — plus a vendor format the engine has
    // no variant for at all. This is what `ReceivedMime` exists for.
    peer.shell
        .clipboard_offer(
            peer_window,
            &[
                crcbl_shell::ClipboardOffer {
                    mime: crcbl_shell::MimeType::Other("text/plain"),
                    bytes: b"from another application",
                },
                crcbl_shell::ClipboardOffer {
                    mime: crcbl_shell::MimeType::Other("application/vnd.some-editor.scene+json"),
                    bytes: b"{\"scene\":1}",
                },
            ],
        )
        .expect("the peer claims the selection");
    peer.pump();

    // The selection reaches us only when we have focus again.
    focus_app(APP_ID);
    pump_pair(&mut session, &mut peer, "focus to come back", |ours, _| {
        ours.shell
            .window_state(window)
            .is_ok_and(|state| state.focused)
    });

    let (mime, data) = paste_bytes(
        &mut session,
        Some(&mut peer),
        window,
        crcbl_shell::MimeType::TextUtf8,
        "our paste",
    );
    assert_eq!(data, b"from another application");
    assert_eq!(
        mime.as_str(),
        "text/plain",
        "the peer's spelling survives; canonicalizing it would make an X11 \
         backend answer a target atom nobody asked for"
    );
    assert!(
        mime.matches(crcbl_shell::MimeType::TextUtf8),
        "and a paste handler asking for utf-8 text still recognises it"
    );
    assert_ne!(
        mime,
        crcbl_shell::ReceivedMime::from(crcbl_shell::MimeType::TextUtf8),
        "which is not the same thing as being equal to our own spelling"
    );

    // A format the engine cannot name in its own source is still readable, and
    // still reported with the string the peer chose.
    let (mime, data) = paste_bytes(
        &mut session,
        Some(&mut peer),
        window,
        crcbl_shell::MimeType::Other("application/vnd.some-editor.scene+json"),
        "the foreign paste",
    );
    assert_eq!(data, b"{\"scene\":1}");
    assert_eq!(mime.recognized(), None, "not a format the engine knows");
    assert_eq!(mime.as_str(), "application/vnd.some-editor.scene+json");

    // A format nobody offered is an answer, not an error, and not a hang.
    let missing = session
        .shell
        .clipboard_request(window, crcbl_shell::MimeType::UriList)
        .expect("clipboard capability");
    pump_pair(&mut session, &mut peer, "the empty answer", |ours, _| {
        ours.clipboard_answer(missing).is_some()
    });
    assert_eq!(
        session.clipboard_answer(missing).expect("answered").1,
        crcbl_shell::ClipboardContent::Empty,
        "the clipboard is readable and holds no uri-list — an answer, not a failure"
    );

    drop(input);
    peer.shell.destroy_window(peer_window).expect("destroy");
    session.shell.destroy_window(window).expect("destroy");
}

/// A peer that takes the descriptor and never writes must not stall the frame
/// loop — and must eventually answer.
///
/// This is the failure a blocking `read` on the event-loop thread turns into a
/// hang, and there is no protocol event for it: the transfer is over a pipe the
/// compositor only brokered.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_peer_that_never_writes_does_not_block_the_event_loop() {
    let mut session = Session::open();
    let (window, input) = session.with_keyboard("crcbl e2e stall");

    let mut peer = Session::open();
    let peer_window = peer.mapped_peer_window("crcbl e2e stall peer");
    peer.shell
        .clipboard_offer(
            peer_window,
            &[crcbl_shell::ClipboardOffer::text("never delivered")],
        )
        .expect("the peer claims the selection");
    peer.pump();

    focus_app(APP_ID);
    pump_pair(&mut session, &mut peer, "focus to come back", |ours, _| {
        ours.shell
            .window_state(window)
            .is_ok_and(|state| state.focused)
    });

    // From here the peer is never pumped again: its `wl_data_source.send`
    // arrives and is never serviced, so nothing is ever written to the pipe.
    let request = session
        .shell
        .clipboard_request(window, crcbl_shell::MimeType::TextUtf8)
        .expect("clipboard capability");

    session.slowest_pump = Duration::ZERO;
    let deadline = Instant::now() + Duration::from_secs(8);
    while session.clipboard_answer(request).is_none() {
        session.pump();
        assert!(
            Instant::now() < deadline,
            "a stalled transfer must answer rather than staying outstanding forever"
        );
        // A short blocking wait, which is what an editor idling at zero frames
        // per second does. It must not swallow the transfer's deadline.
        session.shell.wait_events(Some(Duration::from_millis(20)));
    }
    assert!(
        session.slowest_pump < SLOWEST_PUMP,
        "no single pump may wait on the peer; the slowest took {:?}",
        session.slowest_pump
    );
    assert_eq!(
        session.clipboard_answer(request).expect("answered").1,
        crcbl_shell::ClipboardContent::Unavailable,
        "a transfer that never delivered is a failure, not an empty clipboard — \
         there is something on it, we just could not get it"
    );

    drop(input);
    peer.shell.destroy_window(peer_window).expect("destroy");
    session.shell.destroy_window(window).expect("destroy");
}

/// A drop delivers a real path, parsed out of a `text/uri-list`.
///
/// The drag is started by [`DragSource`], which is harness scaffolding rather
/// than shell — see its docs for why the drag *source* is not in this slice.
/// The payload is transferred over a pipe between two objects on the same
/// connection, which is also the self-transfer case that deadlocks a backend
/// reading on its event-loop thread.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_drop_delivers_a_path_parsed_out_of_a_uri_list() {
    let mut session = Session::open();
    let (window, input) = session.dropping_window("crcbl e2e drop", true);
    let mut drag = DragSource::attach(&*session.shell, window).expect("drag origin");

    // The implicit grab: a press over our own surface, which is what a
    // compositor requires before it will start a drag at all.
    input.move_to(400, 300, LAYOUT_SIZE);
    input.button(evdev::BTN_LEFT, true);
    session.pump_until("the button press to reach the drag origin", |_| {
        drag.press_serial() != 0
    });

    // Two files and a comment line, with a percent-encoded space: what a file
    // manager sends, rather than what is convenient to parse.
    let uri_list = b"#dropped\r\nfile:///tmp/crcbl%20e2e.ron\r\nfile:///tmp/second.png\r\n";
    drag.start(&*session.shell, window, c"text/uri-list", uri_list)
        .expect("start_drag");
    session.settle(Duration::from_millis(200));

    // Move under the grab so the compositor sends `enter`/`motion`, then let go.
    input.move_to(900, 500, LAYOUT_SIZE);
    session.settle(Duration::from_millis(100));
    input.button(evdev::BTN_LEFT, false);

    session.pump_until("the dropped files", |session| session.drops().len() >= 2);
    let drops = session.drops();
    assert_eq!(
        drops[0].0,
        std::path::PathBuf::from("/tmp/crcbl e2e.ron"),
        "percent-encoding is decoded and the comment line is not a file"
    );
    assert_eq!(drops[1].0, std::path::PathBuf::from("/tmp/second.png"));
    let position = drops[0].1.expect("a drop has a position");
    assert!(
        position.x > 0.0 && position.y > 0.0,
        "the drop position is where the pointer was, in window pixels: {position:?}"
    );
    assert!(
        drag.was_asked_for_data(),
        "the source was asked for the payload, so the transfer was real"
    );

    drop(drag);
    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// A window that did not ask for drops does not get them.
///
/// `WindowDesc::accept_drops` is off by default, and the reason it is a request
/// rather than a note is here: the source is told we accept nothing, so its
/// cursor says so, and the payload is never even read.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_window_that_did_not_ask_for_drops_receives_none() {
    let mut session = Session::open();
    let (window, input) = session.dropping_window("crcbl e2e no drop", false);
    let mut drag = DragSource::attach(&*session.shell, window).expect("drag origin");

    input.move_to(400, 300, LAYOUT_SIZE);
    input.button(evdev::BTN_LEFT, true);
    session.pump_until("the button press to reach the drag origin", |_| {
        drag.press_serial() != 0
    });
    drag.start(
        &*session.shell,
        window,
        c"text/uri-list",
        b"file:///tmp/rejected.ron\r\n",
    )
    .expect("start_drag");
    session.settle(Duration::from_millis(200));
    input.move_to(900, 500, LAYOUT_SIZE);
    session.settle(Duration::from_millis(100));
    input.button(evdev::BTN_LEFT, false);

    // The only way to assert that something does *not* happen is to wait.
    session.settle(Duration::from_millis(800));
    assert!(
        session.drops().is_empty(),
        "a window without accept_drops must not receive DroppedFile: {:?}",
        session.drops()
    );
    assert!(
        !drag.was_asked_for_data(),
        "and the payload must never even be requested"
    );

    drop(drag);
    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// Pasting our **own** selection, which is the case that deadlocks a backend
/// that reads on the event-loop thread.
///
/// The compositor asks *this* client for the bytes, over a pipe *this* client
/// is reading, on the connection it is pumping. A blocking read waits for a
/// write that only a later pump could perform, and a blocking write waits for a
/// read that only a later pump could perform; the payload here is eight times
/// the size of a pipe buffer, so both halves have to be interleaved for it to
/// complete at all. `docs/plan/15-windowing.md`'s clipboard notes name the same
/// hazard for X11, where the owner answering its own `ConvertSelection` is the
/// textbook deadlock.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_client_can_paste_its_own_selection_without_deadlocking() {
    let mut session = Session::open();
    let (window, input) = session.with_keyboard("crcbl e2e self paste");

    // 512 KiB — a Linux pipe holds 64.
    let payload: String = core::iter::repeat_n("crcbl", 512 * 1024 / 5 + 1).collect();
    session
        .shell
        .clipboard_offer(window, &[crcbl_shell::ClipboardOffer::text(&payload)])
        .expect("claiming the selection");
    session.pump();

    session.slowest_pump = Duration::ZERO;
    let (mime, data) = paste_bytes(
        &mut session,
        None,
        window,
        crcbl_shell::MimeType::TextUtf8,
        "our own selection, pasted back",
    );
    assert_eq!(
        data.len(),
        payload.len(),
        "every byte came back, across as many pipe-fulls as it took"
    );
    assert_eq!(data, payload.as_bytes());
    assert!(mime.matches(crcbl_shell::MimeType::TextUtf8));
    assert!(
        session.slowest_pump < SLOWEST_PUMP,
        "no single pump may stall on the transfer; the slowest took {:?}",
        session.slowest_pump
    );

    drop(input);
    session.shell.destroy_window(window).expect("destroy");
}

/// A read issued while another client has focus is **held** until focus comes
/// back, and then answered with the bytes.
///
/// This is the exact race that used to be papered over by a retry loop in the
/// suite, driven deliberately instead: at the moment of asking, this client
/// cannot see the clipboard at all, and the seam's obligation 5 is that the
/// request waits rather than being answered "there is nothing to paste".
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn a_read_issued_before_the_clipboard_is_readable_is_held_not_answered_empty() {
    let mut session = Session::open();
    let (window, input) = session.with_keyboard("crcbl e2e held");

    let mut peer = Session::open();
    let peer_window = peer.mapped_peer_window("crcbl e2e held peer");
    peer.shell
        .clipboard_offer(
            peer_window,
            &[crcbl_shell::ClipboardOffer::text("published while focused")],
        )
        .expect("the peer claims the selection");
    peer.pump();

    // The peer has focus, so this client is not told what is on the clipboard.
    // The seam says so out loud rather than leaving it to be discovered.
    pump_pair(
        &mut session,
        &mut peer,
        "focus to move to the peer",
        |ours, _| !ours.shell.clipboard_readable(window),
    );
    assert!(
        !session.shell.clipboard_readable(window),
        "a background window cannot read the Wayland clipboard"
    );

    let request = session
        .shell
        .clipboard_request(window, crcbl_shell::MimeType::TextUtf8)
        .expect("a read is still accepted — it is held, not refused");
    // Held: several frames go by with no answer at all. Answering `Empty` here
    // would be the lie the whole revision exists to prevent.
    for _ in 0..10 {
        session.pump();
        peer.pump();
        assert!(
            session.clipboard_answer(request).is_none(),
            "an unreadable clipboard must not be answered as an empty one"
        );
        session.shell.wait_events(Some(Duration::from_millis(5)));
    }

    // Focus returns — which is all that was missing — and the *same* request
    // answers, with no second request from the caller.
    focus_app(APP_ID);
    pump_pair(
        &mut session,
        &mut peer,
        "the held read to answer",
        |ours, _| ours.clipboard_answer(request).is_some(),
    );
    let (_, content) = session.clipboard_answer(request).expect("answered");
    assert_eq!(
        content.bytes(),
        Some(&b"published while focused"[..]),
        "the held request was answered with the bytes, not with Empty"
    );
    assert!(
        session.shell.clipboard_readable(window),
        "and the clipboard reads as readable again"
    );

    drop(input);
    peer.shell.destroy_window(peer_window).expect("destroy");
    session.shell.destroy_window(window).expect("destroy");
}

/// Claiming the clipboard without an input serial is refused by name.
///
/// Not a bug and not a backend quirk: a Wayland client may not take the
/// selection without a serial from real user input, which is what stops a
/// background process stealing the clipboard. The seam has a named error for it
/// so that a caller can tell "ask the user to press the key again" apart from
/// "this is broken", and so that an X11 backend which never returns it is
/// obviously correct.
#[test]
#[ignore = "needs a Wayland compositor; run tests/run-wayland-e2e.sh"]
fn claiming_the_clipboard_without_user_input_is_refused_by_name() {
    let mut session = Session::open();
    // A window that exists and has never been touched: no key, no button, so
    // no serial on any seat.
    let window = session.create(&desc("crcbl e2e serial", LogicalSize::new(320.0, 240.0)));
    session.pump_until("the first configure", |session| {
        session.size(window).is_some()
    });

    let refused = session
        .shell
        .clipboard_offer(window, &[crcbl_shell::ClipboardOffer::text("stolen")]);
    assert!(
        matches!(refused, Err(ShellError::NeedsUserInteraction { .. })),
        "expected a named refusal, got {refused:?}"
    );

    session.shell.destroy_window(window).expect("destroy");
}
