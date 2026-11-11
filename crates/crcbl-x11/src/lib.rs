//! Crucible's binding to libxcb: the X11 core protocol and RandR.
//!
//! `crcbl-shell`'s X11 backend is built on it, and so can a program outside the
//! engine that talks to an X server on its own connection — to read and set
//! the output configuration through RandR, say. `crcbl-shell` adds the
//! extension libraries only a windowing backend needs (XInput 2, XKB,
//! libxcb-cursor, SHAPE, DRI3) on top of this crate's loader.
//!
//! **Linux and FreeBSD only.** Elsewhere there is no libxcb, and the crate is
//! empty rather than a binding that fails at run time.

#[cfg(desktop_unix)]
pub mod ffi;
