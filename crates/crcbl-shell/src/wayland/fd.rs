//! File descriptors the compositor hands over, and the non-blocking transfers
//! that run across them.
//!
//! # Decision: the ownership discipline is lifted here; the `mmap` is not
//!
//! P0.5b's [`xkb`](super::xkb) module had the only descriptor this backend ever
//! received: `wl_keyboard.keymap` hands over an fd that the client owns and
//! must close on **every** path, because the compositor cannot reclaim the
//! memory behind it until the last client has. Its report flagged that
//! `wl_data_offer.receive` and `wl_data_source.send` want the same discipline
//! and asked whether the helper should be lifted.
//!
//! It is, and the split is deliberate:
//!
//! * **Lifted: who owns the descriptor and when it is closed.** That question
//!   has one answer for all three sites, and the answer is
//!   [`adopt`] — wrap the raw `int` in an owning [`File`] at the boundary and
//!   let scope close it. A hand-written `close` on each early return is the
//!   shape that leaks the fourth time somebody adds a `return`.
//! * **Not lifted: `mmap`.** Only the keymap is a *file*; a clipboard
//!   descriptor is one end of a **pipe**, which cannot be mapped and has no
//!   length to map. Moving `mmap` here would produce a module whose two halves
//!   share nothing but the word "descriptor".
//!
//! So `xkb` keeps its mapping and loses its `close`, and the clipboard gets
//! [`Reading`] and [`Writing`] on top of the same ownership rule.
//!
//! # Decision: `std::io::pipe` and `File`, not more hand-written libc
//!
//! [`ffi`](super::ffi) declares libwayland by hand because there is no
//! alternative. There is one here: `std::io::pipe` creates the
//! `O_CLOEXEC` pipe a `receive` needs, and `File` reads and writes it with
//! errors already classified — `ErrorKind::WouldBlock` is `EAGAIN` and
//! `Interrupted` is `EINTR`, both of which a hand-rolled `read` would have to
//! recognise from `errno`. The only `unsafe` in this module is the one that
//! cannot be avoided: taking ownership of an `int` that arrived over a socket.
//!
//! # Why neither side may block, and how that is guaranteed without `O_NONBLOCK`
//!
//! [`Shell::pump`](crate::Shell::pump) promises to be finite and non-blocking.
//! A clipboard read is a pipe fed by *another application*, which may be slow,
//! stopped in a debugger, or gone; a `read` on the event-loop thread is a frame
//! hitch at best. Worse, the peer can be **this process**: pasting our own
//! selection means the compositor asks us for the bytes on the same connection
//! we are waiting on, so a blocking read deadlocks against a write that only a
//! later `pump` could perform.
//!
//! Neither end sets `O_NONBLOCK`, and neither can block, because of what
//! `poll` guarantees for a pipe:
//!
//! * `POLLIN` means a `read` returns immediately — with data, or with `0` for
//!   end-of-file.
//! * `POLLOUT` means a write of up to `PIPE_BUF` (4096) bytes will not block.
//!   [`Writing`] therefore writes in [`CHUNK`]-sized pieces and re-polls
//!   between them, rather than handing the kernel a megabyte and hoping.
//!
//! Both go through [`ffi::poll_ready`], which counts `POLLHUP` and `POLLERR` as
//! ready as well — and must. An empty pipe whose writer has closed reports
//! `POLLHUP` and *no* `POLLIN`, so a strict readability test would wait forever
//! for an end-of-file that has already happened. That is not a hypothetical:
//! every completed transfer ends in exactly that state.
//!
//! Not setting `O_NONBLOCK` matters for a second reason: the write end of the
//! pipe we create is handed to the *peer*, and flags travel with the open file
//! description. A peer doing an ordinary blocking `write` must not have it fail
//! with `EAGAIN` because of a flag we set for our own convenience.
//!
//! Every transfer also carries a deadline. A peer that accepts the descriptor
//! and never writes to it would otherwise be an entry in a list that is never
//! removed and an answer the caller never receives; see [`TIMEOUT`].

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};

use super::ffi;

/// How long a transfer may make no progress before it is abandoned.
///
/// The failure this bounds is a peer that takes the descriptor and never writes
/// (or never reads). There is no protocol event for it — the transfer is
/// outside Wayland entirely, over a pipe the compositor only brokered — so a
/// timeout is the only way it can ever end.
///
/// It is an **idle** deadline, exactly as that sentence says: every byte that
/// moves buys another `TIMEOUT`. Applied to the transfer as a whole instead, it
/// would abandon a paste that is streaming perfectly well and merely happens to
/// be larger than two seconds' worth of pipe — reporting
/// [`Unavailable`](crate::ClipboardContent::Unavailable) for a clipboard that
/// was being delivered.
///
/// Two seconds: long enough that a large paste from a busy application is not
/// cut off, short enough that a paste which is never going to arrive answers
/// while the user is still looking at the menu they picked it from. The answer
/// is [`ClipboardData`](crate::ShellEvent::ClipboardData) with `data: None`,
/// which that event already defines as "there is nothing to paste".
pub const TIMEOUT: core::time::Duration = core::time::Duration::from_secs(2);

/// The most a single transfer may accumulate, in bytes.
///
/// The peer decides how much it sends and this process has to hold it. 64 MiB
/// is far past any real clipboard payload and far short of a memory problem;
/// without a cap, an application on the other side of the pipe chooses how much
/// of this engine's address space it uses.
const MAX_BYTES: usize = 64 << 20;

/// Bytes per `write`, chosen so that a `POLLOUT` makes it non-blocking.
///
/// `PIPE_BUF` is 4096 on Linux, and `poll` only promises "will not block" for a
/// write of at most that much.
const CHUNK: usize = 4096;

/// Read buffer size. One page per `read` syscall on a pipe whose kernel buffer
/// is 64 KiB by default.
const READ_CHUNK: usize = 8192;

/// Takes ownership of a descriptor that arrived over the Wayland socket.
///
/// `None` for the negative value libwayland yields when a compositor sends a
/// malformed message, which is the only invalid case: everything else is a
/// descriptor this process now owns and must close exactly once. The returned
/// [`File`] is that "exactly once".
pub fn adopt(fd: RawFd) -> Option<File> {
    if fd < 0 {
        return None;
    }
    // SAFETY: `fd` was created by libwayland's own `recvmsg` of an
    // `SCM_RIGHTS` message and handed to the dispatcher, which transfers
    // ownership to the client. It is adopted exactly once — every call site
    // takes the value out of the decoded event and never reads it again — so
    // no other owner can close it.
    Some(unsafe { File::from_raw_fd(fd) })
}

/// Returns a descriptor we are not going to use.
///
/// `wl_keyboard.keymap` and `wl_data_source.send` both transfer ownership
/// whether or not the client wants the data, and a descriptor dropped on the
/// floor is one leaked per layout switch or per paste.
pub fn close(fd: RawFd) {
    drop(adopt(fd));
}

/// How a transfer is getting on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Still in progress; poll it again next frame.
    Pending,
    /// Complete. For a [`Reading`], the peer closed its end.
    Done,
    /// The peer went away, misbehaved, or ran out of time.
    Failed,
}

/// One end of a pipe being drained into a buffer.
///
/// Created by [`Shell::clipboard_request`](crate::Shell::clipboard_request) and
/// by a drop; serviced once per [`pump`](crate::Shell::pump) until the peer
/// closes its end.
#[derive(Debug)]
pub struct Reading {
    file: File,
    buffer: Vec<u8>,
    /// `CLOCK_MONOTONIC` nanoseconds after which this is abandoned **if
    /// nothing has moved since**. Pushed forward by every byte that arrives;
    /// see [`TIMEOUT`].
    idle_deadline_nanos: u64,
}

impl Reading {
    /// Adopts the read end of a pipe whose write end has gone to the peer.
    #[must_use]
    pub fn new(file: File, now_nanos: u64) -> Self {
        Self {
            file,
            buffer: Vec::new(),
            idle_deadline_nanos: now_nanos.saturating_add(deadline_nanos()),
        }
    }

    /// The descriptor, so a blocking wait can include it.
    #[must_use]
    pub fn raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    /// Everything read so far, consumed.
    #[must_use]
    pub fn take(self) -> Vec<u8> {
        self.buffer
    }

    /// Reads whatever is available without blocking.
    ///
    /// Returns whether any byte moved as well as the state, because the caller
    /// alternates reads and writes for a transfer whose two ends are both this
    /// process, and "nothing moved anywhere" is what ends that loop.
    pub fn service(&mut self, now_nanos: u64) -> (State, bool) {
        let mut moved = false;
        let mut chunk = [0u8; READ_CHUNK];
        // `POLLIN` on a pipe means the `read` below returns immediately, with
        // data or with the zero that says the peer closed. Re-polling each time
        // round is what keeps that true after the buffer is drained.
        while ffi::poll_ready(self.file.as_raw_fd(), false, 0) {
            match self.file.read(&mut chunk) {
                Ok(0) => return (State::Done, moved),
                Ok(read) => {
                    if self.buffer.len().saturating_add(read) > MAX_BYTES {
                        crcbl_core::log::warn!(
                            "a clipboard transfer exceeded {MAX_BYTES} bytes and was abandoned"
                        );
                        return (State::Failed, moved);
                    }
                    self.buffer.extend_from_slice(&chunk[..read]);
                    moved = true;
                }
                Err(error) if would_retry(&error) => break,
                Err(error) => {
                    crcbl_core::log::debug!("clipboard read failed: {error}");
                    return (State::Failed, moved);
                }
            }
        }
        if moved {
            self.idle_deadline_nanos = now_nanos.saturating_add(deadline_nanos());
        }
        (self.state_at(now_nanos), moved)
    }

    fn state_at(&self, now_nanos: u64) -> State {
        if now_nanos >= self.idle_deadline_nanos {
            crcbl_core::log::debug!("a clipboard transfer timed out with no data from the peer");
            State::Failed
        } else {
            State::Pending
        }
    }
}

/// A payload being fed into a descriptor the compositor handed us.
///
/// Created by `wl_data_source.send`, which is the compositor saying "a peer
/// asked for this format; write it here". The peer may read slowly or not at
/// all — including when the peer is this process pasting its own selection —
/// so this is serviced across frames exactly like a [`Reading`].
#[derive(Debug)]
pub struct Writing {
    file: File,
    bytes: Vec<u8>,
    offset: usize,
    /// As [`Reading::idle_deadline_nanos`]: reset by every byte the peer takes.
    idle_deadline_nanos: u64,
}

impl Writing {
    /// Adopts the descriptor a `send` event carried, with the bytes to put in
    /// it.
    #[must_use]
    pub fn new(file: File, bytes: Vec<u8>, now_nanos: u64) -> Self {
        Self {
            file,
            bytes,
            offset: 0,
            idle_deadline_nanos: now_nanos.saturating_add(deadline_nanos()),
        }
    }

    /// Writes as much as the pipe will take without blocking.
    ///
    /// See [`Reading::service`] for the returned flag.
    pub fn service(&mut self, now_nanos: u64) -> (State, bool) {
        let mut moved = false;
        while self.offset < self.bytes.len() {
            // `POLLOUT` promises only that a `PIPE_BUF`-sized write will not
            // block, so the slice handed to `write` is capped at that.
            if !ffi::poll_ready(self.file.as_raw_fd(), true, 0) {
                break;
            }
            let end = self.bytes.len().min(self.offset + CHUNK);
            match self.file.write(&self.bytes[self.offset..end]) {
                Ok(0) => break,
                Ok(written) => {
                    self.offset += written;
                    moved = true;
                }
                Err(error) if would_retry(&error) => break,
                Err(error) => {
                    // A peer that closed the descriptor before reading it all
                    // is ordinary — `head -c 4` on a paste does exactly that —
                    // so this is not a warning.
                    crcbl_core::log::debug!("clipboard write ended early: {error}");
                    return (State::Failed, moved);
                }
            }
        }
        if self.offset >= self.bytes.len() {
            // Dropping the file closes the descriptor, which is the
            // end-of-file the peer is waiting for. Nothing else says "that was
            // all of it".
            return (State::Done, moved);
        }
        if moved {
            self.idle_deadline_nanos = now_nanos.saturating_add(deadline_nanos());
        }
        if now_nanos >= self.idle_deadline_nanos {
            crcbl_core::log::debug!("a clipboard transfer timed out with the peer not reading");
            return (State::Failed, moved);
        }
        (State::Pending, moved)
    }
}

/// [`TIMEOUT`] in nanoseconds, saturating.
fn deadline_nanos() -> u64 {
    u64::try_from(TIMEOUT.as_nanos()).unwrap_or(u64::MAX)
}

/// Whether an I/O error means "try again later" rather than "this is over".
fn would_retry(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CLOCK_MONOTONIC` now, as every caller passes.
    fn now() -> u64 {
        ffi::monotonic_nanos()
    }

    #[test]
    fn a_read_completes_when_the_peer_closes_its_end() {
        let (reader, mut writer) = std::io::pipe().expect("pipe");
        let mut reading = Reading::new(File::from(std::os::fd::OwnedFd::from(reader)), now());

        writer.write_all(b"hello ").expect("write");
        let (state, moved) = reading.service(now());
        assert_eq!(state, State::Pending, "the peer has not closed yet");
        assert!(moved);

        writer.write_all(b"world").expect("write");
        assert_eq!(reading.service(now()).0, State::Pending);

        // End-of-file is the only thing that says a transfer is complete: a
        // paste is not "done" because the reader ran out of bytes for a moment.
        drop(writer);
        assert_eq!(reading.service(now()).0, State::Done);
        assert_eq!(reading.take(), b"hello world");
    }

    #[test]
    fn a_read_from_a_peer_that_never_writes_times_out_instead_of_hanging() {
        // The case the whole design exists for: the descriptor is accepted and
        // nothing ever arrives. Servicing must return immediately every time,
        // and eventually give up rather than leaving the request outstanding
        // forever.
        let (reader, writer) = std::io::pipe().expect("pipe");
        let mut reading = Reading::new(File::from(std::os::fd::OwnedFd::from(reader)), now());
        let started = std::time::Instant::now();
        assert_eq!(reading.service(now()).0, State::Pending);
        assert!(
            started.elapsed() < core::time::Duration::from_millis(100),
            "servicing a silent transfer must not block, took {:?}",
            started.elapsed()
        );

        // Deadline reached: the clock is an argument precisely so this is
        // testable without waiting two seconds.
        let (state, moved) = reading.service(now() + 2 * deadline_nanos());
        assert_eq!(state, State::Failed);
        assert!(!moved);
        drop(writer);
    }

    #[test]
    fn a_transfer_that_keeps_moving_is_never_abandoned_for_being_slow() {
        // The deadline bounds *idleness*, which is what its own documentation
        // says. A whole-transfer deadline gave up on a paste that was streaming
        // steadily and merely took longer than `TIMEOUT` in total, and answered
        // `Unavailable` for a clipboard that was arriving.
        let (reader, mut writer) = std::io::pipe().expect("pipe");
        let mut reading = Reading::new(File::from(std::os::fd::OwnedFd::from(reader)), now());

        let mut clock = now();
        for _ in 0..5 {
            // Each chunk arrives a whole timeout's worth of time after the
            // last, which is as slow as a transfer can be without stalling.
            clock += deadline_nanos() - 1;
            writer.write_all(b"chunk").expect("write");
            let (state, moved) = reading.service(clock);
            assert_eq!(state, State::Pending, "it is still arriving");
            assert!(moved);
        }
        // Total elapsed is far past `TIMEOUT`, and nothing has been abandoned.
        assert_eq!(reading.buffer.len(), 25);
        // Stop feeding it, and the same deadline ends it.
        assert_eq!(
            reading.service(clock + deadline_nanos()).0,
            State::Failed,
            "silence still runs out"
        );
        drop(writer);
    }

    #[test]
    fn a_write_larger_than_the_pipe_buffer_is_finished_across_several_services() {
        // The self-paste case in miniature: nobody reads until we let them, so
        // one service cannot finish, and a blocking write would never return.
        // A Linux pipe holds 64 KiB.
        let payload: Vec<u8> = (0..200_000u32).map(|byte| byte as u8).collect();
        let (reader, writer) = std::io::pipe().expect("pipe");
        let mut writing = Writing::new(
            File::from(std::os::fd::OwnedFd::from(writer)),
            payload.clone(),
            now(),
        );
        let mut reading = Reading::new(File::from(std::os::fd::OwnedFd::from(reader)), now());

        let started = std::time::Instant::now();
        assert_eq!(
            writing.service(now()).0,
            State::Pending,
            "the pipe is full and the payload is not"
        );
        assert!(
            started.elapsed() < core::time::Duration::from_millis(100),
            "a full pipe must not block the caller"
        );

        // Alternating is what makes progress, which is exactly what `pump`
        // does when both ends of a transfer are this process.
        let mut writing_state = State::Pending;
        for _ in 0..1_000 {
            if writing_state == State::Pending {
                writing_state = writing.service(now()).0;
            }
            if reading.service(now()).0 == State::Done {
                break;
            }
        }
        assert_eq!(writing_state, State::Done);
        drop(writing);
        assert_eq!(reading.service(now()).0, State::Done);
        assert_eq!(reading.take(), payload);
    }

    #[test]
    fn a_write_to_a_reader_that_hung_up_fails_rather_than_raising_sigpipe() {
        // `File::write` reports `EPIPE` as an error; the process-wide `SIGPIPE`
        // that would otherwise kill the engine is ignored by the Rust runtime,
        // which is what makes this safe to do on the event-loop thread.
        let (reader, writer) = std::io::pipe().expect("pipe");
        drop(reader);
        let mut writing = Writing::new(
            File::from(std::os::fd::OwnedFd::from(writer)),
            vec![7u8; 128],
            now(),
        );
        assert_eq!(writing.service(now()).0, State::Failed);
    }

    #[test]
    fn an_invalid_descriptor_is_not_adopted() {
        assert!(adopt(-1).is_none());
        // Closing one is a no-op rather than a `close(-1)`.
        close(-1);
    }
}
