//! A connection to an X server, owned and driven from outside the engine.
//!
//! [`Connection`] opens the display, reads the default screen from the setup,
//! negotiates RandR, and turns libxcb's raw replies, errors and events into
//! owned Rust values. Its RandR requests are in the `randr` module and its
//! events in `event`; this module holds what every request shares — the reply
//! and error plumbing. The decisions that shape it are on [`Connection`],
//! where a caller reads them.

use core::ffi::{CStr, c_int, c_uint, c_void};
use core::fmt;
use core::ptr::{self, NonNull};
use core::time::Duration;
use std::ffi::CString;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::sync::OnceLock;
use std::time::Instant;

use crate::event::{self, Event};
use crate::ffi::{self, Cookie, GenericError, Lib, POLLIN, PollFd, RandrLib};

/// An X atom: an interned name.
pub type Atom = u32;
/// An X window id.
pub type Window = u32;
/// A server timestamp, in milliseconds. `0` is `CurrentTime`.
pub type Timestamp = u32;

/// Why libxcb shut a connection: `xcb_connection_has_error`'s codes.
///
/// A connection that has one is dead for good — libxcb refuses every later
/// request on it — so there is nothing to retry on the same [`Connection`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConnectionError {
    /// `XCB_CONN_ERROR`: the socket failed, or the server went away.
    Socket,
    /// `XCB_CONN_CLOSED_EXT_NOTSUPPORTED`: an extension the request needs is
    /// missing.
    ExtensionNotSupported,
    /// `XCB_CONN_CLOSED_MEM_INSUFFICIENT`.
    OutOfMemory,
    /// `XCB_CONN_CLOSED_REQ_LEN_EXCEED`: a request was longer than the server
    /// accepts.
    RequestTooLong,
    /// `XCB_CONN_CLOSED_PARSE_ERR`: the display name could not be parsed.
    ParseError,
    /// `XCB_CONN_CLOSED_INVALID_SCREEN`: the display name's screen number is
    /// not one the server has.
    InvalidScreen,
    /// `XCB_CONN_CLOSED_FDPASSING_FAILED`.
    FdPassingFailed,
    /// A code this crate does not know, as libxcb returned it.
    Other(c_int),
}

impl ConnectionError {
    /// The error `code` means, or `None` for `0`, which is a healthy
    /// connection.
    fn from_code(code: c_int) -> Option<Self> {
        Some(match code {
            0 => return None,
            1 => Self::Socket,
            2 => Self::ExtensionNotSupported,
            3 => Self::OutOfMemory,
            4 => Self::RequestTooLong,
            5 => Self::ParseError,
            6 => Self::InvalidScreen,
            7 => Self::FdPassingFailed,
            other => Self::Other(other),
        })
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Socket => f.write_str("the connection to the X server failed"),
            Self::ExtensionNotSupported => f.write_str("an X extension libxcb needed is missing"),
            Self::OutOfMemory => f.write_str("libxcb ran out of memory"),
            Self::RequestTooLong => f.write_str("a request exceeded the server's maximum length"),
            Self::ParseError => f.write_str("the display name could not be parsed"),
            Self::InvalidScreen => f.write_str("the server has no such screen"),
            Self::FdPassingFailed => f.write_str("passing a file descriptor failed"),
            Self::Other(code) => write!(f, "libxcb connection error {code}"),
        }
    }
}

impl std::error::Error for ConnectionError {}

/// Why [`Connection::connect`] could not open a connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectError {
    /// libxcb could not be loaded; the text is the loader's, for a log line.
    Library(&'static str),
    /// The display name contains a NUL byte.
    InvalidName,
    /// The server's connection setup describes more screens, depths or
    /// visuals than its own length holds: the server is broken or hostile.
    MalformedSetup,
    /// libxcb could not connect.
    Failed {
        /// The display asked for: the argument, or `DISPLAY` when it was
        /// `None` (itself `None` when that is unset).
        display: Option<String>,
        /// What libxcb reported.
        error: ConnectionError,
    },
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Library(detail) => f.write_str(detail),
            Self::InvalidName => f.write_str("the display name contains a NUL byte"),
            Self::MalformedSetup => f.write_str("the X server's connection setup is malformed"),
            Self::Failed {
                display: Some(name),
                error,
            } => write!(f, "cannot open display {name}: {error}"),
            Self::Failed {
                display: None,
                error,
            } => write!(f, "DISPLAY is not set: {error}"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// An X protocol error: the server refused one request.
///
/// The connection stays usable. Which request is said by the opcodes: the
/// major opcode is the core request, or an extension's (RandR's is
/// [`RandrExtension::major_opcode`]), and the minor opcode is the request
/// within that extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct XError {
    /// The error number: a core error (`BadValue` is `2`, `BadMatch` `8`), or
    /// an extension's `first_error` plus its own number —
    /// [`RandrExtension::error_kind`] reads RandR's.
    pub error_code: u8,
    /// Major opcode of the refused request.
    pub major_opcode: u8,
    /// Minor opcode of the refused request.
    pub minor_opcode: u16,
    /// The resource id, value or atom the error is about.
    pub resource_id: u32,
    /// Full sequence number of the refused request.
    pub sequence: u32,
}

impl XError {
    pub(crate) const fn from_raw(raw: &GenericError) -> Self {
        Self {
            error_code: raw.error_code,
            major_opcode: raw.major_code,
            minor_opcode: raw.minor_code,
            resource_id: raw.resource_id,
            sequence: raw.full_sequence,
        }
    }
}

impl fmt::Display for XError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "X error {} on request {}.{} (resource {:#x}, sequence {})",
            self.error_code, self.major_opcode, self.minor_opcode, self.resource_id, self.sequence
        )
    }
}

impl std::error::Error for XError {}

/// A RandR protocol version.
///
/// Ordered as versions are: `1.2 < 1.5 < 2.0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RandrVersion {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
}

impl RandrVersion {
    /// The version [`Connection`] asks the server for: the one `GetMonitors`
    /// arrived in, and the newest this crate speaks.
    pub const REQUESTED: Self = Self { major: 1, minor: 5 };
    /// The oldest version [`Connection`] uses RandR at: CRTCs, outputs and
    /// modes. Below it, every RandR method returns [`Error::RandrTooOld`].
    pub const MINIMUM: Self = Self { major: 1, minor: 2 };
}

impl fmt::Display for RandrVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// What a request on a [`Connection`] can fail with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Error {
    /// The connection is dead; see [`ConnectionError`].
    Connection(ConnectionError),
    /// The server refused the request; the connection is still usable.
    Protocol(XError),
    /// libxcb-randr could not be loaded; the text is the loader's.
    RandrLibrary(&'static str),
    /// The server does not have the RandR extension.
    RandrMissing,
    /// The server's RandR is older than the request needs.
    RandrTooOld {
        /// The version the server settled on.
        have: RandrVersion,
        /// The version the request was introduced in.
        need: RandrVersion,
    },
    /// An argument is longer than its field in the request can count.
    TooLong {
        /// Which argument.
        what: &'static str,
    },
    /// A reply's counts point outside the reply; the server is broken or
    /// hostile.
    MalformedReply {
        /// Which request's reply.
        request: &'static str,
    },
    /// The server did not answer within the
    /// [reply timeout](Connection::set_reply_timeout).
    ///
    /// The connection has given up on its replies: every later request
    /// returns this same error, naming the request that timed out, without
    /// being sent. Reconnect to go on.
    Timeout {
        /// The request that went unanswered.
        request: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(error) => error.fmt(f),
            Self::Protocol(error) => error.fmt(f),
            Self::RandrLibrary(detail) => f.write_str(detail),
            Self::RandrMissing => f.write_str("the X server has no RandR extension"),
            Self::RandrTooOld { have, need } => {
                write!(f, "the X server has RandR {have}; this needs {need}")
            }
            Self::TooLong { what } => write!(f, "{what} is too long for its request"),
            Self::MalformedReply { request } => {
                write!(f, "the server's {request} reply is malformed")
            }
            Self::Timeout { request } => {
                write!(f, "the X server did not answer {request} in time")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<XError> for Error {
    fn from(error: XError) -> Self {
        Self::Protocol(error)
    }
}

impl From<ConnectionError> for Error {
    fn from(error: ConnectionError) -> Self {
        Self::Connection(error)
    }
}

/// The screen a [`Connection`] opened, as the connection setup described it.
///
/// A snapshot from connect time: the size changes with `SetScreenSize`, and
/// the new one arrives in [`Event::ScreenChange`], not here.
/// [`Connection::geometry`] on [`root`](Self::root) asks the server for the
/// current one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Screen {
    /// The screen's number — the `.1` in `:0.1`, `0` when the name has none.
    pub number: u32,
    /// The root window, which RandR requests and event selection name.
    pub root: Window,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Width in millimetres.
    pub width_mm: u16,
    /// Height in millimetres.
    pub height_mm: u16,
}

/// A drawable's geometry, as [`Connection::geometry`] reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Geometry {
    /// The root window of the drawable's screen.
    pub root: Window,
    /// The drawable's depth: bits per pixel that hold colour.
    pub depth: u8,
    /// X of the outside of the border, relative to the parent — *not* the
    /// desktop, under a reparenting window manager. `0` for a root window or
    /// a pixmap.
    pub x: i16,
    /// Y, as `x`.
    pub y: i16,
    /// Width in pixels, inside the border.
    pub width: u16,
    /// Height in pixels, inside the border.
    pub height: u16,
    /// Border width in pixels; `0` for a pixmap.
    pub border_width: u16,
}

/// An extension the server has, as [`Connection::query_extension`] reads it.
///
/// An extension with no events or no errors of its own has `0` for that
/// number, as the protocol says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExtensionInfo {
    /// The major opcode the extension's requests carry, which is what an
    /// [`XError::major_opcode`] is compared to.
    pub major_opcode: u8,
    /// The event number of the extension's first event.
    pub first_event: u8,
    /// The error number of the extension's first error.
    pub first_error: u8,
}

/// RandR as this connection negotiated it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RandrExtension {
    /// The major opcode RandR's requests carry, which is what an
    /// [`XError::major_opcode`] is compared to.
    pub major_opcode: u8,
    /// The event number of RandR's first event, `ScreenChangeNotify`.
    pub first_event: u8,
    /// The error number of RandR's first error, `BadRROutput`.
    pub first_error: u8,
    /// The version the server settled on.
    pub version: RandrVersion,
}

/// RandR's own errors, as [`RandrExtension::error_kind`] reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RandrErrorKind {
    /// `BadRROutput`: no such output.
    BadOutput,
    /// `BadRRCrtc`: no such CRTC.
    BadCrtc,
    /// `BadRRMode`: no such mode.
    BadMode,
    /// `BadRRProvider`: no such provider.
    BadProvider,
    /// `BadRRLease`: no such lease.
    BadLease,
}

impl RandrExtension {
    /// Which RandR error `error` is, or `None` for a core error or another
    /// extension's.
    #[must_use]
    pub fn error_kind(&self, error: &XError) -> Option<RandrErrorKind> {
        Some(match error.error_code.checked_sub(self.first_error)? {
            0 => RandrErrorKind::BadOutput,
            1 => RandrErrorKind::BadCrtc,
            2 => RandrErrorKind::BadMode,
            3 => RandrErrorKind::BadProvider,
            4 => RandrErrorKind::BadLease,
            _ => return None,
        })
    }
}

/// RandR, once negotiated: the library and what the server said.
#[derive(Debug)]
pub(crate) struct Randr {
    pub(crate) lib: &'static RandrLib,
    pub(crate) extension: RandrExtension,
}

/// An open connection to an X server.
///
/// Disconnected when dropped. Every method is a request (or several) and
/// waits for the server's answer — for ever, unless
/// [`set_reply_timeout`](Self::set_reply_timeout) set a deadline.
///
/// # Decision: every request is checked, and answers at its call
///
/// libxcb sends a request without a reply unchecked unless asked otherwise,
/// and then an error from it arrives later, on the event queue, attributed to
/// a sequence number the caller has to match up. A display configurator wants
/// the opposite: `SetScreenSize` refused is a reason to roll back *now*. So
/// each such request goes out as its `_checked` variant and waits in
/// `xcb_request_check` for the server's verdict, and each request with a reply
/// waits for it. That is one round trip per call, which is what `xrandr` pays
/// too and is nothing beside a mode set. A caller wanting to pipeline uses the
/// raw binding in [`crate::ffi`] on [`Connection::as_ptr`].
///
/// # Decision: replies are bounds-checked against their own length
///
/// A reply's lists are located by libxcb's accessors from counts the server
/// wrote, and libxcb does not check them against the reply's length. Every
/// list is therefore copied only after it is shown to lie inside the reply
/// (`32 + 4 * length` bytes, which is exactly what libxcb allocated), so a
/// server that lies about a count produces [`Error::MalformedReply`], not a
/// read past the allocation.
///
/// # Driving it from an async runtime
///
/// The descriptor [`Connection`] lends through `AsFd` is the socket. Select
/// the events wanted ([`Connection::select_input`]), [`Connection::flush`],
/// wait for the descriptor to be readable (`tokio::io::unix::AsyncFd`, or
/// `poll`), then call [`Connection::poll_event`] until it returns `Ok(None)`.
/// A request method blocks for its reply, and reads any events that arrive
/// meanwhile into libxcb's queue, so drain it after each batch of requests as
/// well as on readability: the socket will not wake the runtime for events
/// that are already off it.
///
/// # Threads
///
/// `Send` and `Sync`: libxcb serialises every call on a connection with that
/// connection's own lock, so one thread can wait in `poll_event`'s caller
/// while another makes requests. The reply timeout is set through `&mut`, so
/// before the connection is shared.
#[derive(Debug)]
pub struct Connection {
    lib: &'static Lib,
    raw: NonNull<ffi::Connection>,
    fd: RawFd,
    screen: Screen,
    randr: Result<Randr, Error>,
    /// How long a request waits for its answer; `None` waits for ever.
    reply_timeout: Option<Duration>,
    /// The error that made the connection give up on its replies, once one
    /// has; every later request returns it unsent.
    abandoned: OnceLock<Error>,
}

// SAFETY: libxcb is thread-safe: every entry point that touches an
// `xcb_connection_t` takes the connection's own mutex (`c->iolock`), which is
// what lets Xlib-xcb and the Vulkan WSI share one connection across threads.
// The other fields are a `&'static` table of function pointers, plain
// integers, copies taken at connect time, a timeout only `&mut self` writes,
// and a `OnceLock`, which is `Sync` itself. Moving a `Connection` to another
// thread moves only the pointer; `xcb_disconnect` in `Drop` runs once, after
// every borrow has ended, wherever that is.
unsafe impl Send for Connection {}
// SAFETY: as for `Send` — every `&self` method goes through libxcb, which
// serialises concurrent callers on the connection's mutex, and reads nothing
// of its own that another `&self` call could be writing but the `OnceLock`,
// which synchronises its own writes.
unsafe impl Sync for Connection {}

impl Connection {
    /// Connects to `display` — `":0"`, `"host:1.0"` — or, for `None`, to the
    /// display `DISPLAY` names, and negotiates RandR.
    ///
    /// A server without RandR is still connected to: the core requests work,
    /// and every RandR method returns why it cannot, as [`Connection::randr`]
    /// does.
    ///
    /// # Errors
    ///
    /// [`ConnectError::Library`] when libxcb cannot be loaded,
    /// [`ConnectError::InvalidName`] for a name with a NUL byte,
    /// [`ConnectError::Failed`] when libxcb cannot connect, or the screen the
    /// name asks for does not exist, and [`ConnectError::MalformedSetup`] for a
    /// setup whose screens run past its end.
    ///
    /// # Blocking
    ///
    /// Connecting has no deadline: a [reply timeout](Self::set_reply_timeout)
    /// can only be set on the connection this returns, so the connection
    /// setup and RandR's `QueryVersion` wait for as long as the server takes.
    pub fn connect(display: Option<&str>) -> Result<Self, ConnectError> {
        let lib = ffi::load().map_err(ConnectError::Library)?;
        let name = display
            .map(CString::new)
            .transpose()
            .map_err(|_| ConnectError::InvalidName)?;
        let failed = |error| ConnectError::Failed {
            display: display
                .map(str::to_owned)
                .or_else(|| std::env::var("DISPLAY").ok()),
            error,
        };
        let mut number: c_int = 0;
        // SAFETY: the name is null (read `DISPLAY`) or a NUL-terminated string
        // that outlives the call, and `number` is a live `int`.
        let raw = unsafe {
            (lib.connect)(
                name.as_deref().map_or(ptr::null(), CStr::as_ptr),
                &mut number,
            )
        };
        // `xcb_connect` returns a static error connection rather than null on
        // failure; null would be a libxcb that does not keep its contract.
        let raw = NonNull::new(raw).ok_or_else(|| failed(ConnectionError::OutOfMemory))?;
        // SAFETY: `raw` is what `xcb_connect` returned, and is disconnected on
        // every path below that does not hand it to a `Connection`.
        let code = unsafe { (lib.connection_has_error)(raw.as_ptr()) };
        let screen = match ConnectionError::from_code(code) {
            Some(error) => Err(failed(error)),
            None => match u32::try_from(number) {
                Ok(number) => default_screen(lib, raw, number).map_err(|error| match error {
                    SetupError::NoSuchScreen => failed(ConnectionError::InvalidScreen),
                    SetupError::Malformed => ConnectError::MalformedSetup,
                }),
                Err(_) => Err(failed(ConnectionError::InvalidScreen)),
            },
        };
        let screen = match screen {
            Ok(screen) => screen,
            Err(error) => {
                // SAFETY: as above; this is the only disconnect of `raw`. On
                // the static error connection libxcb makes it a no-op.
                unsafe { (lib.disconnect)(raw.as_ptr()) };
                return Err(error);
            }
        };
        // SAFETY: the connection is live.
        let fd = unsafe { (lib.get_file_descriptor)(raw.as_ptr()) };
        let mut connection = Self {
            lib,
            raw,
            fd,
            screen,
            randr: Err(Error::RandrMissing),
            reply_timeout: None,
            abandoned: OnceLock::new(),
        };
        connection.randr = connection.negotiate_randr();
        Ok(connection)
    }

    /// Loads libxcb-randr, finds the extension, and asks for
    /// [`RandrVersion::REQUESTED`] — which also tells the server which
    /// version's semantics to give this client.
    fn negotiate_randr(&self) -> Result<Randr, Error> {
        let lib = ffi::load_randr().map_err(Error::RandrLibrary)?;
        // SAFETY: the connection is live and `lib.id` is libxcb-randr's
        // `xcb_randr_id`. The reply is libxcb's cached copy, owned by the
        // connection and never freed by the caller.
        let data =
            unsafe { (self.lib.get_extension_data)(self.raw.as_ptr(), ffi::extension_ptr(lib.id)) };
        // SAFETY: a non-null result points at that cached reply, which lives
        // as long as the connection.
        let Some(data) = (unsafe { data.as_ref() }) else {
            return Err(self.lost());
        };
        if data.present == 0 {
            return Err(Error::RandrMissing);
        }
        let version = self.query_version_with(lib)?;
        if version < RandrVersion::MINIMUM {
            return Err(Error::RandrTooOld {
                have: version,
                need: RandrVersion::MINIMUM,
            });
        }
        Ok(Randr {
            lib,
            extension: RandrExtension {
                major_opcode: data.major_opcode,
                first_event: data.first_event,
                first_error: data.first_error,
                version,
            },
        })
    }

    /// `QueryVersion`, asking for [`RandrVersion::REQUESTED`].
    pub(crate) fn query_version_with(&self, lib: &RandrLib) -> Result<RandrVersion, Error> {
        let RandrVersion { major, minor } = RandrVersion::REQUESTED;
        // SAFETY: the connection is live; the arguments are plain integers.
        let cookie = unsafe { (lib.query_version)(self.live()?, major, minor) };
        let reply = self.reply::<ffi::RandrQueryVersionReply>("QueryVersion", cookie)?;
        Ok(RandrVersion {
            major: reply.major_version,
            minor: reply.minor_version,
        })
    }

    /// RandR as negotiated at connect time.
    ///
    /// # Errors
    ///
    /// Why RandR cannot be used on this connection: [`Error::RandrLibrary`],
    /// [`Error::RandrMissing`] or [`Error::RandrTooOld`] — or the
    /// [`Error::Connection`] or [`Error::Protocol`] that negotiating it met.
    pub fn randr(&self) -> Result<RandrExtension, Error> {
        match &self.randr {
            Ok(randr) => Ok(randr.extension),
            Err(error) => Err(*error),
        }
    }

    /// The RandR library, if the server's version is at least `need`.
    pub(crate) fn randr_at(&self, major: u32, minor: u32) -> Result<&'static RandrLib, Error> {
        let randr = self.randr.as_ref().map_err(|error| *error)?;
        let need = RandrVersion { major, minor };
        if randr.extension.version < need {
            return Err(Error::RandrTooOld {
                have: randr.extension.version,
                need,
            });
        }
        Ok(randr.lib)
    }

    /// The default screen: the one the display name chose.
    ///
    /// Its size is the connection setup's, from connect time;
    /// [`geometry`](Self::geometry) on its root reads the current one.
    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.screen
    }

    /// The `xcb_connection_t*`, for requests this type does not wrap. It stays
    /// owned by `self`: never disconnect it.
    #[must_use]
    pub const fn as_ptr(&self) -> NonNull<ffi::Connection> {
        self.raw
    }

    pub(crate) fn raw(&self) -> *mut ffi::Connection {
        self.raw.as_ptr()
    }

    /// The connection, to send a request on — or the error that made it give
    /// up on its replies, so that nothing more is sent after a timeout.
    pub(crate) fn live(&self) -> Result<*mut ffi::Connection, Error> {
        match self.abandoned.get() {
            Some(error) => Err(*error),
            None => Ok(self.raw()),
        }
    }

    /// Sets how long each request waits for the server's answer; `None`, the
    /// default, waits for ever.
    ///
    /// Without a deadline, a server that stops answering — stopped, or
    /// serving only another client that holds a server grab — blocks the
    /// calling thread until it answers again. With one, the wait ends in
    /// [`Error::Timeout`] instead. The deadline is per request, and every
    /// request method sends one request: a request without a reply waits for
    /// a `GetInputFocus` round trip behind it, inside the same deadline.
    ///
    /// A timeout ends the connection's usefulness: the request may still be
    /// answered, so every later request returns the same [`Error::Timeout`]
    /// without being sent, and the late reply is dropped when it arrives.
    /// The request itself is *not* undone — a `SetCrtcConfig` sent while
    /// another client grabs the server is carried out when the grab ends. So
    /// after a timeout, reconnect and read the state again before deciding
    /// anything on it.
    ///
    /// Waiting with a deadline flushes, then asks libxcb for the reply and
    /// sleeps on the socket in turns, waking at least every few milliseconds:
    /// another thread — [`poll_event`](Self::poll_event)'s caller, say — can
    /// read the reply off the socket, and libxcb has no way to wake this one
    /// when it does.
    ///
    /// Three waits still have no deadline. Sending: a request that does not
    /// fit in the socket's buffer while the server reads nothing blocks in the
    /// flush. A reply cut short: once a packet has begun to arrive, libxcb
    /// waits for the rest of it unbounded. And [`connect`](Self::connect),
    /// which returns before a timeout can be set.
    pub fn set_reply_timeout(&mut self, timeout: Option<Duration>) {
        self.reply_timeout = timeout;
    }

    /// Why the connection is dead, or `None` while it is healthy.
    #[must_use]
    pub fn has_error(&self) -> Option<ConnectionError> {
        // SAFETY: the connection is live; the call only reads its state.
        ConnectionError::from_code(unsafe { (self.lib.connection_has_error)(self.raw()) })
    }

    /// Sends every queued request.
    ///
    /// The request methods flush as they wait for their answers; this is for
    /// before waiting on the descriptor.
    ///
    /// # Errors
    ///
    /// [`Error::Connection`] when the connection is dead.
    pub fn flush(&self) -> Result<(), Error> {
        // SAFETY: the connection is live.
        if unsafe { (self.lib.flush)(self.raw()) } > 0 {
            Ok(())
        } else {
            Err(self.lost())
        }
    }

    /// A round trip (`GetInputFocus`): when it returns, the server has
    /// handled every earlier request, and every event they caused is in the
    /// queue [`poll_event`](Self::poll_event) reads.
    ///
    /// # Errors
    ///
    /// [`Error::Connection`] when the connection is dead.
    pub fn sync(&self) -> Result<(), Error> {
        self.round_trip("GetInputFocus")
    }

    /// A `GetInputFocus` round trip, whose wait is named `request`: the one
    /// [`sync`](Self::sync) makes, and the one a checked request waits behind
    /// under a deadline.
    fn round_trip(&self, request: &'static str) -> Result<(), Error> {
        // SAFETY: the connection is live.
        let cookie = unsafe { (self.lib.get_input_focus)(self.live()?) };
        self.reply::<ffi::GetInputFocusReply>(request, cookie)
            .map(drop)
    }

    /// The next event libxcb has, without blocking; `Ok(None)` when there is
    /// none.
    ///
    /// Every event is returned, none skipped: RandR's are decoded, an X error
    /// that arrived as an event — from a request sent unchecked through
    /// [`as_ptr`](Self::as_ptr), since this type's own are all checked — is
    /// [`Event::Error`], and everything else is [`Event::Other`] or
    /// [`Event::OtherRandrNotify`], so a loop can match what it wants and go
    /// on.
    ///
    /// # Errors
    ///
    /// [`Error::Connection`] once the connection is dead — the server went
    /// away, say. Events queued before it died are returned first.
    pub fn poll_event(&self) -> Result<Option<Event>, Error> {
        // SAFETY: the connection is live.
        let raw = unsafe { (self.lib.poll_for_event)(self.raw()) };
        let Some(raw) = NonNull::new(raw) else {
            return match self.has_error() {
                Some(error) => Err(Error::Connection(error)),
                None => Ok(None),
            };
        };
        // SAFETY: libxcb allocates every event at least the 36 bytes of
        // `xcb_generic_event_t` (32 wire bytes and `full_sequence`), and it is
        // all integers. It is copied out and then freed, once.
        let generic = unsafe { raw.as_ptr().read() };
        // SAFETY: as above; nothing reads the event after this.
        unsafe { ffi::free_reply(raw.as_ptr()) };
        let first_event = self
            .randr
            .as_ref()
            .ok()
            .map(|randr| randr.extension.first_event);
        Ok(Some(event::decode(&generic, first_event)))
    }

    /// `InternAtom`: the atom `name` is. With `only_if_exists`, `0` when no
    /// client has interned it yet, rather than creating it.
    ///
    /// # Errors
    ///
    /// [`Error::TooLong`] for a name longer than a request can carry, or what
    /// the request met.
    pub fn intern_atom(&self, name: &str, only_if_exists: bool) -> Result<Atom, Error> {
        let len = u16::try_from(name.len()).map_err(|_| Error::TooLong { what: "atom name" })?;
        // SAFETY: the connection is live, and `name` is `len` bytes that
        // outlive the call (libxcb copies them into its buffer).
        let cookie = unsafe {
            (self.lib.intern_atom)(
                self.live()?,
                u8::from(only_if_exists),
                len,
                name.as_ptr().cast(),
            )
        };
        let reply = self.reply::<ffi::InternAtomReply>("InternAtom", cookie)?;
        Ok(reply.atom)
    }

    /// `QueryExtension`, the core request: whether the server has the
    /// extension called `name`, and its numbers if it does — `Ok(None)` when
    /// it does not. `"RANDR"` answers with the numbers
    /// [`randr`](Self::randr) holds. `"XWAYLAND"` present means the server is
    /// Xwayland, but absent proves nothing: Xwayland before 23.1 lacks it, a
    /// build without `xwaylandproto` registers none, and `-extension XWAYLAND`
    /// turns it off.
    ///
    /// The server compares names byte for byte, so `"randr"` is not RandR;
    /// extension names are ASCII in practice. An empty name is a valid request,
    /// which the server answers with not present.
    ///
    /// Every call is a round trip. libxcb caches an extension's answer only
    /// for `xcb_get_extension_data`, keyed by the `xcb_extension_t` a binding
    /// for that extension defines; this request goes out on its own.
    ///
    /// # Errors
    ///
    /// [`Error::TooLong`] for a name longer than the request's 16-bit length
    /// counts — no extension has one, and a truncated name would ask about a
    /// different extension — or what the request met.
    pub fn query_extension(&self, name: &str) -> Result<Option<ExtensionInfo>, Error> {
        let len = u16::try_from(name.len()).map_err(|_| Error::TooLong {
            what: "extension name",
        })?;
        // SAFETY: the connection is live, and `name` is `len` bytes that
        // outlive the call (libxcb copies them into its buffer).
        let cookie = unsafe { (self.lib.query_extension)(self.live()?, len, name.as_ptr().cast()) };
        let reply = self.reply::<ffi::QueryExtensionReply>("QueryExtension", cookie)?;
        Ok((reply.present != 0).then(|| ExtensionInfo {
            major_opcode: reply.major_opcode,
            first_event: reply.first_event,
            first_error: reply.first_error,
        }))
    }

    /// `GetAtomName`: the name `atom` was interned as.
    ///
    /// Atom names are Latin-1 by the protocol and ASCII in practice; a byte
    /// that is not UTF-8 becomes U+FFFD.
    ///
    /// # Errors
    ///
    /// [`Error::Protocol`] with `BadAtom` for an atom that does not exist, or
    /// what else the request met.
    pub fn atom_name(&self, atom: Atom) -> Result<String, Error> {
        // SAFETY: the connection is live.
        let cookie = unsafe { (self.lib.get_atom_name)(self.live()?, atom) };
        let reply = self.reply::<ffi::GetAtomNameReply>("GetAtomName", cookie)?;
        // SAFETY: the reply is a live GetAtomName reply.
        let name = unsafe { (self.lib.get_atom_name_name)(reply.as_ptr()) };
        let name = reply.list(name.cast::<u8>(), usize::from(reply.name_len))?;
        Ok(String::from_utf8_lossy(&name).into_owned())
    }

    /// `GetGeometry`: `drawable`'s size, position, border and depth.
    ///
    /// On a screen's root window this is the screen's current size, which
    /// follows every `SetScreenSize` — unlike [`screen`](Self::screen), the
    /// copy taken at connect time.
    ///
    /// # Errors
    ///
    /// [`Error::Protocol`] with `BadDrawable` for an id that is no window or
    /// pixmap, or what else the request met.
    pub fn geometry(&self, drawable: Window) -> Result<Geometry, Error> {
        // SAFETY: the connection is live.
        let cookie = unsafe { (self.lib.get_geometry)(self.live()?, drawable) };
        let reply = self.reply::<ffi::GetGeometryReply>("GetGeometry", cookie)?;
        Ok(Geometry {
            root: reply.root,
            depth: reply.depth,
            x: reply.x,
            y: reply.y,
            width: reply.width,
            height: reply.height,
            border_width: reply.border_width,
        })
    }

    /// `GrabServer`: no other client's requests are processed until
    /// [`ungrab_server`](Self::ungrab_server), or until this connection
    /// closes, which ends the grab as well.
    ///
    /// # Errors
    ///
    /// What the request met.
    pub fn grab_server(&self) -> Result<(), Error> {
        // SAFETY: the connection is live.
        let cookie = unsafe { (self.lib.grab_server_checked)(self.live()?) };
        self.check("GrabServer", cookie)
    }

    /// `UngrabServer`.
    ///
    /// # Errors
    ///
    /// What the request met.
    pub fn ungrab_server(&self) -> Result<(), Error> {
        // SAFETY: the connection is live.
        let cookie = unsafe { (self.lib.ungrab_server_checked)(self.live()?) };
        self.check("UngrabServer", cookie)
    }

    /// The error a dead connection has, for a call that failed without an X
    /// error to say why.
    pub(crate) fn lost(&self) -> Error {
        // A failure with no X error and no connection error is not something
        // libxcb produces; `Other(0)` says so rather than inventing a cause.
        Error::Connection(self.has_error().unwrap_or(ConnectionError::Other(0)))
    }

    /// Waits for the verdict on a `_checked` request, named `request` for an
    /// [`Error::Timeout`].
    pub(crate) fn check(&self, request: &'static str, cookie: Cookie) -> Result<(), Error> {
        if self.reply_timeout.is_some() {
            // `xcb_request_check` has no deadline of its own. The server
            // answers in order, so once a round trip sent behind the request
            // is answered, so is the request, and `xcb_request_check` finds
            // its verdict already read rather than waiting for it.
            if let Err(error) = self.round_trip(request) {
                // SAFETY: the connection is live and the sequence is the
                // request's own. Its verdict will not be asked for now, so
                // libxcb drops it on arrival rather than keeping it.
                unsafe { (self.lib.discard_reply)(self.raw(), cookie.sequence) };
                return Err(error);
            }
        }
        // SAFETY: the connection is live, and every caller passes the cookie
        // of a `_checked` request it just sent on it, which is what
        // `xcb_request_check` requires.
        let error = unsafe { (self.lib.request_check)(self.raw(), cookie) };
        // SAFETY: a non-null result is a `malloc`'d error, ours to free.
        if let Some(error) = unsafe { take_error(error) } {
            return Err(Error::Protocol(error));
        }
        // `xcb_request_check` returns null for a dead connection too.
        self.has_error()
            .map_or(Ok(()), |error| Err(Error::Connection(error)))
    }

    /// Waits for the reply to `cookie`, a request of `request`'s whose reply
    /// is an `R` — what its generated `xcb_<request>_reply` would return,
    /// which is `xcb_wait_for_reply` cast to `R`.
    pub(crate) fn reply<R: WireReply>(
        &self,
        request: &'static str,
        cookie: Cookie,
    ) -> Result<Reply<R>, Error> {
        let mut error = ptr::null_mut();
        let raw = self.wait(request, cookie.sequence, &mut error)?.cast::<R>();
        // SAFETY: libxcb sets the out-pointer to a `malloc`'d error or leaves
        // it null; either way it is ours.
        let error = unsafe { take_error(error) };
        // SAFETY: a non-null reply is `malloc`'d and ours. Taking it into a
        // `Reply` first means it is freed on every path below.
        let reply = NonNull::new(raw).map(|raw| unsafe { Reply::adopt(raw, request) });
        if let Some(error) = error {
            return Err(Error::Protocol(error));
        }
        reply.ok_or_else(|| self.lost())?
    }

    /// Waits for the answer to request `sequence`, as `xcb_wait_for_reply`
    /// does — the reply or null, and `error` set to an error or null — but
    /// under the [reply timeout](Self::set_reply_timeout), if there is one.
    /// Leaves `error` null when it returns an error of its own.
    fn wait(
        &self,
        request: &'static str,
        sequence: c_uint,
        error: &mut *mut GenericError,
    ) -> Result<*mut c_void, Error> {
        // A timeout too long for the clock to reach is no deadline at all.
        let deadline = self
            .reply_timeout
            .and_then(|timeout| Instant::now().checked_add(timeout));
        let Some(deadline) = deadline else {
            // SAFETY: the connection is live, `sequence` is a request sent on
            // it whose reply no one else takes, and `error` is a live
            // out-pointer.
            return Ok(unsafe { (self.lib.wait_for_reply)(self.raw(), sequence, error) });
        };
        // `xcb_poll_for_reply`, unlike `xcb_wait_for_reply`, never writes, so
        // the request has to be on its way before its answer can be read.
        self.flush()?;
        loop {
            let mut reply = ptr::null_mut();
            // SAFETY: as for `xcb_wait_for_reply` above, and `reply` is a
            // live out-pointer. It reads what the socket has, queueing any
            // events for `poll_event`. That read is not wholly non-blocking:
            // once a packet's first 32 bytes are in, libxcb waits, holding
            // the connection's lock and with no deadline, for the rest of a
            // longer one, so a server that stops partway through a reply
            // still hangs this call.
            if unsafe { (self.lib.poll_for_reply)(self.raw(), sequence, &mut reply, error) } != 0 {
                return Ok(reply);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(self.abandon(Error::Timeout { request }, sequence));
            }
            // Each `poll` sleeps one `POLL_SLICE` at most, because the
            // socket is not the only way in for the reply. Another thread
            // can read it between `xcb_poll_for_reply` and `poll` —
            // `poll_event`, or a thread blocked in libxcb's own reader — and
            // libxcb then queues the reply with the socket empty and wakes no
            // one who is not waiting inside libxcb, so `poll` alone would
            // sleep to the deadline with the reply already there. The slice
            // bounds that to one turn of the loop.
            //
            // A readable socket returns early. With this thread the only
            // reader, the next `xcb_poll_for_reply` takes those bytes and the
            // `poll` after it sleeps again. With another thread blocked in
            // libxcb's reader, `xcb_poll_for_reply` leaves the bytes to it,
            // so they stay readable until that thread takes them, and the
            // loop turns without sleeping meanwhile; yielding gives that
            // thread the processor rather than racing it.
            match wait_readable(self.fd, left.min(POLL_SLICE)) {
                Ok(true) => std::thread::yield_now(),
                Ok(false) => {}
                // `poll` itself failing — out of memory, say — leaves no way
                // to wait for the reply, which may still come.
                Err(_) => {
                    return Err(self.abandon(Error::Connection(ConnectionError::Socket), sequence));
                }
            }
        }
    }

    /// Gives up on the connection's replies after a wait for `sequence`
    /// ended without one: libxcb is told to drop that reply when it comes,
    /// and every later request returns the first such error, unsent. Returns
    /// `error`, for this call.
    fn abandon(&self, error: Error, sequence: c_uint) -> Error {
        // SAFETY: the connection is live and `sequence` was sent on it. The
        // reply will never be asked for again; without this libxcb would keep
        // it, once it arrives, for as long as the connection lives.
        unsafe { (self.lib.discard_reply)(self.raw(), sequence) };
        self.abandoned.get_or_init(|| error);
        error
    }
}

/// The longest one `poll` in [`Connection::wait`] sleeps before asking libxcb
/// again: the most a reply another thread read off the socket waits to be
/// noticed, and how often a waiting thread wakes. See there for why.
const POLL_SLICE: Duration = Duration::from_millis(10);

/// Waits up to `left` for `fd` to be readable: `Ok(true)` when it is — or has
/// hung up or failed, which the next read reports — and `Ok(false)` when the
/// time ran out or a signal cut the wait short, which the caller's loop
/// treats alike, with whatever time is left.
fn wait_readable(fd: RawFd, left: Duration) -> io::Result<bool> {
    let mut pollfd = PollFd {
        fd,
        events: POLLIN,
        revents: 0,
    };
    // SAFETY: one live `pollfd`, and the count says one.
    let ready = unsafe { ffi::poll(&mut pollfd, 1, poll_millis(left)) };
    if ready >= 0 {
        return Ok(ready > 0);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::Interrupted {
        Ok(false)
    } else {
        Err(error)
    }
}

/// `poll`'s timeout for `left`: whole milliseconds rounded up, so that a wait
/// for less than one still sleeps instead of returning at once, and capped
/// at what a `c_int` holds.
fn poll_millis(left: Duration) -> c_int {
    let nanos_per_milli = u128::from(Duration::from_millis(1).subsec_nanos());
    c_int::try_from(left.as_nanos().div_ceil(nanos_per_milli)).unwrap_or(c_int::MAX)
}

impl AsRawFd for Connection {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl AsFd for Connection {
    fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the descriptor is the connection's socket, which libxcb
        // keeps open until `xcb_disconnect` in `drop`, so it outlives every
        // borrow of `self`.
        unsafe { BorrowedFd::borrow_raw(self.fd) }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: the connection came from `xcb_connect` and is disconnected
        // only here, after every borrow of `self` has ended.
        unsafe { (self.lib.disconnect)(self.raw()) };
    }
}

/// Why [`screen_in_setup`] found no screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SetupError {
    /// The setup lists fewer screens than the number asked for.
    NoSuchScreen,
    /// A count in the setup runs past its end.
    Malformed,
}

/// Screen `number` of the connection's setup.
///
/// The setup is walked here, over its bytes, rather than with libxcb's
/// `xcb_setup_roots_iterator` and `xcb_screen_next`: libxcb reads the setup in
/// whole but never checks the counts inside it against its length, and its
/// iterators step by those counts.
fn default_screen(
    lib: &Lib,
    raw: NonNull<ffi::Connection>,
    number: u32,
) -> Result<Screen, SetupError> {
    // SAFETY: the connection is live and connected, so the setup is there; it
    // is owned by the connection and outlives this function.
    let setup = unsafe { (lib.get_setup)(raw.as_ptr()) }.cast::<u8>();
    if setup.is_null() {
        return Err(SetupError::Malformed);
    }
    // SAFETY: libxcb's `read_setup` allocates the 8-byte setup header, reads
    // it whole, and then reallocates the block to `8 + 4 * length` and reads
    // that much (`xcb_conn.c`), so its first 8 bytes — `length` at byte 6
    // among them — are initialised.
    let length = unsafe { setup.add(6).cast::<u16>().read_unaligned() };
    let len = 8 + 4 * usize::from(length);
    // SAFETY: as above, the block is exactly `len` initialised bytes, owned by
    // the connection and never written after it is read.
    let bytes = unsafe { core::slice::from_raw_parts(setup, len) };
    screen_in_setup(bytes, number)
}

/// Screen `number` of a connection setup's bytes, every count in it checked
/// against them before it is followed.
///
/// The layout, from the protocol: the fixed part, the vendor string padded to
/// four bytes, the pixmap formats, then the screens, each its fixed part
/// followed by its depths, each depth its fixed part followed by its visuals.
fn screen_in_setup(setup: &[u8], number: u32) -> Result<Screen, SetupError> {
    use SetupError::Malformed;

    let header: ffi::Setup = read_plain(setup, 0).ok_or(Malformed)?;
    let vendor = usize::from(header.vendor_len)
        .checked_next_multiple_of(4)
        .ok_or(Malformed)?;
    let formats = usize::from(header.pixmap_formats_len) * PIXMAP_FORMAT_SIZE;
    let mut offset = size_of::<ffi::Setup>()
        .checked_add(vendor)
        .and_then(|offset| offset.checked_add(formats))
        .ok_or(Malformed)?;
    for index in 0..u32::from(header.roots_len) {
        let screen: ffi::Screen = read_plain(setup, offset).ok_or(Malformed)?;
        let mut end = offset
            .checked_add(size_of::<ffi::Screen>())
            .ok_or(Malformed)?;
        for _ in 0..screen.allowed_depths_len {
            let depth: ffi::Depth = read_plain(setup, end).ok_or(Malformed)?;
            end = usize::from(depth.visuals_len)
                .checked_mul(size_of::<ffi::VisualType>())
                .and_then(|visuals| visuals.checked_add(size_of::<ffi::Depth>()))
                .and_then(|depth| end.checked_add(depth))
                .ok_or(Malformed)?;
        }
        if end > setup.len() {
            return Err(Malformed);
        }
        if index == number {
            return Ok(Screen {
                number,
                root: screen.root,
                width: screen.width_in_pixels,
                height: screen.height_in_pixels,
                width_mm: screen.width_in_millimeters,
                height_mm: screen.height_in_millimeters,
            });
        }
        offset = end;
    }
    Err(SetupError::NoSuchScreen)
}

/// Bytes in one `xcb_format_t`: depth, bits per pixel, scanline pad, and five
/// of padding.
const PIXMAP_FORMAT_SIZE: usize = 8;

/// A `T` read from `bytes` at `offset`, or `None` when it would run past them.
fn read_plain<T: Plain>(bytes: &[u8], offset: usize) -> Option<T> {
    let bytes = bytes.get(offset..offset.checked_add(size_of::<T>())?)?;
    // SAFETY: `bytes` is exactly `size_of::<T>()` initialised bytes, and any
    // such bytes are a valid `T` (`Plain`). Unaligned, because a byte slice
    // promises no alignment.
    Some(unsafe { bytes.as_ptr().cast::<T>().read_unaligned() })
}

/// Copies an error libxcb handed over, and frees it.
///
/// # Safety
///
/// `error` must be null or a `malloc`'d `xcb_generic_error_t` no one else will
/// free.
unsafe fn take_error(error: *mut GenericError) -> Option<XError> {
    // SAFETY: the caller's contract: null or a live error, freed only here.
    let copied = unsafe { error.as_ref() }.map(XError::from_raw);
    // SAFETY: as above.
    unsafe { ffi::free_reply(error) };
    copied
}

/// A type any initialised bytes are a valid value of: what a reply's list is
/// copied out as, and what an event's bytes are read as.
///
/// # Safety
///
/// The implementor must be an integer, or a `#[repr(C)]` struct with no
/// implicit padding whose every field is an integer, an array of them, or
/// another `Plain` struct.
pub(crate) unsafe trait Plain: Copy {}

// SAFETY: integers: every bit pattern is a value.
unsafe impl Plain for u8 {}
// SAFETY: as above.
unsafe impl Plain for u16 {}
// SAFETY: as above.
unsafe impl Plain for u32 {}
// SAFETY: as above.
unsafe impl Plain for i32 {}
// SAFETY: `#[repr(C)]` structs of integers, transcribed from libxcb's
// headers with their padding as named fields; their sizes are pinned in
// `ffi`'s layout test.
unsafe impl Plain for ffi::RandrModeInfo {}
// SAFETY: as above.
unsafe impl Plain for ffi::RandrMonitorInfo {}
// SAFETY: as above.
unsafe impl Plain for ffi::Setup {}
// SAFETY: as above.
unsafe impl Plain for ffi::Screen {}
// SAFETY: as above.
unsafe impl Plain for ffi::Depth {}

/// A reply struct from [`crate::ffi`]: one that starts with the 8-byte header
/// every reply has, whose `length` at byte 4 counts the 4-byte units after the
/// first 32.
///
/// # Safety
///
/// The implementor must be the `#[repr(C)]` layout of a protocol reply, so
/// that header is where [`Reply`] reads it.
pub(crate) unsafe trait WireReply: Plain {}

/// Implements [`Plain`] and [`WireReply`] for reply structs.
macro_rules! wire_replies {
    ($($ty:ident),* $(,)?) => {
        $(
            // SAFETY: a `#[repr(C)]` reply struct of integers, transcribed
            // from libxcb's header with its padding as named fields; its size
            // is pinned in `ffi`'s layout test.
            unsafe impl Plain for ffi::$ty {}
            // SAFETY: as above; it is the reply's layout, header first.
            unsafe impl WireReply for ffi::$ty {}
        )*
    };
}

wire_replies!(
    GetAtomNameReply,
    GetGeometryReply,
    GetInputFocusReply,
    InternAtomReply,
    QueryExtensionReply,
    RandrCreateModeReply,
    RandrCrtcGammaSizeReply,
    RandrCrtcInfoReply,
    RandrGetCrtcTransformReply,
    RandrGetMonitorsReply,
    RandrGetOutputPropertyReply,
    RandrGetPanningReply,
    RandrListOutputPropertiesReply,
    RandrOutputInfoReply,
    RandrOutputPrimaryReply,
    RandrQueryOutputPropertyReply,
    RandrQueryVersionReply,
    RandrScreenResourcesReply,
    RandrScreenSizeRangeReply,
    RandrSetCrtcConfigReply,
    RandrSetPanningReply,
);

/// A reply libxcb allocated: read through `Deref`, its trailing lists copied
/// out with [`list`](Self::list), and freed exactly once, when dropped.
pub(crate) struct Reply<R> {
    raw: NonNull<R>,
    /// The allocation's size in bytes: `32 + 4 * length`.
    len: usize,
    /// The request, for [`Error::MalformedReply`].
    request: &'static str,
}

impl<R: WireReply> Reply<R> {
    /// Takes ownership of a reply, and refuses one too short to hold an `R`
    /// (freeing it all the same).
    ///
    /// # Safety
    ///
    /// `raw` must be a `malloc`'d block of exactly `32 + 4 * length`
    /// initialised bytes, `length` being the `u32` at byte 4, that no one else
    /// frees. A reply libxcb returned for `R`'s request is one: `read_packet`
    /// (`xcb_in.c`) allocates and reads a reply as exactly that.
    unsafe fn adopt(raw: NonNull<R>, request: &'static str) -> Result<Self, Error> {
        // SAFETY: the block is at least the 32 bytes of a reply's fixed part,
        // so the header's `length` at byte 4 is inside it.
        let length = unsafe {
            raw.as_ptr()
                .cast::<u8>()
                .add(4)
                .cast::<u32>()
                .read_unaligned()
        };
        let len = usize::try_from(length)
            .ok()
            .and_then(|length| length.checked_mul(4))
            .and_then(|extra| extra.checked_add(32))
            .unwrap_or(usize::MAX);
        let reply = Self { raw, len, request };
        if reply.len < size_of::<R>() {
            return Err(Error::MalformedReply { request });
        }
        Ok(reply)
    }

    /// The reply, for an accessor.
    pub(crate) const fn as_ptr(&self) -> *const R {
        self.raw.as_ptr()
    }

    /// The error for a reply whose contents contradict themselves.
    pub(crate) const fn malformed(&self) -> Error {
        Error::MalformedReply {
            request: self.request,
        }
    }

    /// Copies `count` values starting at `start`, once they are shown to lie
    /// inside the reply.
    pub(crate) fn list<T: Plain>(&self, start: *const T, count: usize) -> Result<Vec<T>, Error> {
        let malformed = self.malformed();
        let base = self.raw.as_ptr() as usize;
        let start_addr = start as usize;
        let end = count
            .checked_mul(size_of::<T>())
            .and_then(|bytes| start_addr.checked_add(bytes))
            .ok_or(malformed)?;
        let limit = base.checked_add(self.len).ok_or(malformed)?;
        if start_addr < base || end > limit {
            return Err(malformed);
        }
        Ok((0..count)
            // SAFETY: `start..end` was just shown to be inside the reply's
            // allocation, which is live for `self`'s lifetime, and any bytes
            // there are a valid `T` (`Plain`). The read is unaligned because
            // the protocol pads lists to four bytes, not to `T`.
            .map(|index| unsafe { start.add(index).read_unaligned() })
            .collect())
    }
}

impl<R> core::ops::Deref for Reply<R> {
    type Target = R;

    fn deref(&self) -> &R {
        // SAFETY: the reply is live until `drop`, and `Connection::reply`
        // only returns one whose allocation holds a whole `R`.
        unsafe { self.raw.as_ref() }
    }
}

impl<R> Drop for Reply<R> {
    fn drop(&mut self) {
        // SAFETY: the reply was handed to `adopt` by its only owner and is
        // freed only here.
        unsafe { ffi::free_reply(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_error_codes_map_to_their_names() {
        assert_eq!(ConnectionError::from_code(0), None);
        assert_eq!(ConnectionError::from_code(1), Some(ConnectionError::Socket));
        assert_eq!(
            ConnectionError::from_code(6),
            Some(ConnectionError::InvalidScreen)
        );
        assert_eq!(
            ConnectionError::from_code(42),
            Some(ConnectionError::Other(42))
        );
    }

    #[test]
    fn randr_errors_are_read_relative_to_the_first_error() {
        let extension = RandrExtension {
            major_opcode: 140,
            first_event: 89,
            first_error: 147,
            version: RandrVersion::REQUESTED,
        };
        let error = |error_code| XError {
            error_code,
            major_opcode: 140,
            minor_opcode: 20,
            resource_id: 0,
            sequence: 0,
        };
        assert_eq!(
            extension.error_kind(&error(148)),
            Some(RandrErrorKind::BadCrtc)
        );
        assert_eq!(extension.error_kind(&error(8)), None, "a core BadMatch");
        assert_eq!(extension.error_kind(&error(152)), None, "past RandR's");
    }

    #[test]
    fn poll_timeouts_round_up_to_whole_milliseconds_and_saturate() {
        assert_eq!(poll_millis(Duration::from_nanos(1)), 1);
        assert_eq!(poll_millis(Duration::from_millis(1)), 1);
        assert_eq!(poll_millis(Duration::from_micros(1001)), 2);
        assert_eq!(poll_millis(Duration::from_millis(300)), 300);
        assert_eq!(poll_millis(Duration::MAX), c_int::MAX);
    }

    #[test]
    fn versions_order_major_first() {
        assert!(RandrVersion { major: 1, minor: 4 } < RandrVersion::REQUESTED);
        assert!(RandrVersion { major: 2, minor: 0 } > RandrVersion::REQUESTED);
        assert!(RandrVersion::MINIMUM < RandrVersion::REQUESTED);
    }

    unsafe extern "C" {
        fn malloc(size: usize) -> *mut core::ffi::c_void;
    }

    /// A `malloc`'d reply block of `32 + 4 * length` zeroed bytes with
    /// `length` at byte 4, as libxcb hands one over.
    fn reply_block(length: u32) -> NonNull<u8> {
        let len = 32 + 4 * usize::try_from(length).expect("a test length");
        // SAFETY: a plain allocation; null is checked below.
        let block = NonNull::new(unsafe { malloc(len) }.cast::<u8>()).expect("malloc");
        // SAFETY: the block is `len` bytes, all written here before any read.
        unsafe {
            block.as_ptr().write_bytes(0, len);
            block.as_ptr().add(4).cast::<u32>().write_unaligned(length);
        }
        block
    }

    /// A `ListOutputProperties` reply whose `length` holds `length` atoms,
    /// counting up from one, and which claims `num_atoms` of them.
    fn atoms_reply(length: u32, num_atoms: u16) -> Reply<ffi::RandrListOutputPropertiesReply> {
        let block = reply_block(length);
        // SAFETY: the block is `32 + 4 * length` bytes; each write is inside.
        unsafe {
            block
                .as_ptr()
                .add(8)
                .cast::<u16>()
                .write_unaligned(num_atoms);
            for atom in 0..length {
                let at = 32 + 4 * usize::try_from(atom).expect("a test length");
                block
                    .as_ptr()
                    .add(at)
                    .cast::<u32>()
                    .write_unaligned(atom + 1);
            }
        }
        // SAFETY: the block is `malloc`'d, `32 + 4 * length` initialised
        // bytes, and owned by nothing else; the `Reply` frees it.
        unsafe { Reply::adopt(block.cast(), "ListOutputProperties") }.expect("a whole header")
    }

    /// Where a `ListOutputProperties` reply's atoms start: straight after the
    /// fixed part, as libxcb's accessor computes.
    fn atoms_of(reply: &Reply<ffi::RandrListOutputPropertiesReply>) -> *const u32 {
        reply.as_ptr().cast::<u8>().wrapping_add(32).cast()
    }

    #[test]
    fn a_list_that_ends_at_the_reply_end_is_copied() {
        let reply = atoms_reply(3, 3);
        assert_eq!(
            reply.list(atoms_of(&reply), usize::from(reply.num_atoms)),
            Ok(vec![1, 2, 3])
        );
    }

    #[test]
    fn a_list_that_runs_one_item_past_the_reply_is_refused() {
        let reply = atoms_reply(3, 4);
        assert_eq!(
            reply.list(atoms_of(&reply), usize::from(reply.num_atoms)),
            Err(Error::MalformedReply {
                request: "ListOutputProperties"
            })
        );
    }

    #[test]
    fn a_list_that_starts_before_the_reply_is_refused() {
        let reply = atoms_reply(3, 1);
        let before = reply.as_ptr().cast::<u32>().wrapping_sub(1);
        assert!(reply.list(before, 1).is_err());
    }

    #[test]
    fn a_reply_shorter_than_its_fixed_part_is_refused_and_freed() {
        // `GetPanning`'s fixed part is 36 bytes. With no extra length the
        // reply is 32, and its last field would be read past the block; one
        // unit more holds it exactly.
        for (length, whole) in [(0, false), (1, true)] {
            let block = reply_block(length);
            // SAFETY: as in `atoms_reply`; `adopt` frees a refused block too.
            let reply =
                unsafe { Reply::<ffi::RandrGetPanningReply>::adopt(block.cast(), "GetPanning") };
            match reply {
                Ok(_) => assert!(whole, "extra length {length} was accepted"),
                Err(error) => {
                    assert!(!whole, "extra length {length} was refused");
                    assert_eq!(
                        error,
                        Error::MalformedReply {
                            request: "GetPanning"
                        }
                    );
                }
            }
        }
    }

    /// A setup with one screen of `depths` depths, each of `visuals` visuals,
    /// after a 5-byte vendor string and two pixmap formats, with `cut` bytes
    /// taken off its end. Field offsets are `xcb_setup_t`'s and
    /// `xcb_screen_t`'s.
    fn setup_bytes(depths: u8, visuals: u16, cut: usize) -> Vec<u8> {
        let vendor: u16 = 5;
        let formats: u8 = 2;
        let mut bytes = vec![0u8; size_of::<ffi::Setup>()];
        bytes[24..26].copy_from_slice(&vendor.to_ne_bytes());
        bytes[28] = 1; // roots_len
        bytes[29] = formats;
        // The vendor string, padded to a multiple of four, then the formats.
        bytes.resize(
            bytes.len() + 8 + usize::from(formats) * PIXMAP_FORMAT_SIZE,
            0,
        );
        let screen = bytes.len();
        bytes.resize(screen + size_of::<ffi::Screen>(), 0);
        bytes[screen..screen + 4].copy_from_slice(&0x100u32.to_ne_bytes());
        bytes[screen + 20..screen + 22].copy_from_slice(&1920u16.to_ne_bytes());
        bytes[screen + 22..screen + 24].copy_from_slice(&1080u16.to_ne_bytes());
        bytes[screen + 39] = depths;
        for _ in 0..depths {
            let depth = bytes.len();
            bytes.resize(depth + size_of::<ffi::Depth>(), 0);
            bytes[depth + 2..depth + 4].copy_from_slice(&visuals.to_ne_bytes());
            bytes.resize(
                bytes.len() + usize::from(visuals) * size_of::<ffi::VisualType>(),
                0,
            );
        }
        bytes.truncate(bytes.len() - cut);
        bytes
    }

    #[test]
    fn a_setup_screen_is_read_past_the_vendor_formats_and_depths() {
        let screen = screen_in_setup(&setup_bytes(2, 3, 0), 0).expect("one whole screen");
        assert_eq!(
            (screen.root, screen.width, screen.height),
            (0x100, 1920, 1080)
        );
        assert_eq!(
            screen_in_setup(&setup_bytes(2, 3, 0), 1),
            Err(SetupError::NoSuchScreen)
        );
    }

    #[test]
    fn a_setup_whose_counts_run_past_its_end_is_malformed() {
        assert_eq!(
            screen_in_setup(&setup_bytes(2, 3, 1), 0),
            Err(SetupError::Malformed),
            "the last visual cut short"
        );
        assert_eq!(
            screen_in_setup(&setup_bytes(1, 0, 1), 0),
            Err(SetupError::Malformed),
            "a depth header cut short"
        );
        assert_eq!(
            screen_in_setup(&setup_bytes(0, 0, 1), 0),
            Err(SetupError::Malformed),
            "a screen cut short"
        );
    }

    /// The tests that load libxcb, which Miri cannot run.
    mod libxcb {
        use super::super::*;

        #[test]
        fn a_display_with_no_server_is_a_typed_connect_error() {
            // A display number no harness or desktop uses, so nothing listens on
            // its socket.
            let error = Connection::connect(Some(":64999")).expect_err("nothing listens there");
            assert_eq!(
                error,
                ConnectError::Failed {
                    display: Some(":64999".to_owned()),
                    error: ConnectionError::Socket,
                }
            );
        }

        #[test]
        fn a_name_with_a_nul_byte_is_refused_before_libxcb_sees_it() {
            assert_eq!(
                Connection::connect(Some(":0\0"))
                    .expect_err("the name has a NUL")
                    .to_string(),
                ConnectError::InvalidName.to_string()
            );
        }
    }
}
