//! The browser half of the WebGPU parity gate: drive crcbl's backend-agnostic
//! golden [`Scene`] set through whichever GPU backend the wasm was built
//! against, render each one offscreen, read the pixels back, and hand them to JS
//! so they can be compared against the very `crcbl/tests/golden/<name>.png` the
//! native `vk`/`mtl`/`dx12` `render_e2e` suite compares against.
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
//! run). Each `step` advances the scene one poll along this path:
//!
//! 1. [`OffscreenSetup::request`] starts the open. Its poll drives the instance
//!    open (adapter enumeration over the stream) and then the device open. On
//!    the WebGPU backend, opening the instance installs the stream channel, so
//!    the pump has something to drain from the very first frame.
//! 2. When the instance is open, the poll synchronously creates the offscreen
//!    surface, selects the adapter, and starts the device request.
//! 3. When the device is ready, the poll builds the swapchain ring and the
//!    scene's renderer and hands over the [`OffscreenSetup`].
//! 4. [`OffscreenSetup::begin_readback`] records and submits the frame;
//!    [`crcbl::screenshot::PendingReadback::poll`] is driven until the copy
//!    lands, which is where the pixels come from.
//! 5. `OffscreenSetup::finish` waits the device idle and tears down — a device
//!    lost during the frame surfaces there and nowhere else, so it runs before
//!    the pixels are believed.
//!
//! ## How the readback is held across frames, without unsafe
//!
//! A `PendingReadback` borrows its `OffscreenSetup`, so keeping one alive from
//! one rAF frame to the next is a self-referential hold: the borrow and the
//! thing borrowed have to live in the same place. That is exactly the shape a
//! `Future` is for, so the whole scene drive is written as one `async fn`
//! (`drive`) and stepped by hand — `Harness::step` polls it once per frame
//! with [`core::task::Waker::noop`], and `yield_now` is what turns "not ready,
//! come back next frame" into a `Poll::Pending`. The compiler builds the
//! self-referential state machine and proves it sound; there is no `unsafe`, no
//! leaked box and no transmuted lifetime anywhere in this crate.
//!
//! No waker is ever registered, because nothing here would wake it: every wait
//! is on the browser replaying a command stream, which happens between `step`
//! calls and reports back through the poll itself. rAF is the executor.
//!
//! # What a run proves
//!
//! A scene that reaches `State::Rendered` has replayed the whole path —
//! adapter enumeration, offscreen surface, swapchain ring, uploads, pipelines,
//! the frame's passes, and the readback copy — on a real browser GPU device, and
//! its pixels are in wasm memory for JS to pull out. It still does **not** prove
//! the image is right: that is the golden comparison, which runs outside the
//! browser in `examples/compare-readback.rs` over the files
//! `web/tools/render-harness-e2e.mjs` writes.
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
//! | [`__crcbl_render_harness_start`](shim::__crcbl_render_harness_start) | `(i32) -> i32` | Begin driving scene `i`. `1` on start, `0` on a bad index. Overrides any scene in flight. |
//! | [`__crcbl_render_harness_step`](shim::__crcbl_render_harness_step) | `() -> i32` | Advance the drive one poll and return the new `State` code. |
//! | [`__crcbl_render_harness_state`](shim::__crcbl_render_harness_state) | `() -> i32` | The `State` code, without advancing. |
//! | [`__crcbl_render_harness_error_ptr`](shim::__crcbl_render_harness_error_ptr) | `() -> i32` | Address of the last error message (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_render_harness_error_len`](shim::__crcbl_render_harness_error_len) | `() -> i32` | Its length in bytes, or `0`. |
//! | [`__crcbl_render_harness_frame_ptr`](shim::__crcbl_render_harness_frame_ptr) | `() -> i32` | Address of the last scene's pixels, or `0` if none landed. |
//! | [`__crcbl_render_harness_frame_len`](shim::__crcbl_render_harness_frame_len) | `() -> i32` | How many bytes those are, or `0`. |
//! | [`__crcbl_render_harness_frame_width`](shim::__crcbl_render_harness_frame_width) | `() -> i32` | The readback's width in pixels, or `0`. |
//! | [`__crcbl_render_harness_frame_height`](shim::__crcbl_render_harness_frame_height) | `() -> i32` | Its height in pixels, or `0`. |
//! | [`__crcbl_render_harness_frame_order_ptr`](shim::__crcbl_render_harness_frame_order_ptr) | `() -> i32` | Address of the channel-order name, `rgba` or `bgra`, or `0`. |
//! | [`__crcbl_render_harness_frame_order_len`](shim::__crcbl_render_harness_frame_order_len) | `() -> i32` | Its length in bytes, or `0`. |
//!
//! ## State codes
//!
//! `State::Idle` `0`, `State::Running` `1`, `State::Rendered` `2`,
//! `State::Failed` `3`. `Failed` is the only one that sets an error message;
//! `Rendered` is the only one with a frame.
//!
//! ## Why the channel order is carried rather than assumed
//!
//! The readback is in the swapchain format's **memory** order, which is
//! `Bgra8Unorm` on most surfaces and `Rgba8Unorm` on some. Comparing BGRA bytes
//! against an RGBA golden does not fail loudly — it fails as a red/blue swap
//! that looks like a shader bug — so the order the frame actually came back in
//! travels with the pixels instead of being guessed at the far end. It is a
//! name rather than a numeric code so that neither the JS in the middle nor the
//! comparator at the end needs a table to decode it.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use crcbl::hal::Format;
use crcbl::screenshot::{OffscreenError, OffscreenSetup, Scene};

/// The frame every golden was blessed at — the same `256x192` the native
/// `crcbl/tests/render_e2e.rs` suite and `crcbl-vk`'s mesh suite use, so a
/// readback here is comparable against `crcbl/tests/golden/<name>.png` at the
/// suite's tolerance.
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
    (Scene::AreaLight, "area_light"),
    (Scene::FillLight, "fill_light"),
    (Scene::AlphaMask, "alpha_mask"),
    (Scene::DoubleSided, "double_sided"),
    (Scene::SpecularAa, "specular_aa"),
    (Scene::Ao, "ao"),
    (Scene::Ssr, "ssr"),
    (Scene::AtmosphereMirror, "atmosphere_mirror"),
    (Scene::Probes, "probes"),
    (Scene::Bloom, "bloom"),
    (Scene::Aa, "aa"),
    (Scene::Ui, "ui"),
];

/// Every golden basename this gate drives, in the order it drives them.
///
/// Public because `examples/compare-readback.rs` needs to *insist* on a
/// readback for each one: a comparator that only looks at the files it happens
/// to find passes a run where nothing was written at all, which is the shape of
/// green light that is worse than none.
pub fn golden_names() -> impl Iterator<Item = &'static str> {
    SCENES.iter().map(|&(_, name)| name)
}

/// The name for a readback in `Format::Bgra8Unorm`-family memory order.
const ORDER_BGRA: &str = "bgra";

/// The name for a readback in `Format::Rgba8Unorm`-family memory order.
const ORDER_RGBA: &str = "rgba";

/// Which of the two names describes `format`'s memory order.
///
/// The same two-arm mapping `crcbl/tests/render_e2e.rs` and
/// `crcbl/tests/tiling_e2e.rs` make onto `crcbl_golden::ChannelOrder`, written
/// out again here because this crate cannot name that type: `crcbl-golden`
/// reads and writes PNGs through `std::fs`, which has no place in a wasm
/// module. `examples/compare-readback.rs` is where the name becomes a
/// `ChannelOrder` again.
const fn order_name(format: Format) -> &'static str {
    match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => ORDER_BGRA,
        _ => ORDER_RGBA,
    }
}

/// One scene's readback: the pixels and everything needed to read them.
struct Frame {
    width: u32,
    height: u32,
    /// [`ORDER_RGBA`] or [`ORDER_BGRA`] — see the module docs on why this
    /// travels with the pixels.
    order: &'static str,
    /// `width * height * 4` bytes, row-major, top row first, in `order`'s
    /// channel order. Owned here so its address is stable between the
    /// `frame_ptr`/`frame_len` pair a reader calls back to back.
    pixels: Vec<u8>,
}

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
    /// A scene drive is in flight; poll it each frame.
    ///
    /// The future owns the `OffscreenSetup` and, while the readback is in
    /// flight, the `PendingReadback` borrowing it — the self-referential hold
    /// the module docs describe.
    Running(Pin<Box<dyn Future<Output = Result<Frame, OffscreenError>>>>),
    /// The frame was drawn, read back, and the device torn down cleanly. The
    /// pixels are in [`Harness::frame`].
    Rendered,
    /// The drive failed; the message is in [`Harness::error`].
    Failed,
}

impl State {
    /// The wire code JS reads, kept next to the `enum` so the two cannot drift.
    fn code(&self) -> u32 {
        match self {
            State::Idle => 0,
            State::Running(_) => 1,
            State::Rendered => 2,
            State::Failed => 3,
        }
    }
}

/// A future that is `Pending` exactly once, then `Ready`.
///
/// The only await point in [`drive`], and the whole of its scheduling: every
/// wait in the offscreen path is on the browser replaying a command stream
/// between `step` calls, so "come back next frame" is the only thing a poll
/// here ever needs to say. Nothing registers the waker because nothing would
/// call it — [`Harness::step`] is the executor and rAF is its timer.
fn yield_now() -> impl Future<Output = ()> {
    let mut yielded = false;
    core::future::poll_fn(move |_| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            Poll::Pending
        }
    })
}

/// Opens `scene` offscreen at `width`x`height`, draws one frame, and reads it
/// back — the whole per-scene drive, as one future.
///
/// Written `async` for the reason the module docs give: the readback borrows the
/// setup, and a future is the language's own way to hold a borrow across a
/// suspension point without `unsafe`.
async fn drive(width: u32, height: u32, scene: Scene) -> Result<Frame, OffscreenError> {
    let mut pending = OffscreenSetup::request(width, height, scene)?;
    let mut setup = loop {
        if let Some(setup) = pending.poll()? {
            break setup;
        }
        yield_now().await;
    };

    let format = setup.format();
    let read = read_frame(&mut setup).await;
    // `finish` waits the device idle, and a device lost during the frame
    // surfaces there and nowhere else — so it runs before the pixels are
    // believed, exactly as `render_e2e` finishes before it asserts. The
    // readback's own error wins when there are two: it is the earlier one, and
    // a teardown failure after it is a consequence rather than a cause.
    let finished = setup.finish();
    let ((width, height), pixels) = read?;
    finished?;

    Ok(Frame {
        width,
        height,
        order: order_name(format),
        pixels,
    })
}

/// Records and submits one frame off `setup`, then polls until the copy lands.
///
/// Split out so the `PendingReadback`'s borrow of `setup` ends at this future's
/// end and [`drive`] can take `setup` by value again to finish it.
async fn read_frame(setup: &mut OffscreenSetup) -> Result<((u32, u32), Vec<u8>), OffscreenError> {
    let mut readback = setup.begin_readback()?;
    loop {
        if let Some(frame) = readback.poll()? {
            return Ok(frame);
        }
        yield_now().await;
    }
}

/// The harness's whole state: the scene it is driving, its pixels, and the last
/// error text.
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
    /// The last completed readback, cleared when the next scene starts so a
    /// scene that fails can never hand back the previous scene's pixels.
    frame: Option<Frame>,
}

thread_local! {
    static HARNESS: RefCell<Harness> = RefCell::new(Harness::default());
}

/// Runs `f` with the single harness borrowed mutably.
fn with_harness<R>(f: impl FnOnce(&mut Harness) -> R) -> R {
    HARNESS.with(|cell| f(&mut cell.borrow_mut()))
}

impl Harness {
    /// Begin driving `scene`, discarding whatever was in flight.
    fn start(&mut self, index: usize) -> bool {
        let Some(&(scene, _)) = SCENES.get(index) else {
            return false;
        };
        self.error.clear();
        self.frame = None;
        self.state = State::Running(Box::pin(drive(EXTENT.0, EXTENT.1, scene)));
        true
    }

    /// Advance the drive one poll and return the new state code.
    fn step(&mut self) -> u32 {
        if let State::Running(future) = &mut self.state {
            // No waker, for the reason `yield_now` documents: nothing in this
            // module would ever call one.
            let mut context = Context::from_waker(Waker::noop());
            match future.as_mut().poll(&mut context) {
                Poll::Pending => {}
                Poll::Ready(Ok(frame)) => {
                    self.frame = Some(frame);
                    self.state = State::Rendered;
                }
                Poll::Ready(Err(error)) => self.fail(error),
            }
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
/// The ones that return an address hand out the address of memory wasm owns —
/// a `&'static str`'s bytes, the error string's, or the frame's — valid until
/// the next call that mutates the harness.
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

    /// Begin driving scene `index`. `1` on start, `0` on a bad index.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_start(index: u32) -> u32 {
        u32::from(with_harness(|harness| harness.start(index as usize)))
    }

    /// Advance the drive one poll and return the new state code.
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

    /// Address of the last scene's pixels, or `0` if no readback landed.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_frame_ptr() -> *const u8 {
        with_harness(|harness| {
            harness
                .frame
                .as_ref()
                .map_or(core::ptr::null(), |frame| frame.pixels.as_ptr())
        })
    }

    /// How many bytes of pixels those are, or `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_frame_len() -> u32 {
        with_harness(|harness| {
            harness.frame.as_ref().map_or(0, |frame| {
                u32::try_from(frame.pixels.len()).unwrap_or(u32::MAX)
            })
        })
    }

    /// The readback's width in pixels, or `0`.
    ///
    /// Read off the swapchain rather than off the size the scene was asked for:
    /// a backend that hands back a different extent is a crack of its own, and
    /// the comparator is what has to see it.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_frame_width() -> u32 {
        with_harness(|harness| harness.frame.as_ref().map_or(0, |frame| frame.width))
    }

    /// The readback's height in pixels, or `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_frame_height() -> u32 {
        with_harness(|harness| harness.frame.as_ref().map_or(0, |frame| frame.height))
    }

    /// Address of the channel-order name — `rgba` or `bgra` — or `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_frame_order_ptr() -> *const u8 {
        with_harness(|harness| {
            harness
                .frame
                .as_ref()
                .map_or(core::ptr::null(), |frame| frame.order.as_ptr())
        })
    }

    /// Length in bytes of the channel-order name, or `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_render_harness_frame_order_len() -> u32 {
        with_harness(|harness| {
            harness
                .frame
                .as_ref()
                .map_or(0, |frame| u32::try_from(frame.order.len()).unwrap_or(0))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ORDER_BGRA, ORDER_RGBA, SCENES, State, order_name, yield_now};
    use crcbl::hal::Format;
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    /// The wire codes are what JS switches on, and `web/harness/main.js` carries
    /// its own copy of them. Nothing but this pins the two together.
    #[test]
    fn the_state_codes_are_the_ones_js_reads() {
        assert_eq!(State::Idle.code(), 0);
        assert_eq!(State::Rendered.code(), 2);
        assert_eq!(State::Failed.code(), 3);
        // `Running` needs a future to name, and any future will do: the code is
        // a property of the variant, not of what is in it.
        assert_eq!(State::Running(Box::pin(async { unreachable!() })).code(), 1);
    }

    /// A BGRA readback compared as RGBA is a red/blue swap — a failure that
    /// looks like a shader bug and is not one. This is the mapping that stops
    /// it, and it is two arms, so it is worth two assertions.
    #[test]
    fn the_bgra_formats_are_the_only_ones_named_bgra() {
        assert_eq!(order_name(Format::Bgra8Unorm), ORDER_BGRA);
        assert_eq!(order_name(Format::Bgra8UnormSrgb), ORDER_BGRA);
        assert_eq!(order_name(Format::Rgba8Unorm), ORDER_RGBA);
        assert_eq!(order_name(Format::Rgba8UnormSrgb), ORDER_RGBA);
    }

    /// The whole scheduling of the drive is this one future. If it ever became
    /// `Ready` on the first poll, every `poll` loop in `drive` would spin the
    /// browser's one thread inside a single `step` instead of coming back next
    /// frame — a hang with no error, which is the failure hardest to read from
    /// outside.
    #[test]
    fn yield_now_is_pending_once_and_then_ready() {
        let mut future = pin!(yield_now());
        let mut context = Context::from_waker(Waker::noop());
        assert_eq!(future.as_mut().poll(&mut context), Poll::Pending);
        assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(()));
    }

    /// The golden names are the filenames the comparator looks up, so a typo
    /// here is a "reference not found" in a gate rather than a compile error.
    #[test]
    fn every_scene_names_a_committed_golden() {
        let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/crcbl/tests/golden");
        for (_, name) in SCENES {
            let path = golden.join(format!("{name}.png"));
            assert!(
                path.exists(),
                "scene `{name}` names {}, which is not committed",
                path.display()
            );
        }
    }
}
