//! The DXGI descriptor a window swapchain is created with.

use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE, DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::core::BOOL;

#[cfg(doc)]
use crate::Composition;

/// The descriptor a window swapchain is created with, `HWND` or composition.
///
/// Here rather than in a backend because more than one backend creates the
/// composition kind, and it must be the same one. One function for both kinds, because the two differ only in `alpha`, and two
/// copies of the rest would be where they drift: a composition swapchain must
/// be flip-model and `DXGI_SCALING_STRETCH` too, which is what an `HWND` one
/// already is here.
pub fn swap_chain_desc(
    extent: (u32, u32),
    format: DXGI_FORMAT,
    buffers: u32,
    flags: DXGI_SWAP_CHAIN_FLAG,
    alpha: DXGI_ALPHA_MODE,
) -> DXGI_SWAP_CHAIN_DESC1 {
    DXGI_SWAP_CHAIN_DESC1 {
        Width: extent.0,
        Height: extent.1,
        Format: format,
        Stereo: BOOL::from(false),
        // Flip-model swapchains are single-sampled by definition: multisampling
        // is done on an offscreen target and resolved into the back buffer,
        // which is what the engine's tonemap pass already does.
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: buffers,
        // Stretch rather than `DXGI_SCALING_NONE`, so the frames rendered
        // between a window resize and the swapchain catching up still cover an
        // `HWND` window instead of leaving an unpainted border. A composition
        // swapchain accepts nothing else; its visual shows it unscaled anyway
        // (see [`Composition`]).
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: alpha,
        Flags: flags.0.cast_unsigned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_ALPHA_MODE_IGNORE, DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
    };
    use windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING;

    /// **Both kinds of window swapchain are the same flip-model, stretched
    /// descriptor, apart from the alpha mode.**
    ///
    /// `CreateSwapChainForComposition` refuses anything but a flip effect and
    /// `DXGI_SCALING_STRETCH`, so a change to the shared descriptor that an
    /// `HWND` swapchain tolerated would break only transparent windows, which
    /// only a device suite would then find.
    #[test]
    fn a_composition_swapchain_descriptor_differs_from_an_hwnd_one_only_in_alpha() {
        const EXTENT: (u32, u32) = (320, 200);
        const BUFFERS: u32 = 3;
        let flags = DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING;
        let format = DXGI_FORMAT_B8G8R8A8_UNORM;
        let hwnd = swap_chain_desc(EXTENT, format, BUFFERS, flags, DXGI_ALPHA_MODE_IGNORE);
        let composed = swap_chain_desc(
            EXTENT,
            format,
            BUFFERS,
            flags,
            DXGI_ALPHA_MODE_PREMULTIPLIED,
        );
        assert_eq!(composed.AlphaMode, DXGI_ALPHA_MODE_PREMULTIPLIED);
        assert_eq!(hwnd.AlphaMode, DXGI_ALPHA_MODE_IGNORE);
        for desc in [hwnd, composed] {
            assert_eq!(desc.SwapEffect, DXGI_SWAP_EFFECT_FLIP_DISCARD);
            assert_eq!(desc.Scaling, DXGI_SCALING_STRETCH);
            assert_eq!((desc.Width, desc.Height), EXTENT);
            assert_eq!(desc.BufferCount, BUFFERS);
            assert_eq!(desc.Format, format);
            assert_eq!(desc.Flags, flags.0.cast_unsigned());
            assert_eq!(desc.SampleDesc.Count, 1);
        }
        assert_eq!(
            DXGI_SWAP_CHAIN_DESC1 {
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                ..composed
            },
            hwnd
        );
    }
}
