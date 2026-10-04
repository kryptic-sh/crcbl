//! A dedicated server's console and its clock: the lines typed at its stdin,
//! read between frames without blocking the loop, and the sleep that keeps
//! the loop on its tick grid.
//!
//! Every headless server that keeps wall time until it is told to stop —
//! towers' `--serve`, `crcbl edit --serve` — runs the same loop: a frame,
//! the console's lines answered, a sleep to the next tick. What a line means
//! is the server's own; reading it is this.
//!
//! **Stdin, through a thread of its own.** A read of stdin blocks, and the
//! standard library has no non-blocking one on every platform, so a thread
//! reads the lines and hands them over a channel the loop drains. `std`
//! alone, so it behaves the same on every OS.
//!
//! **Stdin closing is not a quit.** A server started with no console — its
//! input at its end from the start, under a service manager say — serves on,
//! and the end is logged once. Only the reader thread ends.

use std::io::{self, BufRead};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

/// The lines typed at a server's console, as its loop reads them: whatever
/// sends them — stdin's reader thread, or a test.
#[derive(Debug)]
pub struct ConsoleLines {
    lines: Receiver<String>,
    /// Whether the sender is gone — stdin at its end — which is logged once.
    closed: bool,
}

impl ConsoleLines {
    /// The lines `lines` receives.
    #[must_use]
    pub const fn new(lines: Receiver<String>) -> Self {
        Self {
            lines,
            closed: false,
        }
    }

    /// The next line waiting, or `None` when none is — and when the input
    /// has ended, which the first such call logs.
    pub fn next_line(&mut self) -> Option<String> {
        match self.lines.try_recv() {
            Ok(line) => Some(line),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                if !self.closed {
                    self.closed = true;
                    crate::log::info!(
                        "serve: the console's input ended; serving on, with no console"
                    );
                }
                None
            }
        }
    }
}

/// This process's stdin, a line at a time: a thread named `thread_name`
/// reads it into the channel handed back, and ends when stdin does or a read
/// fails — either logged, neither a quit. [`ConsoleLines::new`] reads it.
#[must_use]
pub fn stdin_lines(thread_name: &str) -> Receiver<String> {
    let (sender, lines) = mpsc::channel();
    let reader = thread::Builder::new()
        .name(thread_name.to_owned())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        crate::log::warn!("serve: the console's input failed: {error}");
                        return;
                    }
                }
            }
        });
    if let Err(error) = reader {
        crate::log::warn!("serve: no console, the reader thread did not start: {error}");
    }
    lines
}

/// How long from `elapsed` to the next whole `tick`: sleeping to the boundary
/// rather than for a whole tick keeps the ticks on the wall clock's grid
/// however long a frame's work took. A frame that overran a tick is caught up
/// by the host's own clock, which runs every tick that came due.
///
/// # Panics
///
/// For a zero `tick`, which no positive rate has.
#[must_use]
pub fn until_next_tick(elapsed: Duration, tick: Duration) -> Duration {
    let tick_ns = tick.as_nanos();
    let into_tick = elapsed.as_nanos() % tick_ns;
    Duration::from_nanos(
        u64::try_from(tick_ns - into_tick).expect("a tick at a positive rate is under a second"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The sleep lands on the next tick boundary**, from anywhere inside a
    /// tick — including exactly on one, which waits a whole tick rather than
    /// none and spinning.
    #[test]
    fn the_sleep_lands_on_the_next_tick_boundary() {
        let tick = Duration::from_millis(16);
        assert_eq!(until_next_tick(Duration::ZERO, tick), tick);
        assert_eq!(
            until_next_tick(Duration::from_millis(5), tick),
            Duration::from_millis(11)
        );
        assert_eq!(
            until_next_tick(Duration::from_millis(16 * 7 + 15), tick),
            Duration::from_millis(1)
        );
        assert_eq!(until_next_tick(Duration::from_millis(32), tick), tick);
    }

    /// **Lines come in the order typed, and an ended input is no line**,
    /// however often it is read.
    #[test]
    fn lines_come_in_order_and_an_ended_input_is_no_line() {
        let (typed, lines) = mpsc::channel();
        let mut console = ConsoleLines::new(lines);
        assert_eq!(console.next_line(), None, "nothing typed yet");
        for line in ["status", "quit"] {
            typed.send(line.to_owned()).expect("the console is open");
        }
        drop(typed);
        assert_eq!(console.next_line().as_deref(), Some("status"));
        assert_eq!(console.next_line().as_deref(), Some("quit"));
        for _ in 0..3 {
            assert_eq!(console.next_line(), None);
        }
    }
}
