//! What the two Linux backends share, and deliberately nothing else.
//!
//! Wayland (P0.5) and X11 (P0.6) are different protocols reached through
//! different libraries, and almost nothing crosses between them. Two things do,
//! because they are properties of **Linux** rather than of either window
//! system:
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
//!
//! What is **not** here is anything protocol-shaped. `wayland/fd.rs` stays
//! where it is even though descriptors are a Linux concept, because what it
//! actually encodes is the discipline for descriptors that arrived over the
//! *Wayland socket* — X11 transfers selections through server properties and
//! has no pipe anywhere. Moving it here to look symmetrical would put a module
//! with one caller behind a name that suggests two.
//!
//! This module is compiled only on Linux, for the same reason the backends
//! are: `#[cfg(target_os = "linux")]` rather than `#[cfg(unix)]`, because macOS
//! is a Unix with neither of these libraries on it.

pub mod keymap;
pub mod xkb;
