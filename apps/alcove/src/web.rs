//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/alcove` is a `cdylib` on `wasm32-unknown-unknown`, and this module is
//! the only thing in it a browser can reach. Everything here is an `extern "C"`
//! export with `#[unsafe(no_mangle)]`; there are **no imports**.
//!
//! # What is this sample's, and what is every sample's
//!
//! The state machine behind the lifecycle exports, the log queue and the
//! five-call protocol are [`crcbl::web`], and [`crcbl::web_exports!`] writes the
//! ten symbols in the first table below. That module is also where the reasons
//! live: why start-up is polled rather than blocking, why the clock is the
//! browser's, and why a sample's wasm module imports nothing of its own.
//!
//! What is genuinely alcove's is the [`WebPending`](crcbl::web::WebPending)
//! impl, which opens the fixture with its own [`Options`], and **the occlusion
//! knobs**, which are the second table. The symbol names stay here too, written
//! out one per line — two demos can be open in one browser and the exports must
//! not collide, so the macro takes each name as an argument rather than building
//! it from a prefix.
//!
//! # Why a page needs knobs of its own
//!
//! Natively every control here is a key or a pause-panel row, and the seam is
//! the interesting one to drive: `,` and `.` walk it a step at a time. **A phone
//! has neither.** So the controls a page offers are these exports, and each one
//! goes through [`crate::occlusion`] — the same `r_ssao_*` console variables the
//! keys write, and the same seam a person typing a console line goes through.
//! There is no second copy of the state anywhere on the page: every one of these
//! calls answers with what the console holds *afterwards*, so a slider that was
//! clamped shows where it actually landed.
//!
//! **Two of them reach a different cell again.** [`__crcbl_alcove_view`] and
//! [`__crcbl_alcove_bent_view`] name a [`crcbl::render::DebugView`], which is
//! the *engine's* one cell rather than an `r_ssao_*` variable of this sample's —
//! shared with every other sample and with the `debug_view` console command. The
//! engine holds exactly one view, so each of them answers with whether its
//! picture is **the** one being drawn.
//!
//! # What the page shows, and what decides it
//!
//! **The browser draws the court through [`crcbl::hal::LightingPath::Rasterised`]
//! by construction.** WebGPU exposes no ray query, so the selector cannot
//! resolve to anything else here — which is also the honest answer to milestone
//! 4 of `docs/plan/sample/19-alcove.md`: the ray-traced rung cannot be looked at
//! on this tier, and the panel's `ray tracing` row says `raster only` rather than
//! implying a choice was made. Screen-space occlusion is the whole of what is
//! being compared here, which is exactly what this fixture is for.
//!
//! `Options::default()` is what fixes the rest of the page: the fixed camera the
//! goldens are taken from, every effect asked for, and every occlusion knob left
//! where `crcbl_render::ssao` declares it. A page has no argv, so `--technique`,
//! `--split` and the `--force-*` flags have no source here — see this sample's
//! `crcbl::engine::PolledGpu` impl, which says the same thing about the device
//! request.
//!
//! **What this sample does not add to the macro.** There is no `asset_source`
//! accessor here, because alcove has nothing to read out of one: it keeps no
//! score — there is no score — and every byte it draws with (the court's
//! geometry, the shaders, the glyph atlas) is compiled into the module. The two
//! backends are still installed by the macro's `prepare`, because the shared
//! shim's boot sequence drives both ABIs before it boots the demo and both must
//! answer.
//!
//! # The ABIs a page has to drive
//!
//! | Prefix | Owner | What it is |
//! | --- | --- | --- |
//! | `__crcbl_web_` (input/frame) | [`crcbl::shell`]'s `web` backend | canvas size, focus, keys, pointer |
//! | `__crcbl_web_audio_` | [`crcbl::audio::web`] | the AudioWorklet pull |
//! | `__crcbl_web_fetch_` | [`crcbl::store::web::fetch`] | assets over `fetch()` |
//! | `__crcbl_web_opfs_` | [`crcbl::store::web::opfs`] | saves in the Origin Private File System |
//! | `__crcbl_alcove_` | this module | boot, one rAF frame, teardown, logs, the knobs |
//!
//! ## Exports: the lifecycle
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_alcove_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_alcove_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_alcove_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_alcove_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_alcove_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_alcove_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_alcove_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_alcove_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_alcove_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_alcove_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
//!
//! ## Exports: the occlusion knobs
//!
//! Every one of them **reads** when it is passed a negative argument and writes
//! otherwise, so a page places its controls and drives them through one symbol
//! each. The answer is always the state the console holds after the call.
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_alcove_view`] | `(i32) -> i32` | The AO-only view — the `V` key and the panel's `AO VIEW` row. `1` on, `0` off. |
//! | [`__crcbl_alcove_bent_view`] | `(i32) -> i32` | The bent-direction view — the `N` key and the panel's `BENT VIEW` row. A non-zero argument toggles, `0` reads. `1`/`0`. |
//! | [`__crcbl_alcove_bent_normals`] | `(i32) -> i32` | `r_ssao_bent_normals` — the `B` key. `1`/`0`. |
//! | [`__crcbl_alcove_technique`] | `(i32) -> i32` | A non-zero argument moves the gather on to the next one the engine declares, as the `T` key does. Returns the **length** of the name now in force. |
//! | [`__crcbl_alcove_technique_ptr`] | `() -> i32` | Address of that name (UTF-8, not NUL-terminated). Read it **after** the call above: the two together are one read. |
//! | [`__crcbl_alcove_seam`] | `(f32) -> f32` | Where the comparison seam stands, as a fraction of the frame's width. `0` is the seam down, which is what the `X` key toggles to and from; anything between the edges is where `,` and `.` walk it. |
//! | [`__crcbl_alcove_radius`] | `(f32) -> f32` | The argument is a **dial position** from 0 to 1 — `[` and `]` step the same scale — and the answer is the radius in world units, which is what a page has to print. |
//! | [`__crcbl_alcove_radius_dial`] | `() -> f32` | Where that dial stands now, for a page placing the slider. |
//! | [`__crcbl_alcove_intensity`] | `(f32) -> f32` | The same pair for the intensity, which `-` and `=` step. |
//! | [`__crcbl_alcove_intensity_dial`] | `() -> f32` | |
//! | [`__crcbl_alcove_reset`] | `()` | Every knob back to the value the engine declares — the `R` key. |
//!
//! ## Status codes
//!
//! [`STATUS_IDLE`] `0`, [`STATUS_PREPARED`] `1`, [`STATUS_BOOTING`] `2`,
//! [`STATUS_RUNNING`] `3`, [`STATUS_STOPPED`] `4`, [`STATUS_FAILED`] `5`,
//! [`STATUS_PAUSED`] `6`.
//!
//! The shim drives `requestAnimationFrame` while the status is `BOOTING`,
//! `RUNNING` **or** `PAUSED` and stops on anything else — a paused page is still
//! drawing, and it is a keystroke away from ticking again. `FAILED` is the only
//! one that sets an error message.
//!
//! ## Call ordering
//!
//! ```text
//! __crcbl_alcove_prepare()                   // storage backends exist
//!   → fetch pre-load       (__crcbl_web_fetch_*)
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                   // which canvas this instance drives
//! __crcbl_alcove_boot()                      // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)      // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())   // the shell's clock reference
//!   __crcbl_alcove_frame(performance.now()) // boot poll, or a frame
//!   __crcbl_alcove_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …              // drain queued saves
//! ```
//!
//! **The first `__crcbl_web_resize` is what starts the device request.** A
//! canvas has no size until the document gives it one, and a swapchain needs
//! one; a shim that never calls `resize` leaves the status at `BOOTING` forever.

use crate::app::{Loop, PendingLoop};
use crate::args::Options;
use crate::occlusion;

// The status codes and the asset base are the shim's wire format, so they have
// exactly one definition; see [`crcbl::web`]. Re-exported rather than reached
// through the path, because this module's own docs name them.
pub use crcbl::web::{
    ASSET_BASE, STATUS_BOOTING, STATUS_FAILED, STATUS_IDLE, STATUS_PAUSED, STATUS_PREPARED,
    STATUS_RUNNING, STATUS_STOPPED,
};

// ---------------------------------------------------------------------------
// This sample's half of the lifecycle
// ---------------------------------------------------------------------------

// **There is no `WebLoop` impl here.** `crcbl::web` blanket-implements it for
// every `crcbl::engine::Loop`, and the two halves that were ever this sample's
// — its name and the log line a finished run is worth — are `HostedGame::NAME`
// and `HostedGame::log_summary` in `app.rs`. What is left below is start-up,
// which stays here because the options it opens with are alcove's.
// **`WebPending` is deliberately not imported.** The macro's guard against a
// missing inherent method resolves `PendingLoop::poll` by path, and an import
// would let that resolve to the trait method instead — which is the infinite
// recursion the guard exists to catch. See `crcbl::impl_web_pending`.
crcbl::impl_web_pending!(PendingLoop, Loop, Options, crate::app::AlcoveError);

// ---------------------------------------------------------------------------
// Exports: the lifecycle
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_alcove_prepare,
    log_level: __crcbl_alcove_log_level,
    boot: __crcbl_alcove_boot,
    frame: __crcbl_alcove_frame,
    status: __crcbl_alcove_status,
    shutdown: __crcbl_alcove_shutdown,
    error_ptr: __crcbl_alcove_error_ptr,
    error_len: __crcbl_alcove_error_len,
    log_take: __crcbl_alcove_log_take,
    log_ptr: __crcbl_alcove_log_ptr,
}

// ---------------------------------------------------------------------------
// Exports: the occlusion knobs
// ---------------------------------------------------------------------------

/// Draw the occlusion channel as grey instead of shading the court.
///
/// A negative `on` reads. The answer is `1` while the AO-only view is what the
/// frame draws and `0` otherwise, read back out of `crcbl::debug_view` rather
/// than remembered here — the console's `debug_view ambient occlusion` writes
/// the same cell, and a page holding a copy would disagree with it.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_view(on: i32) -> i32 {
    if on >= 0 && (on != 0) != occlusion::occlusion_view() {
        occlusion::toggle_occlusion_view();
    }
    i32::from(occlusion::occlusion_view())
}

/// Draw the bent direction the gather reported, or go back to the picture, and
/// answer with whether it is the view in force.
///
/// `toggle` of `0` reads. The `N` key and the pause panel's `BENT VIEW` row as
/// something a finger can reach, through [`occlusion::toggle_bent_normal_view`]
/// — the same `crcbl::debug_view` cell all three write, so a view put up by a
/// typed `debug_view bent normal` is what this answers with too.
///
/// **A different kind of state from every knob above it**, which is why it does
/// not take the negative-reads convention they share: those are `r_ssao_*`
/// console cells this sample owns, and the debug view is the *engine's*, held as
/// one value rather than as a switch per view. So this answers with whether the
/// bent direction is **the** view rather than with a flag of its own — a picture
/// some other view had replaced would otherwise read here as still up — and a
/// caller asking for a state it is already in must not be handed a toggle that
/// leaves it somewhere else.
///
/// # What it is for
///
/// `docs/plan/sample/19-alcove.md`'s milestone 3.
/// [`__crcbl_alcove_bent_normals`] is the switch that makes the gather report a
/// direction; a scalar view of the frame says nothing about **which way** the
/// ambient is being sampled from, and the charter is explicit that a term that
/// steers that cannot be reviewed as a grey image. A visitor with no keyboard
/// could not reach the picture at all.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_bent_view(toggle: i32) -> i32 {
    if toggle != 0 {
        occlusion::toggle_bent_normal_view();
    }
    i32::from(occlusion::bent_normal_view())
}

/// Gather a bent direction beside the scalar, or do not.
///
/// A negative `on` reads. `1`/`0`, off the variable itself.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_bent_normals(on: i32) -> i32 {
    let held = occlusion::var(occlusion::BENT_NORMALS).get_bool();
    if on >= 0 && (on != 0) != held {
        occlusion::toggle_bent_normals();
    }
    i32::from(occlusion::var(occlusion::BENT_NORMALS).get_bool())
}

/// Move the gather on to the next technique the engine declares, and answer with
/// the length of the name now in force.
///
/// `cycle` of `0` reads. The name itself is at [`__crcbl_alcove_technique_ptr`],
/// and the two calls are one read: nothing on a page's thread can move the
/// variable between them.
///
/// A **length and an address** rather than an index into a list the page keeps,
/// for `crate::occlusion`'s reason: the set of techniques is `crcbl_render::ssao`'s,
/// and a page spelling its members would be a copy that goes stale the day a
/// third one lands.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_technique(cycle: i32) -> u32 {
    if cycle != 0 {
        occlusion::cycle(occlusion::TECHNIQUE);
    }
    let name = occlusion::var(occlusion::TECHNIQUE).get_enum();
    u32::try_from(name.len()).unwrap_or(0)
}

/// Address of the technique's name, UTF-8 and not NUL-terminated.
///
/// Read it **after** [`__crcbl_alcove_technique`], whose answer is its length.
/// The bytes are the `&'static str` the engine's own variable holds, so the
/// address stays valid for as long as the module is loaded.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_technique_ptr() -> *const u8 {
    occlusion::var(occlusion::TECHNIQUE).get_enum().as_ptr()
}

/// Put the comparison seam at `at` of the frame's width, and answer with where
/// it stands.
///
/// A negative `at` reads. `0` is no seam at all — the state the `X` key toggles
/// to and from — and so is either edge, which is `crcbl_render::ssao`'s own rule
/// rather than this module's: half a frame comparing nothing is not a
/// comparison. Anything outside the variable's range moves it to the nearest
/// edge rather than being refused, so a slider cannot leave the seam somewhere
/// nobody asked for.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_seam(at: f32) -> f32 {
    if at >= 0.0 {
        occlusion::set_seam(at);
    }
    occlusion::seam().unwrap_or(0.0)
}

/// Put the occlusion radius at dial position `at`, and answer with the radius in
/// world units.
///
/// A negative `at` reads. The dial is the scale `[` and `]` step along — see
/// `occlusion::dial` — and the answer is in metres because that is what a page
/// has to print beside the slider.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_radius(at: f32) -> f32 {
    if at >= 0.0 {
        return occlusion::set_dial(occlusion::RADIUS, at);
    }
    occlusion::var(occlusion::RADIUS).get_f32()
}

/// Where the radius's dial stands, from 0 to 1, for a page placing its slider.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_radius_dial() -> f32 {
    occlusion::dial(occlusion::RADIUS)
}

/// Put the occlusion intensity at dial position `at`, and answer with the
/// exponent itself.
///
/// [`__crcbl_alcove_radius`]'s pair, for the knob `-` and `=` step.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_intensity(at: f32) -> f32 {
    if at >= 0.0 {
        return occlusion::set_dial(occlusion::INTENSITY, at);
    }
    occlusion::var(occlusion::INTENSITY).get_f32()
}

/// Where the intensity's dial stands, from 0 to 1.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_intensity_dial() -> f32 {
    occlusion::dial(occlusion::INTENSITY)
}

/// Put every knob back to the value the engine declares — the `R` key.
///
/// Nothing to answer with: a page that has just reset the knobs reads all of
/// them back through the calls above, which is what puts its own controls where
/// the console now is.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_alcove_reset() {
    occlusion::reset();
}
