//! A real AppKit window session, driven from the **process's main thread**.
//!
//! ```text
//! cargo test -p crcbl-shell --test appkit_session
//! ```
//!
//! # Why this is a `harness = false` target and not a `#[test]`
//!
//! **Rust's `libtest` always runs a test body on a thread it spawns**, and
//! AppKit is main-thread-only: `NSApplication` raises rather than returning an
//! error, and an Objective-C exception unwinding into Rust is undefined
//! behaviour. `crcbl-shell`'s AppKit backend therefore refuses to open off the
//! main thread on purpose (`appkit::app::require_main_thread`), which means a
//! `#[test]` can never drive an AppKit window — not with `--test-threads=1`, not
//! by invoking the binary directly. That was measured on this workspace's
//! toolchain rather than recalled: a probe asserting `gettid() == getpid()`
//! inside a `#[test]` fails both ways.
//!
//! So this target owns its `main`, which cargo and `nextest` run *as* the
//! process. It needs no feature gate and no workflow change: the existing
//! `build + test (macos-latest)` job picks it up, which is the whole point —
//! **the window lifecycle M1 wrote had no executable coverage at all without
//! it.**
//!
//! # It answers one question the rest of the suite cannot
//!
//! Whether a GitHub macOS runner gives a process a usable WindowServer session.
//! The Windows half of P5C asked the same question of its runner and the answer
//! was yes; nothing about that transfers, because a Mac denies windows to a
//! process in an SSH or launchd context in a way Windows does not. If this
//! fails on the first run, *that* is the finding, and it fails naming the step
//! it reached rather than dying quietly.
//!
//! # On every other platform it is a no-op that says so
//!
//! `harness = false` targets are built and run on **every** host, so this must
//! succeed on Linux and Windows. It prints why it did nothing rather than
//! reporting a pass it did not earn — a helper that reports success on a
//! platform where it cannot have done anything is the failure this whole suite
//! exists to avoid.
//!
//! # Owning `main` means owning libtest's command line, not only its body
//!
//! `cargo nextest` — which is what CI runs — enumerates a target by executing it
//! with `--list --format terse` and parsing `<name>: test` lines out of its
//! stdout. A harness that ignores argv answers that with prose, so the
//! *listing* step runs the whole session and then fails to parse it; that is how
//! this target broke three CI jobs at once on the run that introduced it. The
//! listing is therefore answered in [`main`] before anything else happens.

/// The one test this target contains, under the name `nextest` reports it by.
const TEST_NAME: &str = "appkit_session";

fn main() {
    // libtest's list protocol, answered before any work: `--list` prints one
    // `<name>: test` line per test and exits. `--ignored` asks for the ignored
    // subset, and nothing here is `#[ignore]`d — on the same terms as the rest
    // of this crate's suites — so that listing is empty rather than the same
    // line again, which would enumerate one test twice.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--list") {
        if !args.iter().any(|arg| arg == "--ignored") {
            println!("{TEST_NAME}: test");
        }
        return;
    }

    #[cfg(not(target_os = "macos"))]
    {
        println!(
            "crcbl appkit session: skipped — AppKit is a macOS window system and \
             this is not macOS"
        );
    }
    #[cfg(target_os = "macos")]
    {
        macos::run();
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::time::{Duration, Instant};

    use crcbl_shell::{
        CursorIcon, DisplayMode, LogicalSize, PhysicalPoint, PhysicalSize, PointerMode, Shell,
        ShellBackend, ShellCaps, ShellEvent, SizeConstraints, WindowDesc, WindowId, open_backend,
    };

    /// How long any one step may take before the session is called stuck.
    ///
    /// A window system that answers at all answers in milliseconds; this is
    /// long enough that a loaded runner is not mistaken for a broken one, and
    /// short enough that a genuine hang is a failure rather than a timeout of
    /// the job.
    const DEADLINE: Duration = Duration::from_secs(10);

    /// The size asked for. Deliberately small: the Windows runner's desktop
    /// turned out to be 1024×768, and a window that does not fit is one the
    /// system is entitled to move or resize before anyone looks at it.
    const REQUESTED: LogicalSize = LogicalSize::new(640.0, 480.0);

    pub fn run() {
        println!("crcbl appkit session: opening the AppKit backend on the main thread");

        let mut shell = match open_backend(ShellBackend::AppKit) {
            Ok(shell) => shell,
            Err(error) => panic!(
                "crcbl appkit session: the AppKit backend would not open: {error}\n\
                 If this is the first run, the finding is that a GitHub macOS \
                 runner does not give a process a WindowServer session."
            ),
        };
        assert_eq!(shell.backend(), ShellBackend::AppKit);
        println!(
            "crcbl appkit session: opened, caps {:?}, {} monitor(s)",
            shell.caps(),
            shell.monitors().len()
        );
        assert!(
            !shell.monitors().is_empty(),
            "a session with a window server has at least one screen"
        );

        let window = shell
            .create_window(&WindowDesc {
                title: "crcbl appkit session",
                app_id: "sh.kryptic.crcbl.appkit-session",
                size: REQUESTED,
                constraints: SizeConstraints::min(LogicalSize::new(320.0, 180.0)),
                mode: DisplayMode::Windowed,
                resizable: true,
                visible: true,
                accept_drops: false,
            })
            .expect("create_window");

        // The seam's hard ordering constraint: no size until the window system
        // has configured the window, whatever the platform knew at create time.
        assert_eq!(
            shell.window_state(window).expect("state").size(),
            None,
            "a window has no size until the window system says so"
        );

        let size = wait_for(&mut shell, "the first Resized", |shell| {
            shell
                .window_state(window)
                .ok()
                .and_then(|state| state.size())
        });
        println!("crcbl appkit session: configured at {size:?}");
        assert!(
            size.width > 0 && size.height > 0,
            "a configured window has an extent a swapchain could be built at: {size:?}"
        );

        // The surface handle exists as soon as the window does, and a mode
        // change is the only thing that invalidates it.
        shell.surface_target(window).expect("surface_target");

        input(&mut shell, window, size);

        let borderless = flip(
            &mut shell,
            window,
            DisplayMode::Borderless { monitor: None },
        );
        println!("crcbl appkit session: borderless at {borderless:?}");
        let windowed = flip(&mut shell, window, DisplayMode::Windowed);
        println!("crcbl appkit session: windowed again at {windowed:?}");

        shell.destroy_window(window).expect("destroy_window");
        assert!(
            shell.window_state(window).is_err(),
            "a destroyed window is a stale handle"
        );
        println!("crcbl appkit session: a window opened, changed mode twice and closed");
    }

    /// The M2 half: the capability set, the pointer modes, the cursor, and the
    /// one piece of arithmetic in this slice that only a real window can check.
    ///
    /// # The warp is the reason this is here rather than in a `#[test]`
    ///
    /// A position crosses **three** spaces on its way from the seam to
    /// CoreGraphics — the view's own Y-up points, AppKit's Y-up screen points,
    /// and Quartz's Y-down global pixels — and getting either reflection wrong
    /// puts the cursor the same distance on the *wrong side of the middle*. That
    /// is indistinguishable from a working warp for as long as the target
    /// happens to be centred, which is exactly what a hand-written fixture would
    /// pick. The check here is the round trip through the real window: warp the
    /// pointer somewhere known, and read back where the window system says it
    /// landed.
    ///
    /// **The pointer is moved off the window first**, deliberately. Injected
    /// input describes a transition rather than a state — the Windows half of
    /// P5C paid for that lesson — so a warp to a point the cursor is already at
    /// produces no event at all, and a warp that never crosses the window's edge
    /// produces no `PointerFocus`.
    fn input(shell: &mut Box<dyn Shell>, window: WindowId, size: PhysicalSize) {
        let caps = shell.caps();
        assert!(
            caps.has_mouselook(),
            "a locked pointer and a delta beside it are both implemented: {caps:?}"
        );
        assert!(caps.contains(ShellCaps::POINTER_WARP), "{caps:?}");
        assert!(caps.contains(ShellCaps::TEXT_IME), "{caps:?}");
        assert!(
            !caps.contains(ShellCaps::POINTER_CONFINE),
            "macOS has no confine API and this backend must not claim one: {caps:?}"
        );

        // The capability and the method agree, which is what makes the bit
        // checkable rather than decorative.
        let refused = shell
            .set_pointer_mode(window, PointerMode::Confined)
            .expect_err("confinement is not implementable on this platform");
        assert!(
            refused.to_string().contains("confine"),
            "the refusal has to name the mode, or nobody can act on it: {refused}"
        );

        // Somewhere outside the window, so that the warp below is a *crossing*
        // rather than a warp to where the cursor already is — which produces no
        // event at all, and is the lesson the Windows half paid for. Past the
        // bottom-right corner, so the system's clamp to the display arrangement
        // lands it in the screen's corner and a centred window is nowhere near
        // it.
        shell
            .warp_pointer(
                window,
                PhysicalPoint::new(
                    f64::from(size.width) + 200.0,
                    f64::from(size.height) + 200.0,
                ),
            )
            .expect("a warp outside the window is clamped, not refused");
        shell.pump(&mut |_event| {});

        let target =
            PhysicalPoint::new(f64::from(size.width) * 0.75, f64::from(size.height) * 0.50);
        shell.warp_pointer(window, target).expect("warp_pointer");
        let landed = wait_for_pointer(shell);

        // One backing pixel of slack per space crossed: the warp lands on a
        // whole display pixel and the report comes back through the view's
        // points, so the two need not agree exactly.
        let slack = 3.0;
        assert!(
            (landed.x - target.x).abs() <= slack && (landed.y - target.y).abs() <= slack,
            "warped to {target:?} in a {size:?} window and the window system reported \
             {landed:?}; a Y that is the window's height minus the one asked for is a \
             missing flip, and an X that matches while the Y does not is the desktop \
             reflection rather than the window one"
        );
        println!("crcbl appkit session: warped to {target:?} and landed at {landed:?}");

        // Locking is the mode macOS does have, and it is observable through the
        // seam rather than only through the calls it made.
        shell
            .set_pointer_mode(window, PointerMode::Locked)
            .expect("pointer lock");
        assert_eq!(
            shell.window_state(window).expect("state").pointer_mode,
            PointerMode::Locked
        );
        shell
            .set_pointer_mode(window, PointerMode::Free)
            .expect("back to free");
        assert_eq!(
            shell.window_state(window).expect("state").pointer_mode,
            PointerMode::Free,
            "the freeze is desktop-wide, so leaving it set would follow the runner out \
             of this process"
        );

        // Cursor shapes and hiding, which have no seam readback — the contract
        // is that they fail only for a stale handle.
        for cursor in [
            Some(CursorIcon::Crosshair),
            Some(CursorIcon::Text),
            None,
            Some(CursorIcon::Default),
        ] {
            shell.set_cursor(window, cursor).expect("set_cursor");
        }
        println!("crcbl appkit session: pointer modes, cursors and the warp all answered");
    }

    /// Pumps until the window system reports where the pointer is.
    ///
    /// A loop of its own rather than [`wait_for`], because this one needs the
    /// event's **payload** and that function's own pump has already discarded
    /// it by the time its predicate runs.
    ///
    /// Two events can answer and either will do, which is what makes this robust
    /// to a question nobody has settled about the runner. `PointerFocus` comes
    /// from the tracking area, registered `NSTrackingActiveAlways`, so it fires
    /// whether or not this process is frontmost; `PointerMotion` additionally
    /// needs the window to be **key**, because macOS sends mouse-moved events
    /// only to a key window that has asked for them. The last one seen wins: a
    /// CI runner is a real desktop and an event this process did not cause can
    /// arrive at any moment, so the freshest report is the one that describes
    /// the warp.
    fn wait_for_pointer(shell: &mut Box<dyn Shell>) -> PhysicalPoint {
        let started = Instant::now();
        let mut seen = Vec::new();
        loop {
            let mut landed = None;
            shell.pump(&mut |event: ShellEvent| {
                seen.push(event.name());
                match event {
                    ShellEvent::PointerFocus {
                        entered: true,
                        position: Some(at),
                        ..
                    }
                    | ShellEvent::PointerMotion { abs: Some(at), .. } => landed = Some(at),
                    _ => {}
                }
            });
            if let Some(at) = landed {
                return at;
            }
            assert!(
                started.elapsed() < DEADLINE,
                "crcbl appkit session: waited {DEADLINE:?} for the pointer to be reported \
                 somewhere after a warp and nothing arrived; the events that did were \
                 {seen:?}. No PointerFocus at all means the NSTrackingArea is not \
                 delivering, and a PointerFocus with no PointerMotion means the window \
                 never became key."
            );
            shell.wait_events(Some(Duration::from_millis(20)));
        }
    }

    /// Asks for a mode and waits until the window system has answered it.
    fn flip(shell: &mut Box<dyn Shell>, window: WindowId, mode: DisplayMode) -> PhysicalSize {
        shell.set_mode(window, mode).expect("set_mode");
        let what = if matches!(mode, DisplayMode::Windowed) {
            "windowed"
        } else {
            "borderless"
        };
        wait_for(shell, what, |shell| {
            let state = shell.window_state(window).ok()?;
            // Both halves: the request has to be *honoured*, and there has to be
            // a size to render at afterwards. A mode that reported success with
            // no extent behind it would pass on the first check alone.
            state
                .mode_request_honoured()
                .then(|| state.size())
                .flatten()
        })
    }

    /// Pumps until `ready` answers, or fails naming the step that did not.
    fn wait_for<T>(
        shell: &mut Box<dyn Shell>,
        what: &str,
        mut ready: impl FnMut(&mut Box<dyn Shell>) -> Option<T>,
    ) -> T {
        let started = Instant::now();
        let mut seen = Vec::new();
        loop {
            shell.pump(&mut |event: ShellEvent| seen.push(event.name()));
            if let Some(value) = ready(shell) {
                return value;
            }
            assert!(
                started.elapsed() < DEADLINE,
                "crcbl appkit session: waited {DEADLINE:?} for {what} and it never came; \
                 the events that did arrive were {seen:?}"
            );
            // Sleeping is what `wait_events` is for, and exercising it here is
            // free: it is advisory, so a backend that returns immediately just
            // spins this loop as it would have anyway.
            shell.wait_events(Some(Duration::from_millis(20)));
        }
    }
}
