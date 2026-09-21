//! Walks a person through real keyboard, mouse and drag input, and checks what
//! the Win32 shell reports for each.
//!
//! ```text
//! cargo run -p crcbl-shell --features win32-e2e --bin crcbl-e2e-win32-hands-on
//! ```
//!
//! **Compiled only with the `win32-e2e` feature**, like the two senders beside
//! it. Everything `win32_e2e` proves, it proves with injected input: `SendInput`
//! from another process, a hand-built `DROPFILES`, `InjectTouchInput`. This
//! program covers what injection cannot reach: a real keyboard's `lParam`, the
//! driver's typematic repeat, a physical wheel detent, raw motion from a real
//! mouse, and a drag the shell itself starts from Explorer.
//!
//! # The instructions are in the window's title bar
//!
//! Each step is shown in the title of the window it is checking and printed to
//! stdout, so the person doing the steps can follow along without watching a
//! terminal. **Escape skips a step** that this desk cannot do (a mouse with no
//! side buttons, say), and a step nobody completes times out rather than
//! hanging. The exit status is non-zero if any step failed; skipped and
//! timed-out steps are reported but do not fail the run, because they are
//! answers about the desk rather than about the backend.
//!
//! # Windows only, and it says so out loud
//!
//! As with the senders, `--all-features` builds this on every target, and the
//! non-Windows `main` fails and names the reason instead of passing.

use std::process::ExitCode;

#[cfg(not(target_os = "windows"))]
fn main() -> ExitCode {
    eprintln!("crcbl-e2e-win32-hands-on: this checks the Win32 shell; there is no Win32 here");
    ExitCode::FAILURE
}

#[cfg(target_os = "windows")]
fn main() -> ExitCode {
    windows::run()
}

#[cfg(target_os = "windows")]
mod windows {
    use std::process::ExitCode;
    use std::time::{Duration, Instant};

    use crcbl_shell::{
        ButtonState, KeyCode, Keysym, LogicalSize, PointerButton, ScrollDelta, Shell, ShellBackend,
        ShellEvent, WindowDesc, WindowId,
    };

    /// How long a step waits for the person before it is reported as timed out.
    const STEP_WAIT: Duration = Duration::from_secs(60);

    /// PS/2 set 1 scan codes, as `win32::keys::scancode` reports them.
    const ESCAPE: u32 = 0x01;
    const KEY_A: u32 = 0x1E;
    const KEY_J: u32 = 0x24;
    const ARROW_UP: u32 = 0xE048;
    const CONTROL_RIGHT: u32 = 0xE01D;

    /// The fewest driver repeats a two-second hold must produce. Windows'
    /// slowest setting is a one-second delay and about 2.5 repeats a second.
    const MIN_REPEATS: usize = 2;
    /// The bounds a repeat interval must fall in: Windows' keyboard speed
    /// setting ranges from about 2.5 to 30 repeats a second, and the upper end
    /// allows for a busy desktop.
    const REPEAT_INTERVAL: (Duration, Duration) =
        (Duration::from_millis(20), Duration::from_millis(600));

    /// What a step concluded.
    enum Verdict {
        Pass(String),
        Fail(String),
        Skipped,
        TimedOut,
    }

    struct Desk {
        shell: Box<dyn Shell>,
        window: WindowId,
        events: Vec<ShellEvent>,
    }

    impl Desk {
        /// Shows `prompt` and pumps until `done` says the step is complete,
        /// Escape is pressed, or [`STEP_WAIT`] runs out.
        fn step(
            &mut self,
            number: usize,
            total: usize,
            prompt: &str,
            mut done: impl FnMut(&[ShellEvent]) -> Option<Verdict>,
        ) -> Verdict {
            let title = format!("[{number}/{total}] {prompt}  (Esc skips)");
            println!("{title}");
            // A title that cannot be set is the one thing that would leave the
            // person without instructions, so it is worth stopping over.
            self.shell
                .set_title(self.window, &title)
                .expect("the check's own window takes a title");
            self.events.clear();
            let deadline = Instant::now() + STEP_WAIT;
            loop {
                let events = &mut self.events;
                self.shell.pump(&mut |event| events.push(event));
                if self.events.iter().any(|event| {
                    matches!(event, ShellEvent::Key { scancode, state: ButtonState::Pressed, .. }
                        if scancode.0 == ESCAPE)
                }) {
                    return Verdict::Skipped;
                }
                if let Some(verdict) = done(&self.events) {
                    return verdict;
                }
                if Instant::now() >= deadline {
                    return Verdict::TimedOut;
                }
                self.shell.wait_events(Some(Duration::from_millis(10)));
            }
        }
    }

    /// Every key event so far, as `(scancode, key code, keysym, state, repeat,
    /// time)`.
    fn keys(
        events: &[ShellEvent],
    ) -> Vec<(u32, Option<KeyCode>, Keysym, ButtonState, bool, Duration)> {
        events
            .iter()
            .filter_map(|event| match event {
                ShellEvent::Key {
                    scancode,
                    key_code,
                    keysym,
                    state,
                    repeat,
                    time,
                    ..
                } => Some((
                    scancode.0,
                    *key_code,
                    *keysym,
                    *state,
                    *repeat,
                    time.as_duration(),
                )),
                _ => None,
            })
            .collect()
    }

    /// A press and a release of `scancode`, checked against the expected key
    /// code and, where given, keysym.
    fn key_step(
        scancode: u32,
        key_code: KeyCode,
        keysym: Option<Keysym>,
    ) -> impl FnMut(&[ShellEvent]) -> Option<Verdict> {
        move |events| {
            let keys = keys(events);
            let pressed = keys
                .iter()
                .find(|key| key.3 == ButtonState::Pressed && key.0 != ESCAPE)?;
            let released = keys
                .iter()
                .any(|key| key.0 == pressed.0 && key.3 == ButtonState::Released);
            if !released {
                return None;
            }
            if pressed.0 != scancode {
                return Some(Verdict::Fail(format!(
                    "scancode {:#06x}, expected {scancode:#06x} (key code {:?})",
                    pressed.0, pressed.1
                )));
            }
            if pressed.1 != Some(key_code) {
                return Some(Verdict::Fail(format!(
                    "key code {:?}, expected {key_code:?}",
                    pressed.1
                )));
            }
            if let Some(keysym) = keysym
                && pressed.2 != keysym
            {
                return Some(Verdict::Fail(format!(
                    "keysym {:?}, expected {keysym:?}",
                    pressed.2
                )));
            }
            Some(Verdict::Pass(format!(
                "scancode {scancode:#06x}, {key_code:?}, keysym {:?}",
                pressed.2
            )))
        }
    }

    /// A press and release of `button`, somewhere inside the client area.
    fn button_step(button: PointerButton) -> impl FnMut(&[ShellEvent]) -> Option<Verdict> {
        move |events| {
            let edges: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    ShellEvent::Button {
                        button: which,
                        state,
                        position,
                        ..
                    } if *which == button => Some((*state, *position)),
                    _ => None,
                })
                .collect();
            let released = edges
                .iter()
                .position(|&(state, _)| state == ButtonState::Released)?;
            let pressed = edges[..released]
                .iter()
                .any(|&(state, _)| state == ButtonState::Pressed);
            Some(if pressed {
                Verdict::Pass(format!("pressed and released at {:?}", edges[released].1))
            } else {
                Verdict::Fail(format!("a release with no press: {edges:?}"))
            })
        }
    }

    pub fn run() -> ExitCode {
        let mut shell = crcbl_shell::open_backend(ShellBackend::Win32)
            .expect("the Win32 shell opens on an interactive desktop");
        let window = shell
            .create_window(&WindowDesc {
                title: "crcbl real input check",
                app_id: "sh.kryptic.crcbl.hands-on",
                size: LogicalSize::new(900.0, 500.0),
                accept_drops: true,
                ..WindowDesc::default()
            })
            .expect("create_window");
        let mut desk = Desk {
            shell,
            window,
            events: Vec::new(),
        };

        let total = 13;
        let mut results: Vec<(&str, Verdict)> = Vec::new();

        // Focus first: a process started from a terminal is not granted the
        // foreground, and every key step below needs the keyboard.
        let focus = desk.step(1, total, "Click inside this window to focus it", |events| {
            events
                .iter()
                .any(|event| matches!(event, ShellEvent::Focus { focused: true, .. }))
                .then(|| Verdict::Pass("focused".into()))
        });
        results.push(("focus", focus));

        let a = desk.step(
            2,
            total,
            "Press and release the A key",
            key_step(KEY_A, KeyCode::KeyA, Some(Keysym::from_char('a'))),
        );
        results.push(("key A", a));
        // The text half of the same keystroke, which only `TranslateMessage`
        // produces from a real message.
        let text = desk.step(3, total, "Press A once more", |events| {
            events.iter().find_map(|event| match event {
                ShellEvent::TextCommit { text, .. } if text == "a" => {
                    Some(Verdict::Pass("committed \"a\"".into()))
                }
                ShellEvent::TextCommit { text, .. } => {
                    Some(Verdict::Fail(format!("committed {text:?}, expected \"a\"")))
                }
                _ => None,
            })
        });
        results.push(("text from A", text));

        let up = desk.step(
            4,
            total,
            "Press and release the Up arrow",
            key_step(ARROW_UP, KeyCode::ArrowUp, None),
        );
        results.push(("extended key: ArrowUp", up));
        let ctrl = desk.step(
            5,
            total,
            "Press and release the RIGHT Ctrl key",
            key_step(CONTROL_RIGHT, KeyCode::ControlRight, None),
        );
        results.push(("extended key: right Ctrl", ctrl));

        let repeat = desk.step(
            6,
            total,
            "Hold J down for about two seconds, then let go",
            |events| {
                let keys: Vec<_> = keys(events)
                    .into_iter()
                    .filter(|key| key.0 == KEY_J)
                    .collect();
                if !keys.iter().any(|key| key.3 == ButtonState::Released) {
                    return None;
                }
                let presses: Vec<_> = keys
                    .iter()
                    .filter(|key| key.3 == ButtonState::Pressed)
                    .collect();
                let repeats: Vec<Duration> = presses
                    .iter()
                    .filter(|key| key.4)
                    .map(|key| key.5)
                    .collect();
                if presses.first().is_some_and(|key| key.4) {
                    return Some(Verdict::Fail("the first press was marked a repeat".into()));
                }
                if repeats.len() < MIN_REPEATS {
                    return Some(Verdict::Fail(format!(
                        "{} repeat(s) from a two-second hold; the driver's typematic repeat \
                         should give at least {MIN_REPEATS}",
                        repeats.len()
                    )));
                }
                let first_delay = repeats[0].saturating_sub(presses[0].5);
                let mut intervals: Vec<Duration> =
                    repeats.windows(2).map(|pair| pair[1] - pair[0]).collect();
                intervals.sort_unstable();
                let median = intervals[intervals.len() / 2];
                if median < REPEAT_INTERVAL.0 || median > REPEAT_INTERVAL.1 {
                    return Some(Verdict::Fail(format!(
                        "median repeat interval {median:?} from the events' own timestamps, \
                         outside {REPEAT_INTERVAL:?}"
                    )));
                }
                Some(Verdict::Pass(format!(
                    "{} repeats, first after {first_delay:?}, median interval {median:?} \
                     (from GetMessageTime)",
                    repeats.len()
                )))
            },
        );
        results.push(("driver auto-repeat", repeat));

        for (number, prompt, name, button) in [
            (
                7,
                "Left-click inside the window",
                "left button",
                PointerButton::Left,
            ),
            (
                8,
                "Right-click inside the window",
                "right button",
                PointerButton::Right,
            ),
            (
                9,
                "Middle-click (press the wheel down) inside the window",
                "middle button",
                PointerButton::Middle,
            ),
            (
                10,
                "Click the mouse's BACK side button inside the window",
                "back button",
                PointerButton::Back,
            ),
        ] {
            let verdict = desk.step(number, total, prompt, button_step(button));
            results.push((name, verdict));
        }

        let wheel = desk.step(
            11,
            total,
            "Scroll the wheel ONE notch away from you, over the window",
            |events| {
                events.iter().find_map(|event| match event {
                    ShellEvent::Wheel {
                        delta: ScrollDelta::Lines { x, y },
                        ..
                    } => Some(if *x == 0.0 && y.abs() > 0.0 {
                        Verdict::Pass(format!(
                            "Lines {{ y: {y} }}; the sign follows the Settings scroll \
                             direction, which Windows applies before the window sees it"
                        ))
                    } else {
                        Verdict::Fail(format!("Lines {{ x: {x}, y: {y} }}"))
                    }),
                    ShellEvent::Wheel { delta, .. } => Some(Verdict::Fail(format!(
                        "{delta:?}, expected detents as Lines"
                    ))),
                    _ => None,
                })
            },
        );
        results.push(("wheel detent", wheel));

        let motion = desk.step(
            12,
            total,
            "Move the mouse around over the window",
            |events| {
                let deltas: Vec<_> = events
                    .iter()
                    .filter_map(|event| match event {
                        ShellEvent::PointerMotion {
                            raw_delta: Some(delta),
                            ..
                        } => Some(*delta),
                        _ => None,
                    })
                    .collect();
                (deltas.len() >= 20).then(|| {
                    Verdict::Pass(format!(
                        "{} raw relative samples, first {:?}",
                        deltas.len(),
                        deltas[0]
                    ))
                })
            },
        );
        results.push(("raw motion", motion));

        let drop = desk.step(
            13,
            total,
            "Drag any file from Explorer and drop it on this window",
            |events| {
                events.iter().find_map(|event| match event {
                    ShellEvent::DroppedFile { path, position, .. } => Some(if path.exists() {
                        Verdict::Pass(format!("{} at {position:?}", path.display()))
                    } else {
                        Verdict::Fail(format!("{} does not exist", path.display()))
                    }),
                    _ => None,
                })
            },
        );
        results.push(("file drag from Explorer", drop));

        println!("\ncrcbl real input check:");
        let mut failed = false;
        for (name, verdict) in &results {
            let (label, detail) = match verdict {
                Verdict::Pass(detail) => ("PASS", detail.as_str()),
                Verdict::Fail(detail) => {
                    failed = true;
                    ("FAIL", detail.as_str())
                }
                Verdict::Skipped => ("SKIP", "skipped with Escape"),
                Verdict::TimedOut => ("TIME", "nobody did it within the step's wait"),
            };
            println!("  {label}  {name}: {detail}");
        }
        if failed {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    }
}
