//! Core primitives shared by every Crucible crate.
//!
//! Nothing here depends on a backend, a window, or a renderer — this is the
//! vocabulary layer that the rest of the engine is written in:
//!
//! * [`handle`] — [`Handle`], a typed generational id, and [`Pool`], the slot
//!   arena that issues and invalidates them.
//! * [`world`] — [`WorldPos`], the sector-tiled absolute position that every
//!   simulated thing in the engine uses. Plain `Vec3` is *only* ever
//!   camera-relative render space; see [`WorldPos::relative_to`].
//! * [`alloc`] — [`FrameArena`], a fixed-capacity bump allocator for per-frame
//!   transient data.
//! * [`time`] — [`FrameClock`], the fixed-timestep server tick accumulator plus
//!   variable render delta, and [`TickId`].
//! * [`log`] — stderr logging behind the [`log`](::log) facade, filtered by
//!   `CRCBL_LOG`.
//! * [`surface`] — [`SurfaceTarget`], the native window handles the shell
//!   produces and a HAL backend consumes. It lives here, and not in either of
//!   those crates, because neither may depend on the other; see the module docs
//!   for the dependency argument.
//!
//! Input normalization (`crcbl-core::input`) lands with the shell slice, not
//! here.

pub mod alloc;
pub mod handle;
pub mod log;
pub mod surface;
pub mod time;
pub mod world;

pub use alloc::FrameArena;
pub use handle::{Handle, Pool};
pub use surface::SurfaceTarget;
pub use time::{FrameClock, TickId};
pub use world::{SECTOR_SIZE, WorldPos};
