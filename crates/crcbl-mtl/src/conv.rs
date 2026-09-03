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
    BlendFactor, BlendOp, BufferImageCopy, ColorWrites, CompareOp, CullMode, Extent3d, FilterMode,
    Format, FrontFace, ImageAspect, ImageType, ImageUsage, ImageViewType, LoadOp, MemoryLocation,
    Offset3d, PolygonMode, PrimitiveTopology, QueryKind, SamplerAddressMode, StencilOp, StoreOp,
};
use objc2_foundation::NSUInteger;
use objc2_metal::{
    MTLBlendFactor, MTLBlendOperation, MTLCPUCacheMode, MTLClearColor, MTLColorWriteMask,
    MTLCompareFunction, MTLCounterErrorValue, MTLCounterResultStatistic, MTLCounterResultTimestamp,
    MTLCullMode, MTLDepthClipMode, MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType,
    MTLResourceOptions, MTLSamplerAddressMode, MTLSamplerMinMagFilter, MTLSamplerMipFilter,
    MTLSize, MTLStencilOperation, MTLStorageMode, MTLStoreAction, MTLTextureType, MTLTextureUsage,
    MTLTriangleFillMode, MTLWinding,
};

/// [`crate::query`]'s query-result literals against the structures and the
/// sentinel Metal actually resolves.
///
/// **The one thing `crate::query` cannot check itself.** That module is compiled
/// on hosts with no Metal so its arithmetic can be tested at all, which means its
/// widths and its error value are literals — and a literal that drifted from the
/// ABI would size every resolve destination wrongly, or turn an unsampled query
/// into a plausible-looking timing, while every test over it still passed. This
/// is where the two meet, and it is a `const` block, so an `objc2-metal` upgrade
/// that changed any of the three fails the build rather than a run.
/// `crcbl-dx12`'s `crate::conv` carries the same block for the same reason.
const _: () = {
    assert!(
        crate::query::result_bytes(QueryKind::Occlusion) == size_of::<u64>() as u64,
        "an occlusion query counts into one uint64_t of a visibility-result buffer"
    );
    assert!(
        crate::query::result_bytes(QueryKind::Timestamp)
            == size_of::<MTLCounterResultTimestamp>() as u64,
        "a timestamp query resolves as a whole MTLCounterResultTimestamp"
    );
    assert!(
        crate::query::result_bytes(QueryKind::PipelineStatistics)
            == size_of::<MTLCounterResultStatistic>() as u64,
        "a statistics query resolves as a whole MTLCounterResultStatistic"
    );
    assert!(
        crate::query::COUNTER_ERROR == MTLCounterErrorValue,
        "a query nothing sampled resolves to MTLCounterErrorValue"
    );
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
///    does not make.** Metal's own header states the contract: a CPU write to a
///    managed resource must be followed by `didModifyRange:`, and the CPU
///    cannot see GPU writes until a `MTLBlitCommandEncoder`
///    `synchronizeResource:` has completed. Neither call appears anywhere in
///    this crate, and neither needs to while `Managed` is unreachable.
///    Picking `Managed` for [`MemoryLocation::HostReadback`] would return
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

/// How a view reinterprets its texture's dimensionality, or `None` when Metal
/// has no type compatible with the texture the view is of.
///
/// # The sample count comes from the texture, not from the seam
///
/// [`ImageViewType`] has one 2D spelling and no multisampled one, because
/// Vulkan's `VkImageViewType` has none either — a Vulkan view inherits its
/// image's sample count. Metal spells the multisampled types separately and
/// only makes a view of a multisampled texture as one of them, so a `D2` view
/// alone does not say which Metal type is wanted. `source` is the texture's own
/// `textureType`, which [`texture_type`] chose from the image descriptor at
/// creation, and it is what decides which half of the table applies. Passing a
/// `Type2D` here for a texture that is `Type2DMultisample` asks Metal for a view
/// it will not make.
///
/// `None` is a pairing that has no Metal answer at all: a 1D, 3D or cube view of
/// a multisampled texture. Reporting it as the descriptor error it is beats
/// handing it to Metal, whose validation layer answers an incompatible view type
/// by aborting the process rather than by returning nil.
pub(crate) fn view_texture_type(
    view: ImageViewType,
    source: MTLTextureType,
) -> Option<MTLTextureType> {
    let multisampled = source == MTLTextureType::Type2DMultisample
        || source == MTLTextureType::Type2DMultisampleArray;
    Some(match (view, multisampled) {
        (ImageViewType::D2, true) => MTLTextureType::Type2DMultisample,
        (ImageViewType::D2Array, true) => MTLTextureType::Type2DMultisampleArray,
        (
            ImageViewType::D1 | ImageViewType::Cube | ImageViewType::CubeArray | ImageViewType::D3,
            true,
        ) => return None,
        (ImageViewType::D1, false) => MTLTextureType::Type1D,
        (ImageViewType::D2, false) => MTLTextureType::Type2D,
        (ImageViewType::D2Array, false) => MTLTextureType::Type2DArray,
        (ImageViewType::Cube, false) => MTLTextureType::TypeCube,
        (ImageViewType::CubeArray, false) => MTLTextureType::TypeCubeArray,
        (ImageViewType::D3, false) => MTLTextureType::Type3D,
    })
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
/// # Why `PixelFormatView` is set on no image at all
///
/// It used to go on every colour image, and the reason was
/// [`ImageViewDesc::format`](crcbl_hal::ImageViewDesc::format): that field was
/// documented as free to differ from its image's "for sRGB reinterpretation",
/// Metal requires the intent to be declared at *texture* creation — by which
/// time the view does not exist and nothing in
/// [`ImageDesc`](crcbl_hal::ImageDesc) says whether one is coming — so the flag
/// went on unconditionally rather than take the capability away.
///
/// **On 2026-09-03 the seam took it away**, because no backend delivered it as
/// they create images and D3D12 structurally cannot without a typeless
/// resource. A view's format must now equal its image's, `ImageViewDesc::check`
/// refuses anything else before a backend sees it, and this backend's
/// `create_image_view` refuses it again. So the flag now buys a permission
/// nothing can reach, and it is not free: `PixelFormatView` can cost lossless
/// bandwidth compression on some Apple GPUs. It is off.
///
/// Turning it back on is the second half of offering reinterpretation for real,
/// whose first half is the seam carrying the view formats a caller intends, the
/// way WebGPU's `viewFormats` does. `docs/backlog.md` carries why that was
/// declined. Depth and stencil never had it: Metal permits no reinterpretation
/// between depth formats, they are their own compatibility class, so it would
/// have bought nothing there even while the seam named the capability.
pub(crate) fn texture_usage(usage: ImageUsage) -> MTLTextureUsage {
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

// --- pipelines -------------------------------------------------------------

/// How vertices assemble into primitives.
///
/// Metal takes this at the **draw call** rather than in the pipeline state —
/// `MTLRenderPipelineDescriptor::inputPrimitiveTopology` is a coarser
/// *topology class*, needed only for layered rendering and tessellation — so a
/// graphics pipeline entry carries this value and
/// [`CommandEncoder::draw`](crcbl_hal::CommandEncoder::draw) passes it to
/// `drawPrimitives:`. That is why the seam's per-pipeline topology survives a
/// backend whose draws are per-call.
///
/// The seam has no triangle *fan* and Metal has no line loop, so the five
/// variants line up one for one with nothing left over on either side.
pub(crate) const fn primitive_type(topology: PrimitiveTopology) -> MTLPrimitiveType {
    match topology {
        PrimitiveTopology::PointList => MTLPrimitiveType::Point,
        PrimitiveTopology::LineList => MTLPrimitiveType::Line,
        PrimitiveTopology::LineStrip => MTLPrimitiveType::LineStrip,
        PrimitiveTopology::TriangleList => MTLPrimitiveType::Triangle,
        PrimitiveTopology::TriangleStrip => MTLPrimitiveType::TriangleStrip,
    }
}

/// Which faces are discarded.
pub(crate) const fn cull_mode(mode: CullMode) -> MTLCullMode {
    match mode {
        CullMode::None => MTLCullMode::None,
        CullMode::Front => MTLCullMode::Front,
        CullMode::Back => MTLCullMode::Back,
    }
}

/// Which winding is front-facing.
///
/// **No flip.** `crcbl-vk` passes a negative viewport height to put Vulkan's
/// inverted Y the right way up, which reverses the winding a triangle appears
/// to have and is why a Vulkan backend has to think about this. Metal's
/// framebuffer origin is already top-left with Y increasing downwards — the
/// convention the seam is written in, and the reason `set_viewport` performs no
/// flip either — so `Ccw` here means counter-clockwise on screen exactly as the
/// seam says.
pub(crate) const fn winding(face: FrontFace) -> MTLWinding {
    match face {
        FrontFace::Ccw => MTLWinding::CounterClockwise,
        FrontFace::Cw => MTLWinding::Clockwise,
    }
}

/// Solid or wireframe.
pub(crate) const fn fill_mode(mode: PolygonMode) -> MTLTriangleFillMode {
    match mode {
        PolygonMode::Fill => MTLTriangleFillMode::Fill,
        PolygonMode::Line => MTLTriangleFillMode::Lines,
    }
}

/// Whether fragments outside the depth range are clamped or clipped.
pub(crate) const fn depth_clip_mode(clamp: bool) -> MTLDepthClipMode {
    if clamp {
        MTLDepthClipMode::Clamp
    } else {
        MTLDepthClipMode::Clip
    }
}

/// A blend factor.
///
/// The seam's ten factors are the subset every target has; Metal's `Source1*`
/// dual-source family and its `BlendColor` constants have no seam spelling and
/// are therefore unreachable rather than approximated.
pub(crate) const fn blend_factor(factor: BlendFactor) -> MTLBlendFactor {
    match factor {
        BlendFactor::Zero => MTLBlendFactor::Zero,
        BlendFactor::One => MTLBlendFactor::One,
        BlendFactor::Src => MTLBlendFactor::SourceColor,
        BlendFactor::OneMinusSrc => MTLBlendFactor::OneMinusSourceColor,
        BlendFactor::SrcAlpha => MTLBlendFactor::SourceAlpha,
        BlendFactor::OneMinusSrcAlpha => MTLBlendFactor::OneMinusSourceAlpha,
        BlendFactor::Dst => MTLBlendFactor::DestinationColor,
        BlendFactor::OneMinusDst => MTLBlendFactor::OneMinusDestinationColor,
        BlendFactor::DstAlpha => MTLBlendFactor::DestinationAlpha,
        BlendFactor::OneMinusDstAlpha => MTLBlendFactor::OneMinusDestinationAlpha,
    }
}

/// How blended terms combine.
pub(crate) const fn blend_operation(op: BlendOp) -> MTLBlendOperation {
    match op {
        BlendOp::Add => MTLBlendOperation::Add,
        BlendOp::Subtract => MTLBlendOperation::Subtract,
        BlendOp::ReverseSubtract => MTLBlendOperation::ReverseSubtract,
        BlendOp::Min => MTLBlendOperation::Min,
        BlendOp::Max => MTLBlendOperation::Max,
    }
}

/// Which channels a colour target writes.
///
/// **The two bit orders are opposite, and this is the one mapping in this file
/// a bit-cast would silently corrupt.** The seam numbers its channels
/// `R = 1 << 0 … A = 1 << 3`; Metal numbers them the other way round —
/// `MTLColorWriteMaskAlpha` is `1 << 0` and `MTLColorWriteMaskRed` is `1 << 3`.
/// So the two masks *look* interchangeable (both are four low bits, both spell
/// "all" as `0xF`) and are exact mirrors of each other. Writing only red would
/// write only alpha. Hence the flag-by-flag rebuild below and
/// `color_write_masks_are_not_bit_compatible` beside it.
pub(crate) fn color_write_mask(writes: ColorWrites) -> MTLColorWriteMask {
    let mut out = MTLColorWriteMask::None;
    for (seam, metal) in [
        (ColorWrites::R, MTLColorWriteMask::Red),
        (ColorWrites::G, MTLColorWriteMask::Green),
        (ColorWrites::B, MTLColorWriteMask::Blue),
        (ColorWrites::A, MTLColorWriteMask::Alpha),
    ] {
        if writes.contains(seam) {
            out |= metal;
        }
    }
    out
}

/// What a stencil test outcome does to the stencil buffer.
pub(crate) const fn stencil_operation(op: StencilOp) -> MTLStencilOperation {
    match op {
        StencilOp::Keep => MTLStencilOperation::Keep,
        StencilOp::Zero => MTLStencilOperation::Zero,
        StencilOp::Replace => MTLStencilOperation::Replace,
        StencilOp::Invert => MTLStencilOperation::Invert,
        StencilOp::IncrementClamp => MTLStencilOperation::IncrementClamp,
        StencilOp::DecrementClamp => MTLStencilOperation::DecrementClamp,
        StencilOp::IncrementWrap => MTLStencilOperation::IncrementWrap,
        StencilOp::DecrementWrap => MTLStencilOperation::DecrementWrap,
    }
}

// --- render passes ---------------------------------------------------------

/// What a pass does with an attachment's existing contents.
///
/// The three seam ops and Metal's three actions line up one for one, and the
/// pairing is worth spelling out because a transposition here is invisible
/// until someone reads a frame back: [`LoadOp::Load`] as
/// [`MTLLoadAction::Clear`] wipes a depth prepass, and [`LoadOp::Clear`] as
/// [`MTLLoadAction::Load`] leaves whatever the last frame put in the tile.
/// `load_and_store_actions_are_not_transposed` is the assertion that pins it.
pub(crate) const fn load_action(load: LoadOp) -> MTLLoadAction {
    match load {
        LoadOp::Load => MTLLoadAction::Load,
        LoadOp::Clear => MTLLoadAction::Clear,
        LoadOp::DontCare => MTLLoadAction::DontCare,
    }
}

/// What a pass does with an attachment's contents at the end.
///
/// [`MTLStoreAction::Unknown`] is never produced: it is Metal's "the action
/// will be set before the encoder ends" placeholder, and an encoder that ends
/// while an attachment still carries it raises rather than storing.
pub(crate) const fn store_action(store: StoreOp) -> MTLStoreAction {
    match store {
        StoreOp::Store => MTLStoreAction::Store,
        StoreOp::Discard => MTLStoreAction::DontCare,
    }
}

/// The same, for a colour attachment that also resolves into a second texture.
///
/// Metal folds "resolve" into the *store* action rather than carrying it
/// separately the way `VkRenderingAttachmentInfo::resolveMode` does, so a
/// resolving attachment has two store actions rather than one and the seam's
/// [`StoreOp`] picks between them: [`StoreOp::Store`] keeps the multisampled
/// texture as well, [`StoreOp::Discard`] keeps only the resolved one — which is
/// the whole point of resolving an MSAA target nothing reads afterwards.
pub(crate) const fn resolve_store_action(store: StoreOp) -> MTLStoreAction {
    match store {
        StoreOp::Store => MTLStoreAction::StoreAndMultisampleResolve,
        StoreOp::Discard => MTLStoreAction::MultisampleResolve,
    }
}

/// A clear colour, widened to the doubles Metal takes.
///
/// **Nothing is encoded or decoded here.** The seam documents
/// [`ClearValue::color`](crcbl_hal::ClearValue::color) as being "in the
/// attachment's own space", and Metal says the same of `MTLClearColor` — the
/// hardware applies the attachment format's own transfer function, so a clear
/// of `0.5` into an `_sRGB` attachment does **not** land as `128`. A backend
/// that "helpfully" encoded here would apply it twice.
pub(crate) const fn clear_color(color: [f32; 4]) -> MTLClearColor {
    MTLClearColor {
        red: color[0] as f64,
        green: color[1] as f64,
        blue: color[2] as f64,
        alpha: color[3] as f64,
    }
}

// --- copies ----------------------------------------------------------------

/// A copy's texel offset, or `None` if it is negative.
///
/// The seam's [`Offset3d`] is signed because Vulkan's `VkOffset3D` is; Metal's
/// `MTLOrigin` is unsigned, and a negative offset has no meaning for a copy in
/// either API. Returning `None` is what lets the caller report it as the
/// descriptor error it is instead of wrapping it into a colossal `NSUInteger`
/// and letting Metal raise.
pub(crate) fn origin(offset: Offset3d) -> Option<MTLOrigin> {
    let x = NSUInteger::try_from(offset.x).ok()?;
    let y = NSUInteger::try_from(offset.y).ok()?;
    let z = NSUInteger::try_from(offset.z).ok()?;
    Some(MTLOrigin { x, y, z })
}

/// A copy region's size in texels.
///
/// `is_3d` decides what [`Extent3d::depth_or_layers`] means, exactly as it does
/// at image creation: a volume's third component is a depth Metal wants in
/// `MTLSize::depth`, while an array's is a layer count that Metal expresses as
/// a *slice* on the copy call instead — so a 2D copy is always one slice deep.
pub(crate) const fn copy_size(extent: Extent3d, is_3d: bool) -> MTLSize {
    MTLSize {
        width: extent.width as NSUInteger,
        height: extent.height as NSUInteger,
        depth: if is_3d {
            extent.depth_or_layers as NSUInteger
        } else {
            1
        },
    }
}

/// How a [`BufferImageCopy`]'s buffer side is laid out, in the bytes Metal
/// counts rather than the texels the seam counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CopyFootprint {
    /// `sourceBytesPerRow` / `destinationBytesPerRow`.
    pub(crate) bytes_per_row: u64,
    /// `sourceBytesPerImage` / `destinationBytesPerImage` — the stride from one
    /// array slice to the next.
    pub(crate) bytes_per_image: u64,
}

/// Translates a copy's buffer layout from the seam's units into Metal's.
///
/// Two conversions happen here and each is a place a backend gets copies
/// silently wrong:
///
/// * **Texels to bytes.** The seam counts
///   [`buffer_row_length`](BufferImageCopy::buffer_row_length) in *texels* and
///   [`buffer_image_height`](BufferImageCopy::buffer_image_height) in *rows*,
///   with `0` meaning tightly packed; Metal counts both in bytes. A compressed
///   format makes the two differ by a factor of sixteen rather than a constant,
///   which is why the block extent is divided out rather than assumed to be 1.
/// * **The aspect decides the element size.** A depth-aspect copy of a
///   `D32FloatS8Uint` image moves four bytes per texel while the format's own
///   element is eight — [`Format::texel_size`] is the seam's answer to exactly
///   that, and `block_size` would be the wrong number.
///
/// `None` when the aspect names no single plane of this format, which is a
/// caller error the copy cannot describe.
pub(crate) fn copy_footprint(
    format: Format,
    aspect: ImageAspect,
    copy: &BufferImageCopy,
) -> Option<CopyFootprint> {
    let texel = u64::from(format.texel_size(aspect)?);
    let (block_width, block_height) = format.block_extent();
    let row_texels = if copy.buffer_row_length == 0 {
        copy.image_extent.width
    } else {
        copy.buffer_row_length
    };
    let rows = if copy.buffer_image_height == 0 {
        copy.image_extent.height
    } else {
        copy.buffer_image_height
    };
    let bytes_per_row = u64::from(row_texels.div_ceil(block_width)) * texel;
    let bytes_per_image = bytes_per_row * u64::from(rows.div_ceil(block_height));
    Some(CopyFootprint {
        bytes_per_row,
        bytes_per_image,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seam's linear/sRGB pairs, which are the entries a transposition
    /// makes *dark* rather than broken.
    const SRGB_PAIRS: &[(Format, Format)] = &[
        (Format::Rgba8Unorm, Format::Rgba8UnormSrgb),
        (Format::Bgra8Unorm, Format::Bgra8UnormSrgb),
        (Format::Bc1RgbaUnorm, Format::Bc1RgbaUnormSrgb),
        (Format::Bc3RgbaUnorm, Format::Bc3RgbaUnormSrgb),
        (Format::Bc7RgbaUnorm, Format::Bc7RgbaUnormSrgb),
    ];

    /// **The mapping is injective.** Two seam formats sharing one Metal format
    /// is the copy-paste failure this file is most exposed to, and it is
    /// invisible at run time: the image is created, the sample succeeds, the
    /// colour is wrong.
    ///
    /// Driven off [`Format::ALL`] — the seam's list, not a second copy kept
    /// here. The copy that used to sit in this module covered whatever it
    /// happened to name, so a format added to `Format` and forgotten here left
    /// this test green over an incomplete set. `crcbl-vk` and `crcbl-dx12`
    /// deleted their copies for the same reason.
    #[test]
    fn no_two_formats_share_a_metal_format() {
        assert!(!Format::ALL.is_empty(), "nothing to check");
        let mut seen: Vec<(Format, MTLPixelFormat)> = Vec::new();
        for &format in Format::ALL {
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
        assert_eq!(seen.len(), Format::ALL.len());
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
    ///
    /// **`contains` cannot ask "is this Shared?", and that is what this test
    /// originally got wrong.** `MTLResourceOptions` packs two *fields* into one
    /// word rather than holding independent flags, and each field's default is
    /// encoded as zero: `StorageModeShared` is `MTLStorageMode::Shared << shift`
    /// with `Shared` = 0, and `CPUCacheModeDefaultCache` is zero the same way.
    /// A zero-valued flag is contained by every value, so `contains` on one is
    /// vacuously true and its negation is unsatisfiable — the first assertion
    /// pins that fact, because it is the reason the rest is shaped as it is.
    ///
    /// So each half is checked through the mode that *does* have bits, and the
    /// three locations are asserted mutually distinct — which is what catches
    /// two of them collapsing onto one packing.
    #[test]
    fn resource_options_carry_both_halves() {
        assert_eq!(
            MTLResourceOptions::StorageModeShared,
            MTLResourceOptions::empty(),
            "Shared stopped being the zero storage mode, so this test's shape no longer holds"
        );

        let device_local = resource_options(MemoryLocation::DeviceLocal);
        let upload = resource_options(MemoryLocation::HostUpload);
        let readback = resource_options(MemoryLocation::HostReadback);

        // Storage half, through the mode with bits: a `DeviceLocal` buffer that
        // silently became host-visible fails the first, and a host buffer that
        // became private fails the second.
        assert!(device_local.contains(MTLResourceOptions::StorageModePrivate));
        assert!(!upload.contains(MTLResourceOptions::StorageModePrivate));

        // Cache half, likewise: upload is write-combined ("never read it back"),
        // readback must not be.
        assert!(upload.contains(MTLResourceOptions::CPUCacheModeWriteCombined));
        assert!(!readback.contains(MTLResourceOptions::CPUCacheModeWriteCombined));

        assert_ne!(device_local, upload);
        assert_ne!(device_local, readback);
        assert_ne!(upload, readback);
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
        let transfer_only = texture_usage(ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST);
        assert_eq!(transfer_only, MTLTextureUsage::Unknown);
        assert!(
            !transfer_only.contains(MTLTextureUsage::PixelFormatView),
            "ORing into Unknown narrows it from everything to one thing"
        );

        let sampled = texture_usage(ImageUsage::SAMPLED);
        assert!(sampled.contains(MTLTextureUsage::ShaderRead));
        assert!(
            !sampled.contains(MTLTextureUsage::PixelFormatView),
            "the seam refuses a reinterpreting view, so the permission that \
             costs lossless compression buys nothing"
        );

        let storage = texture_usage(ImageUsage::STORAGE);
        assert!(storage.contains(MTLTextureUsage::ShaderRead));
        assert!(storage.contains(MTLTextureUsage::ShaderWrite));

        let colour = texture_usage(ImageUsage::COLOR_ATTACHMENT);
        assert!(colour.contains(MTLTextureUsage::RenderTarget));
    }

    /// A depth attachment is a render target and never a format view — and
    /// since 2026-09-03 neither is anything else.
    #[test]
    fn no_image_asks_for_a_pixel_format_view() {
        let depth = texture_usage(ImageUsage::DEPTH_STENCIL_ATTACHMENT);
        assert!(depth.contains(MTLTextureUsage::RenderTarget));
        for usage in [
            texture_usage(ImageUsage::DEPTH_STENCIL_ATTACHMENT),
            texture_usage(ImageUsage::SAMPLED),
            texture_usage(ImageUsage::STORAGE),
            texture_usage(ImageUsage::COLOR_ATTACHMENT),
            texture_usage(ImageUsage::SAMPLED | ImageUsage::COLOR_ATTACHMENT),
        ] {
            assert!(
                !usage.contains(MTLTextureUsage::PixelFormatView),
                "a view's format equals its image's, so no texture asks to be \
                 reinterpreted: {usage:?}"
            );
        }
    }

    /// Reversed-Z is produced above this seam, so the comparison must arrive
    /// and leave with the same name.
    #[test]
    fn metal_comparisons_are_not_flipped_for_reversed_z() {
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

    /// **Load and store actions, pinned by name.** A transposition here is the
    /// bug the `a_metal_load_action_preserves_what_clear_replaces` device test exists to
    /// catch on hardware; this is the same check without a GPU.
    #[test]
    fn load_and_store_actions_are_not_transposed() {
        assert_eq!(load_action(LoadOp::Load), MTLLoadAction::Load);
        assert_eq!(load_action(LoadOp::Clear), MTLLoadAction::Clear);
        assert_eq!(load_action(LoadOp::DontCare), MTLLoadAction::DontCare);
        // The three are distinct, so none of them collapsed onto another.
        let actions = [
            load_action(LoadOp::Load),
            load_action(LoadOp::Clear),
            load_action(LoadOp::DontCare),
        ];
        for (index, action) in actions.iter().enumerate() {
            assert!(
                !actions[..index].contains(action),
                "two load ops map to {action:?}"
            );
        }

        assert_eq!(store_action(StoreOp::Store), MTLStoreAction::Store);
        assert_eq!(store_action(StoreOp::Discard), MTLStoreAction::DontCare);
        for store in [StoreOp::Store, StoreOp::Discard] {
            assert_ne!(
                store_action(store),
                MTLStoreAction::Unknown,
                "an encoder that ends on Unknown raises: {store:?}"
            );
            // A resolving attachment must still resolve whichever store op it
            // was given; only whether the multisampled texture survives differs.
            let resolving = resolve_store_action(store);
            assert!(
                resolving == MTLStoreAction::MultisampleResolve
                    || resolving == MTLStoreAction::StoreAndMultisampleResolve,
                "{store:?} stopped resolving: {resolving:?}"
            );
        }
        assert_eq!(
            resolve_store_action(StoreOp::Store),
            MTLStoreAction::StoreAndMultisampleResolve
        );
        assert_eq!(
            resolve_store_action(StoreOp::Discard),
            MTLStoreAction::MultisampleResolve
        );
    }

    /// A clear colour crosses unchanged — no sRGB encode, no premultiply, no
    /// clamp. The `0.5` case is the one that matters: a backend that encoded
    /// here would send `0.7353…` and the readback test would land on `188`.
    #[test]
    fn clear_colours_cross_unencoded() {
        let colour = clear_color([0.5, 0.25, 1.0, 0.0]);
        assert_eq!(colour.red, 0.5);
        assert_eq!(colour.green, 0.25);
        assert_eq!(colour.blue, 1.0);
        assert_eq!(colour.alpha, 0.0);
        // The reversed-Z depth default is a `f32` on the seam and a `f64` here;
        // 0.0 must survive the widening as 0.0 and not become the 1.0 a
        // conventional-Z backend would clear to.
        assert_eq!(crcbl_hal::ClearValue::default().color, [0.0; 4]);
        assert_eq!(f64::from(crcbl_hal::depth::CLEAR), 0.0);
    }

    /// A negative copy offset is refused rather than wrapped.
    #[test]
    fn a_negative_copy_offset_has_no_origin() {
        assert_eq!(
            origin(Offset3d { x: 3, y: 4, z: 5 }),
            Some(MTLOrigin { x: 3, y: 4, z: 5 })
        );
        assert_eq!(origin(Offset3d { x: -1, y: 0, z: 0 }), None);
        assert_eq!(origin(Offset3d { x: 0, y: -1, z: 0 }), None);
        assert_eq!(origin(Offset3d { x: 0, y: 0, z: -1 }), None);
    }

    /// A 2D copy is one slice deep whatever its layer count says; a volume's
    /// third component really is a depth.
    #[test]
    fn only_a_volume_copies_more_than_one_slice_deep() {
        let extent = Extent3d {
            width: 8,
            height: 4,
            depth_or_layers: 6,
        };
        assert_eq!(copy_size(extent, false).depth, 1);
        assert_eq!(copy_size(extent, true).depth, 6);
        assert_eq!(copy_size(extent, false).width, 8);
        assert_eq!(copy_size(extent, false).height, 4);
    }

    /// The buffer footprint of a copy, in the three cases that differ:
    /// tightly packed, explicitly padded, and block compressed.
    #[test]
    fn copy_footprints_convert_texels_to_bytes_through_the_block_size() {
        let packed = BufferImageCopy {
            buffer: crcbl_core::Handle::from_bits(1 << 32).expect("generation 1"),
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image: crcbl_core::Handle::from_bits(1 << 32).expect("generation 1"),
            image_subresource: crcbl_hal::ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: Offset3d { x: 0, y: 0, z: 0 },
            image_extent: Extent3d::d2(16, 8),
        };
        let footprint = copy_footprint(Format::Rgba8Unorm, ImageAspect::COLOR, &packed)
            .expect("a colour aspect of a colour format");
        assert_eq!(footprint.bytes_per_row, 16 * 4);
        assert_eq!(footprint.bytes_per_image, 16 * 4 * 8);

        // An explicit row length is in texels, so it must be multiplied, not
        // used as a byte count.
        let padded = BufferImageCopy {
            buffer_row_length: 32,
            buffer_image_height: 16,
            ..packed
        };
        let footprint = copy_footprint(Format::Rgba8Unorm, ImageAspect::COLOR, &padded)
            .expect("a colour aspect of a colour format");
        assert_eq!(footprint.bytes_per_row, 32 * 4);
        assert_eq!(footprint.bytes_per_image, 32 * 4 * 16);

        // BC7 is 16 bytes per 4x4 block: a 16-texel row is four blocks, and
        // eight rows of texels are two rows of blocks.
        let footprint = copy_footprint(Format::Bc7RgbaUnorm, ImageAspect::COLOR, &packed)
            .expect("a colour aspect of a compressed colour format");
        assert_eq!(footprint.bytes_per_row, 4 * 16);
        assert_eq!(footprint.bytes_per_image, 4 * 16 * 2);

        // The aspect picks the plane, and a depth copy of a combined format
        // moves four bytes per texel rather than the format's eight.
        let depth = copy_footprint(Format::D32FloatS8Uint, ImageAspect::DEPTH, &packed)
            .expect("the depth plane");
        assert_eq!(depth.bytes_per_row, 16 * 4);
        let stencil = copy_footprint(Format::D32FloatS8Uint, ImageAspect::STENCIL, &packed)
            .expect("the stencil plane");
        assert_eq!(stencil.bytes_per_row, 16);
        // Two planes at once is not a region Metal (or Vulkan) can copy.
        assert_eq!(
            copy_footprint(
                Format::D32FloatS8Uint,
                ImageAspect::DEPTH | ImageAspect::STENCIL,
                &packed
            ),
            None
        );
        assert_eq!(
            copy_footprint(Format::Rgba8Unorm, ImageAspect::DEPTH, &packed),
            None,
            "a colour format has no depth plane"
        );
    }

    /// **The mask this file is most exposed to.** The seam and Metal number
    /// their colour channels in opposite bit orders, so a mask that was
    /// bit-cast rather than rebuilt writes the mirror-image set of channels —
    /// red becomes alpha — while `ALL` and `None` keep working, which is how it
    /// survives every casual test.
    ///
    /// **What turns it red:** replacing `color_write_mask`'s loop with
    /// `MTLColorWriteMask(writes.bits() as NSUInteger)` fails the first two
    /// assertions and leaves the `ALL`/`None` ones passing, which is the point.
    #[test]
    fn color_write_masks_are_not_bit_compatible() {
        assert_eq!(color_write_mask(ColorWrites::R), MTLColorWriteMask::Red);
        assert_eq!(color_write_mask(ColorWrites::A), MTLColorWriteMask::Alpha);
        assert_ne!(
            MTLColorWriteMask::Red.bits(),
            ColorWrites::R.bits() as NSUInteger,
            "the two bit orders agreed, so this test's whole premise is gone"
        );
        assert_eq!(color_write_mask(ColorWrites::ALL), MTLColorWriteMask::All);
        assert_eq!(
            color_write_mask(ColorWrites::empty()),
            MTLColorWriteMask::None
        );
        assert_eq!(
            color_write_mask(ColorWrites::R | ColorWrites::G),
            MTLColorWriteMask::Red | MTLColorWriteMask::Green
        );
    }

    /// Every pipeline enum, with no two seam variants collapsing onto one Metal
    /// value — the same injectivity property the format table has, and the same
    /// failure if it is lost: the pipeline is created and draws the wrong
    /// thing.
    #[test]
    fn pipeline_enums_are_injective() {
        let topologies = [
            PrimitiveTopology::PointList,
            PrimitiveTopology::LineList,
            PrimitiveTopology::LineStrip,
            PrimitiveTopology::TriangleList,
            PrimitiveTopology::TriangleStrip,
        ];
        assert!(!topologies.is_empty(), "nothing to check");
        let mut seen = Vec::new();
        for topology in topologies {
            let metal = primitive_type(topology);
            assert!(!seen.contains(&metal), "{topology:?} duplicates {metal:?}");
            seen.push(metal);
        }
        assert_eq!(
            primitive_type(PrimitiveTopology::default()),
            MTLPrimitiveType::Triangle,
            "the seam's default topology is a triangle list"
        );

        let factors = [
            BlendFactor::Zero,
            BlendFactor::One,
            BlendFactor::Src,
            BlendFactor::OneMinusSrc,
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendFactor::Dst,
            BlendFactor::OneMinusDst,
            BlendFactor::DstAlpha,
            BlendFactor::OneMinusDstAlpha,
        ];
        assert!(!factors.is_empty(), "nothing to check");
        let mut seen = Vec::new();
        for factor in factors {
            let metal = blend_factor(factor);
            assert!(!seen.contains(&metal), "{factor:?} duplicates {metal:?}");
            seen.push(metal);
        }

        let ops = [
            StencilOp::Keep,
            StencilOp::Zero,
            StencilOp::Replace,
            StencilOp::Invert,
            StencilOp::IncrementClamp,
            StencilOp::DecrementClamp,
            StencilOp::IncrementWrap,
            StencilOp::DecrementWrap,
        ];
        assert!(!ops.is_empty(), "nothing to check");
        let mut seen = Vec::new();
        for op in ops {
            let metal = stencil_operation(op);
            assert!(!seen.contains(&metal), "{op:?} duplicates {metal:?}");
            seen.push(metal);
        }
        // The pair Metal orders differently from the seam's declaration order:
        // `Invert` is 5 in Metal and sits between the two clamps in the seam's
        // enum, so a positional cast would swap it with `IncrementWrap`.
        assert_eq!(
            stencil_operation(StencilOp::Invert),
            MTLStencilOperation::Invert
        );
        assert_eq!(
            stencil_operation(StencilOp::IncrementWrap),
            MTLStencilOperation::IncrementWrap
        );

        assert_eq!(
            blend_operation(BlendOp::Subtract),
            MTLBlendOperation::Subtract
        );
        assert_eq!(
            blend_operation(BlendOp::ReverseSubtract),
            MTLBlendOperation::ReverseSubtract,
            "Subtract and ReverseSubtract differ only in which term is negated"
        );
        assert_eq!(cull_mode(CullMode::Front), MTLCullMode::Front);
        assert_eq!(cull_mode(CullMode::Back), MTLCullMode::Back);
        assert_eq!(cull_mode(CullMode::default()), MTLCullMode::None);
        assert_eq!(fill_mode(PolygonMode::Line), MTLTriangleFillMode::Lines);
        assert_eq!(fill_mode(PolygonMode::default()), MTLTriangleFillMode::Fill);
        assert_eq!(depth_clip_mode(true), MTLDepthClipMode::Clamp);
        assert_eq!(depth_clip_mode(false), MTLDepthClipMode::Clip);
    }

    /// Winding is **not** flipped, and that is a real difference from
    /// `crcbl-vk`, whose negative viewport height reverses it.
    ///
    /// **What turns it red:** swapping the two arms of `winding`, which is the
    /// change someone porting the Vulkan backend's Y-flip reasoning would make
    /// — and which would cull exactly the visible half of every model.
    #[test]
    fn front_face_winding_is_not_flipped_for_metals_framebuffer_origin() {
        assert_eq!(winding(FrontFace::Ccw), MTLWinding::CounterClockwise);
        assert_eq!(winding(FrontFace::Cw), MTLWinding::Clockwise);
        assert_eq!(
            winding(FrontFace::default()),
            MTLWinding::CounterClockwise,
            "the seam's default winding is counter-clockwise"
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
            let metal = view_texture_type(view, MTLTextureType::Type2D)
                .expect("every seam view type has a single-sampled answer");
            assert!(!seen.contains(&metal), "{view:?} duplicates {metal:?}");
            seen.push(metal);
        }
        assert_eq!(
            view_texture_type(ImageViewType::Cube, MTLTextureType::Type2D),
            Some(MTLTextureType::TypeCube)
        );
    }

    /// A view of a multisampled texture must ask Metal for a multisampled type.
    ///
    /// **What turns it red:** dropping `source` and answering from the
    /// [`ImageViewType`] alone, which is what this backend did until
    /// `exercise_msaa_resolve` in `crates/crcbl/tests/hal_seam_e2e.rs` asked it
    /// for the first multisampled view in the workspace. `Type2D` is not among
    /// the types Metal makes a view of a `Type2DMultisample` texture as, so the
    /// call is refused rather than merely losing the sample count.
    #[test]
    fn a_view_of_a_multisampled_texture_keeps_its_sample_count() {
        assert_eq!(
            view_texture_type(ImageViewType::D2, MTLTextureType::Type2DMultisample),
            Some(MTLTextureType::Type2DMultisample),
        );
        assert_eq!(
            view_texture_type(
                ImageViewType::D2Array,
                MTLTextureType::Type2DMultisampleArray
            ),
            Some(MTLTextureType::Type2DMultisampleArray),
            "the array case must not silently become the non-array type",
        );
        // The two cross cases: Metal's compatibility class for a multisampled
        // texture holds both multisampled types, so either view type is
        // creatable from either texture.
        assert_eq!(
            view_texture_type(ImageViewType::D2Array, MTLTextureType::Type2DMultisample),
            Some(MTLTextureType::Type2DMultisampleArray),
        );
        assert_eq!(
            view_texture_type(ImageViewType::D2, MTLTextureType::Type2DMultisampleArray),
            Some(MTLTextureType::Type2DMultisample),
        );
        // And the single-sampled half is untouched by any of it.
        assert_eq!(
            view_texture_type(ImageViewType::D2, MTLTextureType::Type2DArray),
            Some(MTLTextureType::Type2D),
        );
        assert_eq!(
            view_texture_type(ImageViewType::D2Array, MTLTextureType::Type2D),
            Some(MTLTextureType::Type2DArray),
        );
    }

    /// Metal has no multisampled 1D, 3D or cube type, so those pairings are
    /// refused here rather than handed to a validation layer that aborts.
    ///
    /// **What turns it red:** answering a multisampled source with the
    /// single-sampled type — `Type1D`, `TypeCube`, `TypeCubeArray`, `Type3D` —
    /// which Metal would reject as an incompatible view type.
    #[test]
    fn no_multisampled_view_exists_for_the_other_dimensionalities() {
        for view in [
            ImageViewType::D1,
            ImageViewType::Cube,
            ImageViewType::CubeArray,
            ImageViewType::D3,
        ] {
            for source in [
                MTLTextureType::Type2DMultisample,
                MTLTextureType::Type2DMultisampleArray,
            ] {
                assert_eq!(
                    view_texture_type(view, source),
                    None,
                    "{view:?} of a {source:?} texture has no Metal answer"
                );
            }
        }
    }

    /// The two halves agree: a texture this backend created as multisampled is
    /// one `view_texture_type` then treats as multisampled.
    ///
    /// **What turns it red:** either function learning a new multisampled
    /// texture type without the other — the way a `Type2DMultisampleArray`
    /// texture with a `Type2DArray` view would pass both tests above and still
    /// be refused by Metal.
    #[test]
    fn every_multisampled_texture_type_gets_a_multisampled_view_type() {
        for layers in [1, 2] {
            let source = texture_type(ImageType::D2, layers, 4);
            let view = view_texture_type(ImageViewType::D2, source)
                .expect("a 2D view of a multisampled texture");
            assert!(
                view == MTLTextureType::Type2DMultisample
                    || view == MTLTextureType::Type2DMultisampleArray,
                "a {source:?} texture got a {view:?} view"
            );
        }
    }
}
