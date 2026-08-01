//! `crcbl-wgpu` — the engine's wgpu backend (Tier B).
//!
//! Implements `crcbl-hal`'s [`Instance`](crcbl_hal::Instance) and
//! [`Device`](crcbl_hal::Device) traits on `wgpu`.
//!
//! # What this backend does not have
//!
//! Reported through [`DeviceCaps`](crcbl_hal::DeviceCaps) rather than
//! discovered at a call site, so the renderer's Tier B path is a capability
//! decision and not a `cfg`:
//!
//! * **No buffer device address.** wgpu has no raw GPU pointers, so this
//!   backend is Tier B by construction — see `crcbl_hal::caps`.
//! * **No queries.** `create_query_set` returns
//!   [`Unsupported`](crcbl_hal::HalError::Unsupported) and the timestamp
//!   features are therefore never advertised, so the profiler HUD degrades
//!   rather than asking for something that will be refused.
//! * **One queue.** `Device::queue` returns `None` for
//!   [`Compute`](crcbl_hal::QueueKind::Compute) and
//!   [`Transfer`](crcbl_hal::QueueKind::Transfer).
//! * **No semaphore objects.** Submissions on one queue run in order and wgpu
//!   inserts its own hazard barriers, so `pipeline_barrier` is a real no-op and
//!   a timeline "signal" is the completion of the submission that carries it.
//!
//! # Presenting: `Queue::present`, never a drop (P5.11)
//!
//! [`Device::present`](crcbl_hal::Device::present) hands the acquired
//! `wgpu::SurfaceTexture` to `wgpu::Queue::present`. That is not a stylistic
//! choice: `SurfaceTexture`'s `Drop` **discards** the image instead of
//! presenting it, and a discarded image is never returned to the presentation
//! engine. This backend used to drop it — the code said "dropping the
//! SurfaceTexture auto-presents", which is the opposite of what wgpu does — so a
//! windowed run acquired `image_count` images, exhausted the swapchain, and
//! blocked in the next acquire until it reported
//! [`SurfaceError::Timeout`](crcbl_hal::SurfaceError::Timeout). It died on frame
//! four of every run, and any frame budget shorter than the ring hid it.
//!
//! Two consequences are in the code as rules rather than as comments: a second
//! [`acquire_next_frame`](crcbl_hal::Device::acquire_next_frame) with a frame
//! still outstanding is refused (it would drop, and therefore discard, the
//! first), and a reconfigure or destroy presents an outstanding frame rather
//! than dropping it.
//!
//! ## Known gap, upstream: a validation error on every windowed acquire
//!
//! On Linux, a windowed run over wgpu's Vulkan backend logs
//! `VUID-vkAcquireNextImageKHR-fence-10066` once per acquire. **It is not
//! reachable from this crate.** `wgpu-hal` 30.0.0's `NativeSwapchain` owns a
//! `vk::Fence` (`src/vulkan/swapchain/native.rs`), passes it to *every*
//! `vkAcquireNextImageKHR`, and waits on and resets it only inside a
//! `#[cfg(target_os = "windows")]` block — the fence exists for a Windows-only
//! frame-pacing fix (gfx-rs/wgpu#8310, #8354). Off Windows it is therefore still
//! signalled from the previous acquire when the next one is made. Nothing above
//! `wgpu` can see the object, 30.0.0 is the newest published version, and the
//! frames are correct: the run completes its whole budget and presents every
//! frame. Recorded here rather than silenced, because turning validation off is
//! the one response that would make it invisible without making it untrue.
//!
//! # Offscreen: a ring of textures, not a surface (P5.11)
//!
//! [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen) creates a
//! surface with no `wgpu::Surface` behind it, and its "swapchain" is a ring of
//! plain textures this backend allocates —
//! `RENDER_ATTACHMENT | COPY_SRC | COPY_DST | TEXTURE_BINDING`, the same four
//! `crcbl-vk`'s offscreen ring asks for. Acquire rotates the ring and present
//! advances it, so `crcbl screenshot`, the headless shell and the golden-image
//! path run through the *same* caller code on both backends. Unlike the windowed
//! path, the ring owns its images for its whole life, so
//! [`AcquiredFrame::index`](crcbl_hal::AcquiredFrame::index) is a real index
//! there and the constant `0` the seam permits only on a real surface.
//!
//! # Readback: `map_async`, polled (P5.11)
//!
//! [`request_readback`](crcbl_hal::Device::request_readback) starts a
//! `map_async` and keeps the callback's answer;
//! [`poll_readback`](crcbl_hal::Device::poll_readback) drives `Device::poll` once
//! and reports `Pending` until it lands;
//! [`destroy_readback`](crcbl_hal::Device::destroy_readback) is the `unmap`.
//! That is the mapping `crcbl_hal::readback`'s table asked each backend for, and
//! it is why the seam is poll-shaped: nothing here blocks, so the same three
//! calls work on the browser's main thread.
//!
//! Every range is checked before wgpu sees it — a misaligned offset, a size that
//! is not a non-zero multiple of four, a buffer without
//! [`MemoryLocation::HostReadback`](crcbl_hal::MemoryLocation::HostReadback) —
//! because wgpu *panics* on those rather than returning, and a panic through a
//! trait object is not a diagnosis.
//!
//! # Shaders: WGSL first
//!
//! [`create_shader_module`](crcbl_hal::Device::create_shader_module) takes
//! [`ShaderModuleDesc::wgsl`](crcbl_hal::ShaderModuleDesc::wgsl) when it is
//! there and falls back to the SPIR-V only when it is not. Both reach `naga`,
//! but its SPIR-V frontend implements a subset that excludes `DrawParameters` —
//! which every artifact `crcbl-shaders` emits declares, because Slang lowers
//! `SV_VertexID` to `gl_VertexIndex - gl_BaseVertex`. Until the seam grew a
//! WGSL field at P5.9 this backend had **never created a shader module on any
//! target**, native or browser; the failure was never browser-specific. A
//! descriptor carrying neither format is an error naming the gap, not a module
//! handle no pipeline could use.
//!
//! `ui.slang` was the last module that would not compile here: WGSL has no push
//! constants, and Slang's WGSL target emitted the block as a module-scope
//! `var<uniform>` with no `@group`/`@binding`, which `naga` refuses. P5.10's
//! uniform-buffer path in `crcbl-render`'s UI pass — the Tier B data-layout rule
//! `docs/plan/10-wasm-webgpu.md` states for this backend anyway — replaced it,
//! so `apps/breakout` now builds every module it needs and draws its HUD through
//! this backend.
//!
//! # Errors the device reports out of band
//!
//! WebGPU does not answer "did this fail?" at the call. `create_shader_module`
//! and `create_render_pipeline` return objects whatever happens and the reason
//! for a failure arrives separately, so a backend that reads only return values
//! believes it built everything it asked for — then submits command buffers the
//! implementation discards. That is not hypothetical: the run that found the UI
//! shader's uniformity error (above) submitted 384 invalid command buffers
//! while reporting a healthy status.
//!
//! Two mechanisms close it, because the error arrives at two different times:
//!
//! * Every device gets an **uncaptured-error handler** the moment it opens.
//!   Each error is logged at `error` and kept in an `errors::ErrorSink`.
//! * Shader-module and pipeline creation run through `WgpuDevice::checked`,
//!   which reports anything raised *during* the call as
//!   [`HalError::Backend`](crcbl_hal::HalError::Backend) naming the call.
//!   `wgpu-core` raises validation errors synchronously, so on native this is
//!   the whole story and the failure reaches the caller as a plain `Err`.
//! * A browser instead delivers it on a later turn of the event loop, where no
//!   call can be blamed. That half surfaces through
//!   [`Device::take_error`](crcbl_hal::Device::take_error), which
//!   `crcbl::engine::GpuContext::acquire` drains before it records a frame.
//!
//! # wasm32
//!
//! The crate compiles for `wasm32` and opens devices there. Adapter enumeration
//! is [`WgpuInstance::new_async`] and device creation is the seam's polled pair,
//! [`Instance::request_device`](crcbl_hal::Instance::request_device) →
//! [`PendingDevice::poll`](crcbl_hal::PendingDevice::poll):
//!
//! ```text
//! let mut pending = instance.request_device(&desc)?;   // starts requestDevice
//! // …once per rAF frame, never blocking:
//! if let DeviceRequestState::Ready(device) = pending.poll()? { … }
//! ```
//!
//! `request_device` starts `GPUAdapter.requestDevice` and keeps the future;
//! each `poll` polls it once with a no-op waker. The browser's own event loop
//! resolves the promise, so the poll after it lands observes the device — the
//! rAF loop is the executor and no thread is ever parked. On native the same
//! future is `core::future::ready`, so the first poll completes and
//! [`Instance::create_device`](crcbl_hal::Instance::create_device) — the seam's
//! blocking wrapper, which does not exist on `wasm32` — behaves as it always
//! did.
//!
//! What remains wasm32-specific: surfaces come from
//! [`SurfaceTarget::Web`](crcbl_core::SurfaceTarget::Web), and
//! [`create_native`] is absent because *adapter enumeration* still blocks in it.
//! Instance creation itself has no polled seam — it is not a `crcbl-hal` trait
//! method — so a browser caller must `await` [`WgpuInstance::new_async`]; the
//! `crcbl` umbrella's backend registry is where that shows up, and it is
//! recorded there.

mod cell;
mod command;
mod conv;
mod device;
mod errors;
mod instance;
mod resources;

pub use instance::WgpuInstance;

/// Creates a native wgpu instance (Vulkan/Metal/DX12 backends).
///
/// Returns `None` when no adapters are found. Blocks on adapter enumeration,
/// which is why it is absent on wasm32 — use
/// [`WgpuInstance::new_async`] there and see the crate docs for the rest of the
/// story.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn create_native() -> Option<WgpuInstance> {
    WgpuInstance::new_native()
}
