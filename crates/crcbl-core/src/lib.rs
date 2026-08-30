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
//!   variable render delta, [`TickId`], and [`EventTime`], the stated-epoch
//!   timestamp every input event carries.
//! * [`input`] — the names for keys, buttons and axes ([`KeyCode`],
//!   [`Keysym`](input::Keysym), [`PointerButton`](input::PointerButton),
//!   [`Modifiers`](input::Modifiers)). Vocabulary only: the *event* enum lives
//!   in `crcbl-shell`, which is the only crate that produces one.
//! * [`log`] — stderr logging behind the [`log`](::log) facade, filtered by
//!   `CRCBL_LOG`, with the console's log ring in [`log::console`] and the `log`
//!   command that sets the filter while the engine runs.
//! * [`rand`] — deterministic values from an index, for simulations that
//!   replay. Deliberately not a generator; see the module docs for why every
//!   sample independently arrived at the same shape.
//! * [`stats`] — [`percentile_of`](stats::percentile_of) and
//!   [`MIN_PERCENTILE_SAMPLES`](stats::MIN_PERCENTILE_SAMPLES), the nearest-rank
//!   arithmetic the debug panel's budget row and `crcbl bench` both report
//!   through. No mean, on purpose; see the module docs.
//! * [`mod@trace`] — [`Span`](trace::Span) and [`counter`](trace::counter), the CPU
//!   half of the profiler: scoped spans with static names, nesting freely and
//!   carrying the thread they ran on, plus named `u64` counters. Compiled into
//!   every build and gated at runtime; see the module docs for what that costs.
//! * [`surface`] — [`SurfaceTarget`], the native window handles the shell
//!   produces and a HAL backend consumes. It lives here, and not in either of
//!   those crates, because neither may depend on the other; see the module docs
//!   for the dependency argument.

pub mod alloc;
pub mod bounds;
pub mod handle;
pub mod input;
pub mod log;
pub mod rand;
pub mod stats;
pub mod surface;
pub mod time;
pub mod trace;
pub mod world;

/// Everything this crate exposes to the debug console.
///
/// One list per crate, gathered by the engine at one seam —
/// `docs/plan/52-debug-console.md` decision 2. It holds `log`, the command that
/// reads and sets the live filter; `tests/console_table.rs` is what keeps it in
/// step with what the source actually declares.
#[must_use]
pub fn console_table() -> crcbl_console::Table {
    crcbl_console::table![cmd crate::log::log]
}

pub use alloc::FrameArena;
pub use handle::{Handle, Pool};
pub use input::KeyCode;
pub use surface::SurfaceTarget;
pub use time::{EventTime, FrameClock, TickId};
pub use world::{SECTOR_SIZE, WorldPos};
