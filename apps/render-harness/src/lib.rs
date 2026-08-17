//! The browser half of the WebGPU parity gate: drive crcbl's backend-agnostic
//! golden [`Scene`] set through whichever GPU backend
//! the wasm was built against, open each one offscreen, and report — per scene —
//! how far the open got, so the gate can catalogue exactly where the browser
//! backend diverges from the native `vk`/`mtl`/`dx12` suites.
//!
//! # Why this is a crate of its own, and not a demo
//!
//! `apps/breakout` and the other samples are games driven through
//! [`crcbl::web`]'s shell/storage/audio lifecycle. This is not a game: it opens
//! no window, presents to no canvas, and takes no input. It drives
//! [`crcbl::screenshot`]'s poll API — the non-blocking `OffscreenSetup` state
//! machine shipped so a browser could read a frame back across
//! `requestAnimationFrame` frames without blocking the one thread it has — and
//! nothing else. So it hand-writes its own small ABI rather than reaching for
//! [`crcbl::web_exports!`].
//!
//! # The mechanism, frame by frame
//!
//! JS calls [`start`](shim::__crcbl_render_harness_start) once per scene, then
//! [`step`](shim::__crcbl_render_harness_step) once per rAF frame while pumping
//! the GPU command stream (`web/engine/gpu-transport.js` +
//! `web/engine/gpu-replay.js`, the same drain→replay→deliver loop the demos
//! run). Each `step` advances the open one poll:
//!
//! 1. [`OffscreenSetup::request`] built a [`PendingOffscreen`];
//!    its `poll` drives the instance open (adapter enumeration over the stream)
//!    and then the device open. On the WebGPU backend, opening the instance
//!    installs the stream channel, so the pump has something to drain from the
//!    very first frame.
//! 2. When the instance is open, `poll` synchronously creates the offscreen
//!    surface, selects the adapter, and starts the device request. **This is
//!    where the WebGPU backend refuses today** — see the note below.
//! 3. When the device is ready, `poll` hands over the
//!    [`OffscreenSetup`] and the harness
//!    reaches `State::Opened`.
//!
//! # What this slice does NOT do, and why
//!
//! It stops at `State::Opened`: it does not yet drive
//! [`OffscreenSetup::begin_readback`]
//! and its `PendingReadback` to a frame of pixels. Two reasons, and the second
//! makes the first moot:
//!
//! - The readback drive has to persist a `PendingReadback` — which borrows its
//!   `OffscreenSetup` — across rAF frames, a self-referential hold that wants
//!   its own careful slice.
//! - **No backend in the browser can reach it to be tested.** `crcbl-webgpu`'s
//!   HAL refuses the offscreen surface `State::Opened` needs before a single
//!   pixel is drawn: `crates/crcbl-webgpu/src/hal/instance.rs`'s `create_surface`
//!   returns `HalError::Unsupported` for `SurfaceTarget::Offscreen`, and
//!   `crates/crcbl-webgpu/src/command.rs`'s `CreateSurface` doc says the
//!   offscreen variant "gets its own command when the parity gate needs to read
//!   frames back". So opening is the whole of what the mechanism can be driven
//!   through today, and opening is exactly where the wall is.
//!
//! The pixel-readback half of the ABI is the follow-up slice, unblocked once
//! that offscreen surface command lands.
//!
//! # ABI
//!
//! Every symbol is a plain integer in and a plain integer out; wasm owns all the
//! memory and JS never passes a pointer in. `#[unsafe(no_mangle)]` is applied
//! only on `wasm32`, so a native build (the one clippy and `cargo fmt` see)
//! type-checks the module without exporting anything.
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_render_harness_scene_count`](shim::__crcbl_render_harness_scene_count) | `() -> i32` | How many scenes there are to drive. |
//! | [`__crcbl_render_harness_scene_name_ptr`](shim::__crcbl_render_harness_scene_name_ptr) | `(i32) -> i32` | Address of scene `i`'s golden name (UTF-8), or `0` if `i` is out of range. |
//! | [`__crcbl_render_harness_scene_name_len`](shim::__crcbl_render_harness_scene_name_len) | `(i32) -> i32` | That name's length in bytes, or `0`. |
//! | [`__crcbl_render_harness_start`](shim::__crcbl_render_harness_start) | `(i32) -> i32` | Begin opening scene `i`. `1` on start, `0` on a bad index. Overrides any scene in flight. |
//! | [`__crcbl_render_harness_step`](shim::__crcbl_render_harness_step) | `() -> i32` | Advance the open one poll and return the new `State` code. |
//! | [`__crcbl_render_harness_state`](shim::__crcbl_render_harness_state) | `() -> i32` | The `State` code, without advancing. |
//! | [`__crcbl_render_harness_error_ptr`](shim::__crcbl_render_harness_error_ptr) | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_render_harness_error_len`](shim::__crcbl_render_harness_error_len) | `() -> i32` | Its length in bytes, or `0`. |
//!
//! ## State codes
//!
//! `State::Idle` `0`, `State::Opening` `1`, `State::Opened` `2`,
//! `State::Failed` `3`. `Failed` is the only one that sets an error message.

use std::cell::RefCell;

use crcbl::screenshot::{OffscreenSetup, PendingOffscreen, Scene};

/// The frame every golden was blessed at — the same `256x192` the native
/// `crcbl/tests/render_e2e.rs` suite and `crcbl-vk`'s mesh suite use, so a
/// readback here can eventually be compared against
/// `crcbl/tests/golden/<name>.png` at the suite's tolerance.
const EXTENT: (u32, u32) = (256, 192);

/// Every scene this gate drives, paired with the golden basename its native
/// counterpart compares against (`crcbl/tests/golden/<name>.png`).
///
/// One source of truth for the mapping: JS reads the names back through
/// [`scene_name`](shim::__crcbl_render_harness_scene_name_ptr) rather than
/// carrying its own copy that could drift from this one.
const SCENES: &[(Scene, &str)] = &[
    (Scene::Cube, "cube"),
    (Scene::Dunes, "dunes"),
    (Scene::Sprite, "sprite"),
    (Scene::Lights, "lights"),
    (Scene::Spot, "spot"),
    (Scene::SpotShadow, "spot_shadow"),
    (Scene::PointShadow, "point_shadow"),
    (Scene::Ao, "ao"),
    (Scene::Ssr, "ssr"),
    (Scene::Probes, "probes"),
    (Scene::Ui, "ui"),
];

/// Where the harness has got to with the scene it was last [`start`]ed on.
///
/// The numeric values are the wire codes [`state`] hands JS; see the module
/// docs' state table.
///
/// [`start`]: shim::__crcbl_render_harness_start
/// [`state`]: shim::__crcbl_render_harness_state
#[derive(Default)]
enum State {
    /// Nothing has been started.
    #[default]
    Idle,
    /// An open is in flight; `poll` it each frame.
    Opening(Box<PendingOffscreen<'static>>),
    /// The device opened, the scene's renderer built, and the setup was torn
    /// down to idle cleanly. The readback drive is the follow-up slice — see the
    /// module docs — which will keep the setup and draw a frame off it where
    /// this one finishes it; nothing in the browser reaches this state yet.
    Opened,
    /// The open (or a later step) failed; the message is in [`Harness::error`].
    Failed,
}

impl State {
    /// The wire code JS reads, kept next to the `enum` so the two cannot drift.
    fn code(&self) -> u32 {
        match self {
            State::Idle => 0,
            State::Opening(_) => 1,
            State::Opened => 2,
            State::Failed => 3,
        }
    }
}

/// The harness's whole state: the scene it is driving and the last error text.
///
/// One per wasm instance, behind a `thread_local` `RefCell` — the browser's one
/// thread is the only caller, and every export borrows it for the length of one
/// call and no longer.
#[derive(Default)]
struct Harness {
    state: State,
    /// The last error, kept as owned bytes so its pointer is stable between the
    /// `error_ptr`/`error_len` pair a reader calls back to back.
    error: String,
}

thread_local! {
    static HARNESS: RefCell<Harness> = RefCell::new(Harness::default());
}

/// Runs `f` with the single harness borrowed mutably.
fn with_harness<R>(f: impl FnOnce(&mut Harness) -> R) -> R {
    HARNESS.with(|cell| f(&mut cell.borrow_mut()))
}

impl Harness {
    /// Begin opening `scene`, discarding whatever was in flight.
    fn start(&mut self, index: usize) -> bool {
        let Some(&(scene, _)) = SCENES.get(index) else {
            return false;
        };
        self.error.clear();
        match OffscreenSetup::request(EXTENT.0, EXTENT.1, scene) {
            Ok(pending) => self.state = State::Opening(Box::new(pending)),
            // The request itself was accepted as a call — the size was valid —
            // so this still counts as a start; the failure is reported through
            // `state`, which the caller reads next, exactly as a later poll's
            // would be.
            Err(error) => self.fail(error),
        }
        true
    }

    /// Advance the open one poll and return the new state code.
    fn step(&mut self) -> u32 {
        // Take the state out so a borrowed `PendingOffscreen` can be polled and
        // put back, or replaced.
        match std::mem::take(&mut self.state) {
            State::Opening(mut pending) => match pending.poll() {
                Ok(None) => self.state = State::Opening(pending),
                // Finished immediately: this slice proves the open reaches a
                // usable device and releases it cleanly — `finish` waits the
                // device idle, so a device lost during the open surfaces here.
                // The follow-up readback slice keeps the setup instead and
                // draws a frame off it before finishing.
                Ok(Some(setup)) => match setup.finish() {
                    Ok(()) => self.state = State::Opened,
                    Err(error) => self.fail(error),
                },
                Err(error) => self.fail(error),
            },
            // Terminal states put themselves back unchanged. `Opened` is where
            // the mechanism reaches once the offscreen surface command lands;
            // today the WebGPU backend fails before it, in `Opening`.
            other => self.state = other,
        }
        self.state.code()
    }

    /// Record `error` and move to `State::Failed`.
    fn fail(&mut self, error: impl std::fmt::Display) {
        self.error = error.to_string();
        self.state = State::Failed;
    }
}

/// The JS→wasm ABI. See the [module docs](self) for the whole contract.
///
/// None of these is `unsafe`: none dereferences a pointer the caller supplied.
/// The two that return an address hand out the address of memory wasm owns —
/// a `&'static str`'s bytes, or the error string's, valid until the next call
/// that mutates the harness.
pub mod shim {
    use super::{SCENES, with_harness};

    /// How many scenes there are to drive.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_scene_count() -> u32 {
        u32::try_from(SCENES.len()).unwrap_or(u32::MAX)
    }

    /// Address of scene `index`'s golden name, or `0` if out of range.
    ///
    /// The name is a `&'static str` baked into the module, so its address is
    /// fixed for the life of the instance.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_scene_name_ptr(index: u32) -> *const u8 {
        SCENES
            .get(index as usize)
            .map_or(core::ptr::null(), |(_, name)| name.as_ptr())
    }

    /// Length in bytes of scene `index`'s golden name, or `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_scene_name_len(index: u32) -> u32 {
        SCENES
            .get(index as usize)
            .map_or(0, |(_, name)| u32::try_from(name.len()).unwrap_or(u32::MAX))
    }

    /// Begin opening scene `index`. `1` on start, `0` on a bad index.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_start(index: u32) -> u32 {
        u32::from(with_harness(|harness| harness.start(index as usize)))
    }

    /// Advance the open one poll and return the new state code.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_step() -> u32 {
        with_harness(super::Harness::step)
    }

    /// The current state code, without advancing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_state() -> u32 {
        with_harness(|harness| harness.state.code())
    }

    /// Address of the last error message, or `0` when there is none.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_error_ptr() -> *const u8 {
        with_harness(|harness| {
            if harness.error.is_empty() {
                core::ptr::null()
            } else {
                harness.error.as_ptr()
            }
        })
    }

    /// Length in bytes of the last error message, or `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_error_len() -> u32 {
        with_harness(|harness| u32::try_from(harness.error.len()).unwrap_or(u32::MAX))
    }
}
