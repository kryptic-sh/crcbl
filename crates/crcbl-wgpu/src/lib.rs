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
//! * **No readback.** wgpu's `map_async` completes on a later turn of the event
//!   loop; the polling ring the seam's `request_readback` needs is not wired
//!   yet, and the method says so.
//! * **One queue.** `Device::queue` returns `None` for
//!   [`Compute`](crcbl_hal::QueueKind::Compute) and
//!   [`Transfer`](crcbl_hal::QueueKind::Transfer).
//! * **No semaphore objects.** Submissions on one queue run in order and wgpu
//!   inserts its own hazard barriers, so `pipeline_barrier` is a real no-op and
//!   a timeline "signal" is the completion of the submission that carries it.
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
//! ## Known gap: `ui.slang` still does not compile here
//!
//! WGSL has no push constants, and Slang's WGSL target emits the block as a
//! module-scope `var<uniform>` with no `@group`/`@binding` — invalid WGSL,
//! which `naga` refuses. `mesh`, `tonemap` and `triangle` compile; `ui` does
//! not, so `apps/breakout` under `--backend wgpu` stops at that module while
//! `apps/sandbox`, which has no UI pass, gets past every shader it needs. The
//! fix is a uniform buffer in place of the push-constant block in
//! `crcbl-render`'s UI pass and in `ui.slang` — the Tier B data-layout rule
//! `docs/plan/10-wasm-webgpu.md` states for this backend anyway. Recorded in
//! full in `crcbl-shaders`' crate docs.
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
