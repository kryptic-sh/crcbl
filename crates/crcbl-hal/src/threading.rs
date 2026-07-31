//! Thread-safety marker for HAL traits.
//!
//! `CommandEncoder`, `Device`, and `Instance` require `Send + Sync` on native
//! platforms so the job system can move them across threads. On `wasm32`, wgpu's
//! web types are `!Send` (they use `Rc` internally), and the browser is
//! single-threaded anyway — so the bound is a no-op.
//!
//! [`HalThreadSafe`] captures this conditionally: it requires `Send + Sync` on
//! native and is implemented for **everything** on wasm.

/// Marker: implementations of HAL traits must satisfy this.
///
/// On native: `Send + Sync`. On wasm32: automatically satisfied.
#[cfg(not(target_arch = "wasm32"))]
pub trait HalThreadSafe: Send + Sync {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> HalThreadSafe for T {}

#[cfg(target_arch = "wasm32")]
pub trait HalThreadSafe {}

#[cfg(target_arch = "wasm32")]
impl<T> HalThreadSafe for T {}
