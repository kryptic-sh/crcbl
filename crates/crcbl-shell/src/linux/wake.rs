//! The Wayland and X11 waits' doorbell: a pipe, polled beside the window
//! system's socket.
//!
//! Both waits are a `poll` on descriptors, so the portable way in from another
//! thread is one more descriptor: [`wake`](WakeSignal::wake) writes a byte to a
//! pipe whose read end is in the poll set, and the poll returns. A pipe rather
//! than an `eventfd` because it is the one that exists on every Unix the
//! Wayland backend may one day build on, and `std::io::pipe` makes it without
//! a line of FFI.
//!
//! # A pipe that never fills and a read that never waits
//!
//! [`rung`](WakePipe::rung) is the bell's state. A wake that flips it from
//! false to true writes exactly one byte, and every later wake sees it already
//! true and writes nothing; [`clear`](WakePipe::clear) flips it back and reads
//! exactly one byte. So the pipe holds at most the byte of the ring that has
//! not been cleared yet — a waker can never fill it and block — and a clear
//! always has its byte, at worst a moment after the waker's flip, which is
//! immediately followed by its write.
//!
//! The shell holds the read end for the poll, and the waker's [`Arc`] holds
//! both: a wake after the shell is gone writes into a pipe that still has a
//! reader, so it can never raise `SIGPIPE`.
//!
//! [`Arc`]: std::sync::Arc

use std::io::{self, PipeReader, PipeWriter, Read as _, Write as _};
use std::os::fd::{AsRawFd as _, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::waker::WakeSignal;

/// The doorbell: the pipe and whether it has been rung since the last clear.
#[derive(Debug)]
pub struct WakePipe {
    reader: PipeReader,
    writer: PipeWriter,
    /// True from a ring until [`clear`](Self::clear). The byte in the pipe
    /// exists exactly while this is true — see the module docs.
    rung: AtomicBool,
}

impl WakePipe {
    /// A new, unrung doorbell.
    ///
    /// # Errors
    ///
    /// The OS refusing a pipe, which in practice means the process is out of
    /// descriptors.
    pub fn new() -> io::Result<Self> {
        let (reader, writer) = io::pipe()?;
        Ok(Self {
            reader,
            writer,
            rung: AtomicBool::new(false),
        })
    }

    /// The descriptor to poll for readability: readable while the bell is
    /// rung.
    pub fn fd(&self) -> RawFd {
        self.reader.as_raw_fd()
    }

    /// Unrings the bell, after a wait: takes the ring's byte off the pipe if
    /// there was a ring, and does nothing otherwise.
    ///
    /// Never waits for longer than a waker takes to finish its write — see the
    /// module docs.
    pub fn clear(&self) {
        if self.rung.swap(false, Ordering::AcqRel) {
            let mut byte = [0];
            if let Err(error) = (&self.reader).read_exact(&mut byte) {
                crcbl_core::log::warn!("the shell's wake pipe could not be read: {error}");
            }
        }
    }
}

impl WakeSignal for WakePipe {
    fn wake(&self) {
        if self.rung.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Err(error) = (&self.writer).write_all(&[1]) {
            // No byte, so no ring: put the flag back, or the next clear would
            // wait forever for a byte that is never coming.
            self.rung.store(false, Ordering::Release);
            crcbl_core::log::warn!("the shell's wake pipe could not be written: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WakePipe;
    use crate::waker::WakeSignal as _;

    /// Whether `fd` is readable right now, without waiting.
    fn readable(fd: std::os::fd::RawFd) -> bool {
        crate::x11::ffi::poll_readable(fd, 0)
    }

    #[test]
    fn a_ring_makes_the_pipe_readable_and_a_clear_empties_it() {
        let bell = WakePipe::new().expect("a pipe");
        assert!(!readable(bell.fd()), "a new bell is silent");
        bell.wake();
        assert!(readable(bell.fd()), "a ring is a readable byte");
        bell.clear();
        assert!(!readable(bell.fd()), "a clear takes the byte");
    }

    #[test]
    fn rings_coalesce_into_one_byte() {
        let bell = WakePipe::new().expect("a pipe");
        for _ in 0..1000 {
            bell.wake();
        }
        bell.clear();
        assert!(
            !readable(bell.fd()),
            "a thousand rings left more than one byte"
        );
        // And a clear with nothing rung does not wait for a byte.
        bell.clear();
    }

    #[test]
    fn a_ring_from_another_thread_wakes_a_poll() {
        let bell = std::sync::Arc::new(WakePipe::new().expect("a pipe"));
        let ringer = std::sync::Arc::clone(&bell);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            ringer.wake();
        });
        // Long enough that a poll which was not woken fails the test by
        // timing out rather than passing by accident.
        assert!(
            crate::x11::ffi::poll_readable(bell.fd(), 60_000),
            "the ring did not wake the poll"
        );
        thread.join().expect("the ringer");
        bell.clear();
    }
}
