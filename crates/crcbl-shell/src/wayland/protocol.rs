//! The generated protocol layer.
//!
//! Produced by `crcbl-wl-scanner` from the XML in
//! `crates/crcbl-shell/wayland-protocols/`; see `build.rs`. The file is
//! `include!`d rather than checked in so that a protocol update is a one-file
//! change with no chance of the committed Rust drifting from the XML it claims
//! to come from.
//!
//! Layout of what lands here:
//!
//! ```text
//! protocol::wayland::wl_surface::{INTERFACE, REQ_COMMIT, commit, Event, decode_event}
//! protocol::xdg_shell::xdg_toplevel::{INTERFACE, set_title, state::FULLSCREEN, …}
//! ```
//!
//! Every request wrapper is `unsafe` and marshals through
//! [`ffi::marshal`](super::ffi::marshal); every event decoder is `unsafe` and
//! borrows from libwayland's closure storage for the duration of the dispatch
//! call. See `crcbl-wl-scanner`'s docs for the shape and the tests that pin the
//! signature strings and type tables.

// The generator emits every interface in every vendored XML, which is what
// makes P0.5b's `wl_seat` and P0.5c's `wl_data_device` free. Most of it is
// unused in this slice by design, and `wl_shm`/`wl_shell` will stay unused
// forever — the engine presents through Vulkan, not through shared memory.
#![allow(dead_code)]

include!(concat!(env!("OUT_DIR"), "/wayland_protocol.rs"));
