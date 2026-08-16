//! The HAL trait implementations: the seam the engine drives this backend
//! through.
//!
//! Where [`instance`](crate::instance) and [`device`](crate::device) hold the
//! *probes* — the state a command's answer is absorbed into — this module holds
//! the [`Instance`](crcbl_hal::Instance), [`PendingDevice`](crcbl_hal::PendingDevice),
//! [`Device`](crcbl_hal::Device) and [`CommandEncoder`](crcbl_hal::CommandEncoder)
//! implementations that wrap them, so a `Box<dyn Device>` can be handed to
//! `crcbl-render`. Each method encodes through the shared
//! [`StreamWriter`](crate::StreamWriter) or reads a [`Reply`](crate::Reply) the
//! probe already knows how to absorb; `poll` becomes `drain` + `absorb` + a
//! match, exactly as [`crate::device`] promised it would.
//!
//! # What this slice does and does not do
//!
//! It builds the trait impls and tests them against an injected reply channel.
//! It does **not** register the backend or render in a browser — the JS shim
//! wiring and the registry entry are a later slice. The one browser-shaped seam
//! it touches, [`SharedChannel::install`](channel::SharedChannel::install), is a
//! no-op off the web.
//!
//! # Every method is one of three kinds
//!
//! Named so a reader — and the next slice — knows which without reading the
//! body:
//!
//! * **Wired**: a stream command exists; the call encodes it.
//! * **Legitimately refused**: WebGPU cannot do it and never will (a mesh
//!   pipeline; the semaphore calls, which are no-ops because WebGPU
//!   auto-synchronises), so refusing or no-op'ing is correct.
//! * **Loudly unsupported**: the stream has no command *yet*, so the call fails
//!   with [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) naming the
//!   gap rather than a silent success. [`device`] and [`encoder`] list which.

pub mod channel;
pub mod device;
pub mod encoder;
pub mod instance;
pub mod open;

#[cfg(test)]
mod tests;

pub use channel::{HandlePool, SharedChannel};
pub use device::{WebGpuDevice, WebGpuPendingDevice};
pub use encoder::WebGpuCommandEncoder;
pub use instance::WebGpuInstance;
pub use open::WebGpuInstanceOpen;
