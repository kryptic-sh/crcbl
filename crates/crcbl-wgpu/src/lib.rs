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
//! # wasm32
//!
//! The crate compiles for `wasm32` and can enumerate adapters there through
//! [`WgpuInstance::new_async`], but **it cannot open a device**:
//! [`Instance::create_device`](crcbl_hal::Instance::create_device) is
//! synchronous and `wgpu`'s `request_device` is a future the browser main
//! thread must not block on. `create_device` therefore returns a
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) naming that gap
//! rather than deadlocking. Closing it needs an async device-creation path
//! above the seam; until then, surfaces from
//! [`SurfaceTarget::Web`](crcbl_core::SurfaceTarget::Web) are created (so the
//! shell half is testable) and nothing else on wasm32 is.

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
