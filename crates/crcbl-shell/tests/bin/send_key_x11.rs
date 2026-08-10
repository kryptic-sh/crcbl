//! Types keys at whatever the X server is delivering them to — a *different*
//! process.
//!
//! ```text
//! crcbl-e2e-x11-key <pointer-x> <pointer-y> <wm-class> < a stream of X11 keycodes and `close`, one per line
//! ```
//!
//! **Compiled only with the `x11-e2e` feature**, which nothing but
//! `tests/run-x11-e2e.sh` turns on. The X11 counterpart of
//! `tests/bin/send_key_wayland.rs`, and a `[[bin]]` for the same reason: the thing it
//! drives is another program — the sandbox running against the same display,
//! whose `F11` handler is the one part of the display-mode story no in-process
//! test can reach.
//!
//! # Why this one needs no window, and the Wayland one does
//!
//! `XTEST` is a server extension: `xcb_test_fake_input` asks the *server* to
//! generate an event as though a device had, and it routes through the ordinary
//! focus rules from there. Any client may call it, so this needs a connection
//! and nothing else. Wayland has no such thing — a key comes from a seat, and a
//! seat needs a device to plug into — which is why its sender has to create a
//! window it never presents to.
//!
//! # Where the key goes, and why the pointer is warped first
//!
//! With **no window manager** the input focus is left at its default of
//! `PointerRoot`, which means keys are delivered to the window under the
//! pointer. So this parks the pointer inside the game's window before typing:
//! without that the key goes to the root and nothing is listening, silently.
//! The position is the caller's, because only the harness knows where the
//! window it started ended up.
//!
//! With a **window manager** the focus is the manager's to give, it gives it to
//! the window it just mapped, and the warp is harmless — the key follows the
//! focus rather than the pointer.
//!
//! # Why it reads a stream instead of taking the keys as arguments
//!
//! Not for the Wayland sender's reason — there is no hotplug to race here — but
//! for the harness's: it has to type *after* the game is up and has reported
//! itself windowed, and a process that took its keys as arguments would have to
//! be started at that moment and would race its own connection setup. Reading a
//! stream lets the harness start it once and decide when.
//!
//! Keys are **X11 keycodes**, which are evdev codes plus eight — the number
//! `xev` prints. `KEY_F11` is evdev 87 and X11 95.
//!
//! # Closing the game, not killing it
//!
//! The line `close` makes the sender find the sandbox's window — the third
//! argument, the instance half of its `WM_CLASS` — and send it
//! `WM_DELETE_WINDOW`, exactly as a window manager's close button would. The
//! sandbox answers the question, tears down, and prints its end-of-run summary,
//! which is what the harness asserts. That is the difference between a game
//! that ended on its own terms and a SIGTERM that left the extent after F11
//! never checked; see `run_sandbox_toggle` in `tests/run-x11-e2e.sh`.
//!
//! # Linux only, and it says so out loud
//!
//! `x11_test_support` is `#[cfg(target_os = "linux")]` and `--all-features`
//! turns `x11-e2e` on everywhere, so this target is built on macOS, Windows and
//! `wasm32` by CI's `--all-targets --all-features` lint jobs. They get a `main`
//! that fails and names the reason rather than a `cfg` that quietly compiles to
//! nothing: a helper reporting success on a platform where it cannot have typed
//! anything is the failure this harness exists to avoid.

#[cfg(target_os = "linux")]
use std::io::{BufRead, Write};
use std::process::ExitCode;

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!(
        "crcbl-e2e-x11-key: X11 is a Linux window system here; there is no display to type at"
    );
    ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    crcbl_core::log::init_logging();

    let mut args = std::env::args().skip(1);
    let (Some(x), Some(y), Some(class)) = (args.next(), args.next(), args.next()) else {
        eprintln!("crcbl-e2e-x11-key: usage: crcbl-e2e-x11-key <pointer-x> <pointer-y> <wm-class>");
        return ExitCode::from(2);
    };
    let (Ok(x), Ok(y)) = (x.parse::<i16>(), y.parse::<i16>()) else {
        eprintln!("crcbl-e2e-x11-key: the pointer position must be two screen coordinates");
        return ExitCode::from(2);
    };

    let Some(mut peer) = crcbl_shell::x11_test_support::Peer::new() else {
        eprintln!("crcbl-e2e-x11-key: no display, or libxcb-xtest is missing");
        return ExitCode::FAILURE;
    };
    // See the module docs: with no window manager this is what decides which
    // window the key reaches.
    peer.motion(x, y);
    say(&format!("ready at {x},{y}"));

    for line in std::io::stdin().lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("crcbl-e2e-x11-key: could not read stdin: {error}");
                return ExitCode::FAILURE;
            }
        };
        let code = line.trim();
        if code.is_empty() {
            continue;
        }
        if code == "close" {
            // A clean close instead of a SIGTERM: the sandbox answers
            // `WM_DELETE_WINDOW`, tears down, and prints the summary the
            // harness asserts. See the module docs.
            match peer.find_window(&class) {
                Some(xid) => {
                    peer.request_close(xid);
                    say(&format!("closing {xid}"));
                }
                None => {
                    eprintln!(
                        "crcbl-e2e-x11-key: no window has WM_CLASS instance {class:?}; \
                         nothing to close"
                    );
                    return ExitCode::FAILURE;
                }
            }
            continue;
        }
        let Ok(keycode) = code.parse::<u8>() else {
            eprintln!("crcbl-e2e-x11-key: {code:?} is not an X11 keycode (KEY_F11 is 95)");
            return ExitCode::from(2);
        };
        peer.key(keycode, true);
        peer.key(keycode, false);
        say(&format!("tapped {keycode}"));
    }
    ExitCode::SUCCESS
}

/// Says something the harness may be blocked waiting to read.
#[cfg(target_os = "linux")]
fn say(what: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "crcbl-e2e-x11-key: {what}");
    let _ = out.flush();
}
