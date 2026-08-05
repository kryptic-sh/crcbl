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
    /// Every variant, in declaration order.
    ///
    /// The seam owns this list so a backend does not keep a second copy of it.
    /// That mattered: `crcbl-vk`'s reverse mapping ends in `_ => None`, so a
    /// format added forward and forgotten in reverse is not a compile error —
    /// it is a format its `surface_caps` silently drops — and the round-trip
    /// test that would have caught it was iterating the backend's *own* copy of
    /// this list, which had the same omission. Iterating `ALL` ties the two
    /// together.
    ///
    /// **Add a variant above, add it here.** Nothing on stable Rust can count
    /// an enum's variants, so this list is hand-maintained; the closest guard
    /// is this module's `the_format_table_is_in_declaration_order` test, which
    /// catches an entry inserted anywhere but the very end.
    pub const ALL: &'static [Self] = &[
        Self::R8Unorm,
        Self::Rg8Unorm,
        Self::Rgba8Unorm,
        Self::Rgba8UnormSrgb,
        Self::Bgra8Unorm,
        Self::Bgra8UnormSrgb,
        Self::Rgb10a2Unorm,
        Self::R11g11b10Float,
        Self::R16Float,
        Self::Rg16Float,
        Self::Rgba16Float,
        Self::R32Float,
        Self::Rg32Float,
        Self::Rgba32Float,
        Self::R32Uint,
        Self::Rg32Uint,
        Self::D32Float,
        Self::D32FloatS8Uint,
        Self::D24UnormS8Uint,
        Self::D16Unorm,
        Self::Bc1RgbaUnorm,
        Self::Bc1RgbaUnormSrgb,
        Self::Bc3RgbaUnorm,
        Self::Bc3RgbaUnormSrgb,
        Self::Bc4RUnorm,
        Self::Bc5RgUnorm,
        Self::Bc6hRgbUfloat,
        Self::Bc7RgbaUnorm,
        Self::Bc7RgbaUnormSrgb,
    ];

    /// Bytes per block. For uncompressed formats a "block" is one texel.
    ///
    /// # Not the copy footprint of a depth/stencil format
    ///
    /// A combined depth+stencil format has **two planes**, and a copy names one
    /// of them: `D32FloatS8Uint` reports `8` here (its in-memory element size)
    /// while a depth-aspect copy moves 4 bytes per texel and a stencil-aspect
    /// copy moves 1. `D24UnormS8Uint` reports `4` while its stencil aspect is
    /// again 1 byte and its depth aspect is, on every API that permits the copy
    /// at all, a 4-byte element. Use [`Format::texel_size`] when sizing a
    /// [`BufferImageCopy`](crate::BufferImageCopy); `block_size` answers "how
    /// wide is one element of this format", which is the right question for a
    /// colour format and only that.
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

    /// Bytes one block of `aspect` occupies in a buffer during a copy.
    ///
    /// This is the number a [`BufferImageCopy`](crate::BufferImageCopy) is
    /// sized against, and it is **not** [`Format::block_size`] for a combined
    /// depth/stencil format — see that method for why. `None` means the format
    /// does not have that aspect, or the aspect set names more than one plane,
    /// neither of which a single copy region can describe.
    ///
    /// ```
    /// use crcbl_hal::{Format, ImageAspect};
    ///
    /// assert_eq!(Format::D32FloatS8Uint.block_size(), 8);
    /// assert_eq!(Format::D32FloatS8Uint.texel_size(ImageAspect::DEPTH), Some(4));
    /// assert_eq!(Format::D32FloatS8Uint.texel_size(ImageAspect::STENCIL), Some(1));
    /// assert_eq!(Format::D32FloatS8Uint.texel_size(ImageAspect::COLOR), None);
    /// assert_eq!(Format::Rgba8Unorm.texel_size(ImageAspect::COLOR), Some(4));
    /// ```
    #[must_use]
    pub const fn texel_size(self, aspect: crate::ImageAspect) -> Option<u32> {
        use crate::ImageAspect as A;
        // Exactly one plane, or the question has no single answer.
        if aspect.bits().count_ones() != 1 {
            return None;
        }
        if aspect.contains(A::COLOR) {
            return if self.is_depth_stencil() {
                None
            } else {
                Some(self.block_size())
            };
        }
        if aspect.contains(A::STENCIL) {
            return if self.has_stencil() { Some(1) } else { None };
        }
        // Depth.
        match self {
            Self::D16Unorm => Some(2),
            // The 24-bit plane is addressed as a 32-bit element by every API
            // that permits copying it at all.
            Self::D32Float | Self::D32FloatS8Uint | Self::D24UnormS8Uint => Some(4),
            _ => None,
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

    /// [`Format::ALL`] must list every variant exactly once, in declaration
    /// order — that is what lets every consumer of it, this module and
    /// `crcbl-vk` alike, treat "iterated `ALL`" as "covered every format".
    ///
    /// Casting a variant to its discriminant is what makes it checkable at all,
    /// and it only works because `Format` is fieldless. An entry inserted or
    /// duplicated anywhere shifts every index after it and fails here; a
    /// variant *appended* to the enum and not appended here does not, and
    /// nothing on stable Rust can see it. That gap is the reason `ALL` carries
    /// the instruction it does.
    #[test]
    fn the_format_table_is_in_declaration_order() {
        for (index, format) in Format::ALL.iter().enumerate() {
            assert_eq!(*format as usize, index, "{format:?} is out of place");
        }
    }

    #[test]
    fn every_format_has_a_nonzero_block_size() {
        for format in Format::ALL {
            assert!(format.block_size() > 0, "{format:?}");
        }
    }

    #[test]
    fn compressed_formats_use_four_by_four_blocks() {
        for format in Format::ALL {
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
        for format in Format::ALL {
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

    /// The gap `block_size` cannot express: a combined format's two planes have
    /// different copy footprints, and neither is the element size.
    #[test]
    fn depth_stencil_aspects_have_their_own_copy_footprint() {
        use crate::ImageAspect as A;

        assert_eq!(Format::D32FloatS8Uint.block_size(), 8);
        assert_eq!(Format::D32FloatS8Uint.texel_size(A::DEPTH), Some(4));
        assert_eq!(Format::D32FloatS8Uint.texel_size(A::STENCIL), Some(1));
        assert_eq!(Format::D24UnormS8Uint.texel_size(A::STENCIL), Some(1));
        assert_eq!(Format::D16Unorm.texel_size(A::DEPTH), Some(2));

        for format in Format::ALL {
            // A colour format has exactly one plane, and it is the block size.
            if !format.is_depth_stencil() {
                assert_eq!(format.texel_size(A::COLOR), Some(format.block_size()));
                assert_eq!(format.texel_size(A::DEPTH), None, "{format:?}");
                assert_eq!(format.texel_size(A::STENCIL), None, "{format:?}");
            } else {
                assert_eq!(format.texel_size(A::COLOR), None, "{format:?}");
                assert!(format.texel_size(A::DEPTH).is_some(), "{format:?}");
                assert_eq!(
                    format.texel_size(A::STENCIL).is_some(),
                    format.has_stencil(),
                    "{format:?}"
                );
            }
            // More than one plane, or none, is not a copy region.
            assert_eq!(format.texel_size(A::DEPTH | A::STENCIL), None, "{format:?}");
            assert_eq!(format.texel_size(A::empty()), None, "{format:?}");
        }
    }

    #[test]
    fn index_sizes() {
        assert_eq!(IndexFormat::Uint16.size(), 2);
        assert_eq!(IndexFormat::Uint32.size(), 4);
    }
}
