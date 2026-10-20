//! What the two Linux backends share, and deliberately nothing else.
//!
//! Not `pub`, like the backends themselves.
//!
//! Wayland (P0.5) and X11 (P0.6) are different protocols reached through
//! different libraries, and almost nothing crosses between them. What does
//! cross does so because it is a property of **Linux** rather than of either
//! window system:
//!
//! * [`keymap`] — the `linux/input-event-codes.h` table that turns a physical
//!   key position into a [`KeyCode`](crcbl_core::KeyCode). Wayland delivers
//!   evdev codes directly; X11 delivers the same numbers plus eight (see
//!   [`xkb::EVDEV_OFFSET`], which is X11's offset in the first place and was
//!   inherited by Wayland's keymaps, not the other way round). Two copies of
//!   this table would drift, and the drift would show up as one backend
//!   disagreeing with the other about what `WASD` is.
//! * [`xkb`] — libxkbcommon, which is where both backends get keysyms, text
//!   and modifier state from. The keymap *source* differs (Wayland sends a
//!   descriptor, X11 has the keymap on the server) and the compiled artefact
//!   and every question asked of it are identical.
//! * [`wake`] — the pipe a [`ShellWaker`](crate::ShellWaker) rings. Both
//!   waits are a `poll`, so both are woken the same way: one more descriptor
//!   in the poll set.
//!
//! What is **not** here is anything protocol-shaped. `wayland/fd.rs` stays
//! where it is even though descriptors are a Linux concept, because what it
//! actually encodes is the discipline for descriptors that arrived over the
//! *Wayland socket* — X11 transfers selections through server properties and
//! has no pipe anywhere. Moving it here to look symmetrical would put a module
//! with one caller behind a name that suggests two.
//!
//! This module is compiled where the backends are: under `desktop_unix`,
//! which `build.rs` sets for Linux and FreeBSD, rather than `#[cfg(unix)]`,
//! because macOS is a Unix with neither of these libraries on it. The name
//! stays `linux` because Linux is where it is run and tested; FreeBSD is
//! checked to compile, by CI.
//!
//! The two systems agree on every number this crate's hand-written libc
//! declarations use but the two below, which are defined once here so neither
//! backend can carry Linux's value onto FreeBSD.

use core::ffi::c_int;

/// `CLOCK_MONOTONIC`, the clock Wayland and X11 stamp input with: 1 on Linux,
/// 4 on FreeBSD (`sys/_clock_id.h`).
#[cfg(target_os = "linux")]
pub(crate) const CLOCK_MONOTONIC: c_int = 1;
/// See the Linux definition.
#[cfg(target_os = "freebsd")]
pub(crate) const CLOCK_MONOTONIC: c_int = 4;

/// `nfds_t`, `poll`'s descriptor count: `unsigned long` on Linux, `unsigned
/// int` on FreeBSD.
#[cfg(target_os = "linux")]
pub(crate) type Nfds = core::ffi::c_ulong;
/// See the Linux definition.
#[cfg(target_os = "freebsd")]
pub(crate) type Nfds = core::ffi::c_uint;

#[cfg(feature = "accesskit")]
pub(crate) mod access;
pub mod keymap;
pub mod wake;
pub mod xkb;
