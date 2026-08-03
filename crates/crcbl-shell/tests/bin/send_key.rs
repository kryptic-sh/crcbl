//! Types keys into whatever the compositor has focused — a *different* process.
//!
//! ```text
//! crcbl-e2e-key < a stream of evdev codes, one per line
//! ```
//!
//! **Compiled only with the `wayland-e2e` feature**, which nothing but
//! `tests/run-wayland-e2e.sh` turns on. It is a `[[bin]]` rather than part of a
//! test target because the thing it drives is another program: the sandbox
//! running under the same sway, whose `F11` handler is the one path in the
//! display-mode story that no in-process test can reach.
//!
//! # Why a whole process, and why it does not steal focus
//!
//! [`VirtualInput`] needs a Wayland connection, and it takes one the only way
//! the seam allows: out of a window's
//! [`SurfaceTarget`](crcbl_shell::SurfaceTarget). So this creates a window —
//! and then deliberately never presents to it.
//!
//! That is not a shortcut, it is the mechanism. A Wayland surface is mapped
//! exactly while it has a buffer, and nothing here attaches one, so sway never
//! puts this window in its tree: it cannot be focused, cannot be tiled, and
//! cannot take the keyboard away from the process under test. The same fact
//! `run-wayland-e2e.sh` documents as the reason the null GPU backend gets no
//! mode assertion is what makes this helper invisible.
//!
//! # Why it reads a stream instead of taking the keys as arguments
//!
//! **The seat has to have a keyboard before the program under test binds it.**
//! A `wl_seat` on a headless compositor starts with no capabilities; a client
//! only calls `wl_seat.get_keyboard` when it sees the keyboard capability
//! arrive, and until it has that object the compositor has nowhere to deliver a
//! key to. A sender that attached its virtual keyboard and immediately typed
//! would race the game's reaction to the hotplug, and the key would land in the
//! gap — silently, because a key nobody is listening for is not an error.
//!
//! So this attaches first and then blocks on stdin. The harness starts it,
//! starts the game against a seat that already has a keyboard, waits for the
//! window to be mapped and focused, and only then writes a code. Closing the
//! stream unplugs the devices and exits.
//!
//! Keys are **evdev** codes, the numbering `wl_keyboard.key` reports and
//! `crcbl-shell`'s own `linux/keymap.rs` translates: `KEY_F11` is 87. Naming
//! them here would mean a second table to keep in step with that one.
//!
//! # Linux only, and it says so out loud
//!
//! `wayland_test_support` is `#[cfg(target_os = "linux")]`, and `--all-features`
//! turns `wayland-e2e` on everywhere — so this target is built on macOS,
//! Windows and `wasm32` by CI's `--all-targets --all-features` lint jobs, which
//! is how it first went red on three of them at once. The other platforms get a
//! `main` that fails and names the reason rather than a `cfg` that quietly
//! compiles to nothing: a helper that reports success on a platform where it
//! cannot possibly have typed anything is the failure this whole harness is
//! trying to avoid.

#[cfg(target_os = "linux")]
use std::io::{BufRead, Write};
use std::process::ExitCode;

#[cfg(target_os = "linux")]
use crcbl_shell::wayland_test_support::VirtualInput;
#[cfg(target_os = "linux")]
use crcbl_shell::{LogicalSize, ShellBackend, WindowDesc, open_backend};

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!(
        "crcbl-e2e-key: Wayland is a Linux window system; there is no seat here to plug \
         a keyboard into"
    );
    ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    crcbl_core::log::init_logging();

    // `open_backend` rather than `open`: this helper is only meaningful against
    // a compositor, and a silent fallback to another backend would type into
    // nothing and report success.
    let mut shell = match open_backend(ShellBackend::Wayland) {
        Ok(shell) => shell,
        Err(error) => {
            eprintln!("crcbl-e2e-key: no compositor: {error}");
            return ExitCode::FAILURE;
        }
    };
    let window = match shell.create_window(&WindowDesc {
        title: "crcbl e2e key sender",
        app_id: "sh.kryptic.crcbl.e2e.keys",
        // Never mapped, so this size is never used. It still has to be legal.
        size: LogicalSize::new(64.0, 64.0),
        ..WindowDesc::default()
    }) {
        Ok(window) => window,
        Err(error) => {
            eprintln!("crcbl-e2e-key: could not create a window: {error}");
            return ExitCode::FAILURE;
        }
    };

    let input = match VirtualInput::attach(&*shell, window) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("crcbl-e2e-key: no virtual devices: {error}");
            return ExitCode::FAILURE;
        }
    };
    // The line the harness waits for before it starts the program under test:
    // from here on the seat has a keyboard, so nothing that binds it afterwards
    // can miss the hotplug.
    //
    // Flushed by hand, every time: stdout is a pipe here, so it is
    // block-buffered, and a reader waiting on a line that is sitting in this
    // process's buffer waits forever.
    say("ready");

    for line in std::io::stdin().lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("crcbl-e2e-key: could not read stdin: {error}");
                return ExitCode::FAILURE;
            }
        };
        let code = line.trim();
        if code.is_empty() {
            continue;
        }
        let Ok(evdev) = code.parse::<u32>() else {
            eprintln!("crcbl-e2e-key: {code:?} is not an evdev code (KEY_F11 is 87)");
            return ExitCode::from(2);
        };
        input.tap(evdev);
        // The compositor has *processed* the tap once it answers this, which is
        // stronger than the flush `tap` already did: a flush only means the
        // bytes left this process, and the client they are meant for is another
        // program that the harness is about to start watching.
        input.sync();
        say(&format!("tapped {evdev}"));
    }
    ExitCode::SUCCESS
}

/// Says something the harness may be blocked waiting to read.
#[cfg(target_os = "linux")]
fn say(what: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "crcbl-e2e-key: {what}");
    let _ = out.flush();
}
