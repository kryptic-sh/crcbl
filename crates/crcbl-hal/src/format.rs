//! Pixel and index formats.
//!
//! # Exhaustiveness
//!
//! [`Format`] is deliberately **not** `#[non_exhaustive]`. The whole engine is
//! one workspace, so adding a format *should* break every backend's `match` at
//! compile time. A format silently falling into a `_ => unsupported` arm is a
//! runtime bug three backends deep; a compile error is a five-line patch.
//!
//! # HDR
//!
//! [`Format::Rgba16Float`] and [`Format::R11g11b10Float`] exist from P0 because
//! the renderer targets `RGBA16F` + tonemap from the first lit mesh
//! (`docs/plan/02-vulkan-backend.md`), so P7's real HDR stack does not force a
//! wholesale re-bless of the golden images.

/// A texel format.
///
/// The set is the intersection of what Vulkan, Metal, DX12 and WebGPU all
/// support without optional features, plus the depth formats the engine
/// actually asks for. Anything that is not universally available (BC on mobile,
/// `D24UnormS8Uint` on some drivers) is reported through
/// [`Features`](crate::Features) or refused at surface/image creation, never
/// silently substituted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    // --- 8-bit ---
    /// Single-channel 8-bit unsigned normalised.
    R8Unorm,
    /// Two-channel 8-bit unsigned normalised.
    Rg8Unorm,
    /// Four-channel 8-bit unsigned normalised, linear.
    Rgba8Unorm,
    /// Four-channel 8-bit unsigned normalised, sRGB-encoded.
    Rgba8UnormSrgb,
    /// Swapchain-native byte order on most desktop platforms, linear.
    Bgra8Unorm,
    /// Swapchain-native byte order on most desktop platforms, sRGB-encoded.
    Bgra8UnormSrgb,
    // --- packed ---
    /// Packed 10/10/10/2 unsigned normalised — the usual normal-buffer format.
    Rgb10a2Unorm,
    /// Packed 11/11/10 float — half the bandwidth of `Rgba16Float` for
    /// non-alpha HDR intermediates (bloom chains, light accumulation).
    R11g11b10Float,
    // --- 16-bit float ---
    /// Single-channel half float.
    R16Float,
    /// Two-channel half float — motion vectors, packed BRDF LUTs.
    Rg16Float,
    /// **The engine's HDR render-target format.** Four-channel half float.
    Rgba16Float,
    // --- 32-bit ---
    /// Single-channel 32-bit float.
    R32Float,
    /// Two-channel 32-bit float.
    Rg32Float,
    /// Four-channel 32-bit float — readback and reference targets, not a
    /// steady-state render target.
    Rgba32Float,
    /// Single-channel 32-bit unsigned integer — visibility/ID buffers.
    R32Uint,
    /// Two-channel 32-bit unsigned integer.
    Rg32Uint,
    // --- depth/stencil ---
    /// **The engine's depth format.** 32-bit float depth, reversed-Z.
    D32Float,
    /// 32-bit float depth plus 8-bit stencil (padded).
    D32FloatS8Uint,
    /// 24-bit unorm depth plus 8-bit stencil. Present on most desktop GPUs but
    /// not guaranteed; prefer [`Format::D32Float`].
    D24UnormS8Uint,
    /// 16-bit unorm depth — shadow maps where precision allows.
    D16Unorm,
    // --- block compressed (BC / DXT) ---
    /// BC1 (DXT1) RGB(A1), linear.
    Bc1RgbaUnorm,
    /// BC1 (DXT1) RGB(A1), sRGB.
    Bc1RgbaUnormSrgb,
    /// BC3 (DXT5) RGBA, linear.
    Bc3RgbaUnorm,
    /// BC3 (DXT5) RGBA, sRGB.
    Bc3RgbaUnormSrgb,
    /// BC4 single-channel — roughness/metallic/AO atlases.
    Bc4RUnorm,
    /// BC5 two-channel — tangent-space normal maps.
    Bc5RgUnorm,
    /// BC6H unsigned half float — compressed HDR (environment maps).
    Bc6hRgbUfloat,
    /// BC7 RGBA, linear.
    Bc7RgbaUnorm,
    /// BC7 RGBA, sRGB — the default albedo format.
    Bc7RgbaUnormSrgb,
}

impl Format {
    /// Bytes per block. For uncompressed formats a "block" is one texel.
    #[must_use]
    pub const fn block_size(self) -> u32 {
        match self {
            Self::R8Unorm => 1,
            Self::Rg8Unorm | Self::R16Float | Self::D16Unorm => 2,
            Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb
            | Self::Rgb10a2Unorm
            | Self::R11g11b10Float
            | Self::Rg16Float
            | Self::R32Float
            | Self::R32Uint
            | Self::D32Float
            | Self::D24UnormS8Uint => 4,
            Self::Rgba16Float | Self::Rg32Float | Self::Rg32Uint | Self::D32FloatS8Uint => 8,
            Self::Rgba32Float => 16,
            Self::Bc1RgbaUnorm | Self::Bc1RgbaUnormSrgb | Self::Bc4RUnorm => 8,
            Self::Bc3RgbaUnorm
            | Self::Bc3RgbaUnormSrgb
            | Self::Bc5RgUnorm
            | Self::Bc6hRgbUfloat
            | Self::Bc7RgbaUnorm
            | Self::Bc7RgbaUnormSrgb => 16,
        }
    }

    /// Block footprint in texels — `(1, 1)` unless the format is block
    /// compressed, in which case `(4, 4)`.
    #[must_use]
    pub const fn block_extent(self) -> (u32, u32) {
        if self.is_compressed() { (4, 4) } else { (1, 1) }
    }

    /// Whether this is a BC-compressed format.
    #[must_use]
    pub const fn is_compressed(self) -> bool {
        matches!(
            self,
            Self::Bc1RgbaUnorm
                | Self::Bc1RgbaUnormSrgb
                | Self::Bc3RgbaUnorm
                | Self::Bc3RgbaUnormSrgb
                | Self::Bc4RUnorm
                | Self::Bc5RgUnorm
                | Self::Bc6hRgbUfloat
                | Self::Bc7RgbaUnorm
                | Self::Bc7RgbaUnormSrgb
        )
    }

    /// Whether the format carries a depth channel.
    #[must_use]
    pub const fn has_depth(self) -> bool {
        matches!(
            self,
            Self::D32Float | Self::D32FloatS8Uint | Self::D24UnormS8Uint | Self::D16Unorm
        )
    }

    /// Whether the format carries a stencil channel.
    #[must_use]
    pub const fn has_stencil(self) -> bool {
        matches!(self, Self::D32FloatS8Uint | Self::D24UnormS8Uint)
    }

    /// Whether the format may be used as a depth/stencil attachment rather than
    /// a colour one.
    #[must_use]
    pub const fn is_depth_stencil(self) -> bool {
        self.has_depth() || self.has_stencil()
    }

    /// Whether sampling this format applies sRGB→linear decode in hardware.
    #[must_use]
    pub const fn is_srgb(self) -> bool {
        matches!(
            self,
            Self::Rgba8UnormSrgb
                | Self::Bgra8UnormSrgb
                | Self::Bc1RgbaUnormSrgb
                | Self::Bc3RgbaUnormSrgb
                | Self::Bc7RgbaUnormSrgb
        )
    }

    /// Whether the format stores floating-point components wider than 8 bits —
    /// i.e. whether it can hold values outside `[0, 1]` and is therefore usable
    /// as an HDR intermediate.
    #[must_use]
    pub const fn is_hdr_capable(self) -> bool {
        matches!(
            self,
            Self::R11g11b10Float
                | Self::R16Float
                | Self::Rg16Float
                | Self::Rgba16Float
                | Self::R32Float
                | Self::Rg32Float
                | Self::Rgba32Float
                | Self::Bc6hRgbUfloat
        )
    }
}

/// Index buffer element width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndexFormat {
    /// 16-bit indices — half the bandwidth, capped at 65 535 vertices per
    /// index-buffer range.
    Uint16,
    /// 32-bit indices.
    Uint32,
}

impl IndexFormat {
    /// Size of one index in bytes.
    #[must_use]
    pub const fn size(self) -> u64 {
        match self {
            Self::Uint16 => 2,
            Self::Uint32 => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[Format] = &[
        Format::R8Unorm,
        Format::Rg8Unorm,
        Format::Rgba8Unorm,
        Format::Rgba8UnormSrgb,
        Format::Bgra8Unorm,
        Format::Bgra8UnormSrgb,
        Format::Rgb10a2Unorm,
        Format::R11g11b10Float,
        Format::R16Float,
        Format::Rg16Float,
        Format::Rgba16Float,
        Format::R32Float,
        Format::Rg32Float,
        Format::Rgba32Float,
        Format::R32Uint,
        Format::Rg32Uint,
        Format::D32Float,
        Format::D32FloatS8Uint,
        Format::D24UnormS8Uint,
        Format::D16Unorm,
        Format::Bc1RgbaUnorm,
        Format::Bc1RgbaUnormSrgb,
        Format::Bc3RgbaUnorm,
        Format::Bc3RgbaUnormSrgb,
        Format::Bc4RUnorm,
        Format::Bc5RgUnorm,
        Format::Bc6hRgbUfloat,
        Format::Bc7RgbaUnorm,
        Format::Bc7RgbaUnormSrgb,
    ];

    #[test]
    fn every_format_has_a_nonzero_block_size() {
        for format in ALL {
            assert!(format.block_size() > 0, "{format:?}");
        }
    }

    #[test]
    fn compressed_formats_use_four_by_four_blocks() {
        for format in ALL {
            let expected = if format.is_compressed() {
                (4, 4)
            } else {
                (1, 1)
            };
            assert_eq!(format.block_extent(), expected, "{format:?}");
        }
    }

    #[test]
    fn depth_formats_are_classified_consistently() {
        assert!(Format::D32Float.has_depth());
        assert!(!Format::D32Float.has_stencil());
        assert!(Format::D32FloatS8Uint.has_stencil());
        assert!(Format::D24UnormS8Uint.has_stencil());
        for format in ALL {
            assert_eq!(
                format.is_depth_stencil(),
                format.has_depth() || format.has_stencil(),
                "{format:?}"
            );
            if format.has_stencil() {
                assert!(
                    format.has_depth(),
                    "no stencil-only formats exist: {format:?}"
                );
            }
        }
    }

    /// The HDR pipeline is specified in terms of `Rgba16Float`; if it ever
    /// stops being classified as HDR-capable the P1 target selection is broken.
    #[test]
    fn hdr_target_format_is_available_and_classified() {
        assert!(Format::Rgba16Float.is_hdr_capable());
        assert!(Format::R11g11b10Float.is_hdr_capable());
        assert!(!Format::Rgba8Unorm.is_hdr_capable());
        assert!(!Format::Rgba8UnormSrgb.is_hdr_capable());
    }

    #[test]
    fn srgb_variants_are_marked_and_pair_with_a_linear_one() {
        assert!(Format::Bgra8UnormSrgb.is_srgb());
        assert!(!Format::Bgra8Unorm.is_srgb());
        // sRGB is a decode rule, not a layout change: paired formats must agree
        // on size.
        assert_eq!(
            Format::Rgba8Unorm.block_size(),
            Format::Rgba8UnormSrgb.block_size()
        );
        assert_eq!(
            Format::Bc7RgbaUnorm.block_size(),
            Format::Bc7RgbaUnormSrgb.block_size()
        );
    }

    #[test]
    fn index_sizes() {
        assert_eq!(IndexFormat::Uint16.size(), 2);
        assert_eq!(IndexFormat::Uint32.size(), 4);
    }
}
