//! DirectComposition for the Win32 render backends: how a transparent window
//! shows a swapchain.
//!
//! A transparent Win32 window shows only a DirectComposition visual tree, so a
//! backend that presents to one needs that tree, with its ownership and
//! teardown rules. This crate holds it once, for every backend that presents
//! to such a window. See [`Composition`] for the shape and the rules.
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
pub use composition::{Binding, Composition};
