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
//!   [`crcbl_shell::session_support::window_facts`] reads them off `NSWindow`
//!   rather than out of the backend's own record of what it asked for.
//!
//! # M5: only the injection needs the keyboard, and the runner may refuse it
//!
//! The first macOS run of M4 got as far as the key window and stopped there.
//! `-[NSApp keyWindow]` was nil, and the diagnostic beside it was decisive:
//! `can_become_key: true`, `visible: true`, one window with the right title and
//! the right class, `app_active: false`. The `canBecomeKeyWindow` override is
//! installed and answering; the **application** never becomes active, which is
//! what a GitHub runner does to an unbundled binary and which no backend can
//! fix. Three things follow, and they are this slice:
//!
//! * **The readback stops going through the key window.** Every fact M4 asserts
//!   — the three view switches, the geometry, the style mask, the backing scale
//!   — is an ordinary accessor that any window answers, active or not. They come
//!   from this process's own window, found by title among `-[NSApp windows]`, so
//!   they run on a runner that never activates instead of being discarded.
//! * **The harness asks for activation itself**, through
//!   [`macos::frontmost`] — exactly parallel to `tests/win32_e2e.rs`'s
//!   `desktop::take_foreground`, and in a harness for the same reason: a game
//!   does not get to steal the focus, so the lever must not live in
//!   `src/appkit/`.
//! * **If it is still refused, the injection is skipped out loud.**
//!   `CGEventPost` puts an event in the session's stream and the session gives
//!   it to whoever is frontmost, so it is the one part that cannot proceed.
//!   `docs/plan/12-testing.md` calls a silently-skipped end-to-end check a known
//!   trap, so the skip prints what did not run, why, and the evidence — and
//!   `docs/backlog.md` carries `interpretKeyEvents:` as unverified on CI.
//!
//! **The runner granted the activation**, so the skip branch is written and
//! unrun and the whole M4 judge passes. It also found the one thing M4 had
//! genuinely wrong: **a warp is not an event.**
//! `CGWarpMouseCursorPosition` moves the cursor and posts nothing, so a wait for
//! "the pointer to be reported after a warp" is a wait for something that does
//! not exist. It looked correct because [`macos::input`]'s warp crossed the
//! window's edge and AppKit re-evaluates its tracking areas against where the
//! cursor actually is — a `mouseEntered:`, not a report of the warp — while
//! [`macos::injected_input`]'s warp stayed inside the window, crossed nothing
//! and waited ten seconds. [`macos::wait_for_pointer`] now posts a real
//! `kCGEventMouseMoved` to ask the question, and the warp is left to do only
//! what it promises.
//!
//! **And a posted mouse event carries no delta unless the poster puts one on
//! it.** `CGEventCreateMouseEvent` places an event at an absolute location and
//! leaves `kCGMouseEventDeltaX`/`Y` at zero; `appkit::view` reads exactly those
//! fields into `raw_delta`, so the next run reported `(0.0, 0.0)` — faithfully.
//! The assertion had been checking a value this harness never supplied, and
//! could only ever have passed by accident. [`macos::quartz::move_mouse_by`]
//! now sets the field, so the seam is held to reporting *the* delta rather than
//! *a* delta, and the Y-up position against Y-down delta asymmetry that
//! `appkit::pointer` exists for is finally observable.
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

    use crcbl_core::log::Filter;
    use crcbl_shell::{
        ButtonState, ClipboardContent, ClipboardOffer, CursorIcon, DisplayMode, KeyCode, Keysym,
        LogicalSize, MimeType, PhysicalPoint, PhysicalSize, PointerButton, PointerMode,
        ReceivedMime, ScrollDelta, Shell, ShellBackend, ShellCaps, ShellEvent, SizeConstraints,
        WindowDesc, WindowId, open_backend, session_support,
    };

    /// The window's title, which is **how this session finds its own window**.
    ///
    /// [`session_support::window_facts`] looks it up by this among
    /// `-[NSApp windows]`, so it is the identity every readback rests on rather
    /// than a decoration — and the same string is what tells
    /// [`session_support::key_window`]'s answer apart from another
    /// application's.
    const TITLE: &str = "crcbl appkit session";

    /// `kVK_ANSI_A`, which `appkit::keys::key_code` maps to
    /// [`KeyCode::KeyA`] — and which, unlike an arrow key, is a character.
    const VK_A: u16 = 0x00;

    /// `kVK_UpArrow`. A key that moves a cursor and commits no text, which is
    /// the other half of the text assertion.
    const VK_UP_ARROW: u16 = 0x7E;

    /// The relative delta the injected pointer move carries, in Quartz's global
    /// display coordinates — **right and up**, since Quartz's Y is down.
    ///
    /// It is both the distance the cursor travels and the value written into
    /// the event's `kCGMouseEventDeltaX`/`Y`, so the two cannot drift apart.
    ///
    /// Every part of the pair is doing a job:
    ///
    /// * **The magnitudes differ**, so a backend reading `deltaX` and `deltaY`
    ///   out of each other's field reports a pair that is wrong rather than one
    ///   that happens to be symmetric.
    /// * **The signs differ**, so the reflection this whole check exists for
    ///   fails *distinctly* from a swap: reflecting Y makes the second
    ///   component positive, while swapping makes the first one negative.
    ///   Having a negative component at all is also the only way to see that one
    ///   survives the trip rather than being clamped or `abs`ed somewhere.
    /// * **It is large enough to be identifiable.** `wait_for_pointer` posts
    ///   moves at the parked point, so this move's report has to be
    ///   recognisable as this one and not as one of those; half of each
    ///   component is the margin that decides it.
    /// * **It is small enough to stay on the window**, since a mouse-moved
    ///   event goes to whatever is under the cursor, and the parked point is a
    ///   quarter of the way in from the window's top-left corner.
    const NUDGE: (i64, i64) = (40, -30);

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

    /// How far a reported pointer position may sit from the one that was asked
    /// for and still be a report *of* it.
    ///
    /// One backing pixel per space crossed: a warp lands on a whole display
    /// pixel and the report comes back through the view's points, so the two
    /// need not agree exactly. It is also what makes a position *identifying* —
    /// [`wait_for_pointer`] uses it to tell its own report from somebody
    /// else's, so it has to stay far smaller than the distance between the
    /// points this session parks the cursor at.
    const POINTER_SLACK: f64 = 3.0;

    pub fn run() {
        install_logger();
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

        // **The one precondition this harness arranges for itself**, before
        // anything reads the window: see [`frontmost`] for why it is here and
        // not in the backend. Its answer decides only whether input may be
        // injected — nothing below it is gated on the keyboard.
        let activation = take_activation(&mut shell);

        let facts = appkit_agrees(size);
        input(&mut shell, window, size);
        match &activation {
            Ok(active) => {
                println!(
                    "crcbl appkit session: this process is frontmost and its window has the \
                     keyboard, so input can be injected: {active:?}"
                );
                warp_round_trip(&mut shell, window, size);
                injected_input(&mut shell, window, size);
            }
            Err(refused) => injection_skipped(refused),
        }
        clipboard(&mut shell, window);
        clipboard_peer(&mut shell, window);
        resize(&mut shell, window, facts.backing_scale);

        // **The whole placement before the flip**, so the way back can be judged
        // on more than its style mask. Position is invisible to the seam —
        // `WindowState` carries an extent and no origin — so AppKit is the only
        // party that can say whether a restore put the window back where it was.
        // `win32/window.rs` restores a whole `WINDOWPLACEMENT` for exactly this
        // reason and the Win32 suite asserts it for exactly this reason.
        let before_borderless = appkit_says("before going borderless").frame;

        // **Both flips run before either is judged**, and that is not a
        // loosening — every assertion below is made about the same snapshot it
        // was made about before, `WindowFacts` being owned data. It is a round
        // trip: the borderless origin is a known open defect, so asserting it
        // here would panic before the *restore* ever ran and throw away half the
        // evidence of a run that costs ten minutes. Both halves are gathered,
        // then both are checked.
        let borderless = flip(
            &mut shell,
            window,
            DisplayMode::Borderless { monitor: None },
        );
        println!("crcbl appkit session: borderless at {borderless:?}");
        // Judged against `NSScreen` rather than against the request: borderless
        // here is "a frameless window at screen size", and both halves of that
        // are things AppKit will say out loud.
        let covering = appkit_says("borderless");

        let windowed = flip(&mut shell, window, DisplayMode::Windowed);
        println!("crcbl appkit session: windowed again at {windowed:?}");
        let restored = appkit_says("windowed again");
        println!(
            "crcbl appkit session: the mode round trip — before {before_borderless:?}, \
             borderless {:?}, restored {:?}; first responder {} -> {} -> {}",
            covering.frame,
            restored.frame,
            facts.first_responder,
            covering.first_responder,
            restored.first_responder
        );

        assert!(
            !covering.titled,
            "a borderless window has no title bar; the style mask is {:#x}",
            covering.style_mask
        );
        // **The keyboard survives the flip**, asserted here and not only after
        // the round trip. `setStyleMask:` takes the first responder off the
        // content view, so a mode change used to leave the window swallowing
        // every keystroke — and a game goes borderless and *stays* there, so a
        // responder that came back only on the way out would be a game that is
        // deaf for exactly as long as anybody is playing it. An end-to-end check
        // would have passed that.
        assert_eq!(
            covering.first_responder, "CrcblView",
            "a borderless window still routes key events to the view: `sendEvent:` delivers to \
             the first responder, so the window being its own is every keystroke going nowhere. \
             {covering:?}"
        );
        let screen = covering
            .screen_frame
            .expect("a visible window is on a screen");
        // **The origin is the half of this the seam cannot see, and the half that
        // was wrong.** The first run to reach here reported the size exactly
        // right and the origin at (192, 160) — the window's *creation* origin,
        // `geometry::centred` of the requested size on the visible frame — which
        // is a screen-sized window hanging off two edges of the display. Every
        // earlier run printed `PhysicalSize { 1024, 768 }` and was happy, because
        // that is all `WindowState` carries. Only asking AppKit for the frame
        // shows it, which is the entire argument for this readback layer: **this
        // is the first defect it caught, and it is the kind only it can catch.**
        //
        // **Two candidate mechanisms are ruled out by evidence and neither
        // should be tried again.**
        //
        // * `constrainFrameRect:toScreen:`. `CrcblWindow` overrides it, the host
        //   test asserting the override is installed passes on the runner, and
        //   the window still comes out at (192, 160). Its default moves a window
        //   *down* to clear the menu bar, which was never this failure's shape.
        // * A corrupted `NSRect` argument. The backend's own trail reported
        //   `setFrame:display: from [192,256,512,416] asked [0,0,1024,768]
        //   landed [0,0,1024,768]` — the rectangle arrived intact and was
        //   applied, so the HFA-in-`v0`–`v3` theory is dead too.
        //
        // * **The pump**, a delegate callback, and macOS state restoration. The
        //   frame is wrong before the first pump, and `setRestorable:NO` took
        //   without changing the outcome.
        //
        // **It was `setPresentationOptions:`**, which returns every window of the
        // application to its *creation* frame — which is why the origin the
        // window reverted to was exactly `centred([0, 63, 1024, 674], 640x480)`,
        // the creation rectangle, on both axes. `apply_mode` now applies the
        // presentation options before the mask and the frame; `appkit::window`'s
        // module docs carry the measurement and the rule.
        assert_eq!(
            covering.frame, screen,
            "borderless covers the screen AppKit says it is on, exactly.\n\
             The window was at {before_borderless:?} before the flip, and everything AppKit says \
             about it while borderless is {covering:?}.\n\
             The mechanism was `setPresentationOptions:`, which returns every window of the \
             application to its **creation** frame — so `apply_mode` now applies the \
             presentation options before the style mask and the frame, and the frame is the \
             last geometry it sets. A failure here means that ordering has been undone or \
             something new below the arm repositions the window; the `apply_mode:` readings in \
             this test's stderr say which statement, because each of them prints the frame \
             after one step."
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

        assert!(
            restored.titled,
            "a windowed window has its title bar back; the style mask is {:#x}",
            restored.style_mask
        );
        // And the whole rectangle, not just the mask. The backend saves the frame
        // before it goes borderless and puts it back, so a window that was
        // somewhere before a mode round trip is in the same place after one —
        // which is what a player alt-tabbing out of fullscreen expects and what
        // nothing below AppKit could report.
        //
        // **This half was corrupted too and nobody could see it**, because the
        // borderless assertion above fired first for eight rounds. The restore
        // leg came back at `[192,160,640,512]` — the creation frame, moved *and*
        // resized — from the same `setPresentationOptions:` mechanism as the way
        // in. One bug, both directions, and this is the assertion that pins the
        // direction that had none.
        assert_eq!(
            restored.frame, before_borderless,
            "the way back restores the whole placement, origin included, and not merely the size"
        );
        // And the keyboard again, because the way back changes the style mask
        // too and therefore takes the responder a second time.
        assert_eq!(
            restored.first_responder, "CrcblView",
            "and the view still has the keyboard after a full mode round trip: {restored:?}"
        );

        // A second window, created hidden, is flipped borderless after the main
        // window's own round trip is fully asserted — so the regression this
        // guards cannot disturb a single one of the assertions above.
        hidden_window_stays_hidden(&mut shell);

        shell.destroy_window(window).expect("destroy_window");
        assert!(
            shell.window_state(window).is_err(),
            "a destroyed window is a stale handle"
        );
        println!("crcbl appkit session: a window opened, changed mode twice and closed");
    }

    /// The M2 half: the capability set, the pointer modes and the cursor.
    ///
    /// **Nothing here posts an event**, which is what keeps it on this side of
    /// the activation gate. Every assertion is either a property of the
    /// capability set or a call whose contract is that it fails only for a stale
    /// handle, so all of it runs on a runner that never activates. The half that
    /// needs the window server to answer is [`warp_round_trip`].
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

        // A warp to somewhere outside the window is **clamped, not refused** —
        // a seam claim that needs nobody to be listening, so it stays here.
        // Past the bottom-right corner, so the system's clamp to the display
        // arrangement lands it in the screen's corner.
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

    /// The warp, judged by where the **window system** says the pointer ended
    /// up.
    ///
    /// # The one piece of arithmetic in this slice that only a real window can check
    ///
    /// A position crosses **three** spaces on its way from the seam to
    /// CoreGraphics — the view's own Y-up points, AppKit's Y-up screen points,
    /// and Quartz's Y-down global pixels — and getting either reflection wrong
    /// puts the cursor the same distance on the *wrong side of the middle*. That
    /// is indistinguishable from a working warp for as long as the target
    /// happens to be centred, which is exactly what a hand-written fixture would
    /// pick.
    ///
    /// # The warp is only half the round trip, and the other half must be posted
    ///
    /// `CGWarpMouseCursorPosition` moves the cursor and generates no event, so
    /// there is nothing to read back from the warp itself. The seam converts
    /// `target` into Quartz's global space and puts the cursor there;
    /// `quartz::cursor` reads back the global point it chose;
    /// [`wait_for_pointer`] posts a real `kCGEventMouseMoved` *at that point*,
    /// and the backend converts what the window server delivers back into the
    /// seam's space. **Both conversions are therefore checked against each
    /// other** — a warp that put the cursor in the wrong place posts the move in
    /// the wrong place and comes back disagreeing with `target`.
    ///
    /// # Why this is behind the activation gate and [`input`] is not
    ///
    /// It posts. `CGEventPost` hands the event to the session and the session
    /// gives it to whatever is frontmost, so running this without the keyboard
    /// would move the cursor inside somebody else's application and then fail
    /// waiting for a report that was never coming to us — which is the whole
    /// failure M5 exists to stop. [`input`] posts nothing and stays unconditional.
    fn warp_round_trip(shell: &mut Box<dyn Shell>, window: WindowId, size: PhysicalSize) {
        let target =
            PhysicalPoint::new(f64::from(size.width) * 0.75, f64::from(size.height) * 0.50);
        shell.warp_pointer(window, target).expect("warp_pointer");
        let landed = wait_for_pointer(shell, quartz::cursor(), target);
        println!("crcbl appkit session: warped to {target:?} and landed at {landed:?}");
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

    /// Puts a logger behind the backend's own `log::` calls.
    ///
    /// # Instrumentation that cannot report is the same defect as a check that cannot fail
    ///
    /// `appkit::window::set_frame` reads the frame back after every placement
    /// and warns when the window did not go where it was put — and that warning
    /// was added, shipped, and **produced nothing**, because nothing in this
    /// process had ever installed a logger. `log::warn!` with no logger behind it
    /// is a discarded string. A whole CI round trip was spent on a diagnostic
    /// that was structurally incapable of speaking, which is exactly the shape
    /// this suite keeps finding in assertions and had not thought to look for in
    /// logging.
    ///
    /// # The filter is fixed here rather than read from the environment
    ///
    /// `Filter::from_env` would leave the session's own diagnosis depending on
    /// whether a CI job happened to set `CRCBL_LOG`, and the job does not. The
    /// backend's window and shell modules are turned up to `debug` and
    /// everything else left at `info`, so the placement trail is complete and
    /// nothing else buries it — `shell` is where `set_mode` brackets what
    /// `window` reports, and it contains no other `debug!` at all, so widening
    /// to it costs exactly one line of output. `try_init_logging` rather than
    /// `init_logging` because its `Result` says whether a logger was already
    /// installed, and ignoring that silently is how this went wrong the first
    /// time.
    fn install_logger() {
        let filter = Filter::parse(
            "info,crcbl_shell::appkit::window=debug,crcbl_shell::appkit::shell=debug",
        );
        match crcbl_core::log::try_init_logging(filter) {
            Ok(()) => println!(
                "crcbl appkit session: logging installed — backend warnings and the window \
                 module's placement trail follow on stderr"
            ),
            Err(error) => println!(
                "crcbl appkit session: a logger was already installed, so the backend's own \
                 account of where it put the window may be filtered out: {error}"
            ),
        }
    }

    /// Everything AppKit will say about **this session's own window**, or a
    /// failure naming the step that asked.
    ///
    /// The window is found by [`TITLE`] among `-[NSApp windows]`, so this is
    /// also the check that the window is still ours and still named: a title
    /// that changed is a lookup that fails, carrying every title that was there
    /// instead. That is what replaced an `assert_eq!` on the title, which could
    /// only have been made about a window already found by some other means.
    fn appkit_says(step: &str) -> session_support::WindowFacts {
        session_support::window_facts(TITLE).unwrap_or_else(|detail| {
            panic!(
                "crcbl appkit session: {step}: {detail}\n\
                 What the application says: {:?}",
                session_support::activation(TITLE)
            )
        })
    }

    /// Tries to make this process frontmost and its window key, and answers what
    /// AppKit said once it stopped trying.
    ///
    /// # Judged by the mechanism, not by a return value
    ///
    /// [`frontmost::ask`] answering `true` means a method returned `YES`, which
    /// is not the same as the session having activated anything. What is read
    /// instead is [`session_support::activation`], which is `-[NSApp isActive]`
    /// and `-[NSWindow isKeyWindow]` — the two things that decide where an
    /// injected event actually lands. That is the same standard
    /// `tests/win32_e2e.rs` holds `take_foreground` to, which judges by
    /// `GetForegroundWindow` and ignores what `SetForegroundWindow` returned.
    ///
    /// The lever is pulled once per turn rather than once, for the reason that
    /// suite gives too: a single refusal must not decide a test.
    ///
    /// `Ok` when both are true, `Err` carrying the last state otherwise —
    /// **which is not a failure**, only the fact that injection cannot proceed.
    /// See [`injection_skipped`].
    fn take_activation(
        shell: &mut Box<dyn Shell>,
    ) -> Result<session_support::Activation, session_support::Activation> {
        let started = Instant::now();
        let mut announced = false;
        loop {
            let asked = frontmost::ask();
            if !announced {
                announced = true;
                // Printed once, and it is the line that separates "the lever
                // does not exist on this system" from "the lever said yes and
                // the session did nothing" — two findings a bare timeout could
                // not tell apart.
                println!(
                    "crcbl appkit session: asked the session for activation; \
                     -[NSRunningApplication activateWithOptions:] answered {asked}"
                );
            }
            shell.pump(&mut |_event: ShellEvent| {});
            let state = session_support::activation(TITLE).unwrap_or_else(|detail| {
                panic!(
                    "crcbl appkit session: the shell is open and there is no NSApplication \
                     behind it: {detail}"
                )
            });
            // Both halves: an active application whose key window belongs to
            // somebody else sends this session's keystrokes to that window, and
            // a key window in an inactive application receives nothing at all.
            if state.app_active && state.is_key {
                return Ok(state);
            }
            if started.elapsed() >= DEADLINE {
                return Err(state);
            }
            shell.wait_events(Some(Duration::from_millis(20)));
        }
    }

    /// Says — at length, and where nobody can miss it — that the injected-input
    /// half of this session did not run, and why.
    ///
    /// # A skip that is quiet is worse than a failure
    ///
    /// `docs/plan/12-testing.md` names a silently-skipped end-to-end check as a
    /// known trap: it reports the same green as one that ran, so a path stops
    /// being covered and nothing says so. This is the opposite of that. It names
    /// what was skipped, names what is therefore unverified, and prints the
    /// [`session_support::Activation`] evidence inline so the next reader can
    /// tell the session's refusal from a defect of ours without another CI round
    /// trip.
    ///
    /// It does **not** fail the session, and that is deliberate: whether a CI
    /// runner activates an unbundled binary is not a property of this engine,
    /// and a red run here would say the backend is broken when it is not.
    fn injection_skipped(refused: &session_support::Activation) {
        // Which of the two refused, read off the evidence rather than assumed:
        // the observed case is an inactive application, and an active one whose
        // keyboard is elsewhere is a different sentence with the same
        // consequence.
        let mechanism = if refused.app_active {
            "the application is active but the keyboard is on another window"
        } else {
            "the application never became active"
        };
        // The half that says whose defect this is, and it is a genuine branch:
        // `can_become_key: false` on a `CrcblWindow` would be **ours** — the
        // `canBecomeKeyWindow` override missing from the class the window turned
        // out to be, which is a borderless window that silently stops taking
        // keystrokes. A CI run must not have to know which one it got.
        let whose = if refused.can_become_key {
            "can_become_key is true, so the CrcblWindow canBecomeKeyWindow override is installed \
             and answering: this is the session refusing to activate an unbundled binary, and no \
             backend can fix it"
        } else {
            "can_become_key is FALSE on our own window, which is a defect of this backend and not \
             the runner's refusal: the canBecomeKeyWindow override is not on the class the window \
             turned out to be, and a borderless window would silently stop receiving keystrokes"
        };
        println!(
            "crcbl appkit session: ============================================================\n\
             crcbl appkit session: INJECTED INPUT WAS NOT EXERCISED — {mechanism}, so no window \
             of this process holds it.\n\
             crcbl appkit session: CGEventPost puts an event in the *session's* stream and the \
             session gives it to whatever is frontmost, so posting now would type this test's \
             keystrokes into some other process on the runner rather than into our window.\n\
             crcbl appkit session: What AppKit says: {refused:?}\n\
             crcbl appkit session: {whose}. The harness asked for activation itself, once per \
             turn for {DEADLINE:?}, through -[NSRunningApplication activateWithOptions:] and \
             -[NSApp activateIgnoringOtherApps:], and was refused.\n\
             crcbl appkit session: SKIPPED: the injected key and its release, the TextCommit \
             behind interpretKeyEvents:, the arrow key, the window-server pointer motion, the \
             click, the wheel notch, and the warp round trip — which is skipped because \
             reading a warp back needs a posted mouse-moved, a warp generating no event of \
             its own. Nothing else: the capability set, the confine refusal, the pointer \
             modes, the cursors, the clipboard, pbcopy/pbpaste, the resize, both mode flips \
             and every AppKit readback all ran and asserted.\n\
             crcbl appkit session: STILL UNVERIFIED ON CI: interpretKeyEvents: and the whole \
             injected-input path. docs/backlog.md carries it as a gap, on the same terms as \
             the F11 pass that needs a renderer.\n\
             crcbl appkit session: ============================================================"
        );
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
    /// # None of it needs the window to be key, and M4 learned that the hard way
    ///
    /// The first version reached the window through `-[NSApp keyWindow]` and
    /// waited for one, so on a runner that never activates this whole function
    /// — the three switches included — never ran at all. Every field it reads is
    /// an accessor any `NSWindow` answers whether or not it has the keyboard, so
    /// it now reads them off this session's own window, found by title. Whether
    /// the window *is* key is reported by [`session_support::WindowFacts`] and
    /// gates only the injection.
    fn appkit_agrees(size: PhysicalSize) -> session_support::WindowFacts {
        let facts = appkit_says("the window AppKit knows about");
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
             types {:?}, scale {}, application active {}, window key {}",
            facts.first_responder,
            facts.accepts_mouse_moved,
            facts.dragged_types,
            facts.backing_scale,
            facts.app_active,
            facts.is_key
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
        session_support::resize_window(TITLE, POINTS.0, POINTS.1).expect("resize_window");

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
        let after = appkit_says("after the resize");
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
        // **The one place `-[NSApp keyWindow]` is still the question asked**,
        // because it is the one place the answer changes what may happen: the
        // session hands an injected event to whoever is frontmost. Reached only
        // after `take_activation` said both halves are true, so a failure here
        // is the keyboard moving between that check and this one.
        let before = session_support::key_window()
            .unwrap_or_else(|detail| panic!("crcbl appkit session: before injecting: {detail}"));
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
        //
        // **This is the call that failed on the first macOS run**, and the
        // reason is worth keeping next to it: the cursor is already inside the
        // window by now, so this warp crosses no boundary — no `mouseEntered:`,
        // and a warp generates no `mouseMoved:` either, so the old version waited
        // ten seconds for an event nothing had asked for.
        // [`wait_for_pointer`] posts the question now instead of assuming the
        // warp was one.
        let target =
            PhysicalPoint::new(f64::from(size.width) * 0.25, f64::from(size.height) * 0.25);
        shell.warp_pointer(window, target).expect("warp_pointer");
        let parked = wait_for_pointer(shell, quartz::cursor(), target);

        // **Identified by how far it went, not by being the last one.**
        // `wait_for_pointer` posts a move at the parked point on every turn
        // until one is reported, and the last of those can still be in flight
        // when this starts collecting — so "the most recent `PointerMotion`" can
        // be a stale report of `parked`, which would then be compared against
        // itself. `NUDGE` is far enough outside that ambiguity that a report of
        // it cannot be one of them, which is the same rule the Win32 suite
        // arrived at: on a live desktop, find your own event by its payload.
        //
        // The filter takes the **distance** and the assertions below take the
        // **direction**, deliberately: a backend that reflected the Y would
        // report a point `NUDGE.1` on the wrong side of `parked`, which this
        // admits and the assertion catches. Filtering on direction would have
        // hidden exactly the bug this is here for.
        let from = quartz::cursor();
        quartz::move_mouse_by(from, NUDGE);
        let reach = (NUDGE.0.abs() as f64 / 2.0, NUDGE.1.abs() as f64 / 2.0);
        let injected = |event: &ShellEvent| match event {
            ShellEvent::PointerMotion {
                abs: Some(at),
                raw_delta: Some(delta),
                ..
            } if (at.x - parked.x).abs() >= reach.0 && (at.y - parked.y).abs() >= reach.1 => {
                Some((*at, *delta))
            }
            _ => None,
        };
        let moved = collect_until(shell, "the injected motion", |seen| {
            seen.iter().any(|event| injected(event).is_some())
        });
        let (at, delta) = moved
            .iter()
            .rev()
            .find_map(injected)
            .expect("just waited for it");
        // **The asymmetry `appkit::pointer` exists to make visible, observed at
        // last.** `locationInWindow` is Y-up and is flipped into this seam's
        // Y-down space; the delta beside it is Quartz's own, which is Y-down
        // already and must *not* be flipped. So this move — right and up the
        // screen — has to come back as a larger window X, a **smaller** window
        // Y, and a delta whose second component is still negative. A backend
        // that reflected the delta as well as the position passes the first half
        // and fails the second, and the symptom in a game is an inverted
        // first-person camera.
        assert!(
            at.x > parked.x && at.y < parked.y,
            "the cursor was moved right and up the screen by {NUDGE:?} from {parked:?} and the \
             window reported {at:?}; the desktop is {:?} and the events were {:?}",
            quartz::cursor(),
            names(&moved)
        );
        assert!(
            delta.0 > 0.0 && delta.1 < 0.0,
            "the raw delta beside it is Quartz's own, unflipped: this move set \
             kCGMouseEventDeltaX/Y to {NUDGE:?} and the seam reported {delta:?}. (0, 0) means \
             the posted event carried no delta at all — CGEventCreateMouseEvent computes none, \
             which is what the run before this one found. A *positive* second component with \
             the position above correct is the delta being reflected into the position's Y-up \
             convention, which is an inverted camera. A negative *first* component is deltaX \
             and deltaY being read from each other's field."
        );
        // Proportion, on top of the signs, and it is scale-independent on
        // purpose: `-[NSEvent deltaX]` is documented in device-independent
        // points while the field set above is in the event stream's own units,
        // and nothing here can say whether a Retina host scales between the two.
        // A uniform factor is a fact about the display rather than a defect and
        // passes; one axis scaled differently from the other is a defect and
        // does not. The signs above have already ruled out reflection and swap,
        // so this is the remaining way the pair can be wrong.
        let asked = NUDGE.0 as f64 / NUDGE.1 as f64;
        let got = delta.0 / delta.1;
        assert!(
            (got - asked).abs() <= asked.abs() * 0.05,
            "the delta's two axes are out of proportion with each other: {NUDGE:?} was posted \
             and {delta:?} came back, a ratio of {got} where {asked} was asked for. The signs \
             are right, so this is one axis being scaled and the other not"
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
                assert!(
                    y != 0.0,
                    "a notch of one line is not zero lines, and a zero here is far more likely \
                     to be this harness than the backend: it means the amount never reached \
                     CGEventCreateScrollWheelEvent. `wheel1` is that function's last **named** \
                     parameter and only `wheel2`/`wheel3` are variadic, so a declaration that \
                     starts its `...` one parameter early passes the amount on the stack while \
                     the callee reads register w3 — which is exactly how this assertion first \
                     went red. Check the declaration before suspecting `appkit::events`."
                );
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

    /// Posts a mouse-moved at `post_at` until the window system reports where
    /// the pointer is, and answers the position it reported.
    ///
    /// A loop of its own rather than [`wait_for`], because this one needs the
    /// event's **payload** and that function's own pump has already discarded
    /// it by the time its predicate runs.
    ///
    /// # A warp is not an event, and the first macOS run proved it twice over
    ///
    /// This used to warp and then wait, which is a category error:
    /// **`CGWarpMouseCursorPosition` moves the cursor and posts nothing.** Apple
    /// documents it that way, and the run showed both halves of what that means
    /// in one log. [`input`] warps *outside* the window and then to a point
    /// inside it, and passed — because AppKit re-evaluates its tracking areas
    /// against where the cursor actually is, so crossing the boundary produces a
    /// `mouseEntered:` and therefore a `PointerFocus` with a position, warp or no
    /// warp. [`injected_input`] then warped from one point inside the window to
    /// another, crossed nothing, and waited ten seconds for an event that was
    /// never going to exist:
    ///
    /// ```text
    /// waited 10s for the pointer to be reported somewhere after a warp and
    /// nothing arrived; the events that did were ["MonitorsChanged"]
    /// ```
    ///
    /// So the warp is left to do its own job — moving the cursor, which is the
    /// thing [`Shell::warp_pointer`] promises — and the *question* is asked with
    /// a real `kCGEventMouseMoved` through the window server, which is the same
    /// machinery the click and the wheel already use. Posted every turn rather
    /// than once: `CGWarpMouseCursorPosition` suppresses local events for a
    /// short interval afterwards, and a single post swallowed by that window
    /// would be a flake rather than a finding.
    ///
    /// Either event answers. `PointerFocus` comes from the tracking area,
    /// registered `NSTrackingActiveAlways`, so it fires whether or not this
    /// process is frontmost; `PointerMotion` additionally needs the window to be
    /// **key**, because macOS sends mouse-moved events only to a key window that
    /// has asked for them.
    ///
    /// # `want` is what makes a report *this* report, and taking the freshest
    /// one instead was a flake
    ///
    /// This used to accept the first position of any value, on the reasoning
    /// that the freshest report describes where the cursor was just put. It does
    /// not, and CI showed it: `warp_round_trip` asked for x = 480 in a 640×480
    /// window and was told 320, which is the window's exact **centre** —
    /// `PointerMode::Locked` warps there before it freezes the cursor
    /// (`appkit::input::centre_pointer`), [`input`] leaves that lock behind
    /// without pumping, and AppKit re-evaluates its tracking areas against a
    /// warp. So a truthful report of where the cursor was one step ago was still
    /// in flight, arrived first, and was read as the answer to a question asked
    /// after it.
    ///
    /// Draining before the warp would not fix it — the window server delivers
    /// asynchronously, so the stale report need not have *arrived* yet by the
    /// time anything drains. The position it carries is the only thing that
    /// distinguishes it, which is the rule the Win32 suite reached first and the
    /// nudge below already follows: **on a live desktop, identify your own event
    /// by its payload.**
    ///
    /// It keeps its teeth in the direction that matters. A backend that converts
    /// the position wrongly reports the same wrong point every turn, never
    /// matches, and fails on the deadline below with every position it saw
    /// printed — the reflection this exists to catch cannot slip through, since
    /// [`POINTER_SLACK`] is a few pixels and a reflection is off by hundreds.
    fn wait_for_pointer(
        shell: &mut Box<dyn Shell>,
        post_at: quartz::Point,
        want: PhysicalPoint,
    ) -> PhysicalPoint {
        let started = Instant::now();
        let mut seen = Vec::new();
        let mut reported: Vec<PhysicalPoint> = Vec::new();
        loop {
            quartz::move_mouse(post_at.x, post_at.y);
            let mut landed = None;
            shell.pump(&mut |event: ShellEvent| {
                seen.push(event.name());
                match event {
                    ShellEvent::PointerFocus {
                        entered: true,
                        position: Some(at),
                        ..
                    }
                    | ShellEvent::PointerMotion { abs: Some(at), .. } => {
                        reported.push(at);
                        if (at.x - want.x).abs() <= POINTER_SLACK
                            && (at.y - want.y).abs() <= POINTER_SLACK
                        {
                            landed = Some(at);
                        }
                    }
                    _ => {}
                }
            });
            if let Some(at) = landed {
                return at;
            }
            assert!(
                started.elapsed() < DEADLINE,
                "crcbl appkit session: waited {DEADLINE:?} for the pointer to be reported at \
                 {want:?}. A kCGEventMouseMoved was posted at {post_at:?} on every turn of \
                 that wait; the positions that came back were {reported:?} and the events \
                 that arrived were {seen:?}. What AppKit says about this window: {:?}.\n\
                 Read the reported positions first, because they separate the mechanisms:\n\
                 * **Positions arrived, none of them {want:?}.** The conversion is wrong \
                 rather than absent. A Y that is the window's height minus the one asked \
                 for is a missing flip; an X that matches while the Y does not is the \
                 desktop reflection rather than the window one; both wrong by the window's \
                 origin is the screen's space reaching the seam unconverted.\n\
                 * **No positions at all, and nothing was posted.** \
                 CGWarpMouseCursorPosition generates no event, so a wait that relies on the \
                 warp alone sees nothing — that is what this function was rewritten to stop \
                 doing. If the posted move is also arriving nowhere, CGEventPost is being \
                 refused, which is TCC and which the injected keyboard above would have \
                 failed on first.\n\
                 * **No PointerFocus and no PointerMotion, with the post landing.** The \
                 NSTrackingArea is not delivering; it is registered NSTrackingActiveAlways \
                 and NSTrackingInVisibleRect, so it should fire even for an inactive \
                 application.\n\
                 * **A PointerFocus with no PointerMotion.** acceptsMouseMovedEvents is off, \
                 or the window is not key — read is_key in the facts above rather than \
                 guessing, because both are printed there.",
                session_support::activation(TITLE)
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

    /// A window hidden at creation must stay hidden across a mode change.
    ///
    /// `apply_mode` used to order the window front unconditionally in the
    /// borderless arm — `makeKeyAndOrderFront:` behind no `isVisible` check —
    /// so `set_mode(Borderless)` popped a hidden window on screen and took key
    /// focus with it, and `window_state().visible` reported true for a window
    /// nobody showed. Creation guards its own show behind `desc.visible` and
    /// the Win32 sibling carries `WS_VISIBLE` across its style change; this
    /// step pins the same guarantee on AppKit, which reads `isVisible` for the
    /// truth.
    ///
    /// A second window, deliberately: the main window is visible and key for
    /// the whole session, so only a separate one can carry hidden state. Its
    /// title is distinct, because [`session_support::window_facts`] finds
    /// windows by title.
    ///
    /// Visibility is judged through the seam (`window_state().visible`), and
    /// the keyboard through AppKit's own `isKeyWindow` readback — the half
    /// [`session_support::window_facts`] answers without any activation.
    fn hidden_window_stays_hidden(shell: &mut Box<dyn Shell>) {
        const HIDDEN_TITLE: &str = "crcbl appkit session — hidden mode flip";
        let hidden = shell
            .create_window(&WindowDesc {
                title: HIDDEN_TITLE,
                app_id: "sh.kryptic.crcbl.appkit-session",
                size: REQUESTED,
                constraints: SizeConstraints::min(LogicalSize::new(320.0, 180.0)),
                mode: DisplayMode::Windowed,
                resizable: true,
                visible: false,
                accept_drops: false,
            })
            .expect("create_window");

        // Creation must not show it.
        assert!(
            !shell.window_state(hidden).expect("state").visible,
            "a window created visible: false is not on screen"
        );

        // The flip, and a pump until the window system has honoured it.
        let _ = flip(shell, hidden, DisplayMode::Borderless { monitor: None });
        assert!(
            !shell.window_state(hidden).expect("state").visible,
            "set_mode(Borderless) must not show a hidden window: apply_mode used to order it \
             front unconditionally, so window_state().visible reported true for a window nobody \
             showed"
        );
        let facts = session_support::window_facts(HIDDEN_TITLE)
            .expect("the hidden window is still this process's to read");
        assert!(
            !facts.is_key,
            "a hidden window takes no key focus across a mode change: {facts:?}"
        );

        shell.destroy_window(hidden).expect("destroy_window");
        println!(
            "crcbl appkit session: a window hidden at creation stayed hidden across a mode flip"
        );
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
    // M5: the activation a harness may arrange and a backend may not
    // -----------------------------------------------------------------------

    /// Asking the session to make **this** process frontmost.
    ///
    /// # Decision: this is a harness, so it is not in `src/appkit/`
    ///
    /// `tests/win32_e2e.rs`'s `desktop::take_foreground` states the rule for the
    /// other platform and it is the same rule here: **a game does not get to
    /// steal the focus.** A backend that knew how to force itself frontmost is a
    /// backend that could do it to a user mid-sentence, so
    /// `appkit::app::activate` does the polite thing — `-[NSApp activate]` where
    /// it exists, `activateIgnoringOtherApps:` before that — and stops. What is
    /// here is what an automated harness does to arrange a precondition a human
    /// would have arranged by clicking on the window, and it belongs on this side
    /// of the seam for exactly that reason.
    ///
    /// # Why `NSRunningApplication` and not more of what the backend already did
    ///
    /// The backend's own activation runs on every `open()` and the runner still
    /// reported `app_active: false`. On macOS 14 `-[NSApplication activate]` is
    /// *cooperative*: it asks, and an application that is not entitled to
    /// interrupt whatever is frontmost does not get to. `respondsToSelector:`
    /// finds that selector on any modern system, so the backend takes that branch
    /// and never reaches the forceful one.
    ///
    /// `-[NSRunningApplication activateWithOptions:]` is the lever from the other
    /// side — the process object rather than the application object — and
    /// `NSApplicationActivateIgnoringOtherApps` is the bit that says "do it
    /// anyway". Both spellings are deprecated as of macOS 14 and neither is
    /// removed; deprecated is a compiler diagnostic in Objective-C and nothing at
    /// all across `objc_msgSend`. The legacy `activateIgnoringOtherApps:` is sent
    /// as well, because it is the branch the backend skips and it costs one
    /// message.
    ///
    /// **Every selector is checked with `respondsToSelector:` first.** Sending
    /// one a system does not have is a crash, not a graceful nothing, and this
    /// crate has no deployment target to reason from — the same discipline
    /// `appkit::ffi::responds_to` exists for.
    ///
    /// # Hand-written FFI rather than the backend's
    ///
    /// `crcbl_shell::appkit::ffi` is `pub(crate)`, rightly, and [`quartz`] gives
    /// the argument in full: a harness that drove the backend through the
    /// backend's own ABI declarations would be grading its own homework. So this
    /// declares the three runtime entry points it needs and transmutes
    /// `objc_msgSend` per call site, which is the only form of the call that is
    /// correct on `aarch64-apple-darwin` — see [`msg_send`].
    mod frontmost {
        use core::ffi::{CStr, c_char, c_void};

        /// Any Objective-C object pointer: `id`, and also `Class`.
        type Id = *mut c_void;
        /// `SEL`.
        type Sel = *const c_void;
        /// `BOOL`, as `i8`: it is `signed char` on x86_64 and `_Bool` on arm64,
        /// and only the second has exactly two valid bit patterns — so reading
        /// one straight into a Rust `bool` would be undefined behaviour the day
        /// a method answered `2`.
        type ObjcBool = i8;

        /// `NSApplicationActivateAllWindows` — bring this process's other
        /// windows up with it, rather than only the frontmost one.
        const ACTIVATE_ALL_WINDOWS: usize = 1 << 0;
        /// `NSApplicationActivateIgnoringOtherApps` — the bit that makes this a
        /// harness's call and not a well-behaved application's.
        const ACTIVATE_IGNORING_OTHER_APPS: usize = 1 << 1;

        #[link(name = "objc")]
        unsafe extern "C" {
            fn objc_getClass(name: *const c_char) -> Id;
            fn sel_registerName(name: *const c_char) -> Sel;
            /// The dispatch trampoline. **Never called through this
            /// declaration** — see [`msg_send`].
            fn objc_msgSend();
            fn objc_autoreleasePoolPush() -> *mut c_void;
            fn objc_autoreleasePoolPop(pool: *mut c_void);
        }

        /// A scope in which autoreleased objects are valid.
        ///
        /// **`+[NSRunningApplication currentApplication]` returns an
        /// autoreleased object**, and [`ask`] is called once per turn of a poll
        /// that can run for the whole deadline. With no pool the runtime logs
        /// `autoreleased with no pool in place — just leaking` and does exactly
        /// that, several hundred times, which buries the session's own narrative
        /// in the one log a reader has. The backend's `appkit::ffi::Pool` is
        /// `pub(crate)` and this is a harness, so this is the same two runtime
        /// calls declared here.
        struct Pool(*mut c_void);

        impl Pool {
            /// Pushes a pool, popped by [`Drop`] in the reverse order.
            fn push() -> Self {
                // SAFETY: the runtime's own pool stack; the token is opaque and
                // is handed back unmodified in `drop`.
                Self(unsafe { objc_autoreleasePoolPush() })
            }
        }

        impl Drop for Pool {
            fn drop(&mut self) {
                // SAFETY: `self.0` is the token this pool pushed and has not
                // been popped before. Rust's drop order gives the nesting the
                // runtime requires.
                unsafe { objc_autoreleasePoolPop(self.0) };
            }
        }

        /// `objc_msgSend`, transmuted to the exact signature of the method being
        /// sent.
        ///
        /// **There is no variadic ABI to declare it with on
        /// `aarch64-apple-darwin`.** Apple's arm64 convention passes variadic
        /// arguments on the stack and ordinary ones in registers, and
        /// `objc_msgSend` is a trampoline that must be called with the signature
        /// of the method it dispatches to, so that every argument is already in
        /// the register the implementation reads it from. Declaring it
        /// `fn(Id, Sel, ...)` compiles, links, runs, and hands the method
        /// whatever happened to be lying in that register. The same hazard
        /// [`quartz`](super::quartz) documents for
        /// `CGEventCreateScrollWheelEvent`, arriving through the runtime instead.
        ///
        /// # Safety
        ///
        /// `F` must be a function pointer type whose signature is exactly the
        /// receiver, the selector, the method's arguments and its return type.
        unsafe fn msg_send<F: Copy>() -> F {
            assert!(
                size_of::<F>() == size_of::<*const c_void>(),
                "msg_send's type parameter must be a function pointer"
            );
            // SAFETY: `F` is a function pointer of pointer size, asserted above,
            // and the value copied into it is the address of a real function.
            // The caller carries the obligation that the signature matches.
            unsafe { core::mem::transmute_copy(&(objc_msgSend as *const c_void)) }
        }

        /// The selector for `name`, interned by the runtime.
        fn sel(name: &CStr) -> Sel {
            // SAFETY: a NUL-terminated C string that outlives the call; the
            // result is a pointer into the runtime's own table.
            unsafe { sel_registerName(name.as_ptr()) }
        }

        /// Whether `receiver` implements `selector`.
        ///
        /// # Safety
        ///
        /// `receiver` must be a live object; every object answers this.
        unsafe fn responds_to(receiver: Id, selector: Sel) -> bool {
            // SAFETY: `respondsToSelector:` is on `NSObject`, so every object
            // implements it with this signature.
            let send: unsafe extern "C" fn(Id, Sel, Sel) -> ObjcBool = unsafe { msg_send() };
            unsafe { send(receiver, sel(c"respondsToSelector:"), selector) != 0 }
        }

        /// Pulls both levers once, and answers whether
        /// `activateWithOptions:` said yes.
        ///
        /// **That answer is not the judgment** and no caller should treat it as
        /// one: a method returning `YES` is not the session having activated
        /// anything. `take_activation` reads `-[NSApp isActive]` afterwards
        /// instead, exactly as the Win32 suite judges `take_foreground` by
        /// `GetForegroundWindow`. What the return value is good for is telling
        /// "this system has no such lever" apart from "the lever was pulled and
        /// nothing moved", which are different findings and would otherwise both
        /// arrive as a timeout.
        #[must_use]
        pub fn ask() -> bool {
            let _pool = Pool::push();
            // SAFETY: every call below is a class lookup, a `respondsToSelector:`
            // or a message whose signature is written at its own call site.
            // `currentApplication` and `sharedApplication` are class methods
            // returning objects the runtime owns for the life of the process,
            // and each is null-checked before anything is sent to it.
            unsafe {
                let running = objc_getClass(c"NSRunningApplication".as_ptr());
                let application = objc_getClass(c"NSApplication".as_ptr());
                if running.is_null() || application.is_null() {
                    return false;
                }
                let class_method: unsafe extern "C" fn(Id, Sel) -> Id = msg_send();

                // The legacy, forceful spelling on `NSApp`. The backend skips
                // this branch on any system new enough to have `activate`, so
                // sending it here is not a repeat of what `open()` already did.
                let app = class_method(application, sel(c"sharedApplication"));
                let ignoring = sel(c"activateIgnoringOtherApps:");
                if !app.is_null() && responds_to(app, ignoring) {
                    let send: unsafe extern "C" fn(Id, Sel, ObjcBool) = msg_send();
                    send(app, ignoring, 1);
                }

                // And the process object's own, which is the lever this module
                // exists for.
                let current = class_method(running, sel(c"currentApplication"));
                let with_options = sel(c"activateWithOptions:");
                if current.is_null() || !responds_to(current, with_options) {
                    return false;
                }
                let activate: unsafe extern "C" fn(Id, Sel, usize) -> ObjcBool = msg_send();
                activate(
                    current,
                    with_options,
                    ACTIVATE_ALL_WINDOWS | ACTIVATE_IGNORING_OTHER_APPS,
                ) != 0
            }
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

        // The `CGEventField` numbers this harness sets. **Every one of them is
        // zero on an event `CGEventCreateMouseEvent` just built**, which is the
        // whole reason they are here: that function places an event at an
        // absolute location and computes nothing else about it.

        /// `kCGMouseEventClickState` — which click of a multiple-click this is.
        ///
        /// **Not read by this seam**, which takes the button from
        /// `buttonNumber` alone, so nothing asserted here depends on it. Set
        /// anyway because a press whose click state is zero is not what a real
        /// click looks like, and AppKit is entitled to act on that rather than
        /// on nothing — which would be a defect of the harness reported as one
        /// of the backend.
        const MOUSE_CLICK_STATE: u32 = 1;
        /// `kCGMouseEventDeltaX`.
        const MOUSE_DELTA_X: u32 = 4;
        /// `kCGMouseEventDeltaY`.
        const MOUSE_DELTA_Y: u32 = 5;
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
            /// **Partly variadic, and the boundary is the whole point.** The C
            /// signature is:
            ///
            /// ```c
            /// CGEventRef CGEventCreateScrollWheelEvent(CGEventSourceRef source,
            ///                                          CGScrollEventUnit units,
            ///                                          CGWheelCount wheelCount,
            ///                                          int32_t wheel1, ...);
            /// ```
            ///
            /// **`wheel1` is a named parameter.** Only `wheel2` and `wheel3` are
            /// variadic, and getting that boundary wrong is not a diagnostic —
            /// it is a wrong-register read at run time. On
            /// `aarch64-apple-darwin` — the runner's architecture and every
            /// Mac's since 2020 — variadic arguments go on the **stack** while
            /// named ones go in registers, so declaring `wheel1` inside the
            /// variadic list puts the amount on the stack while the callee reads
            /// it from `w3`.
            ///
            /// That is not hypothetical: this was declared with `...` starting
            /// one parameter too early, and the macOS run scrolled by whatever
            /// happened to be in that register, which was **zero** —
            /// `a notch of one line is not zero lines`. The previous comment here
            /// described this exact hazard and then committed it, which is the
            /// part worth remembering: `appkit::ffi` documents it at length for
            /// `objc_msgSend` and everybody was watching the Objective-C
            /// dispatch, while the defect arrived through a plain C function
            /// nobody had counted the parameters of.
            ///
            /// Posting one axis means the variadic list is **empty**, which the
            /// declaration still has to say correctly — an empty list is not the
            /// same as no list.
            fn CGEventCreateScrollWheelEvent(
                source: *mut c_void,
                units: u32,
                wheels: u32,
                wheel1: i32,
                ...
            ) -> EventRef;
            fn CGEventPost(tap: u32, event: EventRef);
            /// Sets one integer-valued field on an event before it is posted.
            ///
            /// The only way to put a **relative delta** on a synthesized mouse
            /// move: `CGEventCreateMouseEvent` takes an absolute location and
            /// leaves `kCGMouseEventDeltaX`/`Y` at zero, and `-[NSEvent deltaX]`
            /// reads exactly those fields.
            fn CGEventSetIntegerValueField(event: EventRef, field: u32, value: i64);
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
        ///
        /// **The one field this leaves at its default is the right one.**
        /// `kCGKeyboardEventAutorepeat` is zero on a freshly built event and
        /// `-[NSEvent isARepeat]` reads it, which is what
        /// [`injected_input`](super::injected_input) asserts about the first
        /// press — so the default is the value under test rather than an
        /// oversight. Checked when the mouse delta turned out not to be.
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

        /// Moves the cursor to a point in Quartz's global space, carrying **no**
        /// relative delta.
        ///
        /// Which is honest rather than convenient: the event genuinely says the
        /// pointer did not travel, and this is used to ask where the pointer is
        /// rather than to claim it moved. [`move_mouse_by`] is the one that
        /// makes a claim about travel.
        pub fn move_mouse(x: f64, y: f64) {
            mouse(MOUSE_MOVED, Point { x, y }, &[]);
        }

        /// Moves the cursor `delta` from `from`, and **says so on the event**.
        ///
        /// # `CGEventCreateMouseEvent` does not compute a delta, and that cost a round trip
        ///
        /// It places an event at an absolute location and leaves
        /// `kCGMouseEventDeltaX`/`Y` at zero. `-[NSEvent deltaX]` reads exactly
        /// those fields, so a backend passing them through faithfully — which
        /// `appkit::view` does, straight into `raw_delta` — reports the zero it
        /// was handed. The first run to reach this asserted the delta was
        /// positive in both axes and got `(0.0, 0.0)`.
        ///
        /// **The assertion was checking a value this harness had never
        /// supplied**, so it could only ever have passed by accident, from some
        /// unrelated movement on the runner's desk. Setting the field turns it
        /// into a real check: the window server is told a known delta, and the
        /// seam can be held to reporting *that* rather than merely something
        /// non-zero.
        pub fn move_mouse_by(from: Point, delta: (i64, i64)) {
            let at = Point {
                x: from.x + delta.0 as f64,
                y: from.y + delta.1 as f64,
            };
            mouse(
                MOUSE_MOVED,
                at,
                &[(MOUSE_DELTA_X, delta.0), (MOUSE_DELTA_Y, delta.1)],
            );
        }

        /// A press and its release, both at the same point, each carrying a
        /// click state of one.
        pub fn click(x: f64, y: f64) {
            let at = Point { x, y };
            let single = &[(MOUSE_CLICK_STATE, 1)];
            mouse(LEFT_DOWN, at, single);
            mouse(LEFT_UP, at, single);
        }

        /// One mouse event, with `fields` set on it before it is posted.
        fn mouse(kind: u32, at: Point, fields: &[(u32, i64)]) {
            // SAFETY: as in `key`; `at` is a `CGPoint` passed by value, and each
            // entry of `fields` is a documented `CGEventField` of a mouse event
            // set on the live event before it is posted.
            unsafe {
                let event = CGEventCreateMouseEvent(core::ptr::null_mut(), kind, at, BUTTON_LEFT);
                if event.is_null() {
                    return;
                }
                for (field, value) in fields {
                    CGEventSetIntegerValueField(event, *field, *value);
                }
                CGEventPost(HID_TAP, event);
                CFRelease(event);
            }
        }

        /// Scrolls `lines` notches on the vertical axis.
        ///
        /// **Nothing here rests on a default.** The unit is an argument of the
        /// constructor rather than a field left unset, and it is the whole of
        /// what decides `hasPreciseScrollingDeltas` at the far end and therefore
        /// `ScrollDelta::Lines` against `Pixels`; the per-axis amount is the
        /// variadic argument below. Checked alongside the mouse delta, which did
        /// rest on one.
        pub fn scroll_lines(lines: i32) {
            // SAFETY: as in `key`. `wheels: 1` promises exactly one axis, so
            // `lines` is `wheel1` — the last **named** parameter — and the
            // variadic list is empty, which is what one axis means. Passing it
            // as `i32` is the declaration's `int32_t` rather than a widened
            // integer literal.
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
