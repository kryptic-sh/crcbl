//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/tide` is a `cdylib` on `wasm32-unknown-unknown`, and this module is the
//! only thing in it a browser can reach. Everything here is an `extern "C"`
//! export with `#[unsafe(no_mangle)]`; there are **no imports**.
//!
//! # What is this sample's, and what is every sample's
//!
//! The lifecycle's state machine, the log queue and the five-call protocol are
//! [`crcbl::web`], and [`crcbl::web_exports!`] writes the ten symbols listed
//! below; that module is where the reasons live. What is tide's is the
//! [`WebPending`](crcbl::web::WebPending) impl, which opens the gallery with its
//! own [`Options`], and **the knobs** — written out one symbol per line, because
//! two demos can be open in one browser and their exports must not collide.
//!
//! # Why a page needs knobs of its own
//!
//! Natively the scene, the medium and the camera are `N`, `M` and `C` and rows
//! on the pause panel. A phone has none of those, and a gallery whose scenes
//! cannot be switched is one scene. So each is an export here, writing
//! [`crate::knobs`]' cell — the same cell a key and a pause row write — and
//! answering with what the cell holds **after** the write, which is what the
//! next frame stages. `crate::app::Tide`'s `[HUD]` heartbeat then prints what
//! the frame actually staged, which is where a browser gate reads the effect.
//!
//! # The symbols this module exports
//!
//! `__crcbl_tide_` is this module's prefix. The ten lifecycle symbols are
//! [`__crcbl_tide_prepare`], [`__crcbl_tide_log_level`], [`__crcbl_tide_boot`],
//! [`__crcbl_tide_frame`], [`__crcbl_tide_status`], [`__crcbl_tide_shutdown`],
//! [`__crcbl_tide_error_ptr`], [`__crcbl_tide_error_len`],
//! [`__crcbl_tide_log_take`] and [`__crcbl_tide_log_ptr`]; what each means is
//! [`crcbl::web`]'s module docs.
//!
//! ## Exports: the knobs
//!
//! Each knob is a **length and an address**, on `apps/sundial/src/web.rs`'
//! filter export's argument: the page reads the name the engine holds rather
//! than keeping a list of its own that goes stale when a fifth preset lands. A
//! zero argument reads; a non-zero one moves the knob on first.
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_tide_scene`] | `(i32) -> i32` | Non-zero moves on to the next scene, as `N` does. Returns the **length** of the scene's name now in force. |
//! | [`__crcbl_tide_scene_ptr`] | `() -> i32` | Address of that name (UTF-8, not NUL-terminated). Read it after the call above. |
//! | [`__crcbl_tide_medium`] | `(i32) -> i32` | Non-zero moves on to the next medium preset, as `M` does. Returns the length of its name. |
//! | [`__crcbl_tide_medium_ptr`] | `() -> i32` | Address of that name. |
//! | [`__crcbl_tide_camera`] | `(i32) -> i32` | Non-zero moves on to the next camera, as `C` does. Returns the length of its name. |
//! | [`__crcbl_tide_camera_ptr`] | `() -> i32` | Address of that name. |
//! | [`__crcbl_tide_reset`] | `()` | Every knob back to where a fresh run opens — the `R` key. |

use crate::app::{Loop, PendingLoop};
use crate::args::Options;
use crate::knobs::{self, Knobs};

// ---------------------------------------------------------------------------
// This sample's half of the lifecycle
// ---------------------------------------------------------------------------

// **`WebPending` is deliberately not imported**, on sundial's terms: the macro's
// guard resolves `PendingLoop::poll` by path, and an import would let it resolve
// to the trait method instead. See `crcbl::impl_web_pending`.
crcbl::impl_web_pending!(PendingLoop, Loop, Options, crate::app::TideError);

// ---------------------------------------------------------------------------
// Exports: the lifecycle
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_tide_prepare,
    log_level: __crcbl_tide_log_level,
    boot: __crcbl_tide_boot,
    frame: __crcbl_tide_frame,
    status: __crcbl_tide_status,
    shutdown: __crcbl_tide_shutdown,
    error_ptr: __crcbl_tide_error_ptr,
    error_len: __crcbl_tide_error_len,
    log_take: __crcbl_tide_log_take,
    log_ptr: __crcbl_tide_log_ptr,
}

// ---------------------------------------------------------------------------
// Exports: the knobs
// ---------------------------------------------------------------------------

/// A name's length, for a page that reads the bytes at the matching `_ptr`.
fn length(name: &'static str) -> u32 {
    u32::try_from(name.len()).unwrap_or(0)
}

/// The knobs, moved on by `cycle` first when it is non-zero.
fn after(cycle: i32, write: fn() -> Knobs) -> Knobs {
    if cycle == 0 { knobs::read() } else { write() }
}

/// Move on to the next scene, and answer with the length of the name in force.
///
/// `cycle` of `0` reads. The name is at [`__crcbl_tide_scene_ptr`], and the two
/// calls are one read: nothing on a page's thread can move the cell between
/// them.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_tide_scene(cycle: i32) -> u32 {
    length(after(cycle, knobs::cycle_scene).scene.label())
}

/// Address of the scene's name, UTF-8 and not NUL-terminated — a
/// `&'static str`, so it stays valid for as long as the module is loaded.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_tide_scene_ptr() -> *const u8 {
    knobs::read().scene.label().as_ptr()
}

/// Move on to the next medium preset, and answer with the length of its name.
///
/// `cycle` of `0` reads; the name is at [`__crcbl_tide_medium_ptr`].
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_tide_medium(cycle: i32) -> u32 {
    length(after(cycle, knobs::cycle_medium).medium.label())
}

/// Address of the medium preset's name, on [`__crcbl_tide_scene_ptr`]'s terms.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_tide_medium_ptr() -> *const u8 {
    knobs::read().medium.label().as_ptr()
}

/// Move on to the next camera, and answer with the length of its name.
///
/// `cycle` of `0` reads; the name is at [`__crcbl_tide_camera_ptr`].
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_tide_camera(cycle: i32) -> u32 {
    length(after(cycle, knobs::cycle_camera).camera.label())
}

/// Address of the camera's name, on [`__crcbl_tide_scene_ptr`]'s terms.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_tide_camera_ptr() -> *const u8 {
    knobs::read().camera.label().as_ptr()
}

/// Every knob back to where a fresh run opens — the `R` key. A page reads each
/// back through the calls above.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_tide_reset() {
    knobs::reset();
}
