//! `crcbl-wgpu` — the engine's wgpu backend (Tier B).
//!
//! Implements `crcbl-hal`'s [`Instance`] and [`Device`] traits on `wgpu`.
//!
//! # Current status (P5.1 — Slice 2)
//!
//! Instance (adapter enumeration, device creation) and Device stub. Resources,
//! swapchain, and command encoding land in P5.2.

mod command;
mod conv;
mod device;
mod instance;
mod resources;

pub use instance::WgpuInstance;

/// Creates a native wgpu instance.
#[must_use]
pub fn create_native() -> Option<WgpuInstance> {
    WgpuInstance::new_native()
}
