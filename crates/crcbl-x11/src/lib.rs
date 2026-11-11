//! Crucible's binding to libxcb: the X11 core protocol and RandR.
//!
//! `crcbl-shell`'s X11 backend is built on it, and so can a program outside the
//! engine that talks to an X server on its own connection — to read and set
//! the output configuration through RandR, say, which [`Connection`] opens and
//! drives. `crcbl-shell` adds the extension libraries only a windowing backend
//! needs (XInput 2, XKB, libxcb-cursor, SHAPE, DRI3) on top of this crate's
//! loader.
//!
//! **Linux and FreeBSD only.** Elsewhere there is no libxcb, and the crate is
//! empty rather than a binding that fails at run time.

#[cfg(desktop_unix)]
mod connection;
#[cfg(desktop_unix)]
mod event;
#[cfg(desktop_unix)]
pub mod ffi;
#[cfg(desktop_unix)]
mod randr;

#[cfg(desktop_unix)]
pub use connection::{
    Atom, ConnectError, Connection, ConnectionError, Error, RandrErrorKind, RandrExtension,
    RandrVersion, Screen, Timestamp, Window, XError,
};
#[cfg(desktop_unix)]
pub use event::{
    CrtcChangeNotify, Event, OutputChangeNotify, OutputPropertyNotify, PropertyState,
    ScreenChangeNotify,
};
#[cfg(desktop_unix)]
pub use randr::{
    Crtc, CrtcConfig, CrtcInfo, CrtcTransform, Mode, ModeInfo, MonitorInfo, Monitors, NotifyMask,
    Output, OutputConnection, OutputInfo, OutputPropertyData, Panning, PropertyInfo, PropertyMode,
    PropertyValue, Rotation, ScreenResources, ScreenSizeRange, SetConfigReply, SetConfigStatus,
    SubPixel, Transform, mode_flag,
};
