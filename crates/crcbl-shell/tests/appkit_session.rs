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

fn main() {
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
        DisplayMode, LogicalSize, PhysicalSize, Shell, ShellBackend, ShellEvent, SizeConstraints,
        WindowDesc, WindowId, open_backend,
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
