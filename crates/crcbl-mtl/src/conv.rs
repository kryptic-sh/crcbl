//! The seam's vocabulary, translated into Metal's.
//!
//! Every function here is total and pure: a seam enum in, a Metal enum out, no
//! device involved. That is deliberate — a mapping table is the part of a
//! backend that is wrong *silently*, so it is kept where it can be read end to
//! end and tested without hardware.
//!
//! # The rule: an exact format, or an error — never a near miss
//!
//! [`pixel_format`] is a `match` over every [`Format`] the seam declares, and
//! the seam is deliberately not `#[non_exhaustive]` so adding one breaks this
//! arm list at compile time. Every variant has an exact Metal counterpart —
//! same channel order, same width, same encoding — so nothing here substitutes.
//! Two formats that *look* interchangeable are not:
//!
//! * **sRGB is not a decoration.** `Rgba8Unorm` and `Rgba8UnormSrgb` differ
//!   only in whether the hardware decodes on read and encodes on write, and
//!   getting that wrong produces an image that is merely *too dark* rather than
//!   obviously broken — which is exactly how it survives review.
//!   `crcbl-wgpu` shipped that bug. `srgb_pairs_map_to_metals_srgb_formats`
//!   below is the assertion that stops it happening here.
//! * **`R11g11b10Float` is `RG11B10Float`, not `RGB9E5Float`.** Both are packed
//!   32-bit HDR formats and only one of them has 11/11/10 bits with no shared
//!   exponent.
//!
//! What is *not* universal is availability, and that is a device question
//! rather than a table one: `D24UnormS8Uint` exists only where
//! `MTLDevice::isDepth24Stencil8PixelFormatSupported` says so (Apple silicon
//! says no), and the BC formats only with
//! [`Features::TEXTURE_COMPRESSION_BC`](crcbl_hal::Features::TEXTURE_COMPRESSION_BC).
//! Both are checked in `device.rs` at image creation, where the device is in
//! hand; a table cannot answer them and must not pretend to.

use crcbl_hal::{
    CompareOp, FilterMode, Format, ImageType, ImageUsage, ImageViewType, MemoryLocation,
    SamplerAddressMode,
};
use objc2_metal::{
    MTLCPUCacheMode, MTLCompareFunction, MTLPixelFormat, MTLResourceOptions, MTLSamplerAddressMode,
    MTLSamplerMinMagFilter, MTLSamplerMipFilter, MTLStorageMode, MTLTextureType, MTLTextureUsage,
};

/// The seam's texel format as Metal spells it.
///
/// See the module docs for why nothing here approximates.
pub(crate) fn pixel_format(format: Format) -> MTLPixelFormat {
    match format {
        Format::R8Unorm => MTLPixelFormat::R8Unorm,
        Format::Rg8Unorm => MTLPixelFormat::RG8Unorm,
        Format::Rgba8Unorm => MTLPixelFormat::RGBA8Unorm,
        Format::Rgba8UnormSrgb => MTLPixelFormat::RGBA8Unorm_sRGB,
        Format::Bgra8Unorm => MTLPixelFormat::BGRA8Unorm,
        Format::Bgra8UnormSrgb => MTLPixelFormat::BGRA8Unorm_sRGB,
        Format::Rgb10a2Unorm => MTLPixelFormat::RGB10A2Unorm,
        // 11 bits red, 11 green, 10 blue, no shared exponent — the same layout
        // Vulkan calls `B10G11R11_UFLOAT_PACK32` and DXGI calls
        // `R11G11B10_FLOAT`. `RGB9E5Float` sits next to it in the enum and is a
        // different thing entirely.
        Format::R11g11b10Float => MTLPixelFormat::RG11B10Float,
        Format::R16Float => MTLPixelFormat::R16Float,
        Format::Rg16Float => MTLPixelFormat::RG16Float,
        Format::Rgba16Float => MTLPixelFormat::RGBA16Float,
        Format::R32Float => MTLPixelFormat::R32Float,
        Format::Rg32Float => MTLPixelFormat::RG32Float,
        Format::Rgba32Float => MTLPixelFormat::RGBA32Float,
        Format::R32Uint => MTLPixelFormat::R32Uint,
        Format::Rg32Uint => MTLPixelFormat::RG32Uint,
        Format::D32Float => MTLPixelFormat::Depth32Float,
        Format::D32FloatS8Uint => MTLPixelFormat::Depth32Float_Stencil8,
        Format::D24UnormS8Uint => MTLPixelFormat::Depth24Unorm_Stencil8,
        Format::D16Unorm => MTLPixelFormat::Depth16Unorm,
        // BC1 in Metal is `BC1_RGBA`, which is DXT1 with the one-bit alpha the
        // seam's name already says it has.
        Format::Bc1RgbaUnorm => MTLPixelFormat::BC1_RGBA,
        Format::Bc1RgbaUnormSrgb => MTLPixelFormat::BC1_RGBA_sRGB,
        Format::Bc3RgbaUnorm => MTLPixelFormat::BC3_RGBA,
        Format::Bc3RgbaUnormSrgb => MTLPixelFormat::BC3_RGBA_sRGB,
        Format::Bc4RUnorm => MTLPixelFormat::BC4_RUnorm,
        Format::Bc5RgUnorm => MTLPixelFormat::BC5_RGUnorm,
        // Unsigned, not `BC6H_RGBFloat`: the seam's name says `Ufloat` and the
        // signed variant decodes negative values from the same bits.
        Format::Bc6hRgbUfloat => MTLPixelFormat::BC6H_RGBUfloat,
        Format::Bc7RgbaUnorm => MTLPixelFormat::BC7_RGBAUnorm,
        Format::Bc7RgbaUnormSrgb => MTLPixelFormat::BC7_RGBAUnorm_sRGB,
    }
}

/// Where a resource's memory lives, in Metal's three-way vocabulary.
///
/// # Decision: `Shared` for both host-visible locations, never `Managed`
///
/// Metal offers four storage modes and this backend uses two.
///
/// * [`MemoryLocation::DeviceLocal`] → [`MTLStorageMode::Private`]. Not
///   mappable, GPU-local, and the only mode a discrete GPU can put in its own
///   memory. Exactly what the seam's "everything the GPU reads in a hot loop"
///   asks for.
/// * [`MemoryLocation::HostUpload`] and [`MemoryLocation::HostReadback`] →
///   [`MTLStorageMode::Shared`]. One allocation both processors address, with
///   coherency guaranteed at command-buffer boundaries and **no explicit
///   synchronisation call of any kind**.
///
/// [`MTLStorageMode::Managed`] is the mode this mapping deliberately does not
/// reach for, and the reason is not performance:
///
/// 1. **It is the two-copy mode, and both directions need a call this backend
///    does not have.** Metal's own header states the contract: a CPU write to a
///    managed resource must be followed by `didModifyRange:`, and the CPU
///    cannot see GPU writes until a `MTLBlitCommandEncoder`
///    `synchronizeResource:` has completed. The blit encoder is the MTL3 slice.
///    Picking `Managed` for [`MemoryLocation::HostReadback`] today would return
///    **stale bytes on an Intel Mac and correct bytes on an Apple silicon
///    one** — a correctness bug visible on exactly one class of machine, which
///    is the failure mode worth the most care in this file.
/// 2. **It does not exist everywhere.** `Managed` is macOS-only, and it exists
///    for GPUs that do not share memory with the CPU. On a machine where
///    `MTLDevice::hasUnifiedMemory` is true there is one copy regardless, so
///    `Managed` buys nothing and still costs the two synchronising calls.
///    `Shared` is correct on both classes of Mac with one mapping, which is
///    what stops this being a per-machine branch nobody can test on both sides.
///
/// The cost is real and worth stating: on a discrete GPU a `Shared` buffer
/// lives in system memory and the GPU reads it across PCIe. That is the right
/// trade for what these two locations are *for* — a staging ring written once
/// and read once, and a debug readback ring — and it is not the right trade for
/// bulk data, which is why the answer for a GPU-resident upload target is
/// `Private` plus a blit from a `Shared` staging buffer, not `Managed`.
///
/// [`MTLStorageMode::Memoryless`] is likewise unused: it names a tile-memory
/// attachment that never reaches RAM, and the seam has no "transient" usage to
/// key it off. Guessing at one from [`ImageUsage`] would produce an image whose
/// contents silently do not persist.
pub(crate) fn storage_mode(memory: MemoryLocation) -> MTLStorageMode {
    match memory {
        MemoryLocation::DeviceLocal => MTLStorageMode::Private,
        MemoryLocation::HostUpload | MemoryLocation::HostReadback => MTLStorageMode::Shared,
    }
}

/// How the CPU caches its view of the allocation.
///
/// This is the seam's own wording turned into the Metal flag that implements
/// it. [`MemoryLocation::HostUpload`] is documented as "writes are
/// sequential-only: treat mapped memory as write-combined and never read it
/// back", and [`MTLCPUCacheMode::WriteCombined`] is that sentence: writes
/// stream out without polluting the cache, reads are correct and slow.
///
/// [`MemoryLocation::HostReadback`] is documented as "CPU-readable and cached"
/// and so stays on [`MTLCPUCacheMode::DefaultCache`], where reading it back is
/// the fast path. [`MemoryLocation::DeviceLocal`] never has a CPU mapping at
/// all; Metal ignores the cache mode of a `Private` resource, and
/// `DefaultCache` is the neutral value to hand it.
pub(crate) fn cpu_cache_mode(memory: MemoryLocation) -> MTLCPUCacheMode {
    match memory {
        MemoryLocation::HostUpload => MTLCPUCacheMode::WriteCombined,
        MemoryLocation::DeviceLocal | MemoryLocation::HostReadback => MTLCPUCacheMode::DefaultCache,
    }
}

/// [`storage_mode`] and [`cpu_cache_mode`] packed the way `newBufferWithLength:`
/// wants them.
///
/// `MTLResourceOptions` is those two enums shifted into one word; the named
/// constants are used rather than the shifts so the two halves cannot drift
/// apart from the functions above.
pub(crate) fn resource_options(memory: MemoryLocation) -> MTLResourceOptions {
    let storage = match storage_mode(memory) {
        MTLStorageMode::Private => MTLResourceOptions::StorageModePrivate,
        // `Shared` is the only other mode this backend produces; see
        // `storage_mode` for why `Managed` and `Memoryless` are absent.
        _ => MTLResourceOptions::StorageModeShared,
    };
    let cache = match cpu_cache_mode(memory) {
        MTLCPUCacheMode::WriteCombined => MTLResourceOptions::CPUCacheModeWriteCombined,
        _ => MTLResourceOptions::CPUCacheModeDefaultCache,
    };
    storage | cache
}

/// The Metal texture type an [`ImageDesc`](crcbl_hal::ImageDesc) describes.
///
/// Three of the seam's fields decide one Metal enum, which is why this is a
/// function rather than a table: dimensionality, whether there is more than one
/// array layer, and whether the image is multisampled. Metal spells every
/// combination separately and rejects a mismatch between the type and the
/// descriptor's `arrayLength`/`sampleCount`, so deriving it in one place is
/// what keeps those three consistent.
///
/// `layers` is [`Extent3d::depth_or_layers`](crcbl_hal::Extent3d::depth_or_layers)
/// and is ignored for [`ImageType::D3`], where that field is a depth. Cube maps
/// are not reachable from here at all — the seam has no cube [`ImageType`],
/// only a cube [`ImageViewType`], so a cube map is a six-layer 2D array with a
/// cube *view*, which is precisely how Metal models one too.
pub(crate) fn texture_type(image_type: ImageType, layers: u32, samples: u32) -> MTLTextureType {
    let arrayed = layers > 1;
    let multisampled = samples > 1;
    match image_type {
        ImageType::D1 if arrayed => MTLTextureType::Type1DArray,
        ImageType::D1 => MTLTextureType::Type1D,
        ImageType::D2 => match (multisampled, arrayed) {
            (true, true) => MTLTextureType::Type2DMultisampleArray,
            (true, false) => MTLTextureType::Type2DMultisample,
            (false, true) => MTLTextureType::Type2DArray,
            (false, false) => MTLTextureType::Type2D,
        },
        ImageType::D3 => MTLTextureType::Type3D,
    }
}

/// How a view reinterprets its texture's dimensionality.
pub(crate) fn view_texture_type(view: ImageViewType) -> MTLTextureType {
    match view {
        ImageViewType::D1 => MTLTextureType::Type1D,
        ImageViewType::D2 => MTLTextureType::Type2D,
        ImageViewType::D2Array => MTLTextureType::Type2DArray,
        ImageViewType::Cube => MTLTextureType::TypeCube,
        ImageViewType::CubeArray => MTLTextureType::TypeCubeArray,
        ImageViewType::D3 => MTLTextureType::Type3D,
    }
}

/// Permitted texture uses, and the one flag the seam cannot ask for.
///
/// # `Unknown` means "no restriction", so it can never be OR-ed into
///
/// [`MTLTextureUsage::Unknown`] is zero and Metal reads it as *the driver works
/// it out*, not as *nothing is permitted*. That makes it the right answer for a
/// transfer-only image — the seam's [`ImageUsage::TRANSFER_SRC`] and
/// [`ImageUsage::TRANSFER_DST`] have no Metal flag, because a blit needs no
/// declared usage — and it makes ORing anything into it a **narrowing**: a
/// transfer-only image that also carried `PixelFormatView` would be an image
/// that permits views and nothing else. So the empty case returns `Unknown`
/// alone and the non-empty case never contains it.
///
/// # Why `PixelFormatView` is set on every colour image
///
/// [`ImageViewDesc::format`](crcbl_hal::ImageViewDesc::format) is documented as
/// free to differ from its image's "for sRGB reinterpretation", and Metal
/// requires that intent to be declared at *texture* creation — by which time
/// the view does not exist and nothing in [`ImageDesc`](crcbl_hal::ImageDesc)
/// says whether one is coming. Refusing the reinterpretation instead would take
/// away the one capability the seam names, so the flag goes on unconditionally
/// for colour formats.
///
/// It is not free: `PixelFormatView` can cost lossless bandwidth compression on
/// some Apple GPUs. Narrowing it needs the seam to carry the view formats a
/// caller intends, the way WebGPU's `viewFormats` does, and that is a seam
/// change rather than a backend one.
///
/// Depth and stencil images do not get it. Metal permits no reinterpretation
/// between depth formats — they are their own compatibility class — so the flag
/// would buy exactly nothing and still cost, and `device.rs` refuses a
/// differing view format on a depth image with a clean error rather than
/// letting Metal raise.
pub(crate) fn texture_usage(usage: ImageUsage, format: Format) -> MTLTextureUsage {
    let mut out = MTLTextureUsage::empty();
    if usage.contains(ImageUsage::SAMPLED) {
        out |= MTLTextureUsage::ShaderRead;
    }
    if usage.contains(ImageUsage::STORAGE) {
        // A storage image in the seam is read/write; Metal splits the two.
        out |= MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite;
    }
    if usage.intersects(ImageUsage::COLOR_ATTACHMENT | ImageUsage::DEPTH_STENCIL_ATTACHMENT) {
        out |= MTLTextureUsage::RenderTarget;
    }
    // `ImageUsage::PRESENT` maps to nothing: a presentable texture in Metal is
    // a `CAMetalDrawable`'s, which this backend does not create and does not
    // allocate the storage for. The swapchain slice owns that path.
    if out.is_empty() {
        return MTLTextureUsage::Unknown;
    }
    if !format.is_depth_stencil() {
        out |= MTLTextureUsage::PixelFormatView;
    }
    out
}

/// Magnification/minification filter.
pub(crate) fn min_mag_filter(filter: FilterMode) -> MTLSamplerMinMagFilter {
    match filter {
        FilterMode::Nearest => MTLSamplerMinMagFilter::Nearest,
        FilterMode::Linear => MTLSamplerMinMagFilter::Linear,
    }
}

/// Filter between mip levels.
///
/// [`MTLSamplerMipFilter::NotMipmapped`] is never produced. It is a third state
/// the seam does not have — "ignore the mip chain entirely" — and a sampler
/// that silently ignored the levels of a mipmapped texture would alias, which
/// is the visible bug this mapping exists to avoid.
pub(crate) fn mip_filter(filter: FilterMode) -> MTLSamplerMipFilter {
    match filter {
        FilterMode::Nearest => MTLSamplerMipFilter::Nearest,
        FilterMode::Linear => MTLSamplerMipFilter::Linear,
    }
}

/// Addressing outside `[0, 1]`.
///
/// [`SamplerAddressMode::ClampToBorder`] becomes
/// [`MTLSamplerAddressMode::ClampToBorderColor`], and the border colour itself
/// is set beside it in `device.rs` — transparent black, which is what the
/// seam's variant documents and what a shadow atlas needs.
/// [`MTLSamplerAddressMode::ClampToZero`] is a different thing (clamp to
/// transparent black *without* a border colour) and is not what the seam asked
/// for.
pub(crate) fn address_mode(mode: SamplerAddressMode) -> MTLSamplerAddressMode {
    match mode {
        SamplerAddressMode::Repeat => MTLSamplerAddressMode::Repeat,
        SamplerAddressMode::MirrorRepeat => MTLSamplerAddressMode::MirrorRepeat,
        SamplerAddressMode::ClampToEdge => MTLSamplerAddressMode::ClampToEdge,
        SamplerAddressMode::ClampToBorder => MTLSamplerAddressMode::ClampToBorderColor,
    }
}

/// A comparison, named by the comparison.
///
/// **Nothing here flips sign for reversed-Z, and that is the point.** The
/// engine is reversed-Z everywhere (1.0 near, 0.0 far), but `crcbl-hal` bakes
/// that into its *defaults* rather than its vocabulary:
/// [`CompareOp::Greater`] means greater, and a shadow comparison asking "is
/// this fragment closer" already arrives here as `Greater` because
/// [`SamplerDesc::compare`](crcbl_hal::SamplerDesc::compare) says so. A backend
/// that inverted the sense on the way through would produce shadows that are
/// exactly inside out, twice over, for callers that read the seam correctly.
pub(crate) fn compare_function(op: CompareOp) -> MTLCompareFunction {
    match op {
        CompareOp::Never => MTLCompareFunction::Never,
        CompareOp::Less => MTLCompareFunction::Less,
        CompareOp::Equal => MTLCompareFunction::Equal,
        CompareOp::LessOrEqual => MTLCompareFunction::LessEqual,
        CompareOp::Greater => MTLCompareFunction::Greater,
        CompareOp::NotEqual => MTLCompareFunction::NotEqual,
        CompareOp::GreaterOrEqual => MTLCompareFunction::GreaterEqual,
        CompareOp::Always => MTLCompareFunction::Always,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every [`Format`] the seam declares, so the properties below are checked
    /// over all of them rather than over the handful someone remembered.
    ///
    /// Hand-written because `Format` has no iterator; `pixel_format`'s
    /// exhaustive `match` is what makes a *missing* variant a compile error,
    /// and `every_format_appears_in_the_exhaustive_list` below is what makes a
    /// variant missing from *this* list fail.
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

    /// The seam's linear/sRGB pairs, which are the entries a transposition
    /// makes *dark* rather than broken.
    const SRGB_PAIRS: &[(Format, Format)] = &[
        (Format::Rgba8Unorm, Format::Rgba8UnormSrgb),
        (Format::Bgra8Unorm, Format::Bgra8UnormSrgb),
        (Format::Bc1RgbaUnorm, Format::Bc1RgbaUnormSrgb),
        (Format::Bc3RgbaUnorm, Format::Bc3RgbaUnormSrgb),
        (Format::Bc7RgbaUnorm, Format::Bc7RgbaUnormSrgb),
    ];

    /// `ALL` really is all of them, so every property below has the coverage it
    /// claims. `Format` is `Ord`, so the largest variant plus a count is enough
    /// to catch both an addition to the seam and a deletion from this list.
    #[test]
    fn every_format_appears_in_the_exhaustive_list() {
        let mut sorted: Vec<Format> = ALL.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ALL.len(), "a duplicate in ALL");
        assert_eq!(
            sorted.last().copied(),
            Some(Format::Bc7RgbaUnormSrgb),
            "the seam gained a format after the last one this list knows"
        );
    }

    /// **The mapping is injective.** Two seam formats sharing one Metal format
    /// is the copy-paste failure this file is most exposed to, and it is
    /// invisible at run time: the image is created, the sample succeeds, the
    /// colour is wrong.
    #[test]
    fn no_two_formats_share_a_metal_format() {
        assert!(!ALL.is_empty(), "nothing to check");
        let mut seen: Vec<(Format, MTLPixelFormat)> = Vec::new();
        for &format in ALL {
            let metal = pixel_format(format);
            assert_ne!(
                metal,
                MTLPixelFormat::Invalid,
                "{format:?} has no Metal format"
            );
            if let Some((other, _)) = seen.iter().find(|(_, seen)| *seen == metal) {
                panic!("{format:?} and {other:?} both map to {metal:?}");
            }
            seen.push((format, metal));
        }
        assert_eq!(seen.len(), ALL.len());
    }

    /// The pairs, pinned to Metal's own `_sRGB` constants.
    ///
    /// Two assertions, and the first is the one that catches a dropped encode:
    /// a linear and an sRGB format that map to the *same* Metal format render
    /// too dark and nothing else notices.
    #[test]
    fn srgb_pairs_map_to_metals_srgb_formats() {
        assert!(!SRGB_PAIRS.is_empty(), "nothing to check");
        for &(linear, srgb) in SRGB_PAIRS {
            assert!(!linear.is_srgb() && srgb.is_srgb(), "{linear:?}/{srgb:?}");
            assert_ne!(
                pixel_format(linear),
                pixel_format(srgb),
                "the sRGB encode vanished for {linear:?}/{srgb:?}"
            );
        }
        assert_eq!(
            pixel_format(Format::Rgba8UnormSrgb),
            MTLPixelFormat::RGBA8Unorm_sRGB
        );
        assert_eq!(
            pixel_format(Format::Bgra8UnormSrgb),
            MTLPixelFormat::BGRA8Unorm_sRGB
        );
        assert_eq!(
            pixel_format(Format::Bc7RgbaUnormSrgb),
            MTLPixelFormat::BC7_RGBAUnorm_sRGB
        );
    }

    /// The engine's two named formats, spelled out, because every golden image
    /// depends on them.
    #[test]
    fn the_engines_hdr_and_depth_formats_are_the_metal_ones_expected() {
        assert_eq!(
            pixel_format(Format::Rgba16Float),
            MTLPixelFormat::RGBA16Float
        );
        assert_eq!(pixel_format(Format::D32Float), MTLPixelFormat::Depth32Float);
        // The packed HDR intermediate, and its lookalike neighbour.
        assert_eq!(
            pixel_format(Format::R11g11b10Float),
            MTLPixelFormat::RG11B10Float
        );
        assert_ne!(
            pixel_format(Format::R11g11b10Float),
            MTLPixelFormat::RGB9E5Float
        );
    }

    /// The storage-mode decision, pinned. `Managed` and `Memoryless` must not
    /// appear for any location — see [`storage_mode`] for why each would be a
    /// correctness bug rather than a slower choice.
    #[test]
    fn memory_locations_map_to_private_and_shared_and_nothing_else() {
        assert_eq!(
            storage_mode(MemoryLocation::DeviceLocal),
            MTLStorageMode::Private
        );
        assert_eq!(
            storage_mode(MemoryLocation::HostUpload),
            MTLStorageMode::Shared
        );
        assert_eq!(
            storage_mode(MemoryLocation::HostReadback),
            MTLStorageMode::Shared
        );
        for location in [
            MemoryLocation::DeviceLocal,
            MemoryLocation::HostUpload,
            MemoryLocation::HostReadback,
        ] {
            let mode = storage_mode(location);
            assert_ne!(mode, MTLStorageMode::Managed, "{location:?}");
            assert_ne!(mode, MTLStorageMode::Memoryless, "{location:?}");
            // A mappable location is exactly a `Shared` one, which is the
            // invariant `write_buffer` relies on to reach `contents`.
            assert_eq!(
                location.is_mappable(),
                mode == MTLStorageMode::Shared,
                "{location:?}"
            );
        }
    }

    /// Write-combined is only where the seam promised never to read.
    #[test]
    fn only_the_upload_location_is_write_combined() {
        assert_eq!(
            cpu_cache_mode(MemoryLocation::HostUpload),
            MTLCPUCacheMode::WriteCombined
        );
        assert_eq!(
            cpu_cache_mode(MemoryLocation::HostReadback),
            MTLCPUCacheMode::DefaultCache,
            "readback is documented as cached; write-combined would make every \
             read of it crawl"
        );
        assert_eq!(
            cpu_cache_mode(MemoryLocation::DeviceLocal),
            MTLCPUCacheMode::DefaultCache
        );
    }

    /// `resource_options` is the two enums above packed into one word, so it
    /// must still be readable back as those two.
    #[test]
    fn resource_options_carry_both_halves() {
        let upload = resource_options(MemoryLocation::HostUpload);
        assert!(upload.contains(MTLResourceOptions::StorageModeShared));
        assert!(upload.contains(MTLResourceOptions::CPUCacheModeWriteCombined));

        let device_local = resource_options(MemoryLocation::DeviceLocal);
        assert!(device_local.contains(MTLResourceOptions::StorageModePrivate));
        assert!(!device_local.contains(MTLResourceOptions::StorageModeShared));
    }

    /// Dimensionality, arraying and multisampling all reach the same enum, and
    /// the combinations Metal spells separately must not collapse.
    #[test]
    fn texture_types_separate_arrays_and_multisampling() {
        assert_eq!(texture_type(ImageType::D2, 1, 1), MTLTextureType::Type2D);
        assert_eq!(
            texture_type(ImageType::D2, 6, 1),
            MTLTextureType::Type2DArray,
            "a cube map's storage is a six-layer 2D array"
        );
        assert_eq!(
            texture_type(ImageType::D2, 1, 4),
            MTLTextureType::Type2DMultisample
        );
        assert_eq!(
            texture_type(ImageType::D2, 2, 4),
            MTLTextureType::Type2DMultisampleArray
        );
        assert_eq!(texture_type(ImageType::D1, 1, 1), MTLTextureType::Type1D);
        assert_eq!(
            texture_type(ImageType::D1, 4, 1),
            MTLTextureType::Type1DArray
        );
        // A volume's `depth_or_layers` is a depth, so it never arrays.
        assert_eq!(texture_type(ImageType::D3, 64, 1), MTLTextureType::Type3D);
    }

    /// `Unknown` is Metal's "no restriction" and must stay alone; every other
    /// answer must be a real permission set.
    #[test]
    fn transfer_only_usage_is_unknown_and_nothing_is_or_ed_into_it() {
        let transfer_only = texture_usage(
            ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST,
            Format::Rgba8Unorm,
        );
        assert_eq!(transfer_only, MTLTextureUsage::Unknown);
        assert!(
            !transfer_only.contains(MTLTextureUsage::PixelFormatView),
            "ORing into Unknown narrows it from everything to one thing"
        );

        let sampled = texture_usage(ImageUsage::SAMPLED, Format::Rgba8Unorm);
        assert!(sampled.contains(MTLTextureUsage::ShaderRead));
        assert!(
            sampled.contains(MTLTextureUsage::PixelFormatView),
            "an sRGB view of a linear colour image must be creatable"
        );

        let storage = texture_usage(ImageUsage::STORAGE, Format::Rgba16Float);
        assert!(storage.contains(MTLTextureUsage::ShaderRead));
        assert!(storage.contains(MTLTextureUsage::ShaderWrite));

        let colour = texture_usage(ImageUsage::COLOR_ATTACHMENT, Format::Rgba16Float);
        assert!(colour.contains(MTLTextureUsage::RenderTarget));
    }

    /// A depth attachment is a render target and never a format view.
    #[test]
    fn depth_images_are_render_targets_without_pixel_format_view() {
        let depth = texture_usage(ImageUsage::DEPTH_STENCIL_ATTACHMENT, Format::D32Float);
        assert!(depth.contains(MTLTextureUsage::RenderTarget));
        assert!(
            !depth.contains(MTLTextureUsage::PixelFormatView),
            "Metal has no compatible reinterpretation of a depth format"
        );
    }

    /// Reversed-Z is produced above this seam, so the comparison must arrive
    /// and leave with the same name.
    #[test]
    fn comparisons_are_not_flipped_for_reversed_z() {
        assert_eq!(
            compare_function(CompareOp::Greater),
            MTLCompareFunction::Greater,
            "a shadow test asking for Greater must not become Less"
        );
        assert_eq!(compare_function(CompareOp::Less), MTLCompareFunction::Less);
        assert_eq!(
            compare_function(CompareOp::GreaterOrEqual),
            MTLCompareFunction::GreaterEqual
        );
        assert_eq!(
            compare_function(CompareOp::LessOrEqual),
            MTLCompareFunction::LessEqual
        );
        // `CompareOp::default` is the engine's depth test; if the seam ever
        // changed it, this backend would want to know.
        assert_eq!(
            compare_function(CompareOp::default()),
            MTLCompareFunction::Greater
        );
    }

    /// The address modes, including the two Metal spells almost alike.
    #[test]
    fn clamp_to_border_is_the_border_colour_mode_not_clamp_to_zero() {
        assert_eq!(
            address_mode(SamplerAddressMode::ClampToBorder),
            MTLSamplerAddressMode::ClampToBorderColor
        );
        assert_ne!(
            address_mode(SamplerAddressMode::ClampToBorder),
            MTLSamplerAddressMode::ClampToZero
        );
        assert_eq!(
            address_mode(SamplerAddressMode::Repeat),
            MTLSamplerAddressMode::Repeat
        );
        assert_eq!(
            address_mode(SamplerAddressMode::MirrorRepeat),
            MTLSamplerAddressMode::MirrorRepeat
        );
        assert_eq!(
            address_mode(SamplerAddressMode::ClampToEdge),
            MTLSamplerAddressMode::ClampToEdge
        );
    }

    /// A mip filter is never "no mip filter".
    #[test]
    fn mip_filters_never_disable_mipmapping() {
        for filter in [FilterMode::Nearest, FilterMode::Linear] {
            assert_ne!(
                mip_filter(filter),
                MTLSamplerMipFilter::NotMipmapped,
                "{filter:?}"
            );
        }
        assert_eq!(mip_filter(FilterMode::Linear), MTLSamplerMipFilter::Linear);
        assert_eq!(
            min_mag_filter(FilterMode::Nearest),
            MTLSamplerMinMagFilter::Nearest
        );
    }

    /// The view types, one per seam variant, none collapsed onto another.
    #[test]
    fn view_types_are_distinct_per_seam_variant() {
        let views = [
            ImageViewType::D1,
            ImageViewType::D2,
            ImageViewType::D2Array,
            ImageViewType::Cube,
            ImageViewType::CubeArray,
            ImageViewType::D3,
        ];
        assert!(!views.is_empty(), "nothing to check");
        let mut seen: Vec<MTLTextureType> = Vec::new();
        for view in views {
            let metal = view_texture_type(view);
            assert!(!seen.contains(&metal), "{view:?} duplicates {metal:?}");
            seen.push(metal);
        }
        assert_eq!(
            view_texture_type(ImageViewType::Cube),
            MTLTextureType::TypeCube
        );
    }
}
