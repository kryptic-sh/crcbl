//! DirectComposition for the Win32 render backends: how a transparent window
//! shows a swapchain.
//!
//! A transparent Win32 window shows only a DirectComposition visual tree, so a
//! backend that presents to one needs that tree, with its ownership and
//! teardown rules. This crate holds it once, for every backend that presents
//! to such a window. See `Composition` for the shape and the rules.
//!
//! A backend whose own swapchain cannot be a visual's content renders into
//! images shared with a small D3D11 device instead, which copies each frame
//! into a composition swapchain: `SharedPresenter`, and `Interop` for the
//! half of it that needs no window.
//!
//! macOS and Linux need nothing like it: there the swapchain's composite alpha
//! is the whole of transparency. `docs/backlog.md` records why a
//! cross-backend composition seam was declined (_A cross-backend composition
//! seam: considered and declined_).
//!
//! Off Windows this crate is empty.

#[cfg(target_os = "windows")]
mod composition;
#[cfg(target_os = "windows")]
mod desc;
#[cfg(target_os = "windows")]
mod shared;

#[cfg(target_os = "windows")]
pub use composition::{Binding, Composition};
#[cfg(target_os = "windows")]
pub use desc::swap_chain_desc;
#[cfg(target_os = "windows")]
pub use shared::{Interop, SHARED_FORMATS, SharedPresenter, client_extent};
