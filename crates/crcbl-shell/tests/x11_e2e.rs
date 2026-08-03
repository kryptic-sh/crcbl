//! The X11 backend against a real X server.
//!
//! Run with `crates/crcbl-shell/tests/run-x11-e2e.sh`, which starts a private
//! headless `Xvfb` and turns these on. They are gated twice — behind the
//! `x11-e2e` feature *and* `#[ignore]` — so that
//! `cargo nextest run --workspace --all-features` stays green on a machine with
//! no display, which is every CI runner except the one job that launches one.
//! The harness fails when the suite reports zero tests run, which is the
//! counter-measure to `docs/plan/12-testing.md`'s "silently skipped e2e" trap.
//!
//! # What these are for
//!
//! `HeadlessShell` is a *model* of a window system, and the Wayland backend is
//! a second implementation of the same seam. These tests are the only thing
//! that can tell us where either is wrong about X11, so they assert against the
//! P0.4 contract rather than against the backend's own idea of itself, and they
//! go through [`crcbl_shell::open_backend`] and `dyn Shell` — no backend type is
//! named anywhere below, exactly as a consumer would have it.
//!
//! # What "no window manager" means here, and why that is the default
//!
//! Bare `Xvfb` has no window manager, and on X11 that is not a degenerate case
//! — it is a genuine configuration, and it changes more behaviour than anything
//! else on this platform. The suite asserts what it implies rather than working
//! around it: no aspect-hint enforcement, no server-side decorations, no
//! reparenting, no focus policy, and a fullscreen request that is *made* and
//! never *honoured*. Several tests below exist specifically to pin that down,
//! because the same code under a window manager behaves differently and a test
//! that hid the difference would be testing nothing.
//!
//! `run-x11-e2e.sh` documents the split and can start a window manager with
//! `CRCBL_E2E_X11_WM`; the tests read [`Session::has_window_manager`] rather
//! than assuming either way.

#![cfg(all(target_os = "linux", feature = "x11-e2e"))]

use std::time::{Duration, Instant};

use crcbl_shell::x11_test_support::Peer;
use crcbl_shell::{
    ButtonState, ClipboardContent, ClipboardOffer, CloseReply, CursorIcon, DisplayMode, KeyCode,
    LogicalSize, MimeType, Modifiers, PhysicalPoint, PhysicalSize, PointerButton, PointerMode,
    ScrollDelta, Shell, ShellBackend, ShellCaps, ShellError, ShellEvent, SizeConstraints,
    SurfaceTarget, WindowDesc, WindowId,
};

/// X11 keycodes, which are evdev codes plus eight. Spelled out rather than
/// computed so the test states the number the wire actually carries — the same
/// number `xev` prints.
mod keycode {
    /// `KEY_A`, evdev 30.
    pub const A: u8 = 38;
    /// `KEY_LEFTSHIFT`, evdev 42.
    pub const LEFT_SHIFT: u8 = 50;
    /// `KEY_ESC`, evdev 1.
    pub const ESCAPE: u8 = 9;
}

/// How long any single wait may take before the test fails.
///
/// Deliberately far larger than anything here needs on an idle machine, where
/// the slowest of these waits is the two-second clipboard deadline the backend
/// is *supposed* to take. A CI runner sharing its cores can be an order of
/// magnitude slower than a dev box and still be perfectly healthy, and a
/// deadline tight enough to notice that is a deadline that reports the runner
/// rather than the backend.
///
/// Bounded all the same, and bounded *here*: nextest reports a test as SLOW at
/// 60s and kills it at 4× that, so a wait that expires inside this file fails
/// with a message naming what never happened, while one left to nextest is a
/// SIGKILL with no context at all.
const WAIT: Duration = Duration::from_secs(20);

/// The most any single [`Session::pump`] may take before the frame loop is
/// judged to have blocked.
///
/// **Not a performance budget**, and it cannot be one: under contention this
/// process can be descheduled in the middle of a `pump` for as long as the
/// scheduler likes, and that says nothing whatever about the backend. What it
/// still catches, cheaply, is the failure it was written for — a `pump` that
/// *waits on another client* is not late by a scheduling quantum, it is late by
/// the whole conversation: the backend's own two-second selection deadline, or
/// the hundreds of round trips an `INCR` transfer takes.
///
/// The load-independent statement of the same property is the pump **count** —
/// see [`Session::pumps`] — and where a test can make that statement it does,
/// because a blocking implementation completes the transfer in one `pump` at
/// any load and no clock is involved in noticing.
const SLOWEST_PUMP: Duration = Duration::from_millis(1_500);

/// The screen `run-x11-e2e.sh` asks Xvfb for.
const SCREEN: PhysicalSize = PhysicalSize::new(1920, 1080);

/// A shell, a peer client, and the events the shell has produced.
struct Session {
    shell: Box<dyn Shell>,
    peer: Peer,
    events: Vec<ShellEvent>,
    /// The longest any single [`pump`](Self::pump) has taken.
    ///
    /// `Shell::pump` promises to be finite and non-blocking, and the clipboard
    /// is the first thing in this backend that could break that promise — it
    /// waits on another client answering over the same socket. Recording it
    /// here means every test that pastes is also a test that the frame loop
    /// kept running.
    slowest_pump: Duration,
    /// How many times [`pump`](Self::pump) has been called.
    ///
    /// The load-independent half of the same question. A backend that answered
    /// a 334-chunk `INCR` transfer by blocking would finish it inside a single
    /// `pump`; one that alternates with the peer, as the seam requires, cannot
    /// take fewer turns than there are chunks. Counting the turns states that
    /// without consulting a clock, so it means the same thing on an idle
    /// machine and on a runner with nothing left to give.
    pumps: u32,
}

impl Session {
    fn open() -> Self {
        let expect_wm = std::env::var("CRCBL_E2E_EXPECT_WM").is_ok();
        let deadline = Instant::now() + WAIT;
        let shell = loop {
            let shell = crcbl_shell::open_backend(ShellBackend::X11)
                .expect("the harness exported DISPLAY for a live Xvfb");
            if !expect_wm || shell.caps().contains(ShellCaps::SERVER_DECORATIONS) {
                break shell;
            }
            // A window manager takes `_NET_SUPPORTING_WM_CHECK` some time after
            // it starts, and capabilities are latched per connection — so the
            // only way to wait for one is to open again. A deadline and a poll,
            // never a sleep.
            assert!(
                Instant::now() < deadline,
                "CRCBL_E2E_X11_WM was set but no window manager claimed the display"
            );
            drop(shell);
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(shell.backend(), ShellBackend::X11);
        Self {
            shell,
            peer: Peer::new().expect("libxcb-xtest and a second connection"),
            events: Vec::new(),
            slowest_pump: Duration::ZERO,
            pumps: 0,
        }
    }

    /// Whether a window manager was running when the shell connected.
    fn has_window_manager(&self) -> bool {
        self.shell.caps().contains(ShellCaps::SERVER_DECORATIONS)
    }

    /// One turn of the loop: the peer, then the shell.
    ///
    /// Both in one thread and in this order, which is the shape the X11
    /// clipboard's self-selection deadlock is avoided by — see the
    /// `x11_test_support` module docs.
    fn pump(&mut self) {
        self.peer.service();
        let started = Instant::now();
        let events = &mut self.events;
        self.shell.pump(&mut |event| events.push(event));
        self.slowest_pump = self.slowest_pump.max(started.elapsed());
        self.pumps += 1;
    }

    /// Pumps until `ready`, or fails naming what never happened.
    ///
    /// A deadline and a poll, never a fixed sleep: `docs/plan/12-testing.md`
    /// makes that the rule for anything asynchronous, and another X client
    /// answering is the asynchronous case the rule was written for.
    ///
    /// `ready` takes `&mut Session` because some of what has to be waited for
    /// is only observable by *asking the peer* — reading a property off the
    /// server needs an interned atom, and interning caches. A probe that could
    /// only read state the shell had already delivered would push those tests
    /// back onto a fixed pump, which is the trap this exists to close.
    fn pump_until(&mut self, what: &str, mut ready: impl FnMut(&mut Self) -> bool) {
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
            self.shell.wait_events(Some(Duration::from_millis(10)));
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

    /// Creates a window and waits for the first configuration.
    ///
    /// The mandatory create-then-wait loop, in one place because every test
    /// needs it. On X11 it completes on the first pump — the server knew the
    /// size before `create_window` returned — which is exactly what
    /// [`a_window_has_no_size_until_it_is_pumped`] asserts separately.
    fn window(&mut self, desc: &WindowDesc<'_>) -> WindowId {
        let window = self.shell.create_window(desc).expect("create_window");
        self.pump_until("the first configuration", |session| {
            session
                .shell
                .window_state(window)
                .is_ok_and(|state| state.is_configured())
        });
        window
    }

    /// The window's XID, which is what the peer has to name it by.
    fn xid(&self, window: WindowId) -> u32 {
        match self.shell.surface_target(window).expect("surface target") {
            SurfaceTarget::Xcb { window, .. } => window,
            other => panic!(
                "the X11 backend produced a {} target",
                other.platform_name()
            ),
        }
    }

    /// Gives a window keyboard focus and waits for the shell to agree.
    ///
    /// With no window manager nothing else ever sets the input focus, so
    /// without this every keyboard test would assert against events that were
    /// delivered to the root window instead.
    ///
    /// # Asked once per turn, not once
    ///
    /// `SetInputFocus` on a window that is not **viewable** is a `BadMatch`,
    /// and the peer's request is unchecked — so a focus asked for too early is
    /// dropped in silence and nothing ever asks again. Bare `Xvfb` never showed
    /// it: a window there is viewable the moment it is mapped. Under a
    /// reparenting window manager it is not viewable until the manager has
    /// mapped its frame, and `window` above returns as soon as the first
    /// `ConfigureNotify` arrives — which on X11 is *before* `MapNotify`. So the
    /// call usually landed in that gap, and which tests it hit changed from run
    /// to run.
    ///
    /// Retrying inside the poll costs one request per turn against `Xvfb`,
    /// where the first one has already worked.
    fn focus(&mut self, window: WindowId) {
        let xid = self.xid(window);
        self.pump_until("keyboard focus", |session| {
            session.peer.focus(xid);
            session
                .shell
                .window_state(window)
                .is_ok_and(|state| state.focused)
        });
    }

    /// The one `ClipboardData` answering `request`, waiting for it to arrive.
    fn clipboard_answer(
        &mut self,
        request: crcbl_shell::ClipboardRequestId,
    ) -> (crcbl_shell::ReceivedMime, ClipboardContent) {
        self.pump_until("the clipboard answer", |session| {
            session.events.iter().any(|event| {
                matches!(event, ShellEvent::ClipboardData { request: got, .. } if *got == request)
            })
        });
        let mut answers = self.events.iter().filter_map(|event| match event {
            ShellEvent::ClipboardData {
                request: got,
                mime,
                content,
                ..
            } if *got == request => Some((mime.clone(), content.clone())),
            _ => None,
        });
        let answer = answers.next().expect("just waited for it");
        assert!(
            answers.next().is_none(),
            "obligation 4: exactly one answer per accepted request"
        );
        answer
    }
}

fn desc<'a>(title: &'a str) -> WindowDesc<'a> {
    WindowDesc {
        title,
        app_id: "sh.kryptic.crcbl.e2e",
        size: LogicalSize::new(640.0, 480.0),
        ..WindowDesc::default()
    }
}

// ---------------------------------------------------------------------------
// Connection and capabilities
// ---------------------------------------------------------------------------

/// The capability set is the platform's, not a convenient one.
///
/// Every bit below is asserted for a stated reason, because a capability that
/// overstates itself sends a consumer down a path that silently does nothing.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn the_capability_set_is_x11_shaped() {
    let session = Session::open();
    let caps = session.shell.caps();

    // Set here for the first time by any backend in this engine.
    assert!(
        caps.contains(ShellCaps::POINTER_WARP),
        "XWarpPointer exists, and Wayland forbids it"
    );
    assert!(
        caps.contains(ShellCaps::WINDOW_POSITION),
        "one device-pixel space, and ConfigureWindow moves a window in it"
    );
    assert!(
        caps.contains(ShellCaps::FRACTIONAL_SCALE),
        "Xft.dpi is a DPI"
    );

    // Absent for reasons rather than for want of work.
    assert!(
        !caps.contains(ShellCaps::HW_UPSCALE),
        "X11 has no viewporter; the renderer blits"
    );
    assert!(
        !caps.contains(ShellCaps::DRAG_DROP),
        "XDND is cut from this slice, and the bit says so"
    );

    assert!(caps.contains(ShellCaps::CLIPBOARD));
    assert!(caps.contains(ShellCaps::MULTI_WINDOW));
    assert!(caps.contains(ShellCaps::EVENT_WAIT));
    assert!(
        caps.contains(ShellCaps::POINTER_LOCK) && caps.contains(ShellCaps::POINTER_CONFINE),
        "GrabPointer gives both, and the seam keeps them separate"
    );

    // The two that depend on a *separate program* being alive. This is the
    // finding the whole harness is arranged around: under bare Xvfb they are
    // false, and that is the honest answer rather than a CI artefact.
    assert_eq!(
        caps.contains(ShellCaps::ASPECT_HINT_HONORED),
        session.has_window_manager(),
        "an aspect hint is only honoured if something is running to honour it"
    );
    assert_eq!(
        caps.contains(ShellCaps::SERVER_DECORATIONS),
        session.has_window_manager()
    );

    // Raw motion needs XInput 2, which Xvfb has; without it there is no
    // mouselook and the helper says so rather than the test guessing.
    assert!(
        caps.contains(ShellCaps::RAW_POINTER_MOTION),
        "XInput 2 is available on every server this decade"
    );
    assert!(caps.has_mouselook());
}

/// Capabilities are latched, as implementor obligation 3 requires.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn capabilities_never_change_during_a_session() {
    let mut session = Session::open();
    let first = session.shell.caps();
    let window = session.window(&desc("caps"));
    session.focus(window);
    session
        .shell
        .set_mode(window, DisplayMode::Borderless { monitor: None })
        .expect("set_mode");
    for _ in 0..10 {
        session.pump();
        assert_eq!(session.shell.caps(), first, "caps are latched at open");
    }
}

// ---------------------------------------------------------------------------
// The window lifecycle
// ---------------------------------------------------------------------------

/// The P0.4 contract holds on a platform that knew the answer all along.
///
/// X11's `CreateWindow` takes a size and the server keeps it, so there is no
/// round trip. The contract is still satisfied honestly: the size is `None`
/// until a `Resized` has been *delivered*, and that happens on the first pump
/// rather than after a round trip. Nothing is delayed to look symmetrical with
/// Wayland, and nothing skips the loop either.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_window_has_no_size_until_it_is_pumped() {
    let mut session = Session::open();
    let window = session
        .shell
        .create_window(&desc("unconfigured"))
        .expect("create_window");

    let state = session.shell.window_state(window).expect("state");
    assert!(!state.is_configured(), "no event has been delivered yet");
    assert_eq!(state.size(), None);
    assert_eq!(state.scale_factor(), None);
    assert_eq!(state.effective_mode(), None);

    // The surface target, by contrast, exists immediately — the window is real
    // the moment it is created, and only the *swapchain* has to wait.
    assert!(matches!(
        session.shell.surface_target(window).expect("target"),
        SurfaceTarget::Xcb { .. }
    ));

    session.pump();
    let state = session.shell.window_state(window).expect("state");
    assert!(
        state.is_configured(),
        "X11 answers on the first pump: {:?}",
        session.names()
    );
    let size = state.size().expect("configured");
    assert!(!size.is_empty());
    assert_eq!(
        session.take_names(),
        ["Resized"],
        "one Resized, and nothing invented around it"
    );
}

/// A window created at 640×480 is 640×480, and a resize from outside is
/// reported once.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_resize_from_outside_is_reported_exactly_once() {
    let mut session = Session::open();
    let window = session.window(&desc("resize"));
    let xid = session.xid(window);
    let scale = session
        .shell
        .window_state(window)
        .expect("state")
        .scale_factor()
        .expect("configured");
    let expected = LogicalSize::new(640.0, 480.0).to_physical(scale);
    assert_eq!(
        session.shell.window_state(window).expect("state").size(),
        Some(expected),
        "X11 honours the requested size with no window manager to argue"
    );
    session.take_names();

    session.peer.resize(xid, 800, 600);
    session.pump_until("the resize", |session| {
        session
            .shell
            .window_state(window)
            .is_ok_and(|state| state.size() == Some(PhysicalSize::new(800, 600)))
    });
    let resizes = session
        .names()
        .iter()
        .filter(|name| **name == "Resized")
        .count();
    assert_eq!(
        resizes,
        1,
        "one configure, one event: {:?}",
        session.names()
    );
}

/// Title, class and identity properties reach the server as a window manager
/// would read them.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn the_window_carries_the_properties_a_desktop_reads() {
    let mut session = Session::open();
    let window = session.window(&desc("Crucible e2e"));
    let xid = session.xid(window);

    assert_eq!(
        session.peer.window_property(xid, "_NET_WM_NAME").as_deref(),
        Some(&b"Crucible e2e"[..]),
        "the UTF-8 title every modern tool reads"
    );
    assert_eq!(
        session.peer.window_property(xid, "WM_NAME").as_deref(),
        Some(&b"Crucible e2e"[..]),
        "and the ICCCM one the old tools read"
    );
    // `WM_CLASS` is instance NUL class NUL — both terminators included.
    assert_eq!(
        session.peer.window_property(xid, "WM_CLASS").as_deref(),
        Some(&b"sh.kryptic.crcbl.e2e\0sh.kryptic.crcbl.e2e\0"[..])
    );
    // `WM_PROTOCOLS` must contain `WM_DELETE_WINDOW`, or a close is a SIGKILL.
    let delete = session.peer.atom("WM_DELETE_WINDOW");
    let protocols = session
        .peer
        .window_property(xid, "WM_PROTOCOLS")
        .expect("WM_PROTOCOLS");
    let protocols: Vec<u32> = protocols
        .chunks_exact(4)
        .map(|word| u32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
        .collect();
    assert!(protocols.contains(&delete), "{protocols:?}");

    // A rename is one client writing a property and another reading it, and
    // the two conversations are on **different connections**: X11 orders
    // requests within a connection and promises nothing between them, so the
    // peer's `GetProperty` may well be served before our `ChangeProperty`. A
    // fixed pump here is not "one pump is enough", it is a race that happens to
    // be won on an idle machine — it was lost outright on the first loaded run.
    session.shell.set_title(window, "renamed").expect("title");
    session.pump_until("the peer to see the new title", |session| {
        session.peer.window_property(xid, "_NET_WM_NAME").as_deref() == Some(&b"renamed"[..])
    });
}

/// The aspect lock reaches `WM_NORMAL_HINTS`, which is the property that makes
/// `ASPECT_HINT_HONORED` possible at all.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn the_aspect_lock_is_written_into_wm_normal_hints() {
    let mut session = Session::open();
    let window = session.window(&WindowDesc {
        constraints: SizeConstraints {
            min: Some(LogicalSize::new(320.0, 180.0)),
            max: Some(LogicalSize::new(1600.0, 900.0)),
            aspect: Some(crcbl_shell::AspectRatio::WIDESCREEN),
        },
        ..desc("hints")
    });
    let xid = session.xid(window);
    let bytes = session
        .peer
        .window_property(xid, "WM_NORMAL_HINTS")
        .expect("WM_NORMAL_HINTS");
    assert_eq!(
        bytes.len(),
        72,
        "eighteen words, or it is ignored wholesale"
    );
    let words: Vec<i32> = bytes
        .chunks_exact(4)
        .map(|word| i32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
        .collect();

    let scale = session
        .shell
        .window_state(window)
        .expect("state")
        .scale_factor()
        .expect("configured");
    let min = LogicalSize::new(320.0, 180.0).to_physical(scale);
    assert_eq!(
        (words[5], words[6]),
        (min.width as i32, min.height as i32),
        "min size, in pixels rather than logical units"
    );
    assert_eq!(
        &words[11..15],
        &[16, 9, 16, 9],
        "both aspect bounds equal — a lock, not a range"
    );

    // Whether the *window manager* enforces it is a different question, and the
    // capability answers it honestly.
    assert_eq!(
        session
            .shell
            .caps()
            .contains(ShellCaps::ASPECT_HINT_HONORED),
        session.has_window_manager()
    );
}

/// A close request is a question, and answering `Keep` keeps the window.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn the_close_request_is_a_question_not_a_notification() {
    let mut session = Session::open();
    let window = session.window(&desc("close"));
    let xid = session.xid(window);
    session.take_names();

    // Nothing has asked, so the reply is a caller bug and is named as one.
    assert!(matches!(
        session.shell.reply_close_request(window, CloseReply::Keep),
        Err(ShellError::NoPendingCloseRequest { .. })
    ));

    session.peer.request_close(xid);
    session.pump_until("the close request", |session| {
        session.names().contains(&"CloseRequested")
    });
    assert!(
        session
            .shell
            .window_state(window)
            .expect("still open")
            .close_pending
    );

    session
        .shell
        .reply_close_request(window, CloseReply::Keep)
        .expect("keep");
    session.pump();
    let state = session.shell.window_state(window).expect("kept open");
    assert!(!state.close_pending);

    session.take_names();
    session.peer.request_close(xid);
    session.pump_until("the second close request", |session| {
        session.names().contains(&"CloseRequested")
    });
    session
        .shell
        .reply_close_request(window, CloseReply::Close)
        .expect("close");
    session.pump();
    assert!(session.names().contains(&"WindowDestroyed"));
    // Obligation 1: a stale handle fails cleanly rather than acting on whatever
    // took the slot.
    assert!(matches!(
        session.shell.window_state(window),
        Err(ShellError::InvalidWindow { .. })
    ));
    assert!(matches!(
        session.shell.set_title(window, "gone"),
        Err(ShellError::InvalidWindow { .. })
    ));
}

/// Hiding a window is real on X11 — and reversible, which is why the Wayland
/// backend cannot do it.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_window_can_be_unmapped_and_mapped_again() {
    let mut session = Session::open();
    let window = session.window(&desc("visibility"));
    assert!(session.shell.window_state(window).expect("state").visible);

    session.shell.set_visible(window, false).expect("hide");
    session.pump_until("the unmap", |session| {
        session
            .shell
            .window_state(window)
            .is_ok_and(|state| !state.visible)
    });

    // The reversibility is the point: on Wayland an unmapped toplevel has to
    // redo the whole configure handshake and cannot be shown again without a
    // buffer, so that backend records intent instead of acting.
    session.shell.set_visible(window, true).expect("show");
    session.pump_until("the remap", |session| {
        session
            .shell
            .window_state(window)
            .is_ok_and(|state| state.visible)
    });
    assert!(
        session
            .shell
            .window_state(window)
            .expect("state")
            .is_configured(),
        "and it still has its size"
    );
}

/// A borderless request is made, and with no window manager it is not honoured
/// — which is exactly what "requested versus effective" exists to express.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_mode_request_is_a_request_and_the_effective_mode_is_the_answer() {
    let mut session = Session::open();
    let window = session.window(&desc("mode"));
    let borderless = DisplayMode::Borderless { monitor: None };

    session
        .shell
        .set_mode(window, borderless)
        .expect("set_mode");
    session.pump();
    session.pump();
    let state = session.shell.window_state(window).expect("state");
    assert_eq!(
        state.requested_mode, borderless,
        "the request is recorded immediately, because we said it"
    );
    if session.has_window_manager() {
        session.pump_until("the window manager's answer", |session| {
            session
                .shell
                .window_state(window)
                .is_ok_and(|state| state.mode_request_honoured())
        });
    } else {
        assert_eq!(
            state.effective_mode(),
            Some(DisplayMode::Windowed),
            "no window manager is running, so nothing answered"
        );
        assert!(
            !state.mode_request_honoured(),
            "a UI toggle must read this and not the request"
        );
    }
}

/// The same question asked of [`WindowDesc::mode`], which used to answer it
/// differently — and wrongly.
///
/// EWMH has the *client* write `_NET_WM_STATE` to request an initial state,
/// because before a window is mapped there is no window manager conversation to
/// have; a window manager then takes ownership of the property and rewrites it.
/// The backend worked out the effective mode by reading that property back, so
/// with **no** window manager it read back this process's own request and
/// reported it as the answer: `effective_mode()` said borderless, and
/// `mode_request_honoured()` said true, for a window still at its windowed size
/// that nothing had touched.
///
/// A game's `--fullscreen` flag takes exactly this path, so the summary line of
/// every WM-less run — every kiosk, every bare X session, this harness — claimed
/// a fullscreen it did not have.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_window_created_borderless_does_not_report_its_own_request_as_the_answer() {
    let mut session = Session::open();
    let borderless = DisplayMode::Borderless { monitor: None };
    let requested = LogicalSize::new(640.0, 360.0);
    let window = session.window(&WindowDesc {
        mode: borderless,
        size: requested,
        ..desc("born borderless")
    });
    session.pump();
    session.pump();

    let state = session.shell.window_state(window).expect("state");
    assert_eq!(
        state.requested_mode, borderless,
        "the request is recorded immediately, because we said it"
    );
    if session.has_window_manager() {
        session.pump_until("the window manager's answer", |session| {
            session
                .shell
                .window_state(window)
                .is_ok_and(|state| state.mode_request_honoured())
        });
    } else {
        assert_eq!(
            state.effective_mode(),
            Some(DisplayMode::Windowed),
            "nothing is running that could have honoured this",
        );
        assert!(!state.mode_request_honoured());
        // And the size agrees with the mode, which is what made the old answer
        // detectable from outside: a borderless window is the monitor's size.
        assert_eq!(
            state.size(),
            Some(requested.to_physical(state.scale_factor().expect("configured"))),
            "the window is still exactly as big as it asked to be",
        );
    }
}

/// A borderless request naming a monitor that is gone is a clean error.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_missing_monitor_is_named_rather_than_ignored() {
    let mut session = Session::open();
    let window = session.window(&desc("monitor"));
    let absent = crcbl_shell::MonitorId(9_999);
    assert!(matches!(
        session.shell.set_mode(
            window,
            DisplayMode::Borderless {
                monitor: Some(absent)
            }
        ),
        Err(ShellError::NoSuchMonitor(9_999))
    ));
    assert!(matches!(
        session.shell.create_window(&WindowDesc {
            mode: DisplayMode::Borderless {
                monitor: Some(absent)
            },
            ..desc("monitor")
        }),
        Err(ShellError::NoSuchMonitor(9_999))
    ));
}

/// More than one window, which the capability claims.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn two_windows_are_independent() {
    let mut session = Session::open();
    let first = session.window(&desc("first"));
    let second = session.window(&desc("second"));
    assert_ne!(session.xid(first), session.xid(second));

    session.shell.destroy_window(first).expect("destroy");
    session.pump();
    assert!(session.shell.window_state(first).is_err());
    assert!(
        session
            .shell
            .window_state(second)
            .expect("still there")
            .is_configured(),
        "destroying one window does not disturb the other"
    );
}

// ---------------------------------------------------------------------------
// Monitors
// ---------------------------------------------------------------------------

/// RandR enumeration produces a coherent, tiling desktop.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn monitors_are_enumerated_in_one_coordinate_space() {
    let session = Session::open();
    let monitors = session.shell.monitors();
    assert!(!monitors.is_empty(), "a server always has a screen");
    assert_eq!(
        monitors.iter().filter(|monitor| monitor.is_primary).count(),
        1,
        "exactly one primary, so a consumer's default is unambiguous"
    );

    let mut ids: Vec<_> = monitors.iter().map(|monitor| monitor.id).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "ids are unique within a session");

    for monitor in monitors {
        assert!(!monitor.size().is_empty(), "{}", monitor.name);
        assert!(monitor.scale_factor > 0.0);
        // The X11-specific fact: bounds are in the screen's own pixels, so they
        // fit inside it — which is *not* true of Wayland's at mixed scales.
        assert!(
            monitor.bounds.x >= 0 && monitor.bounds.y >= 0,
            "{:?} is outside the screen",
            monitor.bounds
        );
        assert!(
            monitor
                .bounds
                .x
                .saturating_add_unsigned(monitor.bounds.width)
                <= SCREEN.width as i32,
            "{:?} runs past the screen",
            monitor.bounds
        );
        // Work area is the full area with no window manager to shrink it.
        if !session.has_window_manager() {
            assert_eq!(monitor.work_area, monitor.bounds);
        }
        assert_eq!(
            session.shell.monitor(monitor.id).map(|found| &found.name),
            Some(&monitor.name)
        );
    }
    assert_eq!(
        session.shell.monitor(crcbl_shell::MonitorId(9_999)),
        None,
        "a monitor that is not there is None, not a panic"
    );
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// A keystroke arrives with all three views of it, and commits text.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_keystroke_carries_the_keycode_the_keysym_and_the_text() {
    let mut session = Session::open();
    let window = session.window(&desc("keys"));
    session.focus(window);
    session.take_names();

    session.peer.key(keycode::A, true);
    session.peer.key(keycode::A, false);
    session.pump_until("the keystroke", |session| {
        session
            .events
            .iter()
            .filter(|event| matches!(event, ShellEvent::Key { .. }))
            .count()
            >= 2
    });

    let keys: Vec<_> = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Key {
                scancode,
                key_code,
                keysym,
                state,
                repeat,
                ..
            } => Some((*scancode, *key_code, *keysym, *state, *repeat)),
            _ => None,
        })
        .collect();
    let (scancode, key_code, keysym, state, repeat) = keys[0];
    assert_eq!(
        scancode.0, 30,
        "the raw code is evdev's, so it matches the Wayland backend"
    );
    assert_eq!(key_code, Some(KeyCode::KeyA), "the physical position");
    assert_eq!(state, ButtonState::Pressed);
    assert!(!repeat, "the first press is not a repeat");
    if session.shell.caps().contains(ShellCaps::TEXT_IME) {
        assert_eq!(
            keysym,
            crcbl_shell::Keysym::from_char('a'),
            "the layout's symbol"
        );
        assert!(
            session.names().contains(&"TextCommit"),
            "a printable key commits text: {:?}",
            session.names()
        );
    }
    assert_eq!(keys[1].3, ButtonState::Released);
    assert!(!keys[1].4);
}

/// A held key repeats, the repeats say so, and there are no phantom releases.
///
/// The X11-specific finding: auto-repeat is the *server's*, so unlike Wayland
/// there is nothing to synthesize — but without detectable auto-repeat it
/// arrives as a `KeyRelease` immediately followed by a `KeyPress` at the same
/// millisecond, and a backend that forwards both makes a jump button fire
/// thirty times a second while held.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_held_key_repeats_and_the_repeats_are_flagged() {
    let mut session = Session::open();
    let window = session.window(&desc("repeat"));
    session.focus(window);
    session.take_names();

    session.peer.key(keycode::A, true);
    session.pump_until("an auto-repeat", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::Key { repeat: true, .. }))
    });
    session.peer.key(keycode::A, false);
    // Demands the release as the **last** key event, not merely as one of them.
    // `any` is satisfied by a release that was already in the list, which is
    // exactly what a backend forwarding the fake release in front of an
    // auto-repeat produces — so the probe would stop mid-repeat, and the
    // assertions below would be describing the wrong moment. Insisting on the
    // terminal element means this waits for the release it asked for.
    session.pump_until("the real release, last", |session| {
        matches!(
            session.events.iter().rev().find_map(|event| match event {
                ShellEvent::Key { state, .. } => Some(*state),
                _ => None,
            }),
            Some(ButtonState::Released)
        )
    });

    let keys: Vec<_> = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Key { state, repeat, .. } => Some((*state, *repeat)),
            _ => None,
        })
        .collect();
    assert_eq!(
        keys.first(),
        Some(&(ButtonState::Pressed, false)),
        "the first press is a press"
    );
    assert_eq!(
        keys.last(),
        Some(&(ButtonState::Released, false)),
        "and the last release is a release"
    );
    assert!(
        keys.len() > 2,
        "the server repeated at least once: {keys:?}"
    );
    // The whole point: every event in between is a flagged repeat, and none of
    // them is a release. A pattern evaluator keying on `repeat == false` is
    // correct with no coordination, exactly as the seam promises.
    for (state, repeat) in &keys[1..keys.len() - 1] {
        assert_eq!(*state, ButtonState::Pressed, "no phantom release: {keys:?}");
        assert!(*repeat, "and every one is flagged: {keys:?}");
    }
}

/// A control key is a key and commits no text.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_control_key_is_a_key_and_not_committed_text() {
    let mut session = Session::open();
    if !session.shell.caps().contains(ShellCaps::TEXT_IME) {
        return;
    }
    let window = session.window(&desc("escape"));
    session.focus(window);
    session.take_names();

    session.peer.key(keycode::ESCAPE, true);
    session.peer.key(keycode::ESCAPE, false);
    session.pump_until("the escape key", |session| {
        session
            .events
            .iter()
            .filter(|event| matches!(event, ShellEvent::Key { .. }))
            .count()
            >= 2
    });
    assert!(
        session.events.iter().any(|event| matches!(
            event,
            ShellEvent::Key {
                key_code: Some(KeyCode::Escape),
                ..
            }
        )),
        "{:?}",
        session.names()
    );
    assert!(
        !session.names().contains(&"TextCommit"),
        "a text field wants Escape as a key that moves a cursor, not as a \
         literal control character in its buffer: {:?}",
        session.names()
    );
}

/// Modifiers are stamped onto the event they modify, not delivered separately.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn modifiers_are_stamped_on_the_event_they_modify() {
    let mut session = Session::open();
    let window = session.window(&desc("modifiers"));
    session.focus(window);
    session.take_names();

    session.peer.key(keycode::LEFT_SHIFT, true);
    session.peer.key(keycode::A, true);
    // Waits for *the A press*, not for two key events of any kind. Counting is
    // the trap: hold Shift long enough — which a loaded machine does for you —
    // and the server's own auto-repeat supplies the second event, the count is
    // satisfied with no A in sight, and the `expect` below fires on a backend
    // that was about to be right.
    session.pump_until("shift+a", |session| {
        session.events.iter().any(|event| {
            matches!(
                event,
                ShellEvent::Key {
                    key_code: Some(KeyCode::KeyA),
                    ..
                }
            )
        })
    });
    let shifted = session
        .events
        .iter()
        .find_map(|event| match event {
            ShellEvent::Key {
                key_code: Some(KeyCode::KeyA),
                modifiers,
                keysym,
                ..
            } => Some((*modifiers, *keysym)),
            _ => None,
        })
        .expect("the A press");
    session.peer.key(keycode::A, false);
    session.peer.key(keycode::LEFT_SHIFT, false);
    session.pump();

    if session.shell.caps().contains(ShellCaps::TEXT_IME) {
        assert!(
            shifted.0.contains(Modifiers::SHIFT),
            "Shift was held when A went down: {:?}",
            shifted.0
        );
        assert_eq!(
            shifted.1,
            crcbl_shell::Keysym::from_char('A'),
            "and the keysym is the shifted one"
        );
    }
}

/// The pointer's three event kinds, including the wheel that X11 spells as
/// buttons 4 and 5.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn pointer_motion_buttons_and_the_wheel_all_arrive() {
    let mut session = Session::open();
    let window = session.window(&desc("pointer"));
    session.focus(window);
    session.take_names();

    session.peer.motion(100, 100);
    session.pump_until("the pointer to enter", |session| {
        session.events.iter().any(|event| {
            matches!(
                event,
                ShellEvent::PointerFocus { entered: true, .. } | ShellEvent::PointerMotion { .. }
            )
        })
    });

    session.peer.button(1, true);
    session.peer.button(1, false);
    session.pump_until("the click", |session| {
        session
            .events
            .iter()
            .filter(|event| matches!(event, ShellEvent::Button { .. }))
            .count()
            >= 2
    });
    let buttons: Vec<_> = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Button { button, state, .. } => Some((*button, *state)),
            _ => None,
        })
        .collect();
    assert_eq!(buttons[0], (PointerButton::Left, ButtonState::Pressed));
    assert_eq!(buttons[1], (PointerButton::Left, ButtonState::Released));

    session.events.clear();
    // Button 4 is a wheel notch, and its *release* must not double it.
    session.peer.button(4, true);
    session.peer.button(4, false);
    session.pump_until("the wheel", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::Wheel { .. }))
    });
    let wheels: Vec<_> = session
        .events
        .iter()
        .filter_map(|event| match event {
            ShellEvent::Wheel { delta, .. } => Some(*delta),
            _ => None,
        })
        .collect();
    assert_eq!(
        wheels,
        vec![ScrollDelta::Lines { x: 0.0, y: 1.0 }],
        "one notch, one event — the release half carries nothing"
    );
    assert!(
        !session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::Button { .. })),
        "a wheel notch is not a button press: {:?}",
        session.names()
    );
}

/// Raw, unaccelerated relative motion — the thing a first-person camera reads.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn raw_relative_motion_is_delivered_for_mouselook() {
    let mut session = Session::open();
    assert!(session.shell.caps().has_mouselook());
    let window = session.window(&desc("mouselook"));
    session.focus(window);
    // The pointer has to be *parked* before the measured move, not merely
    // asked to park: a single pump that did not yet contain the first motion
    // leaves it to arrive after the clear, and the delta then measured is the
    // one from wherever the pointer started — which points in no particular
    // direction and fails the sign check at random.
    session.peer.motion(300, 300);
    session.pump_until("the pointer to park", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { .. }))
    });
    session.events.clear();

    session.peer.motion(340, 320);
    session.pump_until("raw motion", |session| {
        session.events.iter().any(|event| {
            matches!(
                event,
                ShellEvent::PointerMotion {
                    raw_delta: Some(_),
                    ..
                }
            )
        })
    });
    let (dx, dy) = session
        .events
        .iter()
        .find_map(|event| match event {
            ShellEvent::PointerMotion {
                raw_delta: Some(delta),
                ..
            } => Some(*delta),
            _ => None,
        })
        .expect("just waited for it");
    assert!(
        dx > 0.0 && dy > 0.0,
        "the pointer moved right and down: ({dx}, {dy})"
    );
}

/// Locking the pointer suppresses the absolute position, which is the
/// invariant `PointerMotion` documents for that mode.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_locked_pointer_reports_no_absolute_position() {
    let mut session = Session::open();
    let window = session.window(&desc("lock"));
    session.focus(window);
    // Drained before the lock, not merely pumped once: a motion produced while
    // the pointer was still free carries a position, and one still in flight
    // when `events` is cleared below would be read as a locked-pointer motion
    // that leaked an absolute position — a failure of a passing backend.
    session.peer.motion(300, 300);
    session.pump_until("the pointer to park", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { .. }))
    });

    session
        .shell
        .set_pointer_mode(window, PointerMode::Locked)
        .expect("the server had no other grab");
    assert_eq!(
        session
            .shell
            .window_state(window)
            .expect("state")
            .pointer_mode,
        PointerMode::Locked
    );
    session.events.clear();

    for step in 1..=8 {
        session.peer.motion(300 + step * 7, 300 + step * 5);
        session.pump();
    }
    // Waiting for at least one is what makes the loop below an assertion rather
    // than a formality: eight fixed pumps on a loaded machine can deliver
    // nothing at all, and a `for` over an empty list passes without having
    // looked at anything.
    session.pump_until("motion while locked", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { .. }))
    });
    for event in &session.events {
        if let ShellEvent::PointerMotion { abs, .. } = event {
            assert_eq!(*abs, None, "a locked pointer has no meaningful position");
        }
    }

    session
        .shell
        .set_pointer_mode(window, PointerMode::Free)
        .expect("ungrab");
    assert_eq!(
        session
            .shell
            .window_state(window)
            .expect("state")
            .pointer_mode,
        PointerMode::Free
    );
}

/// Warping moves the pointer — the capability Wayland deliberately lacks.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn warping_the_pointer_moves_it() {
    let mut session = Session::open();
    let window = session.window(&desc("warp"));
    session.focus(window);
    // Parked and drained before the warp: a motion from this move still in
    // flight when `events` is cleared is indistinguishable from the warp's own,
    // and the assertion below would then be checking that the pointer landed at
    // (500, 500) — which it did, at the wrong step.
    session.peer.motion(500, 500);
    session.pump_until("the pointer to park", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { .. }))
    });
    session.events.clear();

    session
        .shell
        .warp_pointer(window, PhysicalPoint::new(40.0, 60.0))
        .expect("warp");
    session.pump_until("the warp's motion event", |session| {
        session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::PointerMotion { abs: Some(_), .. }))
    });
    let landed = session
        .events
        .iter()
        .rev()
        .find_map(|event| match event {
            ShellEvent::PointerMotion { abs: Some(at), .. } => Some(*at),
            _ => None,
        })
        .expect("just waited for it");
    assert!(
        (landed.x - 40.0).abs() < 2.0 && (landed.y - 60.0).abs() < 2.0,
        "the pointer landed at {landed:?}"
    );

    // Hiding the cursor is real; naming a shape is recorded and not applied,
    // and neither is an error.
    session.shell.set_cursor(window, None).expect("hide");
    session
        .shell
        .set_cursor(window, Some(CursorIcon::Crosshair))
        .expect("shape");
}

// ---------------------------------------------------------------------------
// The clipboard
// ---------------------------------------------------------------------------

/// Nobody owns the selection, so the answer is `Empty` — not `Unavailable`,
/// and not a request left outstanding.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn an_unowned_clipboard_answers_empty_immediately() {
    let mut session = Session::open();
    let window = session.window(&desc("clipboard"));
    session.peer.release_clipboard();
    session.pump();
    session.events.clear();

    // Obligation 5 has nothing to hold here: `GetSelectionOwner` is a
    // synchronous question, so the answer is produced on the spot.
    assert!(
        session.shell.clipboard_readable(window),
        "X11 has no focus gate, so the trait's provided default is right"
    );
    let request = session
        .shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("accepted");
    let (mime, content) = session.clipboard_answer(request);
    assert_eq!(content, ClipboardContent::Empty);
    assert!(
        mime.matches(MimeType::TextUtf8),
        "no peer spelling to report, so the format that was asked for"
    );
    assert!(
        session.slowest_pump < SLOWEST_PUMP,
        "pump stayed non-blocking: {:?}",
        session.slowest_pump
    );
}

/// A peer's selection is read in one step.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_peer_selection_is_read_and_keeps_the_peers_spelling() {
    let mut session = Session::open();
    let window = session.window(&desc("paste"));
    session
        .peer
        .own_clipboard(&[("UTF8_STRING", b"from the peer")]);
    session.pump();

    let request = session
        .shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("accepted");
    let (mime, content) = session.clipboard_answer(request);
    assert_eq!(content.text(), Some("from the peer"));
    assert!(mime.matches(MimeType::TextUtf8));
    assert_eq!(
        mime.as_str(),
        "UTF8_STRING",
        "the peer chose the spelling and it survives verbatim"
    );
    assert!(
        session.slowest_pump < SLOWEST_PUMP,
        "pump stayed non-blocking: {:?}",
        session.slowest_pump
    );
}

/// An `INCR` transfer is reassembled, chunk by chunk, without blocking.
///
/// The classic X11 clipboard iceberg. The peer forces three-byte chunks so the
/// whole state machine runs in milliseconds rather than needing a payload above
/// the server's 256 KiB request limit.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn an_incremental_transfer_is_reassembled() {
    /// Bytes per chunk, which is what makes this 334 chunks rather than one.
    const CHUNK: usize = 3;
    let mut session = Session::open();
    let window = session.window(&desc("incr"));
    let payload: Vec<u8> = (0..1_000u32).map(|byte| (byte % 251) as u8).collect();
    let chunks = payload.len().div_ceil(CHUNK);
    session
        .peer
        .own_clipboard_incrementally(&[("UTF8_STRING", &payload)], CHUNK);

    let before = session.pumps;
    let request = session
        .shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("accepted");
    let (_, content) = session.clipboard_answer(request);
    assert_eq!(
        content.bytes(),
        Some(&payload[..]),
        "every chunk, in order, and nothing duplicated"
    );

    // That the frame loop never stalled, said without a clock. The peer writes
    // the next chunk only when it is serviced, and it is serviced only at the
    // top of a pump, so a transfer that really did alternate cannot have taken
    // fewer turns than it had chunks — and a backend that blocked waiting for
    // the peer would have finished inside one. This is the same claim the
    // wall-clock bound below makes, in a form that a busy machine cannot
    // falsify in either direction.
    let turns = session.pumps - before;
    assert!(
        turns as usize >= chunks,
        "{chunks} chunks were fed through {turns} pumps; a transfer this size \
         cannot have alternated with the peer in fewer"
    );
    assert!(
        session.slowest_pump < SLOWEST_PUMP,
        "{chunks} chunks and the frame loop never stalled: {:?}",
        session.slowest_pump
    );
}

/// A peer that takes the selection and then stops answering does not leave the
/// request outstanding forever.
///
/// **The one obligation X11 did not get for free.** There is no descriptor to
/// poll and no protocol event for "the owner gave up", so a deadline is the
/// only thing that can end this — and without it the backend would satisfy
/// obligation 4 for well-behaved peers and violate it for exactly the peers a
/// timeout exists for.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_stalled_transfer_answers_unavailable_rather_than_never() {
    let mut session = Session::open();
    let window = session.window(&desc("stall"));
    session
        .peer
        .own_clipboard_incrementally(&[("UTF8_STRING", b"never finished")], 2);
    session.pump();

    let request = session
        .shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("accepted");

    // Pump the *shell* only: the peer is alive and owns the selection, and
    // simply never answers. That is a crashed clipboard owner, exactly.
    let deadline = Instant::now() + WAIT;
    let before = session.pumps;
    loop {
        let started = Instant::now();
        let events = &mut session.events;
        session.shell.pump(&mut |event| events.push(event));
        session.slowest_pump = session.slowest_pump.max(started.elapsed());
        session.pumps += 1;
        if session
            .events
            .iter()
            .any(|event| matches!(event, ShellEvent::ClipboardData { .. }))
        {
            break;
        }
        assert!(Instant::now() < deadline, "the read was never answered");
        session.shell.wait_events(Some(Duration::from_millis(20)));
    }

    let (_, content) = session.clipboard_answer(request);
    assert_eq!(
        content,
        ClipboardContent::Unavailable,
        "there may well be something on the clipboard, which is why this is \
         not Empty"
    );
    // Where the two seconds were spent, said without a clock. A backend that
    // waited for the peer inside `pump` would have answered on the *first* one;
    // this one gave the frame loop back every time and checked the deadline on
    // the way past, which is the only version of obligation 4 an editor can
    // survive.
    assert!(
        session.pumps - before > 1,
        "the deadline was reached across many pumps, not inside one"
    );
    assert!(
        session.slowest_pump < SLOWEST_PUMP,
        "the wait was bounded but never inside pump: {:?}",
        session.slowest_pump
    );
}

/// We can own the selection, and a peer reads it back — including `TARGETS`.
///
/// Obligation 6 is discharged by never being reached: `clipboard_offer`
/// succeeds on a window that has received no input at all, because X11 has no
/// user-interaction rule for the clipboard. Wayland refuses the same call.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn our_selection_is_readable_by_another_client() {
    let mut session = Session::open();
    let window = session.window(&desc("copy"));
    session.peer.release_clipboard();
    session.pump();

    session
        .shell
        .clipboard_offer(
            window,
            &[
                ClipboardOffer::text("copied from crcbl"),
                ClipboardOffer::ron("(entity: 1)"),
            ],
        )
        .expect("X11 needs no recent user interaction to claim the clipboard");

    session.peer.read_clipboard("TARGETS");
    session.pump_until("the TARGETS reply", |session| session.peer_answered());
    let targets = session
        .peer
        .take_read()
        .expect("answered")
        .expect("targets");
    let targets: Vec<u32> = targets
        .chunks_exact(4)
        .map(|word| u32::from_ne_bytes([word[0], word[1], word[2], word[3]]))
        .collect();
    let expected = [
        session.peer.atom("TARGETS"),
        session.peer.atom("UTF8_STRING"),
        session.peer.atom("application/x-crcbl+ron"),
    ];
    for atom in expected {
        assert!(targets.contains(&atom), "{targets:?} is missing {atom}");
    }

    session.peer.read_clipboard("UTF8_STRING");
    session.pump_until("the text", |session| session.peer_answered());
    assert_eq!(
        session.peer.take_read().expect("answered").expect("text"),
        b"copied from crcbl".to_vec()
    );

    session.peer.read_clipboard("application/x-crcbl+ron");
    session.pump_until("the engine format", |session| session.peer_answered());
    assert_eq!(
        session.peer.take_read().expect("answered").expect("ron"),
        b"(entity: 1)".to_vec()
    );

    // A format we never offered is refused rather than answered with the wrong
    // bytes.
    session.peer.read_clipboard("image/png");
    session.pump_until("the refusal", |session| session.peer_answered());
    assert_eq!(session.peer.take_read().expect("answered"), None);
}

/// A payload larger than one request goes out as `INCR`, driven by the peer.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn a_large_offer_is_sent_incrementally() {
    let mut session = Session::open();
    let window = session.window(&desc("big copy"));
    // Comfortably above the 256 KiB a single `ChangeProperty` can carry on a
    // server with the usual 64 KiB-word request limit.
    let payload: Vec<u8> = (0..600_000u32).map(|byte| (byte % 251) as u8).collect();
    let text =
        String::from_utf8(payload.iter().map(|byte| byte % 26 + b'a').collect()).expect("ascii");
    session
        .shell
        .clipboard_offer(window, &[ClipboardOffer::text(&text)])
        .expect("claim");

    session.peer.read_clipboard("UTF8_STRING");
    let before = session.pumps;
    session.pump_until("the whole INCR transfer", |session| session.peer_answered());
    let received = session.peer.take_read().expect("answered").expect("bytes");
    assert_eq!(received.len(), text.len());
    assert_eq!(received, text.as_bytes());
    // The sending half of the same claim the receiving test makes: the peer
    // pulls each chunk by deleting the property, and it can only do that when
    // it is serviced, so a transfer fed through the frame loop takes a turn per
    // chunk and one fed inside a single `pump` takes exactly one.
    assert!(
        session.pumps - before > 1,
        "the payload went out across many pumps, not inside one"
    );
    assert!(
        session.slowest_pump < SLOWEST_PUMP,
        "the frame loop never stalled feeding it: {:?}",
        session.slowest_pump
    );
}

/// Pasting our own copy does not deadlock.
///
/// The failure P0.5c predicted would be worse on X11 than on Wayland, and is:
/// the owner half of the conversation arrives as a `SelectionRequest` on the
/// **same connection** the read is waiting on, so any synchronous read is an
/// immediate self-deadlock. Nothing waits, so both halves make progress in one
/// `pump`.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn pasting_our_own_copy_does_not_deadlock() {
    let mut session = Session::open();
    let window = session.window(&desc("self"));
    session
        .shell
        .clipboard_offer(window, &[ClipboardOffer::text("round trip")])
        .expect("claim");

    let request = session
        .shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("accepted");
    let (_, content) = session.clipboard_answer(request);
    assert_eq!(content.text(), Some("round trip"));
    assert!(
        session.slowest_pump < SLOWEST_PUMP,
        "owner and requestor alternated inside one loop: {:?}",
        session.slowest_pump
    );

    // A second window in the same process reads it too, which is the editor's
    // real case and the one a blocking implementation cannot survive at all.
    let second = session.window(&desc("self 2"));
    let request = session
        .shell
        .clipboard_request(second, MimeType::TextUtf8)
        .expect("accepted");
    let (_, content) = session.clipboard_answer(request);
    assert_eq!(content.text(), Some("round trip"));
}

/// Releasing the clipboard leaves nobody owning it.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn an_empty_offer_releases_the_selection() {
    let mut session = Session::open();
    let window = session.window(&desc("release"));
    session
        .shell
        .clipboard_offer(window, &[ClipboardOffer::text("temporary")])
        .expect("claim");
    session.pump();

    session.shell.clipboard_offer(window, &[]).expect("release");
    session.pump();

    let request = session
        .shell
        .clipboard_request(window, MimeType::TextUtf8)
        .expect("accepted");
    let (_, content) = session.clipboard_answer(request);
    assert_eq!(
        content,
        ClipboardContent::Empty,
        "nobody owns the selection any more"
    );
}

/// Every method taking a stale handle fails cleanly — implementor obligation 1.
#[test]
#[ignore = "needs an X server; run tests/run-x11-e2e.sh"]
fn every_call_rejects_a_stale_handle() {
    let mut session = Session::open();
    let window = session.window(&desc("stale"));
    session.shell.destroy_window(window).expect("destroy");
    session.pump();

    let stale = |result: Result<(), ShellError>| {
        assert!(
            matches!(result, Err(ShellError::InvalidWindow { .. })),
            "expected InvalidWindow, got {result:?}"
        );
    };
    stale(session.shell.set_title(window, "x"));
    stale(session.shell.set_visible(window, true));
    stale(session.shell.set_mode(window, DisplayMode::Windowed));
    stale(session.shell.set_constraints(window, SizeConstraints::NONE));
    stale(session.shell.set_pointer_mode(window, PointerMode::Free));
    stale(session.shell.set_cursor(window, None));
    stale(
        session
            .shell
            .warp_pointer(window, PhysicalPoint::new(0.0, 0.0)),
    );
    stale(session.shell.destroy_window(window));
    stale(session.shell.clipboard_offer(window, &[]));
    assert!(matches!(
        session.shell.clipboard_request(window, MimeType::TextUtf8),
        Err(ShellError::InvalidWindow { .. })
    ));
    assert!(matches!(
        session.shell.window_state(window),
        Err(ShellError::InvalidWindow { .. })
    ));
    assert!(matches!(
        session.shell.surface_target(window),
        Err(ShellError::InvalidWindow { .. })
    ));
    assert!(
        !session.shell.clipboard_readable(window),
        "a stale handle is not readable"
    );
}

impl Session {
    /// Whether the peer's outstanding read has been answered.
    fn peer_answered(&self) -> bool {
        self.peer.has_answer()
    }
}
