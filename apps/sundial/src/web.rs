//! The browser entry point: what the JS shim in `web/` calls.
//!
//! `apps/sundial` is a `cdylib` on `wasm32-unknown-unknown`, and this module is
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
//! What is genuinely sundial's is the [`WebPending`](crcbl::web::WebPending)
//! impl, which opens the plaza with its own [`Options`], and **the knobs**,
//! which are the second table. The symbol names stay here too, written out one
//! per line — two demos can be open in one browser and the exports must not
//! collide, so the macro takes each name as an argument rather than building it
//! from a prefix.
//!
//! # Why a page needs knobs of its own
//!
//! `apps/alcove/src/web.rs`' argument, and this fixture has one more control to
//! carry than that one does. Natively every knob here is a key or a pause-panel
//! row: `F` cycles the filter, `X` raises the seam, `,` and `.` walk it, `T`
//! puts the shadow atlas up, `P` stops the sun and `-` and `=` scrub it. **A
//! phone has none of them**, and a shadow fixture with a sun nobody can stop is
//! one whose artefacts cannot be looked at — acne, peter-panning and a swimming
//! cascade edge each want the sun held at a pose.
//!
//! # Three kinds of knob, and they take different routes
//!
//! | Knob | Where the state lives | How this module reaches it |
//! | --- | --- | --- |
//! | the filter, the seam | a `r_shadow_*` console cell | [`crate::filter`], the same cell a key and a typed line write |
//! | the atlas viewer | the engine's `r_debug_view` cell | `crate::app::toggle_atlas_view`, the same cell `T` and the pause panel's `ATLAS` row write |
//! | the sun's tick, and whether it runs | `crate::app::Sundial` | [`crate::sun::page_clock`] and its `ask_*` pair, adopted by the next fixed step |
//!
//! **There is no second copy of either on the page.** Every call below answers
//! with what the console or the clock holds *afterwards*, so a slider the engine
//! clamped shows where it actually landed, and a value moved by a key, by the
//! pause panel or by a typed console line is picked up the next time anything on
//! the page reads it back. The sun's half is the one that is not instant: a
//! request is taken up on the next fixed step, and what these exports answer is
//! the request itself, which is where the clock is about to be.
//!
//! # What the page shows, and what decides it
//!
//! `Options::default()` fixes it: the fixed camera the goldens are taken from,
//! every effect asked for, the shipped filter, no seam, and the clock at
//! [`crate::sun::FIXTURE_TICK`] and running. A page has no argv, so `--filter`,
//! `--split`, `--sun-tick` and the `--force-*` flags have no source here — see
//! this sample's `crcbl::engine::PolledGpu` impl, which says the same thing
//! about the device request.
//!
//! **What this sample does not add to the macro.** There is no `asset_source`
//! accessor here, because sundial has nothing to read out of one: it keeps no
//! score — there is no score — and every byte it draws with (the plaza's
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
//! | `__crcbl_sundial_` | this module | boot, one rAF frame, teardown, logs, the knobs |
//!
//! ## Exports: the lifecycle
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_sundial_prepare`] | `() -> i32` | Install the log sink and the browser storage backends. **First call**, before any `__crcbl_web_fetch_*` or `__crcbl_web_opfs_*`. `1`, or `0` if it was already called. |
//! | [`__crcbl_sundial_log_level`] | `(i32) -> i32` | Set the log filter: `0` off … `5` trace. `1`/`0`. |
//! | [`__crcbl_sundial_boot`] | `() -> i32` | Open the shell on the canvas `__crcbl_web_canvas` announced, create the window, start the polled device request. `1`/`0`. |
//! | [`__crcbl_sundial_frame`] | `(f64) -> i32` | One `requestAnimationFrame`, given `performance.now()`. Returns the new status. |
//! | [`__crcbl_sundial_status`] | `() -> i32` | The status, without advancing anything. |
//! | [`__crcbl_sundial_shutdown`] | `() -> i32` | Tear the loop down. `1` if there was one. |
//! | [`__crcbl_sundial_error_ptr`] | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_sundial_error_len`] | `() -> i32` | Its length in bytes. |
//! | [`__crcbl_sundial_log_take`] | `() -> i32` | Pop one log line into the scratch buffer and return its length; `0` when the queue is empty. |
//! | [`__crcbl_sundial_log_ptr`] | `() -> i32` | Address of that scratch buffer. Read it **after** `log_take`. |
//!
//! ## Exports: the knobs
//!
//! Each one **reads** rather than writing when it is passed an argument that
//! could not be a value — a negative number for the two that take a position,
//! and a zero for the two that cycle or toggle — so a page places its controls
//! and drives them through one symbol each. The answer is always the state that
//! is in force after the call.
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_sundial_filter`] | `(i32) -> i32` | A non-zero argument moves the shadow filter on to the next one the engine declares, as the `F` key does. Returns the **length** of the name now in force. |
//! | [`__crcbl_sundial_filter_ptr`] | `() -> i32` | Address of that name (UTF-8, not NUL-terminated). Read it **after** the call above: the two together are one read. |
//! | [`__crcbl_sundial_seam`] | `(i32) -> f32` | A non-zero argument raises the comparison seam at the centre or drops it, as the `X` key does. Returns where it stands, and `0` for a frame comparing nothing. |
//! | [`__crcbl_sundial_seam_at`] | `(f32) -> f32` | Where the seam stands, as a fraction of the frame's width — what `,` and `.` walk. Either edge takes it down. |
//! | [`__crcbl_sundial_atlas_view`] | `(i32) -> i32` | A non-zero argument draws the shadow atlas over the frame or takes it away, as the `T` key does. `1`/`0` for whether it is the picture in force. |
//! | [`__crcbl_sundial_sun_tick`] | `(f64) -> f64` | Which tick of the clock the sun is drawn at. Writing one **stops** the clock, as a scrub does. |
//! | [`__crcbl_sundial_sun_sweep`] | `() -> f64` | How many ticks one sweep of the sun takes, so a page's slider spans the engine's own arc rather than a number written on the page. |
//! | [`__crcbl_sundial_sun_running`] | `(i32) -> i32` | Whether the clock is moving — the `P` key. `1`/`0`. |
//! | [`__crcbl_sundial_reset`] | `()` | Both knobs and the clock back to where a fresh run opens — the `R` key. |
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
//! __crcbl_sundial_prepare()                  // storage backends exist
//!   → fetch pre-load       (__crcbl_web_fetch_*)
//!   → OPFS restore + ready (__crcbl_web_opfs_*)
//! __crcbl_web_canvas(id)                   // which canvas this instance drives
//! __crcbl_sundial_boot()                     // shell + window; no size yet
//! rAF loop, every frame:
//!   __crcbl_web_resize(id, w, h, dpr)      // from ResizeObserver, when it changes
//!   __crcbl_web_frame(performance.now())   // the shell's clock reference
//!   __crcbl_sundial_frame(performance.now()) // boot poll, or a frame
//!   __crcbl_sundial_log_take() … while non-zero
//!   __crcbl_web_opfs_take() …              // drain queued saves
//! ```
//!
//! **The first `__crcbl_web_resize` is what starts the device request.** A
//! canvas has no size until the document gives it one, and a swapchain needs
//! one; a shim that never calls `resize` leaves the status at `BOOTING` forever.

use crate::app::{Loop, PendingLoop};
use crate::args::Options;
use crate::{filter, sun};

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
// which stays here because the options it opens with are sundial's.
// **`WebPending` is deliberately not imported.** The macro's guard against a
// missing inherent method resolves `PendingLoop::poll` by path, and an import
// would let that resolve to the trait method instead — which is the infinite
// recursion the guard exists to catch. See `crcbl::impl_web_pending`.
crcbl::impl_web_pending!(PendingLoop, Loop, Options, crate::app::SundialError);

// ---------------------------------------------------------------------------
// Exports: the lifecycle
// ---------------------------------------------------------------------------

crcbl::web_exports! {
    pending: PendingLoop<dyn crcbl::shell::Shell>,
    prepare: __crcbl_sundial_prepare,
    log_level: __crcbl_sundial_log_level,
    boot: __crcbl_sundial_boot,
    frame: __crcbl_sundial_frame,
    status: __crcbl_sundial_status,
    shutdown: __crcbl_sundial_shutdown,
    error_ptr: __crcbl_sundial_error_ptr,
    error_len: __crcbl_sundial_error_len,
    log_take: __crcbl_sundial_log_take,
    log_ptr: __crcbl_sundial_log_ptr,
}

// ---------------------------------------------------------------------------
// Exports: the filter knobs, which are console cells
// ---------------------------------------------------------------------------

/// Move the shadow filter on to the next one the engine declares, and answer
/// with the length of the name now in force.
///
/// `cycle` of `0` reads. The name itself is at [`__crcbl_sundial_filter_ptr`],
/// and the two calls are one read: nothing on a page's thread can move the
/// variable between them.
///
/// A **length and an address** rather than an index into a list the page keeps,
/// for [`crate::filter`]'s reason: the set of filters is
/// `crcbl::render::shadow`'s, and a page spelling its members would be a copy
/// that goes stale the day a fourth rung lands.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_sundial_filter(cycle: i32) -> u32 {
    if cycle != 0 {
        filter::cycle(filter::FILTER);
    }
    let name = filter::var(filter::FILTER).get_enum();
    u32::try_from(name.len()).unwrap_or(0)
}

/// Address of the filter's name, UTF-8 and not NUL-terminated.
///
/// Read it **after** [`__crcbl_sundial_filter`], whose answer is its length. The
/// bytes are the `&'static str` the engine's own variable holds, so the address
/// stays valid for as long as the module is loaded.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_sundial_filter_ptr() -> *const u8 {
    filter::var(filter::FILTER).get_enum().as_ptr()
}

/// Raise the comparison seam at the centre of the frame, or drop it, and answer
/// with where it stands.
///
/// `toggle` of `0` reads. The answer is `0` for a frame comparing nothing, which
/// is the state the `X` key toggles to and from; [`__crcbl_sundial_seam_at`] is
/// how a page moves it off the centre this puts it at.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_sundial_seam(toggle: i32) -> f32 {
    if toggle != 0 {
        filter::toggle_seam();
    }
    filter::seam().unwrap_or(0.0)
}

/// Put the comparison seam at `at` of the frame's width, and answer with where
/// it stands.
///
/// A negative `at` reads. `0` is no seam at all — and so is either edge, which
/// is `crcbl::render::shadow::split_at`'s own rule rather than this module's:
/// half a frame comparing nothing is not a comparison. Anything outside the
/// variable's range moves it to the nearest edge rather than being refused, so a
/// slider cannot leave the seam somewhere nobody asked for.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_sundial_seam_at(at: f32) -> f32 {
    if at >= 0.0 {
        return filter::set_seam(at).unwrap_or(0.0);
    }
    filter::seam().unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// Exports: the atlas viewer, which is the engine's own debug-view cell
// ---------------------------------------------------------------------------

/// Draw the shadow atlas over the frame or take it away, and answer with
/// whether it is the picture in force.
///
/// `toggle` of `0` reads. The `T` key and the pause panel's `ATLAS` row as
/// something a finger can reach, through `crate::app::toggle_atlas_view` —
/// the same [`crcbl::debug_view`] cell all three write, so a view put up by a
/// typed `debug_view shadow atlas` is what this answers with too.
///
/// **A third route to a third kind of state**, and this page drives all three
/// now: the filter and the seam are [`crate::filter`]'s console cells, the sun
/// is the fixture's own clock, and the debug view is the engine's. The engine
/// holds exactly **one** view, so this answers with whether the shadow atlas is
/// *the* one rather than with a flag of its own — a picture some other view had
/// replaced would otherwise read here as still up.
///
/// # What it is for
///
/// `docs/plan/sample/18-sundial.md`'s milestone 1 diagnostic. The plaza's sun
/// and its three punctual lights all ask `crcbl::render::shadow` for a run of
/// tiles, and a light that was refused one still lights — so the frame looks the
/// same either way and the atlas is the only place the answer is written down.
/// A visitor with no keyboard could not reach it at all.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_sundial_atlas_view(toggle: i32) -> i32 {
    if toggle != 0 {
        crate::app::toggle_atlas_view();
    }
    i32::from(crcbl::debug_view::current() == crcbl::render::DebugView::ShadowAtlas)
}

// ---------------------------------------------------------------------------
// Exports: the sun, which is this run's own clock
// ---------------------------------------------------------------------------

/// Draw the sun at tick `at` of its sweep, and answer with the tick in force.
///
/// A negative `at` reads. **Writing one stops the clock**, which is
/// [`crate::sun::ask_tick`]'s rule and [`crate::sun::Clock::scrub`]'s before it:
/// a tick written onto a running clock is a pose the next fixed step moves off,
/// so a slider would fight the sun it is placing.
///
/// An `f64` rather than the `u64` the clock keeps, because a wasm `i64` reaches
/// JavaScript as a `BigInt` and a page's `<input type="range">` yields a
/// `Number`. Every tick a run can reach inside a human lifetime is an integer an
/// `f64` holds exactly; a fractional or out-of-range argument is truncated
/// towards zero, which is where a slider's own step already puts it.
#[unsafe(no_mangle)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "guarded above and saturating: an `as` cast from f64 to u64 clamps \
              at both ends rather than wrapping"
)]
#[expect(
    clippy::cast_precision_loss,
    reason = "a tick count is exact in an f64 until it passes 2^53, which this \
              clock does not reach in a lifetime of running"
)]
pub extern "C" fn __crcbl_sundial_sun_tick(at: f64) -> f64 {
    let clock = if at >= 0.0 {
        sun::ask_tick(at as u64)
    } else {
        sun::page_clock()
    };
    clock.tick() as f64
}

/// How many ticks one sweep of the sun takes.
///
/// [`crate::sun::SWEEP_TICKS`], so a page's slider spans the arc the engine
/// declares rather than a number written into the markup — the same argument
/// [`__crcbl_sundial_filter_ptr`] makes about the set of filters.
#[unsafe(no_mangle)]
#[expect(
    clippy::cast_precision_loss,
    reason = "the sweep is a few hundred ticks"
)]
pub extern "C" fn __crcbl_sundial_sun_sweep() -> f64 {
    sun::SWEEP_TICKS as f64
}

/// Start the sun's clock or stop it, and answer with whether it is moving.
///
/// A negative `on` reads. The `P` key, as something a finger can reach: a
/// stopped sun is what every one of this fixture's artefact claims is read at,
/// because acne and a peter-panning contact are read off a pose rather than off
/// a sweep.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_sundial_sun_running(on: i32) -> i32 {
    let clock = if on >= 0 {
        sun::ask_running(on != 0)
    } else {
        sun::page_clock()
    };
    i32::from(clock.running())
}

/// Put both knobs and the clock back to where a fresh run opens — the `R` key.
///
/// Nothing to answer with: a page that has just reset reads all of them back
/// through the calls above, which is what puts its own controls where the engine
/// now is.
#[unsafe(no_mangle)]
pub extern "C" fn __crcbl_sundial_reset() {
    filter::reset();
    sun::ask_reset();
}
