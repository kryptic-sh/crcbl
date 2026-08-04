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
//! # M4 extends this target rather than adding a second one
//!
//! The end-to-end pass could have been a `appkit_e2e` target of its own, on the
//! model of `tests/win32_e2e.rs`. It is here instead, and the reason is not
//! tidiness:
//!
//! * **`nextest` runs test binaries in parallel.** Two harness-less targets that
//!   each bootstrap an `NSApplication`, activate it and take the key window
//!   would be two processes fighting over which one is frontmost — and
//!   `CGEventPost` delivers to *whoever is frontmost*, so the loser would report
//!   that injected input never arrived. Serialising them means a test group in
//!   `.config/nextest.toml`, which is a configuration change for a problem one
//!   process does not have.
//! * **There is one `NSApplication` per process and it is not restartable.** A
//!   second target would repeat the bootstrap, the activation and the window,
//!   and would then be asserting the same M1 lifecycle a second time to get to
//!   its own subject.
//!
//! The cost is that this target is one `nextest` test, so a failure anywhere in
//! it reports the whole session as failed. That is paid for by every step
//! printing what it reached before the next one starts, which is what a
//! `harness = false` target has instead of a test name.
//!
//! # What M4 adds, and the one thing it cannot do
//!
//! * **Input the window system generated**, through `CGEventPost` — the macOS
//!   counterpart of `SendInput`. That is what reaches `interpretKeyEvents:`,
//!   which no test had ever reached: `appkit::view`'s module docs list five
//!   switches that decide
//!   whether an event is *generated or routed* rather than what a responder does
//!   with it, and every one of them is invisible to a test that calls the
//!   responder method itself. The Win32 half of P5C shipped a backend whose
//!   `TextCommit` was unreachable from a real keyboard for exactly that reason.
//! * **A pasteboard round trip against `pbcopy` and `pbpaste`**, which are
//!   Apple's own processes and have no `crcbl-shell` in them. A shell answering
//!   its own reads out of a cache passes an in-process round trip and fails this.
//! * **AppKit as the judge** of the first responder, the mouse-moved flag, the
//!   dragged types, the style mask, the frame and the screen —
//!   [`crcbl_shell::session_support::key_window`] reads them off `NSWindow`
//!   rather than out of the backend's own record of what it asked for.
//!
//! What is **not** here is the sample-level pass: driving a running game and
//! pressing F11 at it, which the two Linux suites do. That needs a renderer, and
//! macOS has no Vulkan until MoltenVK clears its P14 gate —
//! `docs/plan/ROADMAP.md`'s 2026-08-04 correction says so in as many words.
//! `docs/backlog.md` carries it as the gap it is rather than approximating it.
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
        ButtonState, ClipboardContent, ClipboardOffer, CursorIcon, DisplayMode, KeyCode, Keysym,
        LogicalSize, MimeType, PhysicalPoint, PhysicalSize, PointerButton, PointerMode,
        ReceivedMime, ScrollDelta, Shell, ShellBackend, ShellCaps, ShellEvent, SizeConstraints,
        WindowDesc, WindowId, open_backend, session_support,
    };

    /// The window's title, which is also how [`session_support::key_window`]'s
    /// answer is told apart from some other application's window.
    const TITLE: &str = "crcbl appkit session";

    /// `kVK_ANSI_A`, which `appkit::keys::key_code` maps to
    /// [`KeyCode::KeyA`] — and which, unlike an arrow key, is a character.
    const VK_A: u16 = 0x00;

    /// `kVK_UpArrow`. A key that moves a cursor and commits no text, which is
    /// the other half of the text assertion.
    const VK_UP_ARROW: u16 = 0x7E;

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

        // The checks that need AppKit *objects* rather than only the
        // Objective-C runtime, run now that `NSApplication` is up and this is
        // the main thread. Each of them was, or would have been, a `#[test]`
        // that passes on a developer's host and fails on the runner — see
        // `crcbl_shell::session_support`, which states the rule.
        for (name, outcome) in [
            (
                "every cursor shape",
                session_support::every_cursor_shape_exists(),
            ),
            (
                "the dragged-type registration",
                session_support::dragged_types_register_on_a_view(),
            ),
            (
                "the general pasteboard",
                session_support::pasteboard_round_trip(),
            ),
        ] {
            match outcome {
                Ok(()) => println!("crcbl appkit session: {name} answered"),
                Err(detail) => panic!("crcbl appkit session: {name} — {detail}"),
            }
        }

        // **`accept_drops: true`**, so that the drop path is registered on a
        // real window rather than only on the throwaway view above. There is no
        // way to *deliver* a drop from inside this process — a drag comes from
        // another application's mouse — so what this earns is that nothing
        // about the registration refuses a window that also has a tracking area
        // and a first responder.
        let window = shell
            .create_window(&WindowDesc {
                title: TITLE,
                app_id: "sh.kryptic.crcbl.appkit-session",
                size: REQUESTED,
                constraints: SizeConstraints::min(LogicalSize::new(320.0, 180.0)),
                mode: DisplayMode::Windowed,
                resizable: true,
                visible: true,
                accept_drops: true,
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

        let facts = appkit_agrees(&mut shell, size);
        input(&mut shell, window, size);
        injected_input(&mut shell, window, size);
        clipboard(&mut shell, window);
        clipboard_peer(&mut shell, window);
        resize(&mut shell, window, facts.backing_scale);

        let borderless = flip(
            &mut shell,
            window,
            DisplayMode::Borderless { monitor: None },
        );
        println!("crcbl appkit session: borderless at {borderless:?}");
        // Judged against `NSScreen` rather than against the request: borderless
        // here is "a frameless window at screen size", and both halves of that
        // are things AppKit will say out loud.
        let covering = key_window("borderless");
        assert!(
            !covering.titled,
            "a borderless window has no title bar; the style mask is {:#x}",
            covering.style_mask
        );
        let screen = covering
            .screen_frame
            .expect("a visible window is on a screen");
        assert_eq!(
            covering.frame, screen,
            "borderless covers the screen AppKit says it is on, exactly"
        );
        assert_eq!(
            borderless,
            PhysicalSize::new(
                (screen[2] * covering.backing_scale).round() as u32,
                (screen[3] * covering.backing_scale).round() as u32,
            ),
            "and the extent the seam reports is that screen in backing pixels, at a scale of \
             {}",
            covering.backing_scale
        );

        let windowed = flip(&mut shell, window, DisplayMode::Windowed);
        println!("crcbl appkit session: windowed again at {windowed:?}");
        let restored = key_window("windowed again");
        assert!(
            restored.titled,
            "a windowed window has its title bar back; the style mask is {:#x}",
            restored.style_mask
        );
        assert_eq!(
            restored.title, TITLE,
            "and it is still this session's window"
        );

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

    /// The M3 half: a real round trip through the **system** pasteboard, in
    /// both formats, through the seam rather than through the FFI.
    ///
    /// # What this earns over `session_support::pasteboard_round_trip`
    ///
    /// That one proves the pasteboard calls work and that the change count
    /// advances. This one proves the *seam* does: that
    /// [`clipboard_offer`](Shell::clipboard_offer) publishes both formats under
    /// types the reader half finds, that
    /// [`clipboard_request`](Shell::clipboard_request) answers **exactly once**
    /// per accepted read with the id it returned, and that the three
    /// [`ClipboardContent`] outcomes are told apart — a backend that answered
    /// `Empty` for "I did not look" would pass a bytes-only check and violate
    /// obligation 5.
    ///
    /// It ends by clearing the pasteboard, which is what an empty offer means
    /// on this platform and is also the state to leave a shared runner in.
    fn clipboard(shell: &mut Box<dyn Shell>, window: WindowId) {
        const TEXT: &str = "crcbl M3 — clipboard 🎮";
        const RON: &str = "(kind:\"node\",id:7)";

        let caps = shell.caps();
        assert!(caps.contains(ShellCaps::CLIPBOARD), "{caps:?}");
        assert!(caps.contains(ShellCaps::DRAG_DROP), "{caps:?}");
        assert!(
            shell.clipboard_readable(window),
            "any process may read the general pasteboard at any time, so this backend has \
             nothing to gate on and must say so"
        );

        // Both formats at once, which is what `15-windowing.md` specifies: the
        // text is what TextEdit pastes and the RON is what another Crucible
        // pastes, from one write.
        shell
            .clipboard_offer(
                window,
                &[ClipboardOffer::text(TEXT), ClipboardOffer::ron(RON)],
            )
            .expect("clipboard_offer");

        let (mime, content) = paste(shell, window, MimeType::TextUtf8);
        assert_eq!(
            content.text(),
            Some(TEXT),
            "the general pasteboard answered {content:?} for the text that was just written"
        );
        assert!(
            mime.matches(MimeType::TextUtf8),
            "a pasteboard type is a UTI and not a mime, so the answer must name the format \
             that was asked for: {mime}"
        );

        let (_, content) = paste(shell, window, MimeType::CrcblRon);
        assert_eq!(
            content.bytes(),
            Some(RON.as_bytes()),
            "the engine's own format has to be byte-exact or a pasted scene node will not \
             parse: {content:?}"
        );

        // A format nobody published. `Empty` and not `Unavailable`: the
        // pasteboard was read and holds nothing in that type, which
        // `ClipboardContent` is explicit is not a failure.
        let (_, content) = paste(shell, window, MimeType::UriList);
        assert_eq!(
            content,
            ClipboardContent::Empty,
            "a type nobody wrote is an empty answer, not a failed read"
        );

        // An empty offer, which on this platform can only mean "clear" — there
        // is no owner to give up.
        shell
            .clipboard_offer(window, &[])
            .expect("an empty offer clears the pasteboard");
        let (_, content) = paste(shell, window, MimeType::TextUtf8);
        assert_eq!(
            content,
            ClipboardContent::Empty,
            "clearContents left {content:?} behind"
        );

        println!("crcbl appkit session: both formats round-tripped through the system pasteboard");
    }

    // -----------------------------------------------------------------------
    // M4: AppKit as the judge
    // -----------------------------------------------------------------------

    /// Pumps until a window of this process has the keyboard, or fails with
    /// **which of the two mechanisms** refused.
    ///
    /// The first version of this discarded
    /// [`session_support::key_window`]'s error with `.ok()` and reported only
    /// "it never came", which is precisely the shape of failure the Windows
    /// half of P5C spent four CI round trips on: a symptom that cannot say
    /// which mechanism produced it. [`session_support::activation`] separates
    /// them, and the two want opposite responses:
    ///
    /// * `can_become_key: false` on a `CrcblWindow` is **our** defect — the
    ///   `canBecomeKeyWindow` override is not on the class the window turned
    ///   out to be, and a borderless window would silently stop taking
    ///   keystrokes.
    /// * `app_active: false` with `can_become_key: true` is the **session**
    ///   refusing to activate an unbundled binary, which no backend can fix.
    fn wait_for_key_window(shell: &mut Box<dyn Shell>) -> session_support::KeyWindow {
        let started = Instant::now();
        let mut last;
        loop {
            shell.pump(&mut |_event: ShellEvent| {});
            match session_support::key_window() {
                Ok(facts) => return facts,
                Err(detail) => last = detail,
            }
            assert!(
                started.elapsed() < DEADLINE,
                "crcbl appkit session: waited {DEADLINE:?} for a key window and none came.\n\
                 The last answer was: {last}\n\
                 What the application and its first window say: {:?}",
                session_support::activation()
            );
            shell.wait_events(Some(Duration::from_millis(20)));
        }
    }

    /// [`session_support::key_window`], or a failure naming the step that asked.
    fn key_window(step: &str) -> session_support::KeyWindow {
        session_support::key_window()
            .unwrap_or_else(|detail| panic!("crcbl appkit session: {step}: {detail}"))
    }

    /// What `NSWindow` says about the window the shell just built.
    ///
    /// # Three structural claims stop being structural here
    ///
    /// `appkit::view`'s module docs list five switches that decide whether an
    /// event is *generated or routed*, and `docs/backlog.md` has carried all five
    /// as "structural rather than verified" since M2 — because each is invisible
    /// to a test that calls the responder method itself. Three of them have a
    /// readback on a live window, and this is where they are read:
    ///
    /// * **`setAcceptsMouseMovedEvents:`** — the system generates no
    ///   `NSEventTypeMouseMoved` at all for a window that answers `false`, so a
    ///   game would have a pointer that only moves while a button is held.
    /// * **`makeFirstResponder:`** — `sendEvent:` delivers key events to the
    ///   first responder, which is the *window* until something claims it. A
    ///   window that is its own first responder swallows every keystroke.
    /// * **`registerForDraggedTypes:`** — on the real window this time, rather
    ///   than on the throwaway view
    ///   [`session_support::dragged_types_register_on_a_view`] builds, so it also
    ///   says that a window carrying a tracking area and a first responder still
    ///   ends up registered.
    ///
    /// The window being **key** is waited for rather than asserted outright: it
    /// has just been ordered front, and which window has the keyboard is the
    /// window server's to decide on its own schedule.
    fn appkit_agrees(shell: &mut Box<dyn Shell>, size: PhysicalSize) -> session_support::KeyWindow {
        let facts = wait_for_key_window(shell);
        assert_eq!(
            facts.title, TITLE,
            "the key window is this session's own and not some other application's: {facts:?}"
        );
        assert!(
            facts.accepts_mouse_moved,
            "-[NSWindow acceptsMouseMovedEvents] is what makes pointer motion exist at all; \
             without it a game's camera only moves while a button is held: {facts:?}"
        );
        assert_eq!(
            facts.first_responder, "CrcblView",
            "the content view has the first responder, so `sendEvent:` routes key events to it \
             rather than to the window: {facts:?}"
        );
        assert_eq!(
            facts.content_view, "CrcblView",
            "and the content view is this backend's own class: {facts:?}"
        );
        assert_eq!(
            facts.dragged_types,
            vec!["public.file-url".to_owned()],
            "a window created with accept_drops: true is registered for exactly the type this \
             backend reads; AppKit sends no dragging message to a view registered for nothing: \
             {facts:?}"
        );
        assert!(facts.titled, "a windowed window is titled: {facts:?}");

        // The extent, judged the long way round: the content view's frame in
        // points times the window's backing scale. The backend computes the same
        // number through `convertRectToBacking:` on the view's *bounds*, so this
        // is two different AppKit reads agreeing rather than the backend agreeing
        // with itself.
        assert_eq!(
            (
                (facts.content_points[0] * facts.backing_scale).round() as u32,
                (facts.content_points[1] * facts.backing_scale).round() as u32,
            ),
            (size.width, size.height),
            "the seam reported {size:?}; AppKit says the content view is {:?} points at a \
             backing scale of {}",
            facts.content_points,
            facts.backing_scale
        );
        println!(
            "crcbl appkit session: AppKit agrees — first responder {}, mouse-moved {}, dragged \
             types {:?}, scale {}",
            facts.first_responder,
            facts.accepts_mouse_moved,
            facts.dragged_types,
            facts.backing_scale
        );
        facts
    }

    /// A resize AppKit performed, delivered as one [`ShellEvent::Resized`] at the
    /// size the window system ended at.
    ///
    /// `-[NSWindow setContentSize:]` is what a user dragging the window's edge
    /// produces, minus the modal drag loop that `pump` cannot return from — the
    /// same substitution the Win32 suite makes with `SetWindowPos`, and for the
    /// same reason: CI has nobody to hold a mouse button.
    fn resize(shell: &mut Box<dyn Shell>, window: WindowId, scale: f64) {
        /// The content size to ask for, in points. Small enough to fit on the
        /// 1024×768 display both CI runners turned out to have.
        const POINTS: (f64, f64) = (512.0, 384.0);
        session_support::resize_key_window(POINTS.0, POINTS.1).expect("resize_key_window");

        let expected = PhysicalSize::new(
            (POINTS.0 * scale).round() as u32,
            (POINTS.1 * scale).round() as u32,
        );
        let got = wait_for(shell, "the resize AppKit was asked for", |shell| {
            shell
                .window_state(window)
                .ok()
                .and_then(|state| state.size())
                .filter(|size| *size == expected)
        });
        // And AppKit's own account of the same window, which is what makes this
        // a resize rather than the seam echoing a number back.
        let after = key_window("after the resize");
        assert_eq!(
            after.content_points,
            [POINTS.0, POINTS.1],
            "-[NSWindow setContentSize:] took: {after:?}"
        );
        println!("crcbl appkit session: resized to {got:?} backing pixels");
    }

    // -----------------------------------------------------------------------
    // M4: input the window system generated
    // -----------------------------------------------------------------------

    /// What a failure to see injected input most likely means, printed with
    /// every one of them.
    ///
    /// **Nothing here has run on a Mac.** The one thing that could stop all of
    /// it is TCC: macOS 10.14 and later require a process to hold the
    /// Accessibility right before it may synthesize keyboard events, and a
    /// GitHub runner has nobody to grant one. What is *not* known — and what the
    /// first run of this answers — is whether that gate applies when the events
    /// are delivered back to the posting process itself, which is the whole of
    /// what happens here. If it does, every assertion below fails with this text
    /// beside it and the finding is the runner's, not the backend's.
    const INJECTION_HINT: &str = "\nNo injected event arriving at all, with the window key and \
         this process frontmost, is the signature of TCC refusing CGEventPost: macOS 10.14+ \
         gates synthetic keyboard events behind the Accessibility right, and a CI runner has \
         nobody to grant one. Whether that gate applies to events posted back to the posting \
         process is what this run answers. If it does, the fallback is \
         `-[NSApplication postEvent:atStart:]`, which needs no permission and still goes through \
         nextEventMatchingMask:, sendEvent:, the first responder and interpretKeyEvents: — \
         everything but the window server's own leg.";

    /// Keyboard, text and pointer, all of it generated by the window system
    /// rather than by a call into a responder.
    ///
    /// # This is the test M4 exists for
    ///
    /// `TextCommit` on macOS comes out of `insertText:replacementRange:`, which
    /// is called **by the input method**, from inside `interpretKeyEvents:`,
    /// which `keyDown:` calls on a real key event that `sendEvent:` routed to a
    /// first responder whose `inputContext` is non-nil — and that last part is
    /// only true because `CrcblView` conforms to `NSTextInputClient`. Every link
    /// in that chain was written on a Linux machine and none of them had ever
    /// run. The Win32 half of P5C was in exactly this state and its e2e suite
    /// found `TranslateMessage` missing from the pump, which is the same defect
    /// one platform over.
    ///
    /// # Nothing is posted until this process is the one input goes to
    ///
    /// `CGEventPost` puts the event in the **session's** stream and the session
    /// hands it to whatever is frontmost. Posting while another application holds
    /// the key window types into that application — on a CI runner, into the
    /// job's own shell. So the key window is checked first, by title, and this
    /// fails rather than posting if it is not ours.
    fn injected_input(shell: &mut Box<dyn Shell>, window: WindowId, size: PhysicalSize) {
        let before = key_window("before injecting input");
        assert_eq!(
            before.title, TITLE,
            "input follows the key window, so nothing may be posted while somebody else has it: \
             {before:?}"
        );

        // --- a printable key, and the text it commits ---
        quartz::tap_key(VK_A);
        let typed = collect_until(shell, "the injected 'a' and its release", |seen| {
            seen.iter()
                .filter(|event| matches!(event, ShellEvent::Key { .. }))
                .count()
                >= 2
        });
        let typed_keys = keys(&typed);
        assert_eq!(
            typed_keys[0].0,
            u32::from(VK_A),
            "the scan code is the `kVK_*` the event carried: {:?}",
            names(&typed)
        );
        assert_eq!(
            typed_keys[0].1,
            Some(KeyCode::KeyA),
            "the physical position"
        );
        assert_eq!(typed_keys[0].3, ButtonState::Pressed);
        assert!(!typed_keys[0].4, "the first press is not a repeat");
        assert_eq!(
            typed_keys[0].2,
            Keysym::from_char('a'),
            "the layout's symbol, lowercased so a rebind menu reads the same as it does on Linux"
        );
        assert_eq!(typed_keys[1].3, ButtonState::Released);

        let committed: String = typed
            .iter()
            .filter_map(|event| match event {
                ShellEvent::TextCommit { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            committed,
            "a",
            "**the first text this backend has ever committed from a real keystroke.** It needs \
             `sendEvent:` to route the event to the first responder, `keyDown:` to hand it to \
             `interpretKeyEvents:`, and the view's `inputContext` to be non-nil — which it is \
             only because CrcblView conforms to NSTextInputClient. The events that did arrive \
             were {:?}",
            names(&typed)
        );

        // --- and a key that is not a character ---
        quartz::tap_key(VK_UP_ARROW);
        let arrow = collect_until(shell, "the injected arrow key", |seen| {
            seen.iter()
                .any(|event| matches!(event, ShellEvent::Key { .. }))
        });
        let arrow_keys = keys(&arrow);
        assert_eq!(
            arrow_keys[0].1,
            Some(KeyCode::ArrowUp),
            "{:?}",
            names(&arrow)
        );
        assert_eq!(arrow_keys[0].0, u32::from(VK_UP_ARROW));
        assert!(
            !names(&arrow).contains(&"TextCommit"),
            "an arrow key moves a cursor; it is not a character in a text field, and a backend \
             reading `-[NSEvent characters]` instead of asking the input method would commit one \
             here: {:?}",
            names(&arrow)
        );

        // --- the pointer, moved by the window server ---
        //
        // Parked inside the window with the seam's own warp first, because a
        // mouse-moved event goes to the window under the cursor: posting motion
        // while the cursor sits over some other application's window is a test of
        // that application.
        let target =
            PhysicalPoint::new(f64::from(size.width) * 0.25, f64::from(size.height) * 0.25);
        shell.warp_pointer(window, target).expect("warp_pointer");
        let parked = wait_for_pointer(shell);

        let from = quartz::cursor();
        quartz::move_mouse(from.x + 40.0, from.y + 30.0);
        let moved = collect_until(shell, "the injected motion", |seen| {
            seen.iter().any(|event| {
                matches!(
                    event,
                    ShellEvent::PointerMotion {
                        abs: Some(_),
                        raw_delta: Some(_),
                        ..
                    }
                )
            })
        });
        let (at, delta) = moved
            .iter()
            .rev()
            .find_map(|event| match event {
                ShellEvent::PointerMotion {
                    abs: Some(at),
                    raw_delta: Some(delta),
                    ..
                } => Some((*at, *delta)),
                _ => None,
            })
            .expect("just waited for it");
        // **The asymmetry `appkit::pointer` exists to make visible, observed.**
        // `locationInWindow` is Y-up and the delta beside it is Quartz's, which
        // is Y-down and already agrees with this seam — so a move that goes down
        // the screen has to come back as a *larger* window Y **and** a positive
        // raw delta. A backend that reflected the delta as well as the position
        // would fail the second half and pass the first, and the symptom in a
        // game is an inverted first-person camera.
        assert!(
            at.x > parked.x && at.y > parked.y,
            "the cursor was moved right and down the screen from {parked:?} and the window \
             reported {at:?}; the desktop is {:?} and the events were {:?}",
            quartz::cursor(),
            names(&moved)
        );
        assert!(
            delta.0 > 0.0 && delta.1 > 0.0,
            "and the raw delta beside it is Quartz's own, unflipped: {delta:?}. A negative Y \
             here with a correct position above is the delta being reflected, which is an \
             inverted camera"
        );

        // --- a click, and a scroll ---
        let at = quartz::cursor();
        quartz::click(at.x, at.y);
        let clicked = collect_until(shell, "the injected click", |seen| {
            seen.iter()
                .filter(|event| matches!(event, ShellEvent::Button { .. }))
                .count()
                >= 2
        });
        let buttons: Vec<_> = clicked
            .iter()
            .filter_map(|event| match event {
                ShellEvent::Button { button, state, .. } => Some((*button, *state)),
                _ => None,
            })
            .collect();
        assert_eq!(
            buttons,
            vec![
                (PointerButton::Left, ButtonState::Pressed),
                (PointerButton::Left, ButtonState::Released),
            ],
            "one click, two events: {:?}",
            names(&clicked)
        );

        quartz::scroll_lines(1);
        let scrolled = collect_until(shell, "the injected scroll", |seen| {
            seen.iter()
                .any(|event| matches!(event, ShellEvent::Wheel { .. }))
        });
        let wheel = scrolled
            .iter()
            .find_map(|event| match event {
                ShellEvent::Wheel { delta, .. } => Some(*delta),
                _ => None,
            })
            .expect("just waited for it");
        // **The unit, not the sign.** `kCGScrollEventUnitLine` produces an
        // `NSEvent` whose `hasPreciseScrollingDeltas` is `NO`, which is the whole
        // of what decides `Lines` against `Pixels` — and that is the branch worth
        // pinning. The *sign* is not: "natural" scrolling is a per-user system
        // preference that inverts it, so an assertion on it would be an assertion
        // about the runner's settings. `docs/backlog.md` carries the horizontal
        // sign as unverified for the same reason.
        match wheel {
            ScrollDelta::Lines { x, y } => {
                assert_eq!(x, 0.0, "a vertical notch has no horizontal component");
                assert!(y != 0.0, "a notch of one line is not zero lines");
            }
            other => panic!(
                "a line-unit scroll wheel event is Lines and not {other:?}; a backend reading \
                 hasPreciseScrollingDeltas backwards would report a trackpad's pixels here: {:?}",
                names(&scrolled)
            ),
        }
        println!(
            "crcbl appkit session: the window server's own keyboard, text, pointer, click and \
             scroll all arrived"
        );
    }

    /// Every key event in `events`, flattened for assertion.
    ///
    /// Fails rather than answering an empty list: every caller indexes it, and
    /// "the slice was empty" is a worse message than the one the wait already
    /// printed.
    fn keys(events: &[ShellEvent]) -> Vec<(u32, Option<KeyCode>, Keysym, ButtonState, bool)> {
        let keys: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ShellEvent::Key {
                    scancode,
                    key_code,
                    keysym,
                    state,
                    repeat,
                    ..
                } => Some((scancode.0, *key_code, *keysym, *state, *repeat)),
                _ => None,
            })
            .collect();
        assert!(!keys.is_empty(), "no key events in {:?}", names(events));
        keys
    }

    /// The names of everything in `events`, for a failure message.
    fn names(events: &[ShellEvent]) -> Vec<&'static str> {
        events.iter().map(ShellEvent::name).collect()
    }

    /// Pumps until everything collected so far satisfies `enough`, and answers
    /// all of it.
    ///
    /// A deadline and a poll rather than a sleep, which
    /// `docs/plan/12-testing.md` makes the rule for anything asynchronous — and
    /// here the asynchronous thing is the **window server**, which is the case
    /// the rule was written for. Everything is kept rather than only the
    /// matching event, because the assertions that follow need to see what
    /// *else* arrived: a CI runner is a real desktop and an event this process
    /// did not cause can turn up at any moment.
    fn collect_until(
        shell: &mut Box<dyn Shell>,
        what: &str,
        mut enough: impl FnMut(&[ShellEvent]) -> bool,
    ) -> Vec<ShellEvent> {
        let started = Instant::now();
        let mut seen = Vec::new();
        loop {
            shell.pump(&mut |event| seen.push(event));
            if enough(&seen) {
                return seen;
            }
            assert!(
                started.elapsed() < DEADLINE,
                "crcbl appkit session: waited {DEADLINE:?} for {what} and it never came; the \
                 events that did arrive were {:?}.{INJECTION_HINT}",
                names(&seen)
            );
            shell.wait_events(Some(Duration::from_millis(20)));
        }
    }

    // -----------------------------------------------------------------------
    // M4: the pasteboard, against processes that are not this one
    // -----------------------------------------------------------------------

    /// A round trip through the system pasteboard with **another process** on
    /// the far side, in both directions.
    ///
    /// # What this earns over [`clipboard`]
    ///
    /// That one writes with `NSPasteboard` and reads with `NSPasteboard`, from
    /// one process. It would pass unchanged against a shell that never touched
    /// the pasteboard server at all and answered its own reads out of a cache —
    /// which is precisely the failure the Win32 suite added a second binary to
    /// rule out. Only a reader with no `crcbl-shell` in it can tell those apart.
    ///
    /// # Decision: `pbcopy` and `pbpaste`, not a helper binary of ours
    ///
    /// The Win32 suite ships `crcbl-e2e-win32-clip` because Windows has no stock
    /// command-line clipboard client. macOS has two, in `/usr/bin`, installed on
    /// every Mac and written by Apple — so a peer of our own would be a second
    /// hand-written Objective-C FFI whose only advantage over Apple's is that we
    /// maintain it. It would also have to be a `[[bin]]` with no
    /// `required-features`, since this target has none, and would therefore be
    /// built on Linux, Windows and `wasm32` by every `--all-features` job to do
    /// nothing there.
    ///
    /// What the substitution costs is the **engine's own format**: `pbpaste`
    /// reads text and cannot be asked for `application/x-crcbl+ron`. So the
    /// cross-process claim is made about the format another application would
    /// actually read, and the RON half stays covered in-process by [`clipboard`].
    /// `docs/backlog.md` records the difference rather than implying parity.
    fn clipboard_peer(shell: &mut Box<dyn Shell>, window: WindowId) {
        const OURS: &str = "crcbl M4 — written by the shell 🎮";
        const THEIRS: &str = "crcbl M4 — written by pbcopy 🎮";

        // Out: the shell writes, and a process with none of this code in it
        // reads what the shell wrote.
        shell
            .clipboard_offer(window, &[ClipboardOffer::text(OURS)])
            .expect("clipboard_offer");
        assert_eq!(
            pbpaste().trim_end_matches('\n'),
            OURS,
            "another process read the general pasteboard and did not find what this shell put \
             there, so the bytes never reached the pasteboard server"
        );

        // In: the same, the other way round. `pbcopy` is a whole process
        // starting, claiming the pasteboard and exiting, none of which this one
        // pumped for — which on X11 or Wayland would be a conversation this shell
        // had to take part in, and here is nobody's business but the server's.
        pbcopy(THEIRS);
        let (_, content) = paste(shell, window, MimeType::TextUtf8);
        assert_eq!(
            content.text(),
            Some(THEIRS),
            "the shell read {content:?} rather than what pbcopy had just written, so it is \
             answering out of its own cache rather than from the pasteboard server"
        );

        // Left empty, which is the state to leave a shared runner in.
        shell
            .clipboard_offer(window, &[])
            .expect("an empty offer clears the pasteboard");
        println!("crcbl appkit session: pbcopy and pbpaste both agree with the seam");
    }

    /// Everything `/usr/bin/pbpaste` prints, or a failure naming why it could
    /// not be asked.
    fn pbpaste() -> String {
        let output = std::process::Command::new("/usr/bin/pbpaste")
            .output()
            .expect("pbpaste is part of a stock macOS");
        assert!(
            output.status.success(),
            "pbpaste failed with {:?}: {:?}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Hands `text` to `/usr/bin/pbcopy` and waits for it to exit.
    ///
    /// Waiting matters: the pasteboard is claimed while `pbcopy` runs, so a read
    /// issued before it exits is a race rather than a test.
    fn pbcopy(text: &str) {
        use std::io::Write as _;
        let mut child = std::process::Command::new("/usr/bin/pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("pbcopy is part of a stock macOS");
        child
            .stdin
            .take()
            .expect("stdin was piped")
            .write_all(text.as_bytes())
            .expect("pbcopy reads its stdin");
        let status = child.wait().expect("pbcopy exits");
        assert!(status.success(), "pbcopy failed with {status:?}");
    }

    /// Issues one read and pumps until its answer arrives, asserting there is
    /// **exactly one**.
    ///
    /// The pump after the answer is the half that has teeth: obligation 4 is
    /// "exactly once", and a backend that queued its answer twice would satisfy
    /// a loop that stopped at the first one.
    fn paste(
        shell: &mut Box<dyn Shell>,
        window: WindowId,
        mime: MimeType,
    ) -> (ReceivedMime, ClipboardContent) {
        let request = shell
            .clipboard_request(window, mime)
            .expect("clipboard_request");
        let started = Instant::now();
        let mut answers = Vec::new();
        let mut seen = Vec::new();
        let collect = |event: ShellEvent, answers: &mut Vec<_>, seen: &mut Vec<_>| {
            seen.push(event.name());
            if let ShellEvent::ClipboardData {
                window: asked,
                request: id,
                mime,
                content,
            } = event
            {
                assert_eq!(asked, window, "an answer named a window nobody asked about");
                assert_eq!(id, request, "an answer named a request nobody issued");
                answers.push((mime, content));
            }
        };
        loop {
            shell.pump(&mut |event| collect(event, &mut answers, &mut seen));
            if !answers.is_empty() {
                break;
            }
            assert!(
                started.elapsed() < DEADLINE,
                "crcbl appkit session: waited {DEADLINE:?} for the answer to {request} \
                 ({mime}) and nothing arrived; the events that did were {seen:?}"
            );
            shell.wait_events(Some(Duration::from_millis(20)));
        }
        // One more turn, collecting the same way, so a **second** answer is
        // caught here rather than left in the queue for whatever pumps next.
        shell.pump(&mut |event| collect(event, &mut answers, &mut seen));
        assert_eq!(
            answers.len(),
            1,
            "{request} was answered {} times, and obligation 4 says exactly once: {answers:?}",
            answers.len()
        );
        answers.remove(0)
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

    // -----------------------------------------------------------------------
    // The Quartz event stream, which is the only way in from outside
    // -----------------------------------------------------------------------

    /// The CoreGraphics surface a *harness* needs, as distinct from the
    /// backend's.
    ///
    /// Hand-written here rather than borrowed from `crcbl_shell::appkit::ffi`,
    /// which is `pub(crate)` — and rightly so, on the same grounds
    /// `tests/win32_e2e.rs` gives for its own `user32` table: a test that drove
    /// the backend through the backend's own ABI declarations would be grading
    /// its own homework. These are the calls somebody else's input device makes.
    ///
    /// # Decision: posted from this process, and that is still "from outside"
    ///
    /// The Win32 suite injects from a **second process** because `SendInput`
    /// from the thread that owns the window is the one arrangement that proves
    /// nothing about the message queue. That argument does not transfer.
    /// `CGEventPost` does not put an event in this process's queue: it hands it
    /// to the **window server**, which decides who is frontmost, builds the
    /// `NSEvent`, and delivers it to the application's run loop — so the event
    /// re-enters through `nextEventMatchingMask:` exactly as a keyboard's would,
    /// whoever posted it. A second process would add a TCC surface (synthetic
    /// events *to another application* are unambiguously gated) and one more
    /// binary built on three platforms that cannot use it, and would buy nothing
    /// this does not already have.
    mod quartz {
        use core::ffi::c_void;

        /// `CGEventRef`.
        type EventRef = *mut c_void;

        /// `CGPoint`, in Quartz's global space: **Y down** from the top-left of
        /// the primary display, which is the space this seam already uses and
        /// the opposite of AppKit's.
        #[repr(C)]
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct Point {
            /// Rightwards from the primary display's left edge.
            pub x: f64,
            /// **Downwards** from the primary display's top edge.
            pub y: f64,
        }

        /// `kCGHIDEventTap` — inject where a device would, ahead of everything
        /// that filters the session's input.
        const HID_TAP: u32 = 0;
        /// `kCGEventLeftMouseDown`.
        const LEFT_DOWN: u32 = 1;
        /// `kCGEventLeftMouseUp`.
        const LEFT_UP: u32 = 2;
        /// `kCGEventMouseMoved`.
        const MOUSE_MOVED: u32 = 5;
        /// `kCGMouseButtonLeft`.
        const BUTTON_LEFT: u32 = 0;
        /// `kCGScrollEventUnitLine` — a wheel notch rather than a trackpad's
        /// pixels, which is what decides `ScrollDelta::Lines` at the far end.
        const UNIT_LINE: u32 = 1;

        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGEventCreate(source: *mut c_void) -> EventRef;
            fn CGEventGetLocation(event: EventRef) -> Point;
            fn CGEventCreateKeyboardEvent(
                source: *mut c_void,
                keycode: u16,
                down: bool,
            ) -> EventRef;
            fn CGEventCreateMouseEvent(
                source: *mut c_void,
                kind: u32,
                at: Point,
                button: u32,
            ) -> EventRef;
            /// **Variadic, and it has to be declared that way.**
            ///
            /// The C signature is
            /// `CGEventCreateScrollWheelEvent(source, units, wheelCount, ...)`,
            /// so the per-axis amounts are variadic arguments. On
            /// `aarch64-apple-darwin` — the architecture of the runner and of
            /// every Mac sold since 2020 — variadic arguments are passed on the
            /// **stack** while ordinary ones are passed in registers, so a
            /// non-variadic declaration of this compiles, links, runs, and
            /// scrolls by whatever was in the register it looked in. It is the
            /// same hazard `appkit::ffi` documents at length for `objc_msgSend`,
            /// arriving through a plain C function.
            fn CGEventCreateScrollWheelEvent(
                source: *mut c_void,
                units: u32,
                wheels: u32,
                ...
            ) -> EventRef;
            fn CGEventPost(tap: u32, event: EventRef);
        }

        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFRelease(object: *mut c_void);
        }

        /// Where the cursor is, in Quartz's global space.
        ///
        /// Read through an event rather than through `NSEvent`'s
        /// `mouseLocation`, so that no part of this file is in AppKit's Y-up
        /// space: mixing the two is the mistake `appkit::geometry` exists to
        /// prevent, and a harness that made it would be reporting the backend's
        /// coordinate handling as broken while its own arithmetic was.
        #[must_use]
        pub fn cursor() -> Point {
            // SAFETY: a null source asks for an event stamped with the current
            // state; the result is a live `CGEventRef` this function owns and
            // releases, and `CGEventGetLocation` only reads it.
            unsafe {
                let event = CGEventCreate(core::ptr::null_mut());
                if event.is_null() {
                    return Point::default();
                }
                let at = CGEventGetLocation(event);
                CFRelease(event);
                at
            }
        }

        /// Presses and releases one key by its `kVK_*` code.
        ///
        /// Two events rather than one, and in that order: an injected key
        /// describes a **transition**, which the Windows half of P5C paid a CI
        /// round trip to learn — a second down for a key already held produces
        /// no event at all, so a press that is never released leaves the session
        /// with a stuck modifier for the next test.
        pub fn tap_key(keycode: u16) {
            key(keycode, true);
            key(keycode, false);
        }

        /// One key edge.
        fn key(keycode: u16, down: bool) {
            // SAFETY: a null source is the documented "no particular source";
            // the event is live, posted, and released exactly once here.
            unsafe {
                let event = CGEventCreateKeyboardEvent(core::ptr::null_mut(), keycode, down);
                if event.is_null() {
                    return;
                }
                CGEventPost(HID_TAP, event);
                CFRelease(event);
            }
        }

        /// Moves the cursor to a point in Quartz's global space.
        pub fn move_mouse(x: f64, y: f64) {
            mouse(MOUSE_MOVED, Point { x, y });
        }

        /// A press and its release, both at the same point.
        pub fn click(x: f64, y: f64) {
            let at = Point { x, y };
            mouse(LEFT_DOWN, at);
            mouse(LEFT_UP, at);
        }

        /// One mouse event.
        fn mouse(kind: u32, at: Point) {
            // SAFETY: as in `key`; `at` is a `CGPoint` passed by value.
            unsafe {
                let event = CGEventCreateMouseEvent(core::ptr::null_mut(), kind, at, BUTTON_LEFT);
                if event.is_null() {
                    return;
                }
                CGEventPost(HID_TAP, event);
                CFRelease(event);
            }
        }

        /// Scrolls `lines` notches on the vertical axis.
        pub fn scroll_lines(lines: i32) {
            // SAFETY: as in `key`. One axis is declared and one variadic
            // argument is passed, which is what `wheels: 1` promises the call.
            unsafe {
                let event =
                    CGEventCreateScrollWheelEvent(core::ptr::null_mut(), UNIT_LINE, 1, lines);
                if event.is_null() {
                    return;
                }
                CGEventPost(HID_TAP, event);
                CFRelease(event);
            }
        }
    }
}
