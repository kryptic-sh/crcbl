//! Crucible's binding to libwayland-client.
//!
//! The runtime half of `crcbl-wl-scanner`: generated protocol code names this
//! crate's `ffi` module as its `Options::ffi_path`, and marshals every request
//! and decodes every event through it. `crcbl-shell`'s Wayland backend is built
//! on it, and so can a program outside the engine that generates its own
//! protocols and talks to the compositor on its own connection, which
//! `Display` opens and drives.
//!
//! **Linux and FreeBSD only.** Elsewhere there is no libwayland, and the crate
//! is empty rather than a binding that fails at run time.

#[cfg(desktop_unix)]
mod display;
#[cfg(desktop_unix)]
pub mod ffi;

#[cfg(desktop_unix)]
pub use display::{ConnectError, Disconnected, Display, ReadGuard};
