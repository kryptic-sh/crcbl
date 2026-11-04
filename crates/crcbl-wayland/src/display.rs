//! A connection to the compositor, and the read cycle that drives it.
//!
//! Everything libwayland asks of a client that drives its own event loop,
//! wrapped so the invariants are kept by the types rather than by the caller:
//! the connection is closed exactly once, by [`Display`]'s `Drop`, and every
//! `wl_display_prepare_read` is paired with exactly one `wl_display_read_events`
//! or `wl_display_cancel_read`, by [`ReadGuard`]. A missed pairing is not a
//! leak but a deadlock: another thread reading the same display — the Vulkan
//! WSI during a present — waits on the read barrier forever.
//!
//! # Driving it from an async runtime
//!
//! The descriptor [`Display`] lends through `AsFd` is the socket. The cycle is
//! the one `wl_display_prepare_read`'s documentation gives:
//!
//! 1. [`Display::prepare_read`] until it yields a guard, calling
//!    [`Display::dispatch_pending`] each time it does not;
//! 2. [`Display::flush`];
//! 3. wait for the descriptor to be readable — `tokio::io::unix::AsyncFd`, or
//!    `poll` — then [`ReadGuard::read`], or drop the guard to cancel;
//! 4. [`Display::dispatch_pending`] again.

use core::ffi::c_int;
use core::fmt;
use core::ptr::{self, NonNull};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, IntoRawFd as _, OwnedFd, RawFd};

use crate::ffi::{self, Lib, WlDisplay, WlProxy};

/// Why [`Display::connect`] could not open a connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectError {
    /// libwayland-client could not be loaded; the text is the loader's, for a
    /// log line.
    Library(&'static str),
    /// The library loaded, but no compositor answered at `WAYLAND_DISPLAY` (or
    /// `WAYLAND_SOCKET`). Holds the variable's value, `None` when it is unset.
    NoCompositor(Option<String>),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Library(detail) => f.write_str(detail),
            Self::NoCompositor(Some(name)) => write!(f, "no compositor at WAYLAND_DISPLAY={name}"),
            Self::NoCompositor(None) => f.write_str("WAYLAND_DISPLAY is not set"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// The connection failed, and is unusable from here on.
///
/// libwayland marks a display dead on the first protocol or socket error and
/// fails every later call; `errno` is what `wl_display_get_error` reports —
/// `EPROTO` for a protocol error the compositor raised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disconnected {
    /// The `errno` the display recorded.
    pub errno: c_int,
}

impl fmt::Display for Disconnected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "errno {}", self.errno)
    }
}

impl std::error::Error for Disconnected {}

/// An open `wl_display`.
///
/// Closed when dropped. Every proxy made on it must be destroyed first:
/// libwayland frees the display's memory in `wl_display_disconnect`, and a
/// proxy outliving it is a dangling pointer to the library, not to Rust.
#[derive(Debug)]
pub struct Display {
    lib: &'static Lib,
    raw: NonNull<WlDisplay>,
    fd: RawFd,
}

impl Display {
    /// Connects to the compositor `WAYLAND_DISPLAY` names, or the socket
    /// `WAYLAND_SOCKET` hands over — libwayland's own choice between them.
    ///
    /// # Errors
    ///
    /// [`ConnectError::Library`] when libwayland-client cannot be loaded, and
    /// [`ConnectError::NoCompositor`] when nothing answers.
    pub fn connect() -> Result<Self, ConnectError> {
        let lib = ffi::load().map_err(ConnectError::Library)?;
        // SAFETY: a null name means "read WAYLAND_DISPLAY", which is
        // `wl_display_connect`'s documented behaviour.
        let raw = unsafe { (lib.display_connect)(ptr::null()) };
        Self::adopt(lib, raw)
            .ok_or_else(|| ConnectError::NoCompositor(std::env::var("WAYLAND_DISPLAY").ok()))
    }

    /// Connects over a socket already open to the compositor — one a parent
    /// process handed down, or one end of a `socketpair`.
    ///
    /// # Errors
    ///
    /// [`ConnectError::Library`] when libwayland-client cannot be loaded, and
    /// [`ConnectError::NoCompositor`] when libwayland refuses the socket. The
    /// socket is closed either way.
    pub fn connect_to_fd(socket: OwnedFd) -> Result<Self, ConnectError> {
        let lib = ffi::load().map_err(ConnectError::Library)?;
        // SAFETY: the descriptor is open and ours to give: libwayland takes
        // ownership of it, and closes it on failure as well as on disconnect.
        let raw = unsafe { (lib.display_connect_to_fd)(socket.into_raw_fd()) };
        Self::adopt(lib, raw).ok_or(ConnectError::NoCompositor(None))
    }

    /// Takes ownership of what a `wl_display_connect*` returned; `None` for
    /// its null.
    fn adopt(lib: &'static Lib, raw: *mut WlDisplay) -> Option<Self> {
        let raw = NonNull::new(raw)?;
        // SAFETY: the display was just returned live by libwayland.
        let fd = unsafe { (lib.display_get_fd)(raw.as_ptr()) };
        Some(Self { lib, raw, fd })
    }

    /// The `wl_display*`, for what takes one by ABI: a Vulkan or EGL surface.
    #[must_use]
    pub const fn as_ptr(&self) -> NonNull<WlDisplay> {
        self.raw
    }

    /// The display as the `wl_proxy` it also is, for generated requests on
    /// `wl_display` — `get_registry` and `sync`.
    #[must_use]
    pub const fn as_proxy(&self) -> *mut WlProxy {
        ffi::display_as_proxy(self.raw.as_ptr())
    }

    /// Sends every queued request the socket takes.
    ///
    /// # Errors
    ///
    /// When the socket would not take all of it. `ErrorKind::WouldBlock` is
    /// the ordinary case — the compositor is behind, and the rest goes on the
    /// next flush; anything else means the connection is gone.
    pub fn flush(&self) -> std::io::Result<()> {
        // SAFETY: the display is live for `self`'s lifetime.
        if unsafe { (self.lib.display_flush)(self.raw.as_ptr()) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Sends everything, then blocks until the compositor has answered it all,
    /// dispatching what arrives meanwhile.
    ///
    /// # Errors
    ///
    /// [`Disconnected`] if the connection fails.
    pub fn roundtrip(&self) -> Result<u32, Disconnected> {
        // SAFETY: the display is live for `self`'s lifetime.
        self.count(unsafe { (self.lib.display_roundtrip)(self.raw.as_ptr()) })
    }

    /// Dispatches the events already read off the socket, without blocking.
    /// Returns how many were dispatched.
    ///
    /// # Errors
    ///
    /// [`Disconnected`] if the connection fails.
    pub fn dispatch_pending(&self) -> Result<u32, Disconnected> {
        // SAFETY: the display is live for `self`'s lifetime.
        self.count(unsafe { (self.lib.display_dispatch_pending)(self.raw.as_ptr()) })
    }

    /// Announces that this thread is about to read the socket.
    ///
    /// `None` while events are already queued: they must be dispatched first,
    /// with [`dispatch_pending`](Self::dispatch_pending), and the call made
    /// again.
    #[must_use]
    pub fn prepare_read(&self) -> Option<ReadGuard<'_>> {
        // SAFETY: the display is live for `self`'s lifetime. On success the
        // guard owns the one `read_events` or `cancel_read` this owes.
        (unsafe { (self.lib.display_prepare_read)(self.raw.as_ptr()) } == 0)
            .then_some(ReadGuard { display: self })
    }

    /// Why the connection failed, once it has.
    #[must_use]
    pub fn disconnected(&self) -> Disconnected {
        // SAFETY: the display is live; `wl_display_get_error` only reads it.
        Disconnected {
            errno: unsafe { (self.lib.display_get_error)(self.raw.as_ptr()) },
        }
    }

    /// A count libwayland returned, or the failure a negative one means.
    fn count(&self, result: c_int) -> Result<u32, Disconnected> {
        u32::try_from(result).map_err(|_| self.disconnected())
    }
}

impl AsRawFd for Display {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl AsFd for Display {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the descriptor is the display's socket, which libwayland
        // keeps open until `wl_display_disconnect` in `drop`, so it outlives
        // every borrow of `self`.
        unsafe { BorrowedFd::borrow_raw(self.fd) }
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        // SAFETY: the display came from `wl_display_connect` and is closed
        // only here. Its proxies are the owner's to have destroyed — see the
        // type's docs.
        unsafe { (self.lib.display_disconnect)(self.raw.as_ptr()) };
    }
}

/// A read [`Display::prepare_read`] announced, and owes.
///
/// [`read`](Self::read) completes it; dropping the guard cancels it. Either
/// way libwayland hears exactly once, which is what keeps another reader of the
/// display from waiting forever.
#[derive(Debug)]
#[must_use = "dropping the guard cancels the read"]
pub struct ReadGuard<'a> {
    display: &'a Display,
}

impl ReadGuard<'_> {
    /// Reads what the socket has into the queue, for
    /// [`Display::dispatch_pending`] to dispatch.
    ///
    /// Blocks while nothing is there; call it once the descriptor polls
    /// readable.
    ///
    /// # Errors
    ///
    /// [`Disconnected`] if the connection fails.
    pub fn read(self) -> Result<(), Disconnected> {
        let display = self.display;
        core::mem::forget(self);
        // SAFETY: the display is live, and this is the one completion the
        // guard owed: `forget` above keeps `drop` from cancelling it as well.
        if unsafe { (display.lib.display_read_events)(display.raw.as_ptr()) } < 0 {
            return Err(display.disconnected());
        }
        Ok(())
    }
}

impl Drop for ReadGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: the display is live, and the read this guard announced has
        // not been completed — `read` forgets the guard instead of dropping it.
        unsafe { (self.display.lib.display_cancel_read)(self.display.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// A display on one end of a socket pair, and the other end standing in
    /// for the compositor. Nothing is said on it: Wayland has no handshake, so
    /// a connection is usable before the peer sends a byte.
    fn connected() -> (Display, UnixStream) {
        let (ours, peer) = UnixStream::pair().expect("socketpair");
        let display = Display::connect_to_fd(OwnedFd::from(ours)).expect("libwayland-client loads");
        (display, peer)
    }

    /// Runs `body` on its own thread and fails, rather than hangs, if it does
    /// not finish: a read libwayland is still owed blocks the next one forever.
    fn within_a_deadline(body: impl FnOnce() + Send + 'static) {
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            body();
            done.send(()).expect("the test is still waiting");
        });
        finished
            .recv_timeout(Duration::from_secs(5))
            .expect("the read finished, so no earlier read was left owed");
    }

    #[test]
    fn a_dropped_read_guard_cancels_its_read() {
        within_a_deadline(|| {
            let (display, _peer) = connected();
            drop(display.prepare_read().expect("nothing is queued"));
            // Had the first guard not cancelled, libwayland would count it as
            // a reader still to come, and this read would wait for it.
            let read = display.prepare_read().expect("nothing is queued");
            read.read().expect("an idle socket reads as no events");
        });
    }

    #[test]
    fn dropping_the_display_closes_the_connection() {
        let (display, mut peer) = connected();
        drop(display);
        let mut byte = [0u8; 1];
        peer.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        assert_eq!(
            peer.read(&mut byte)
                .expect("the peer sees end of file, not a timeout"),
            0,
            "the client's end is closed"
        );
    }

    #[test]
    fn a_peer_that_hangs_up_reads_as_disconnected() {
        let (display, peer) = connected();
        drop(peer);
        let read = display.prepare_read().expect("nothing is queued");
        let error = read.read().expect_err("the socket is closed");
        assert_ne!(error.errno, 0, "libwayland recorded why");
        assert_eq!(display.disconnected(), error);
        assert!(
            display.dispatch_pending().is_err(),
            "a dead display stays dead"
        );
    }

    #[test]
    fn the_descriptor_lent_out_is_the_connections_socket() {
        let (display, _peer) = connected();
        assert_eq!(display.as_fd().as_raw_fd(), display.as_raw_fd());
        assert!(display.as_raw_fd() >= 0);
    }
}
