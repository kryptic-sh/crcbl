//! libwayland-client, and the descriptor waits and clock the backend runs on.
//!
//! The binding itself — the `dlopen`ed library, the descriptor types and the
//! facade generated protocol code marshals through — is `crcbl_wayland::ffi`,
//! re-exported whole so that `crate::wayland::ffi` is still the path the
//! generated code names. What stays here is the backend's own use of libc:
//! `poll` on the compositor socket and on clipboard pipes, and
//! `CLOCK_MONOTONIC` for input timestamps.

pub use crcbl_wayland::ffi::*;

use core::ffi::{c_int, c_long};

use crate::linux::{CLOCK_MONOTONIC, Nfds};

// The symbols come from libc, which `std` already links on every Unix target.
unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: Nfds, timeout: c_int) -> c_int;
    fn clock_gettime(clock_id: c_int, tp: *mut Timespec) -> c_int;
}

/// `struct pollfd`.
#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

/// `struct timespec`.
#[repr(C)]
struct Timespec {
    tv_sec: c_long,
    tv_nsec: c_long,
}

/// `POLLIN`.
const POLLIN: i16 = 0x001;
/// `POLLOUT`.
const POLLOUT: i16 = 0x004;

/// Whether `fd` is readable right now.
///
/// A zero timeout makes this a poll in the strict sense — it never blocks,
/// which is what [`Shell::pump`](crate::Shell::pump)'s finite-drain contract
/// requires. `timeout_ms` of `-1` blocks indefinitely, which is what
/// [`Shell::wait_events`](crate::Shell::wait_events) wants.
pub fn poll_readable(fd: c_int, timeout_ms: c_int) -> bool {
    let mut fds = PollFd {
        fd,
        events: POLLIN,
        revents: 0,
    };
    // SAFETY: `fds` is one initialised `pollfd` and the count matches.
    let result = unsafe { poll(&raw mut fds, 1, timeout_ms) };
    result > 0 && fds.revents & POLLIN != 0
}

/// Whether an I/O on `fd` would make progress rather than block.
///
/// Distinct from [`poll_readable`] in what it counts as ready: **any**
/// `revents`, not only the bit that was asked for. That is what a pipe needs
/// and what a socket does not —
///
/// * a read end whose writer has closed reports `POLLHUP` and **no** `POLLIN`
///   when it is empty, and the `read` that returns `0` there is the only signal
///   a transfer has completed. A strict `POLLIN` test waits forever for an
///   end-of-file that has already happened.
/// * a write end whose reader has closed reports `POLLERR`, and the `write`
///   that turns it into an `EPIPE` error is how the caller learns to give up.
///
/// For the ready case the guarantees are the ones
/// [`fd`](super::fd) is built on: `POLLIN` means a `read` returns immediately,
/// and `POLLOUT` means a write of up to `PIPE_BUF` bytes does not block.
#[must_use]
pub fn poll_ready(fd: c_int, for_write: bool, timeout_ms: c_int) -> bool {
    let mut fds = PollFd {
        fd,
        events: if for_write { POLLOUT } else { POLLIN },
        revents: 0,
    };
    // SAFETY: `fds` is one initialised `pollfd` and the count matches.
    let result = unsafe { poll(&raw mut fds, 1, timeout_ms) };
    result > 0 && fds.revents != 0
}

/// Waits until `primary` is readable, one of `aux` is ready, or the timeout
/// elapses; reports whether **`primary`** is readable.
///
/// The auxiliary descriptors are clipboard transfers in flight. They do not
/// change the answer — only the Wayland socket drives the protocol — but an
/// editor blocked in [`wait_events`](crate::Shell::wait_events) has to wake for
/// them too, or a paste whose bytes arrive over a pipe would sit in the pipe
/// until something unrelated happened on the compositor connection.
#[must_use]
pub fn poll_readable_with(primary: c_int, aux: &[c_int], timeout_ms: c_int) -> bool {
    if aux.is_empty() {
        return poll_readable(primary, timeout_ms);
    }
    let mut fds: Vec<PollFd> = core::iter::once(PollFd {
        fd: primary,
        events: POLLIN,
        revents: 0,
    })
    .chain(aux.iter().map(|fd| PollFd {
        fd: *fd,
        // Both directions: a transfer is waiting either for the peer's bytes
        // or for room to put ours.
        events: POLLIN | POLLOUT,
        revents: 0,
    }))
    .collect();
    let count = Nfds::try_from(fds.len()).unwrap_or(1);
    // SAFETY: `fds` is a live, initialised array of exactly `count` `pollfd`s.
    let result = unsafe { poll(fds.as_mut_ptr(), count, timeout_ms) };
    result > 0 && fds[0].revents & POLLIN != 0
}

/// `CLOCK_MONOTONIC` in nanoseconds — the clock Wayland stamps input events
/// with.
///
/// Returns zero if the call fails, which cannot happen for `CLOCK_MONOTONIC` on
/// any Linux this engine supports; propagating an error for it would put a
/// `Result` on every timestamp for no reachable case.
#[must_use]
pub fn monotonic_nanos() -> u64 {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is an initialised `timespec` and `CLOCK_MONOTONIC` is valid.
    if unsafe { clock_gettime(CLOCK_MONOTONIC, &raw mut ts) } != 0 {
        return 0;
    }
    let seconds = u64::try_from(ts.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(ts.tv_nsec).unwrap_or(0);
    seconds * 1_000_000_000 + nanos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_monotonic_clock_advances_and_is_not_the_epoch() {
        let first = monotonic_nanos();
        assert!(first > 0, "CLOCK_MONOTONIC is available on Linux");
        let second = monotonic_nanos();
        assert!(second >= first, "CLOCK_MONOTONIC never goes backwards");
    }

    #[test]
    fn polling_a_closed_descriptor_does_not_block() {
        // -1 is never readable; the point is that a zero timeout returns.
        assert!(!poll_readable(-1, 0));
        assert!(!poll_ready(-1, true, 0));
        assert!(!poll_ready(-1, false, 0));
        assert!(!poll_readable_with(-1, &[-1, -1], 0));
    }

    #[test]
    fn a_fresh_pipe_is_writable_and_becomes_readable_when_written() {
        use std::io::Write as _;
        use std::os::fd::AsRawFd as _;

        let (reader, mut writer) = std::io::pipe().expect("pipe");
        assert!(
            poll_ready(writer.as_raw_fd(), true, 0),
            "an empty pipe takes a PIPE_BUF write without blocking"
        );
        assert!(
            !poll_ready(reader.as_raw_fd(), false, 0),
            "nothing in it yet"
        );

        writer.write_all(b"x").expect("write");
        assert!(poll_ready(reader.as_raw_fd(), false, 0));

        // The case a strict `POLLIN` test gets wrong: an empty pipe whose
        // writer has closed is *ready*, and the read that reports it is the
        // end-of-file a transfer completes on.
        let (empty, closed) = std::io::pipe().expect("pipe");
        drop(closed);
        assert!(
            !poll_readable(empty.as_raw_fd(), 0),
            "no POLLIN, only POLLHUP"
        );
        assert!(poll_ready(empty.as_raw_fd(), false, 0));
        // The auxiliary form still answers about the *primary* descriptor, not
        // about whichever one happened to become ready.
        assert!(!poll_readable_with(-1, &[reader.as_raw_fd()], 0));
        assert!(poll_readable_with(reader.as_raw_fd(), &[-1], 0));
    }
}
