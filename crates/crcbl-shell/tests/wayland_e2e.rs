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

use crcbl_shell::{
    CloseReply, DisplayMode, LogicalSize, PhysicalSize, Shell, ShellBackend, ShellCaps, ShellEvent,
    SizeConstraints, SurfaceTarget, WindowDesc, WindowId,
};

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
fn capabilities_are_honest_about_what_p05a_implements() {
    let mut session = Session::open();
    let caps = session.shell.caps();
    assert!(caps.contains(ShellCaps::EVENT_WAIT));
    assert!(caps.contains(ShellCaps::MULTI_WINDOW));
    // Permanently absent: Wayland forbids both by design.
    assert!(!caps.contains(ShellCaps::POINTER_WARP));
    assert!(!caps.contains(ShellCaps::WINDOW_POSITION));
    // Absent until P0.5b/P0.5c bind the protocols behind them.
    assert!(!caps.contains(ShellCaps::POINTER_LOCK));
    assert!(!caps.contains(ShellCaps::RAW_POINTER_MOTION));
    assert!(!caps.contains(ShellCaps::CLIPBOARD));
    assert!(!caps.contains(ShellCaps::DRAG_DROP));
    assert!(!caps.contains(ShellCaps::HW_UPSCALE));
    assert!(!caps.contains(ShellCaps::FRACTIONAL_SCALE));
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
