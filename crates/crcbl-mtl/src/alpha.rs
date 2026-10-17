//! Which composite alpha a `CAMetalLayer` surface offers, and what the layer's
//! `opaque` is set to for each.
//!
//! Plain Rust over the seam's [`CompositeAlpha`], with no Objective-C, so that
//! `cargo test` on any host runs the decisions; the swapchain applies them to
//! the layer, which only a Mac can observe.
//!
//! # Why premultiplied, and only for a transparent window
//!
//! Core Animation composites a layer's contents as premultiplied alpha, and a
//! layer with `opaque = NO` is blended by that alpha with what is behind it.
//! The AppKit shell makes a transparent window's layer non-opaque and says so
//! in [`SurfaceTarget::AppKit`](crcbl_core::SurfaceTarget::AppKit)'s
//! `transparent`, so that surface offers
//! [`PreMultiplied`](CompositeAlpha::PreMultiplied) beside
//! [`Opaque`](CompositeAlpha::Opaque). An opaque window's surface offers
//! `Opaque` alone: its window is opaque, so blending its layer would show
//! nothing but the window's own background. There is no straight-alpha layer
//! and no "let the window system decide", so
//! [`PostMultiplied`](CompositeAlpha::PostMultiplied) and
//! [`Inherit`](CompositeAlpha::Inherit) are never offered.

use crcbl_hal::CompositeAlpha;

/// The composite-alpha modes a layer surface offers, `Opaque` first.
#[must_use]
pub(crate) fn layer_composite_alpha(transparent: bool) -> Vec<CompositeAlpha> {
    if transparent {
        vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied]
    } else {
        vec![CompositeAlpha::Opaque]
    }
}

/// `CALayer.opaque` for a swapchain that asked for `alpha`.
///
/// Set on every configure, so a swapchain asking for
/// [`Opaque`](CompositeAlpha::Opaque) on a transparent window's layer makes the
/// layer opaque, as asked. Only premultiplied leaves it blending; the modes
/// [`layer_composite_alpha`] never offers are refused before a layer is
/// configured.
#[must_use]
pub(crate) fn layer_opaque(alpha: CompositeAlpha) -> bool {
    alpha != CompositeAlpha::PreMultiplied
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Red when a transparent layer stops offering premultiplied, when an
    /// opaque one starts, or when `Opaque` falls off either list (the engine's
    /// default must always be configurable).
    #[test]
    fn a_transparent_layer_offers_premultiplied_and_an_opaque_one_does_not() {
        assert_eq!(layer_composite_alpha(false), vec![CompositeAlpha::Opaque]);
        assert_eq!(
            layer_composite_alpha(true),
            vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied]
        );
    }

    /// Red when the layer is left non-opaque for an opaque swapchain, or made
    /// opaque for a premultiplied one, which is the defect this module exists
    /// to fix: the layer forced opaque under a transparent window.
    #[test]
    fn only_premultiplied_leaves_the_layer_non_opaque() {
        assert!(layer_opaque(CompositeAlpha::Opaque));
        assert!(!layer_opaque(CompositeAlpha::PreMultiplied));
    }
}
