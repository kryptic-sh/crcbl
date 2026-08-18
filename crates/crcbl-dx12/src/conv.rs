//! The seam's vocabulary, translated into D3D12's.
//!
//! Every function here is total and pure: a seam enum in, a D3D12 or DXGI enum
//! out, no device involved. That is deliberate — a mapping table is the part of
//! a backend that is wrong *silently*, so it is kept where it can be read end to
//! end and tested without hardware.
//!
//! # The rule: an exact format, or an error — never a near miss
//!
//! [`dxgi_format`] is a `match` over every [`Format`] the seam declares, and the
//! seam is deliberately not `#[non_exhaustive]` so adding one breaks this arm
//! list at compile time. Every variant has an exact DXGI counterpart — same
//! channel order, same width, same encoding — so nothing here substitutes. Two
//! formats that *look* interchangeable are not:
//!
//! * **sRGB is not a decoration.** `Rgba8Unorm` and `Rgba8UnormSrgb` differ only
//!   in whether the hardware decodes on read and encodes on write, and getting
//!   that wrong produces an image that is merely *too dark* rather than
//!   obviously broken — which is exactly how it survives review. `crcbl-wgpu`
//!   shipped that bug and `crcbl-mtl` wrote the assertion that stops it;
//!   `srgb_pairs_map_to_dxgis_srgb_formats` below is this backend's copy.
//! * **`R11g11b10Float` is `R11G11B10_FLOAT`, not `R9G9B9E5_SHAREDEXP`.** Both
//!   are packed 32-bit HDR formats and only one of them has 11/11/10 bits with
//!   no shared exponent.
//!
//! # Depth formats have four spellings in D3D12, not one
//!
//! This is the one place D3D12 needs a table Metal and Vulkan do not.
//! `D3D12_RESOURCE_DESC::Format` names the *storage*, and a resource created as
//! `DXGI_FORMAT_D32_FLOAT` can carry a depth-stencil view and nothing else — an
//! SRV on it is rejected, so a shadow map created that way would be unreadable.
//! The API's answer is a **typeless** storage format plus a concrete format per
//! view — and a fourth for the buffer side of a copy, which is a view of
//! nothing and so gets no say from either of the other two. This module spells
//! all four:
//!
//! | seam | [`resource_format`] (sampled depth) | [`dxgi_format`] (DSV) | [`depth_read_format`] (SRV) | [`copy_footprint_format`] (copy, `DEPTH`) |
//! | --- | --- | --- | --- | --- |
//! | `D32Float` | `R32_TYPELESS` | `D32_FLOAT` | `R32_FLOAT` | `R32_FLOAT` |
//! | `D16Unorm` | `R16_TYPELESS` | `D16_UNORM` | `R16_UNORM` | `R16_UNORM` |
//! | `D24UnormS8Uint` | `R24G8_TYPELESS` | `D24_UNORM_S8_UINT` | `R24_UNORM_X8_TYPELESS` | **none** |
//! | `D32FloatS8Uint` | `R32G8X24_TYPELESS` | `D32_FLOAT_S8X24_UINT` | `R32_FLOAT_X8X24_TYPELESS` | `R32_FLOAT` |
//!
//! A depth image that is never sampled keeps the concrete storage format, which
//! is what lets the driver keep its depth-specific compression.
//!
//! # What a table cannot answer
//!
//! Availability is a device question. The BC formats are reported through
//! [`Features::TEXTURE_COMPRESSION_BC`](crcbl_hal::Features::TEXTURE_COMPRESSION_BC),
//! which `crcbl_dx12::adapter` fills in from a real
//! `CheckFeatureSupport(D3D12_FEATURE_FORMAT_SUPPORT)` call per BC format, and
//! `device.rs` checks it at image creation where the device is in hand.

use crcbl_hal::{
    BindingKind, BlendFactor, BlendOp, BufferUsage, ColorWrites, CompareOp, CullMode, FilterMode,
    Format, ImageAspect, ImageType, ImageUsage, IndexFormat, MemoryLocation, PolygonMode,
    PrimitiveTopology, QueryKind, ResourceState, SamplerAddressMode, SamplerDesc, ShaderStages,
    StencilOp,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_LINELIST, D3D_PRIMITIVE_TOPOLOGY_LINESTRIP,
    D3D_PRIMITIVE_TOPOLOGY_POINTLIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_BLEND, D3D12_BLEND_DEST_ALPHA, D3D12_BLEND_DEST_COLOR, D3D12_BLEND_INV_DEST_ALPHA,
    D3D12_BLEND_INV_DEST_COLOR, D3D12_BLEND_INV_SRC_ALPHA, D3D12_BLEND_INV_SRC_COLOR,
    D3D12_BLEND_ONE, D3D12_BLEND_OP, D3D12_BLEND_OP_ADD, D3D12_BLEND_OP_MAX, D3D12_BLEND_OP_MIN,
    D3D12_BLEND_OP_REV_SUBTRACT, D3D12_BLEND_OP_SUBTRACT, D3D12_BLEND_SRC_ALPHA,
    D3D12_BLEND_SRC_COLOR, D3D12_BLEND_ZERO, D3D12_COLOR_WRITE_ENABLE_ALPHA,
    D3D12_COLOR_WRITE_ENABLE_BLUE, D3D12_COLOR_WRITE_ENABLE_GREEN, D3D12_COLOR_WRITE_ENABLE_RED,
    D3D12_CULL_MODE, D3D12_CULL_MODE_BACK, D3D12_CULL_MODE_FRONT, D3D12_CULL_MODE_NONE,
    D3D12_DESCRIPTOR_RANGE_TYPE, D3D12_DESCRIPTOR_RANGE_TYPE_CBV, D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
    D3D12_DESCRIPTOR_RANGE_TYPE_UAV, D3D12_FILL_MODE, D3D12_FILL_MODE_SOLID,
    D3D12_FILL_MODE_WIREFRAME, D3D12_PRIMITIVE_TOPOLOGY_TYPE, D3D12_PRIMITIVE_TOPOLOGY_TYPE_LINE,
    D3D12_PRIMITIVE_TOPOLOGY_TYPE_POINT, D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
    D3D12_QUERY_DATA_PIPELINE_STATISTICS, D3D12_QUERY_HEAP_TYPE, D3D12_QUERY_HEAP_TYPE_OCCLUSION,
    D3D12_QUERY_HEAP_TYPE_PIPELINE_STATISTICS, D3D12_QUERY_HEAP_TYPE_TIMESTAMP, D3D12_QUERY_TYPE,
    D3D12_QUERY_TYPE_OCCLUSION, D3D12_QUERY_TYPE_PIPELINE_STATISTICS, D3D12_QUERY_TYPE_TIMESTAMP,
    D3D12_SHADER_VISIBILITY, D3D12_SHADER_VISIBILITY_ALL, D3D12_SHADER_VISIBILITY_AMPLIFICATION,
    D3D12_SHADER_VISIBILITY_MESH, D3D12_SHADER_VISIBILITY_PIXEL, D3D12_SHADER_VISIBILITY_VERTEX,
    D3D12_STENCIL_OP, D3D12_STENCIL_OP_DECR, D3D12_STENCIL_OP_DECR_SAT, D3D12_STENCIL_OP_INCR,
    D3D12_STENCIL_OP_INCR_SAT, D3D12_STENCIL_OP_INVERT, D3D12_STENCIL_OP_KEEP,
    D3D12_STENCIL_OP_REPLACE, D3D12_STENCIL_OP_ZERO,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMPARISON_FUNC, D3D12_COMPARISON_FUNC_ALWAYS, D3D12_COMPARISON_FUNC_EQUAL,
    D3D12_COMPARISON_FUNC_GREATER, D3D12_COMPARISON_FUNC_GREATER_EQUAL, D3D12_COMPARISON_FUNC_LESS,
    D3D12_COMPARISON_FUNC_LESS_EQUAL, D3D12_COMPARISON_FUNC_NEVER, D3D12_COMPARISON_FUNC_NOT_EQUAL,
    D3D12_FILTER, D3D12_FILTER_ANISOTROPIC, D3D12_FILTER_COMPARISON_ANISOTROPIC,
    D3D12_FILTER_COMPARISON_MIN_LINEAR_MAG_MIP_POINT,
    D3D12_FILTER_COMPARISON_MIN_LINEAR_MAG_POINT_MIP_LINEAR,
    D3D12_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT, D3D12_FILTER_COMPARISON_MIN_MAG_MIP_LINEAR,
    D3D12_FILTER_COMPARISON_MIN_MAG_MIP_POINT, D3D12_FILTER_COMPARISON_MIN_MAG_POINT_MIP_LINEAR,
    D3D12_FILTER_COMPARISON_MIN_POINT_MAG_LINEAR_MIP_POINT,
    D3D12_FILTER_COMPARISON_MIN_POINT_MAG_MIP_LINEAR, D3D12_FILTER_MIN_LINEAR_MAG_MIP_POINT,
    D3D12_FILTER_MIN_LINEAR_MAG_POINT_MIP_LINEAR, D3D12_FILTER_MIN_MAG_LINEAR_MIP_POINT,
    D3D12_FILTER_MIN_MAG_MIP_LINEAR, D3D12_FILTER_MIN_MAG_MIP_POINT,
    D3D12_FILTER_MIN_MAG_POINT_MIP_LINEAR, D3D12_FILTER_MIN_POINT_MAG_LINEAR_MIP_POINT,
    D3D12_FILTER_MIN_POINT_MAG_MIP_LINEAR, D3D12_HEAP_TYPE, D3D12_HEAP_TYPE_DEFAULT,
    D3D12_HEAP_TYPE_READBACK, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_DIMENSION,
    D3D12_RESOURCE_DIMENSION_TEXTURE1D, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_DIMENSION_TEXTURE3D, D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
    D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
    D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_FLAGS, D3D12_RESOURCE_STATE_COMMON,
    D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE,
    D3D12_RESOURCE_STATE_DEPTH_READ, D3D12_RESOURCE_STATE_DEPTH_WRITE,
    D3D12_RESOURCE_STATE_GENERIC_READ, D3D12_RESOURCE_STATE_INDEX_BUFFER,
    D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET,
    D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATES, D3D12_TEXTURE_ADDRESS_MODE,
    D3D12_TEXTURE_ADDRESS_MODE_BORDER, D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
    D3D12_TEXTURE_ADDRESS_MODE_MIRROR, D3D12_TEXTURE_ADDRESS_MODE_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
    DXGI_FORMAT_BC1_UNORM, DXGI_FORMAT_BC1_UNORM_SRGB, DXGI_FORMAT_BC3_UNORM,
    DXGI_FORMAT_BC3_UNORM_SRGB, DXGI_FORMAT_BC4_UNORM, DXGI_FORMAT_BC5_UNORM,
    DXGI_FORMAT_BC6H_UF16, DXGI_FORMAT_BC7_UNORM, DXGI_FORMAT_BC7_UNORM_SRGB,
    DXGI_FORMAT_D16_UNORM, DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_D32_FLOAT,
    DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R8_UINT, DXGI_FORMAT_R8_UNORM,
    DXGI_FORMAT_R8G8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
    DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R16_FLOAT,
    DXGI_FORMAT_R16_TYPELESS, DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R16_UNORM,
    DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R24_UNORM_X8_TYPELESS,
    DXGI_FORMAT_R24G8_TYPELESS, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS,
    DXGI_FORMAT_R32_TYPELESS, DXGI_FORMAT_R32_UINT, DXGI_FORMAT_R32G8X24_TYPELESS,
    DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32_UINT, DXGI_FORMAT_R32G32B32A32_FLOAT,
};

/// The seam's texel format as DXGI spells it, for a **view**.
///
/// See the module docs for why nothing here approximates, and why a depth
/// format's resource may be created with a different (typeless) spelling.
pub(crate) const fn dxgi_format(format: Format) -> DXGI_FORMAT {
    match format {
        Format::R8Unorm => DXGI_FORMAT_R8_UNORM,
        Format::Rg8Unorm => DXGI_FORMAT_R8G8_UNORM,
        Format::Rgba8Unorm => DXGI_FORMAT_R8G8B8A8_UNORM,
        Format::Rgba8UnormSrgb => DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        Format::Bgra8Unorm => DXGI_FORMAT_B8G8R8A8_UNORM,
        Format::Bgra8UnormSrgb => DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        Format::Rgb10a2Unorm => DXGI_FORMAT_R10G10B10A2_UNORM,
        // 11 bits red, 11 green, 10 blue, no shared exponent — the same layout
        // Vulkan calls `B10G11R11_UFLOAT_PACK32` and Metal calls `RG11B10Float`.
        // `R9G9B9E5_SHAREDEXP` sits beside it in the enum and is a different
        // thing entirely.
        Format::R11g11b10Float => DXGI_FORMAT_R11G11B10_FLOAT,
        Format::R16Float => DXGI_FORMAT_R16_FLOAT,
        Format::Rg16Float => DXGI_FORMAT_R16G16_FLOAT,
        Format::Rgba16Float => DXGI_FORMAT_R16G16B16A16_FLOAT,
        Format::R32Float => DXGI_FORMAT_R32_FLOAT,
        Format::Rg32Float => DXGI_FORMAT_R32G32_FLOAT,
        Format::Rgba32Float => DXGI_FORMAT_R32G32B32A32_FLOAT,
        Format::R32Uint => DXGI_FORMAT_R32_UINT,
        Format::Rg32Uint => DXGI_FORMAT_R32G32_UINT,
        Format::D32Float => DXGI_FORMAT_D32_FLOAT,
        // The stencil plane is eight bits with twenty-four of padding, which is
        // what the `X24` in DXGI's name records and what makes the format eight
        // bytes wide rather than five.
        Format::D32FloatS8Uint => DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
        Format::D24UnormS8Uint => DXGI_FORMAT_D24_UNORM_S8_UINT,
        Format::D16Unorm => DXGI_FORMAT_D16_UNORM,
        // BC1 in DXGI is `BC1_UNORM`, which is DXT1 with the one-bit alpha the
        // seam's name already says it has.
        Format::Bc1RgbaUnorm => DXGI_FORMAT_BC1_UNORM,
        Format::Bc1RgbaUnormSrgb => DXGI_FORMAT_BC1_UNORM_SRGB,
        Format::Bc3RgbaUnorm => DXGI_FORMAT_BC3_UNORM,
        Format::Bc3RgbaUnormSrgb => DXGI_FORMAT_BC3_UNORM_SRGB,
        Format::Bc4RUnorm => DXGI_FORMAT_BC4_UNORM,
        Format::Bc5RgUnorm => DXGI_FORMAT_BC5_UNORM,
        // Unsigned, not `BC6H_SF16`: the seam's name says `Ufloat` and the
        // signed variant decodes negative values from the same bits.
        Format::Bc6hRgbUfloat => DXGI_FORMAT_BC6H_UF16,
        Format::Bc7RgbaUnorm => DXGI_FORMAT_BC7_UNORM,
        Format::Bc7RgbaUnormSrgb => DXGI_FORMAT_BC7_UNORM_SRGB,
    }
}

/// The seam's index width as the `DXGI_FORMAT` a `D3D12_INDEX_BUFFER_VIEW`
/// carries.
///
/// A separate function from [`dxgi_format`] because
/// [`IndexFormat`](crcbl_hal::IndexFormat) is a separate seam type: D3D12
/// accepts only these two spellings in an index-buffer view, so widening it to
/// the texel table would offer a caller formats the view cannot take.
pub(crate) const fn index_format(format: IndexFormat) -> DXGI_FORMAT {
    match format {
        IndexFormat::Uint16 => DXGI_FORMAT_R16_UINT,
        IndexFormat::Uint32 => DXGI_FORMAT_R32_UINT,
    }
}

/// The format a `D3D12_RESOURCE_DESC` carries for an image of this format and
/// usage.
///
/// The same as [`dxgi_format`] for everything except a **sampled depth
/// format**, which becomes the typeless storage its depth-stencil and shader
/// views are both created from. See the module docs for the table and for why a
/// depth image that is never sampled keeps its concrete format.
pub(crate) const fn resource_format(format: Format, usage: ImageUsage) -> DXGI_FORMAT {
    if !usage.contains(ImageUsage::SAMPLED) {
        return dxgi_format(format);
    }
    match format {
        Format::D32Float => DXGI_FORMAT_R32_TYPELESS,
        Format::D16Unorm => DXGI_FORMAT_R16_TYPELESS,
        Format::D24UnormS8Uint => DXGI_FORMAT_R24G8_TYPELESS,
        Format::D32FloatS8Uint => DXGI_FORMAT_R32G8X24_TYPELESS,
        _ => dxgi_format(format),
    }
}

/// The format a shader-resource view of a depth image is created with.
///
/// `None` for a colour format, which needs no separate spelling — the caller
/// uses [`dxgi_format`] and the two questions stay distinguishable rather than
/// one function quietly answering both.
///
/// The stencil plane is deliberately unreachable: each answer names the depth
/// plane and pads out the rest (`X8`, `X8X24`). Reading stencil is a second SRV
/// with a different format and a `PlaneSlice` of 1, and nothing in the seam asks
/// for one — [`ImageViewDesc`](crcbl_hal::ImageViewDesc) carries a single
/// format, so a view that covered both planes could not say which it meant.
pub(crate) const fn depth_read_format(format: Format) -> Option<DXGI_FORMAT> {
    match format {
        Format::D32Float => Some(DXGI_FORMAT_R32_FLOAT),
        Format::D16Unorm => Some(DXGI_FORMAT_R16_UNORM),
        Format::D24UnormS8Uint => Some(DXGI_FORMAT_R24_UNORM_X8_TYPELESS),
        Format::D32FloatS8Uint => Some(DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS),
        _ => None,
    }
}

/// The format a `D3D12_PLACED_SUBRESOURCE_FOOTPRINT` carries when a copy moves
/// **one plane** of an image of this format between the image and a buffer.
///
/// The fourth column of the module's depth table, and the one no view supplies.
/// A footprint describes the *buffer* side of `CopyTextureRegion` — rows of
/// texels at a pitch — so it needs a single-plane format with a defined element
/// width, and neither of the image's own spellings is one: a sampled depth
/// image is stored typeless (see [`resource_format`]) and
/// `D24_UNORM_S8_UINT`/`D32_FLOAT_S8X24_UINT` describe two planes at once.
/// This is the same question `wgpu-hal`'s dx12 backend answers in
/// `auxil::dxgi::conv::map_texture_format_for_copy`, and the same answers.
///
/// `None` means D3D12 has no footprint for that pair and the copy has to be
/// refused; it is **not** a colour/depth distinction, because a colour format's
/// answer is simply [`dxgi_format`].
///
/// # `D24UnormS8Uint`'s depth plane has no entry, and that is the answer
///
/// Not an omission and not work owed: **no fully typed single-plane DXGI format
/// has 24-bit unorm elements.** Enumerating every `DXGI_FORMAT` the `windows`
/// crate declares, the ones naming a 24-bit component are
/// `D24_UNORM_S8_UINT` and `R24G8_TYPELESS` — two planes each — and
/// `R24_UNORM_X8_TYPELESS` and `X24_TYPELESS_G8_UINT`, the per-plane spellings,
/// typeless by name. So there is no typed format of that width to lay a
/// buffer's rows out against. `wgpu-hal` returns `None` for exactly this pair
/// while giving the same format's *stencil* plane `R8_UINT`, and WebGPU
/// withholds the pair too, for a reason of its own that arrives at the same
/// place:
/// [`Capability::DepthImageCopy`](crcbl_hal::Capability::DepthImageCopy)'s
/// documentation has that table.
///
/// The two things that could be tried instead are both declined rather than
/// guessed at. Whether D3D12 accepts a *typeless* format in a placed footprint
/// is untested here and no backend read for this was willing to find out; and
/// `R32_UINT`, the typed format of the right width, is a reinterpretation that
/// would hand the caller the plane's eight `X8` padding bits mixed into every
/// depth value. `crate::command::plan_copy` refuses the pair by name instead.
pub(crate) const fn copy_footprint_format(
    format: Format,
    aspect: ImageAspect,
) -> Option<DXGI_FORMAT> {
    // The seam's own arbiter of "does this format have exactly this one plane",
    // so an aspect set naming two planes and a colour aspect on a depth image
    // are both turned away here rather than re-derived. It also fixes the
    // element width each answer below must have, which is what
    // `every_footprint_format_is_as_wide_as_the_plane_it_copies` holds them to.
    if format.texel_size(aspect).is_none() {
        return None;
    }
    if aspect.contains(ImageAspect::STENCIL) {
        // One byte per texel whichever depth-stencil format it came from: the
        // stencil plane of `D24_UNORM_S8_UINT` and of `D32_FLOAT_S8X24_UINT` is
        // the same eight bits, and `X24_TYPELESS_G8_UINT` — the plane's own
        // spelling — is typeless, so `R8_UINT` is the typed format of that
        // width. `wgpu-hal` makes the same substitution.
        return Some(DXGI_FORMAT_R8_UINT);
    }
    if aspect.contains(ImageAspect::DEPTH) {
        return match format {
            Format::D16Unorm => Some(DXGI_FORMAT_R16_UNORM),
            // `D32_FLOAT_S8X24_UINT`'s depth plane is a plain 32-bit float; the
            // `X24` padding belongs to the *stencil* plane's spelling and is not
            // part of this one.
            Format::D32Float | Format::D32FloatS8Uint => Some(DXGI_FORMAT_R32_FLOAT),
            // `D24UnormS8Uint`, and only it — see above.
            _ => None,
        };
    }
    // Colour, and `texel_size` already established the format has that aspect.
    Some(dxgi_format(format))
}

/// Which D3D12 heap a resource of this memory location lives on.
///
/// The three seam locations are D3D12's three standard heaps verbatim, which is
/// what [`MemoryLocation`]'s own documentation says it was shaped around.
/// `D3D12_HEAP_TYPE_CUSTOM` is never produced: it exists to name a CPU page
/// property and memory pool by hand, and the seam has no vocabulary for either.
/// `D3D12_HEAP_TYPE_GPU_UPLOAD` is likewise absent — it is resizable-BAR upload
/// memory, an optional capability behind its own `CheckFeatureSupport` query,
/// and picking it unqueried would fail resource creation on every machine
/// without it.
pub(crate) const fn heap_type(memory: MemoryLocation) -> D3D12_HEAP_TYPE {
    match memory {
        MemoryLocation::DeviceLocal => D3D12_HEAP_TYPE_DEFAULT,
        MemoryLocation::HostUpload => D3D12_HEAP_TYPE_UPLOAD,
        MemoryLocation::HostReadback => D3D12_HEAP_TYPE_READBACK,
    }
}

/// The resource state a newly created resource must be given.
///
/// **Not a preference — D3D12 rejects the other values.** A resource on the
/// upload heap must start in `GENERIC_READ` and one on the readback heap must
/// start in `COPY_DEST`; only the default heap takes a free choice, and
/// `COMMON` is the state a transition out of costs nothing from.
pub(crate) const fn initial_state(memory: MemoryLocation) -> D3D12_RESOURCE_STATES {
    match memory {
        MemoryLocation::DeviceLocal => D3D12_RESOURCE_STATE_COMMON,
        MemoryLocation::HostUpload => D3D12_RESOURCE_STATE_GENERIC_READ,
        MemoryLocation::HostReadback => D3D12_RESOURCE_STATE_COPY_DEST,
    }
}

/// The D3D12 states a [`ResourceState`] means.
///
/// # Not injective, and it cannot be
///
/// [`ResourceState::Undefined`] and [`ResourceState::Present`] both become
/// `COMMON`, because `D3D12_RESOURCE_STATE_PRESENT` **is** `COMMON` — the two
/// constants are the same zero, spelled twice so a swapchain barrier reads as
/// one. So the table cannot be checked for collisions the way
/// [`dxgi_format`]'s is, and `every_seam_state_maps_to_a_state_d3d12_has`
/// asserts the individual mappings instead.
///
/// # Two shader-read bits, not one
///
/// The seam's [`ResourceState::ShaderRead`] says nothing about which stage
/// reads, and D3D12 splits the read state in two. Naming both is the
/// conservative answer and the only correct one available: a resource
/// transitioned to `PIXEL_SHADER_RESOURCE` alone and then sampled from a
/// compute shader is a read of memory the barrier did not make visible.
/// [`crate::command`]'s barrier is where the cost of that shows up, and it is
/// the over-synchronisation `crcbl-hal`'s module docs already name as this
/// seam's known price.
///
/// # A write state is a single bit, deliberately
///
/// `ShaderWrite` and `ShaderReadWrite` are both `UNORDERED_ACCESS`: D3D12 has
/// no read-modify-write state distinct from the write one, and combining
/// `UNORDERED_ACCESS` with a read state is illegal — the API rejects a write
/// state paired with anything else.
pub(crate) const fn resource_state(state: ResourceState) -> D3D12_RESOURCE_STATES {
    match state {
        // `PRESENT` and `COMMON` are the same value; see the docs above.
        ResourceState::Undefined | ResourceState::Present => D3D12_RESOURCE_STATE_COMMON,
        ResourceState::ShaderRead => D3D12_RESOURCE_STATES(
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0
                | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0,
        ),
        ResourceState::ShaderWrite | ResourceState::ShaderReadWrite => {
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS
        }
        ResourceState::ColorAttachment => D3D12_RESOURCE_STATE_RENDER_TARGET,
        ResourceState::DepthStencilWrite => D3D12_RESOURCE_STATE_DEPTH_WRITE,
        ResourceState::DepthStencilRead => D3D12_RESOURCE_STATE_DEPTH_READ,
        ResourceState::TransferSrc => D3D12_RESOURCE_STATE_COPY_SOURCE,
        ResourceState::TransferDst => D3D12_RESOURCE_STATE_COPY_DEST,
        ResourceState::IndirectArgument => D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT,
        ResourceState::IndexBuffer => D3D12_RESOURCE_STATE_INDEX_BUFFER,
        // A resource the CPU reads is on the readback heap, which D3D12 pins to
        // `COPY_DEST` for its whole lifetime — see `initial_state`. Nothing
        // transitions such a buffer, and `crate::command`'s barrier skips it
        // rather than recording an illegal transition; this arm exists for the
        // `DeviceLocal` resource a caller nonetheless names, whose only honest
        // resting state is `COMMON`.
        ResourceState::HostRead => D3D12_RESOURCE_STATE_COMMON,
    }
}

/// Resource flags for a buffer.
///
/// Only one of D3D12's flags applies to a buffer, and **only on the default
/// heap**: `ALLOW_UNORDERED_ACCESS` is rejected outright on the upload and
/// readback heaps, so a `STORAGE` staging buffer asking for it would fail
/// creation rather than gaining anything. Nor would the flag help if it were
/// accepted — D3D12 pins a resource on either host-visible heap to a state a
/// shader cannot write from, for its whole lifetime.
///
/// So a host-visible storage buffer is a **read-only** storage buffer on this
/// backend: a shader resource view of one is legal and is what the engine's
/// instance and table buffers take, while binding one for writing is refused by
/// [`buffer::check_unordered_access`](crate::buffer::check_unordered_access)
/// naming the rule. That refusal is where the whole story is written down.
pub(crate) fn buffer_flags(usage: BufferUsage, memory: MemoryLocation) -> D3D12_RESOURCE_FLAGS {
    if usage.contains(BufferUsage::STORAGE) && matches!(memory, MemoryLocation::DeviceLocal) {
        D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS
    } else {
        D3D12_RESOURCE_FLAG_NONE
    }
}

/// Resource flags for an image.
///
/// [`ImageUsage::SAMPLED`] and the two transfer usages map to no flag at all:
/// D3D12 permits a shader-resource view and a copy on any texture, and states
/// the *absence* of the ability instead — `DENY_SHADER_RESOURCE`, which this
/// backend never sets, because it would have to be derived from the same usage
/// bits and would turn a caller adding `SAMPLED` later into a silently invalid
/// descriptor rather than a second view.
///
/// [`ImageUsage::PRESENT`] maps to nothing: a presentable image in D3D12 belongs
/// to the DXGI swapchain, which allocates it. The swapchain slice owns that
/// path.
pub(crate) fn image_flags(usage: ImageUsage) -> D3D12_RESOURCE_FLAGS {
    let mut flags = D3D12_RESOURCE_FLAG_NONE;
    if usage.contains(ImageUsage::COLOR_ATTACHMENT) {
        flags |= D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    }
    if usage.contains(ImageUsage::DEPTH_STENCIL_ATTACHMENT) {
        flags |= D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
    }
    if usage.contains(ImageUsage::STORAGE) {
        flags |= D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    }
    flags
}

/// The resource dimension an [`ImageType`] describes.
///
/// `D3D12_RESOURCE_DIMENSION_BUFFER` is unreachable from here: a buffer is not
/// an image and takes the dimension directly at its own creation.
pub(crate) const fn resource_dimension(image_type: ImageType) -> D3D12_RESOURCE_DIMENSION {
    match image_type {
        ImageType::D1 => D3D12_RESOURCE_DIMENSION_TEXTURE1D,
        ImageType::D2 => D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        ImageType::D3 => D3D12_RESOURCE_DIMENSION_TEXTURE3D,
    }
}

/// The single `D3D12_FILTER` a [`SamplerDesc`] describes, or `None` if D3D12 has
/// no such filter.
///
/// # Anisotropy is a filter in D3D12, not a knob beside one
///
/// Metal and Vulkan take `maxAnisotropy` as a separate field that applies to
/// whatever filters were chosen; D3D12 folds it into the filter enum, and
/// `D3D12_FILTER_ANISOTROPIC` **is** linear minification, magnification and mip
/// filtering. So a sampler asking for anisotropy above 1.0 together with
/// [`FilterMode::Nearest`] anywhere describes something D3D12 cannot express,
/// and this answers `None` so `create_sampler` can refuse it by name.
///
/// The alternative — encoding the basic filter and leaving `MaxAnisotropy` set —
/// is the silent one: D3D12 ignores the field for a non-anisotropic filter, so
/// the caller would get point sampling while believing it had asked for sixteen
/// taps.
///
/// The comparison half is a second axis of the same enum ("reduction type"), and
/// every basic filter has a `COMPARISON_` twin. Naming the constants rather than
/// encoding the bit fields by hand is deliberate: the encoding is three
/// two-bit fields and a shift, and a transcription slip in it would produce a
/// *valid* filter that is not the one asked for.
pub(crate) fn filter(desc: &SamplerDesc<'_>) -> Option<D3D12_FILTER> {
    let comparison = desc.compare.is_some();
    if desc.anisotropy > 1.0 {
        let all_linear = matches!(desc.min_filter, FilterMode::Linear)
            && matches!(desc.mag_filter, FilterMode::Linear)
            && matches!(desc.mip_filter, FilterMode::Linear);
        if !all_linear {
            return None;
        }
        return Some(if comparison {
            D3D12_FILTER_COMPARISON_ANISOTROPIC
        } else {
            D3D12_FILTER_ANISOTROPIC
        });
    }
    let basic = match (desc.min_filter, desc.mag_filter, desc.mip_filter) {
        (FilterMode::Nearest, FilterMode::Nearest, FilterMode::Nearest) => (
            D3D12_FILTER_MIN_MAG_MIP_POINT,
            D3D12_FILTER_COMPARISON_MIN_MAG_MIP_POINT,
        ),
        (FilterMode::Nearest, FilterMode::Nearest, FilterMode::Linear) => (
            D3D12_FILTER_MIN_MAG_POINT_MIP_LINEAR,
            D3D12_FILTER_COMPARISON_MIN_MAG_POINT_MIP_LINEAR,
        ),
        (FilterMode::Nearest, FilterMode::Linear, FilterMode::Nearest) => (
            D3D12_FILTER_MIN_POINT_MAG_LINEAR_MIP_POINT,
            D3D12_FILTER_COMPARISON_MIN_POINT_MAG_LINEAR_MIP_POINT,
        ),
        (FilterMode::Nearest, FilterMode::Linear, FilterMode::Linear) => (
            D3D12_FILTER_MIN_POINT_MAG_MIP_LINEAR,
            D3D12_FILTER_COMPARISON_MIN_POINT_MAG_MIP_LINEAR,
        ),
        (FilterMode::Linear, FilterMode::Nearest, FilterMode::Nearest) => (
            D3D12_FILTER_MIN_LINEAR_MAG_MIP_POINT,
            D3D12_FILTER_COMPARISON_MIN_LINEAR_MAG_MIP_POINT,
        ),
        (FilterMode::Linear, FilterMode::Nearest, FilterMode::Linear) => (
            D3D12_FILTER_MIN_LINEAR_MAG_POINT_MIP_LINEAR,
            D3D12_FILTER_COMPARISON_MIN_LINEAR_MAG_POINT_MIP_LINEAR,
        ),
        (FilterMode::Linear, FilterMode::Linear, FilterMode::Nearest) => (
            D3D12_FILTER_MIN_MAG_LINEAR_MIP_POINT,
            D3D12_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT,
        ),
        (FilterMode::Linear, FilterMode::Linear, FilterMode::Linear) => (
            D3D12_FILTER_MIN_MAG_MIP_LINEAR,
            D3D12_FILTER_COMPARISON_MIN_MAG_MIP_LINEAR,
        ),
    };
    Some(if comparison { basic.1 } else { basic.0 })
}

/// Addressing outside `[0, 1]`.
///
/// [`SamplerAddressMode::ClampToBorder`] becomes
/// [`D3D12_TEXTURE_ADDRESS_MODE_BORDER`], and the border colour itself is set
/// beside it in `device.rs` — transparent black, which is what the seam's
/// variant documents and what a shadow atlas needs.
/// `D3D12_TEXTURE_ADDRESS_MODE_MIRROR_ONCE` is never produced: it mirrors once
/// and then clamps, which is a third behaviour the seam does not have.
pub(crate) const fn address_mode(mode: SamplerAddressMode) -> D3D12_TEXTURE_ADDRESS_MODE {
    match mode {
        SamplerAddressMode::Repeat => D3D12_TEXTURE_ADDRESS_MODE_WRAP,
        SamplerAddressMode::MirrorRepeat => D3D12_TEXTURE_ADDRESS_MODE_MIRROR,
        SamplerAddressMode::ClampToEdge => D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        SamplerAddressMode::ClampToBorder => D3D12_TEXTURE_ADDRESS_MODE_BORDER,
    }
}

/// A comparison, named by the comparison.
///
/// **Nothing here flips sign for reversed-Z, and that is the point.** The engine
/// is reversed-Z everywhere (1.0 near, 0.0 far), but `crcbl-hal` bakes that into
/// its *defaults* rather than its vocabulary: [`CompareOp::Greater`] means
/// greater, and a shadow comparison asking "is this fragment closer" already
/// arrives here as `Greater` because
/// [`SamplerDesc::compare`](crcbl_hal::SamplerDesc::compare) says so. A backend
/// that inverted the sense on the way through would produce shadows that are
/// exactly inside out, twice over, for callers that read the seam correctly.
///
/// `D3D12_COMPARISON_FUNC_NONE` is never produced: it means "this sampler does
/// not compare", which the seam expresses as `compare: None` and which therefore
/// never reaches this function.
pub(crate) const fn comparison_func(op: CompareOp) -> D3D12_COMPARISON_FUNC {
    match op {
        CompareOp::Never => D3D12_COMPARISON_FUNC_NEVER,
        CompareOp::Less => D3D12_COMPARISON_FUNC_LESS,
        CompareOp::Equal => D3D12_COMPARISON_FUNC_EQUAL,
        CompareOp::LessOrEqual => D3D12_COMPARISON_FUNC_LESS_EQUAL,
        CompareOp::Greater => D3D12_COMPARISON_FUNC_GREATER,
        CompareOp::NotEqual => D3D12_COMPARISON_FUNC_NOT_EQUAL,
        CompareOp::GreaterOrEqual => D3D12_COMPARISON_FUNC_GREATER_EQUAL,
        CompareOp::Always => D3D12_COMPARISON_FUNC_ALWAYS,
    }
}

/// How primitives assemble, for the **pipeline state object**.
///
/// D3D12 splits what Vulkan and Metal keep in one place: the PSO takes a
/// *category* — point, line or triangle — and the command list takes the exact
/// topology at [`primitive_topology`]. A list and a strip share a PSO and
/// differ only at `IASetPrimitiveTopology`, which is why
/// `crcbl_dx12::pipeline` keeps the second value beside the object.
pub(crate) const fn primitive_topology_type(
    topology: PrimitiveTopology,
) -> D3D12_PRIMITIVE_TOPOLOGY_TYPE {
    match topology {
        PrimitiveTopology::PointList => D3D12_PRIMITIVE_TOPOLOGY_TYPE_POINT,
        PrimitiveTopology::LineList | PrimitiveTopology::LineStrip => {
            D3D12_PRIMITIVE_TOPOLOGY_TYPE_LINE
        }
        PrimitiveTopology::TriangleList | PrimitiveTopology::TriangleStrip => {
            D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE
        }
    }
}

/// How primitives assemble, for the **command list**. See
/// [`primitive_topology_type`] for why there are two.
pub(crate) const fn primitive_topology(topology: PrimitiveTopology) -> D3D_PRIMITIVE_TOPOLOGY {
    match topology {
        PrimitiveTopology::PointList => D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
        PrimitiveTopology::LineList => D3D_PRIMITIVE_TOPOLOGY_LINELIST,
        PrimitiveTopology::LineStrip => D3D_PRIMITIVE_TOPOLOGY_LINESTRIP,
        PrimitiveTopology::TriangleList => D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
        PrimitiveTopology::TriangleStrip => D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
    }
}

/// Which faces the rasteriser discards.
pub(crate) const fn cull_mode(mode: CullMode) -> D3D12_CULL_MODE {
    match mode {
        CullMode::None => D3D12_CULL_MODE_NONE,
        CullMode::Front => D3D12_CULL_MODE_FRONT,
        CullMode::Back => D3D12_CULL_MODE_BACK,
    }
}

/// Solid or wireframe.
pub(crate) const fn fill_mode(mode: PolygonMode) -> D3D12_FILL_MODE {
    match mode {
        PolygonMode::Fill => D3D12_FILL_MODE_SOLID,
        PolygonMode::Line => D3D12_FILL_MODE_WIREFRAME,
    }
}

/// One blend factor.
///
/// **`Src`/`Dst` are the colour factors and `SrcAlpha`/`DstAlpha` the alpha
/// ones, and D3D12 spells the pair `_COLOR`/`_ALPHA` rather than by position.**
/// Picking `D3D12_BLEND_SRC_ALPHA` for [`BlendFactor::Src`] would compile, blend
/// something plausible, and be wrong only where alpha differs from luminance —
/// which is most of a frame and none of a unit test.
pub(crate) const fn blend_factor(factor: BlendFactor) -> D3D12_BLEND {
    match factor {
        BlendFactor::Zero => D3D12_BLEND_ZERO,
        BlendFactor::One => D3D12_BLEND_ONE,
        BlendFactor::Src => D3D12_BLEND_SRC_COLOR,
        BlendFactor::OneMinusSrc => D3D12_BLEND_INV_SRC_COLOR,
        BlendFactor::SrcAlpha => D3D12_BLEND_SRC_ALPHA,
        BlendFactor::OneMinusSrcAlpha => D3D12_BLEND_INV_SRC_ALPHA,
        BlendFactor::Dst => D3D12_BLEND_DEST_COLOR,
        BlendFactor::OneMinusDst => D3D12_BLEND_INV_DEST_COLOR,
        BlendFactor::DstAlpha => D3D12_BLEND_DEST_ALPHA,
        BlendFactor::OneMinusDstAlpha => D3D12_BLEND_INV_DEST_ALPHA,
    }
}

/// How the two blended terms combine.
pub(crate) const fn blend_op(op: BlendOp) -> D3D12_BLEND_OP {
    match op {
        BlendOp::Add => D3D12_BLEND_OP_ADD,
        BlendOp::Subtract => D3D12_BLEND_OP_SUBTRACT,
        BlendOp::ReverseSubtract => D3D12_BLEND_OP_REV_SUBTRACT,
        BlendOp::Min => D3D12_BLEND_OP_MIN,
        BlendOp::Max => D3D12_BLEND_OP_MAX,
    }
}

/// Which channels a colour target writes, as D3D12's 8-bit mask.
///
/// Bit for bit rather than by numeric value: the seam's [`ColorWrites`] and
/// D3D12's `D3D12_COLOR_WRITE_ENABLE_*` happen to agree on R=1, G=2, B=4, A=8,
/// and a `bits() as u8` cast would be a coincidence nothing checks.
pub(crate) fn color_write_mask(writes: ColorWrites) -> u8 {
    let mut mask = 0;
    for (bit, enable) in [
        (ColorWrites::R, D3D12_COLOR_WRITE_ENABLE_RED),
        (ColorWrites::G, D3D12_COLOR_WRITE_ENABLE_GREEN),
        (ColorWrites::B, D3D12_COLOR_WRITE_ENABLE_BLUE),
        (ColorWrites::A, D3D12_COLOR_WRITE_ENABLE_ALPHA),
    ] {
        if writes.contains(bit) {
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            {
                mask |= enable.0 as u8;
            }
        }
    }
    mask
}

/// What a stencil test outcome does to the stored value.
pub(crate) const fn stencil_op(op: StencilOp) -> D3D12_STENCIL_OP {
    match op {
        StencilOp::Keep => D3D12_STENCIL_OP_KEEP,
        StencilOp::Zero => D3D12_STENCIL_OP_ZERO,
        StencilOp::Replace => D3D12_STENCIL_OP_REPLACE,
        StencilOp::Invert => D3D12_STENCIL_OP_INVERT,
        StencilOp::IncrementClamp => D3D12_STENCIL_OP_INCR_SAT,
        StencilOp::DecrementClamp => D3D12_STENCIL_OP_DECR_SAT,
        StencilOp::IncrementWrap => D3D12_STENCIL_OP_INCR,
        StencilOp::DecrementWrap => D3D12_STENCIL_OP_DECR,
    }
}

/// Which descriptor-heap range a binding lands in.
///
/// `None` for [`BindingKind::Sampler`], because a sampler is not a
/// CBV/SRV/UAV range at all — it lives in a different heap type and therefore
/// in a different root parameter. Returning a range type for it would let one
/// descriptor table mix the two, which D3D12 refuses at root-signature
/// serialisation and which is much clearer refused here by shape.
pub(crate) const fn descriptor_range_type(
    kind: BindingKind,
) -> Option<D3D12_DESCRIPTOR_RANGE_TYPE> {
    match kind {
        BindingKind::UniformBuffer { .. } => Some(D3D12_DESCRIPTOR_RANGE_TYPE_CBV),
        // A read-only storage buffer is an SRV and a writable one a UAV — the
        // same split `StructuredBuffer` and `RWStructuredBuffer` make in the
        // HLSL `crcbl-shaders` generates, so the two agree by construction.
        // A storage image splits the same way and drops its `view_type` and its
        // `format` doing it: a `D3D12_UNORDERED_ACCESS_VIEW_DESC` carries both,
        // and it is written when the view is created rather than when the range
        // is declared — see `crate::device`'s view creation.
        BindingKind::StorageBuffer { read_only, .. }
        | BindingKind::StorageImage { read_only, .. } => Some(if read_only {
            D3D12_DESCRIPTOR_RANGE_TYPE_SRV
        } else {
            D3D12_DESCRIPTOR_RANGE_TYPE_UAV
        }),
        // The `view_type` and the `sample_type` are both dropped: a descriptor
        // range names a register and a type, never a dimension or a format. What
        // the shader reads is decided by the `D3D12_SHADER_RESOURCE_VIEW_DESC`
        // the SRV was created with — see `crate::device`'s view creation. Only
        // WebGPU wants either in the layout.
        BindingKind::SampledImage { .. } => Some(D3D12_DESCRIPTOR_RANGE_TYPE_SRV),
        // `comparison` is dropped with it: a `D3D12_SAMPLER_DESC` decides
        // whether it compares through its `ComparisonFunc` and a `_COMPARISON_`
        // filter, which is `sampler_desc` below reading `SamplerDesc::compare`.
        // A `SamplerComparisonState` and a `SamplerState` occupy the same `s#`
        // register space and the same heap.
        BindingKind::Sampler { .. } => None,
    }
}

/// Which stages a root parameter is visible to.
///
/// D3D12 takes visibility per **root parameter**, not per binding, and offers
/// exactly one stage or "all" — so a table whose bindings disagree gets `ALL`.
/// That is wider than the caller asked for and never narrower, which is the
/// only direction that cannot make a legal shader fail to read its own
/// resource.
///
/// # The two mesh stages have their own values, and had to get them here
///
/// [`ShaderStages::MESH`] and [`ShaderStages::TASK`] are
/// `D3D12_SHADER_VISIBILITY_MESH` and `_AMPLIFICATION`. Before the mesh slice
/// they fell into the `ALL` arm, which was sound only in the sense that nothing
/// could reach it: `crcbl_dx12::binding`'s `plan_layout` runs
/// [`ShaderStages::check_supported`](crcbl_hal::ShaderStages::check_supported)
/// first, and that refuses either bit on a device reporting no
/// `Features::MESH_SHADER` — which this backend's adapters do not. A mesh
/// pipeline built here now names those stages, so the arms are written rather
/// than argued away.
pub(crate) const fn shader_visibility(stages: ShaderStages) -> D3D12_SHADER_VISIBILITY {
    if stages.bits() == ShaderStages::VERTEX.bits() {
        D3D12_SHADER_VISIBILITY_VERTEX
    } else if stages.bits() == ShaderStages::FRAGMENT.bits() {
        D3D12_SHADER_VISIBILITY_PIXEL
    } else if stages.bits() == ShaderStages::MESH.bits() {
        D3D12_SHADER_VISIBILITY_MESH
    } else if stages.bits() == ShaderStages::TASK.bits() {
        D3D12_SHADER_VISIBILITY_AMPLIFICATION
    } else {
        D3D12_SHADER_VISIBILITY_ALL
    }
}

/// The heap a [`QueryKind`]'s queries live in, and the type each query is.
///
/// **Two enumerations rather than one**, because D3D12 has two: the heap type
/// goes in `D3D12_QUERY_HEAP_DESC` at creation and the query type goes on every
/// `BeginQuery`, `EndQuery` and `ResolveQueryData` afterwards. They are not
/// interchangeable — `D3D12_QUERY_HEAP_TYPE_TIMESTAMP` is `1` and
/// `D3D12_QUERY_TYPE_TIMESTAMP` is `2` — so a backend that kept one and passed
/// it to both would create an occlusion heap and resolve it as statistics, with
/// no error anywhere.
///
/// # Occlusion counts samples, and `BINARY_OCCLUSION` does not
///
/// [`QueryKind::Occlusion`] is "samples that passed the depth test", so it maps
/// to `D3D12_QUERY_TYPE_OCCLUSION` and not to `_BINARY_OCCLUSION`, which answers
/// zero or non-zero and is what `wgpu-hal`'s dx12 backend uses because WebGPU's
/// occlusion query is a boolean. Both resolve one `UINT64` per query, so the
/// choice is invisible to every size in [`crate::query`] and visible in exactly
/// one place: the number a caller reads. `crcbl-vk` creates
/// `vk::QueryType::OCCLUSION` for the same reason.
pub(crate) const fn query_types(kind: QueryKind) -> (D3D12_QUERY_HEAP_TYPE, D3D12_QUERY_TYPE) {
    match kind {
        QueryKind::Timestamp => (D3D12_QUERY_HEAP_TYPE_TIMESTAMP, D3D12_QUERY_TYPE_TIMESTAMP),
        QueryKind::Occlusion => (D3D12_QUERY_HEAP_TYPE_OCCLUSION, D3D12_QUERY_TYPE_OCCLUSION),
        QueryKind::PipelineStatistics => (
            D3D12_QUERY_HEAP_TYPE_PIPELINE_STATISTICS,
            D3D12_QUERY_TYPE_PIPELINE_STATISTICS,
        ),
    }
}

/// [`crate::query::result_bytes`] against the structures D3D12 actually
/// resolves.
///
/// **The one thing `crate::query` cannot check itself.** That module is compiled
/// on hosts with no D3D12 so its arithmetic can be tested at all, which means
/// its strides are literals — and a literal that drifted from the ABI would size
/// every resolve destination wrongly while every test over it still passed. This
/// is where the two meet, and it is a `const` block, so a `windows` upgrade that
/// changed either structure fails the build rather than a run.
const _: () = {
    assert!(
        crate::query::result_bytes(QueryKind::Timestamp) == size_of::<u64>() as u64,
        "a timestamp resolves as one UINT64"
    );
    assert!(
        crate::query::result_bytes(QueryKind::Occlusion) == size_of::<u64>() as u64,
        "an occlusion query resolves as one UINT64"
    );
    assert!(
        crate::query::result_bytes(QueryKind::PipelineStatistics)
            == size_of::<D3D12_QUERY_DATA_PIPELINE_STATISTICS>() as u64,
        "a statistics query resolves as a whole D3D12_QUERY_DATA_PIPELINE_STATISTICS"
    );
};

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{ImageViewType, SampleType};
    use windows::Win32::Graphics::Direct3D12::{
        D3D12_COMPARISON_FUNC_NONE, D3D12_TEXTURE_ADDRESS_MODE_MIRROR_ONCE,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_R9G9B9E5_SHAREDEXP, DXGI_FORMAT_UNKNOWN,
    };

    /// The seam's linear/sRGB pairs, which are the entries a transposition makes
    /// *dark* rather than broken.
    const SRGB_PAIRS: &[(Format, Format)] = &[
        (Format::Rgba8Unorm, Format::Rgba8UnormSrgb),
        (Format::Bgra8Unorm, Format::Bgra8UnormSrgb),
        (Format::Bc1RgbaUnorm, Format::Bc1RgbaUnormSrgb),
        (Format::Bc3RgbaUnorm, Format::Bc3RgbaUnormSrgb),
        (Format::Bc7RgbaUnorm, Format::Bc7RgbaUnormSrgb),
    ];

    /// Every memory location the seam has, so the heap and state tables are
    /// checked over all of them.
    const LOCATIONS: &[MemoryLocation] = &[
        MemoryLocation::DeviceLocal,
        MemoryLocation::HostUpload,
        MemoryLocation::HostReadback,
    ];

    /// Every seam state, so the table below is checked over all of them rather
    /// than the handful a clear happens to use. Hand-written because
    /// [`ResourceState`] has no list of its own; [`resource_state`]'s
    /// exhaustive `match` is what makes a *missing* variant a compile error.
    const STATES: &[ResourceState] = &[
        ResourceState::Undefined,
        ResourceState::ShaderRead,
        ResourceState::ShaderWrite,
        ResourceState::ShaderReadWrite,
        ResourceState::ColorAttachment,
        ResourceState::DepthStencilWrite,
        ResourceState::DepthStencilRead,
        ResourceState::TransferSrc,
        ResourceState::TransferDst,
        ResourceState::IndirectArgument,
        ResourceState::IndexBuffer,
        ResourceState::HostRead,
        ResourceState::Present,
    ];

    /// **The mapping is injective.** Two seam formats sharing one DXGI format is
    /// the copy-paste failure this file is most exposed to, and it is invisible
    /// at run time: the image is created, the sample succeeds, the colour is
    /// wrong.
    ///
    /// Driven off [`Format::ALL`] — the seam's list, not a second copy kept
    /// here. The copy that used to sit in this module covered whatever it
    /// happened to name, so a format added to `Format` and forgotten here left
    /// this test green over an incomplete set. `crcbl-vk` deleted its copy for
    /// the same reason.
    #[test]
    fn no_two_formats_share_a_dxgi_format() {
        assert!(!Format::ALL.is_empty(), "nothing to check");
        let mut seen: Vec<(Format, DXGI_FORMAT)> = Vec::new();
        for &format in Format::ALL {
            let dxgi = dxgi_format(format);
            assert_ne!(dxgi, DXGI_FORMAT_UNKNOWN, "{format:?} has no DXGI format");
            if let Some((other, _)) = seen.iter().find(|(_, seen)| *seen == dxgi) {
                panic!("{format:?} and {other:?} both map to {dxgi:?}");
            }
            seen.push((format, dxgi));
        }
        assert_eq!(seen.len(), Format::ALL.len());
    }

    /// Every seam state reaches a D3D12 state, and the ones whose confusion is
    /// silent are pinned by name.
    ///
    /// Injectivity is *not* asserted, and the reason is in [`resource_state`]'s
    /// docs: `PRESENT` and `COMMON` are one constant, so `Undefined` and
    /// `Present` genuinely collide. What is asserted instead is the pairs a
    /// transposition would make wrong without failing anything — a copy source
    /// read as a copy destination is a copy that runs and moves nothing, and a
    /// depth attachment transitioned to `DEPTH_READ` when it is written is a
    /// depth buffer that stops updating.
    #[test]
    fn every_seam_state_maps_to_the_d3d12_state_it_names() {
        assert!(!STATES.is_empty(), "nothing to check");
        for &state in STATES {
            // Only `Undefined`, `Present` and `HostRead` may be `COMMON`, which
            // is zero: any other state landing there is a `match` arm that was
            // never written.
            let expected_common = matches!(
                state,
                ResourceState::Undefined | ResourceState::Present | ResourceState::HostRead
            );
            assert_eq!(
                resource_state(state) == D3D12_RESOURCE_STATE_COMMON,
                expected_common,
                "{state:?} landed on COMMON"
            );
        }
        assert_eq!(
            resource_state(ResourceState::TransferSrc),
            D3D12_RESOURCE_STATE_COPY_SOURCE
        );
        assert_eq!(
            resource_state(ResourceState::TransferDst),
            D3D12_RESOURCE_STATE_COPY_DEST
        );
        assert_eq!(
            resource_state(ResourceState::ColorAttachment),
            D3D12_RESOURCE_STATE_RENDER_TARGET
        );
        assert_eq!(
            resource_state(ResourceState::DepthStencilWrite),
            D3D12_RESOURCE_STATE_DEPTH_WRITE
        );
        assert_eq!(
            resource_state(ResourceState::DepthStencilRead),
            D3D12_RESOURCE_STATE_DEPTH_READ
        );
        assert_ne!(
            resource_state(ResourceState::DepthStencilRead),
            resource_state(ResourceState::DepthStencilWrite),
            "the two depth states collapsed, so a written depth buffer would be read-only"
        );
        // A shader read must name **both** stages: a resource made visible only
        // to the pixel stage and then sampled from compute is a read of memory
        // the barrier did not flush, which no call reports.
        let read = resource_state(ResourceState::ShaderRead);
        assert!(
            read.contains(D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE)
                && read.contains(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE),
            "ShaderRead names only one shader stage: {read:?}"
        );
        // And a write state is that bit alone — D3D12 rejects `UNORDERED_ACCESS`
        // combined with any read state.
        assert_eq!(
            resource_state(ResourceState::ShaderWrite),
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS
        );
        assert_eq!(
            resource_state(ResourceState::ShaderReadWrite),
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS
        );
    }

    /// The typeless storage formats are injective too, and for the same reason:
    /// two depth formats sharing one typeless resource format would create the
    /// wrong-width allocation and read back plausible garbage.
    ///
    /// The three answers for one seam format must also stay distinct — the
    /// storage, the depth-stencil view and the shader view are three different
    /// DXGI constants, and a table that collapsed any two of them would produce
    /// a view D3D12 rejects with no return value to say so.
    #[test]
    fn sampled_depth_formats_get_a_distinct_typeless_storage_and_read_format() {
        let depth: Vec<Format> = Format::ALL
            .iter()
            .copied()
            .filter(|format| format.is_depth_stencil())
            .collect();
        assert!(!depth.is_empty(), "nothing to check");

        let mut storages: Vec<DXGI_FORMAT> = Vec::new();
        let mut reads: Vec<DXGI_FORMAT> = Vec::new();
        for format in depth {
            let storage = resource_format(format, ImageUsage::SAMPLED);
            let read = depth_read_format(format).unwrap_or_else(|| {
                panic!("{format:?} is a depth format with no shader-readable spelling")
            });
            let view = dxgi_format(format);
            assert!(
                !storages.contains(&storage),
                "{format:?} reuses the typeless format {storage:?}"
            );
            assert!(
                !reads.contains(&read),
                "{format:?} reuses the read format {read:?}"
            );
            assert_ne!(
                storage, view,
                "{format:?}: storage and DSV format collapsed"
            );
            assert_ne!(
                storage, read,
                "{format:?}: storage and SRV format collapsed"
            );
            assert_ne!(view, read, "{format:?}: DSV and SRV format collapsed");
            storages.push(storage);
            reads.push(read);

            // A depth image nobody samples keeps its concrete format, which is
            // what lets the driver keep depth compression.
            assert_eq!(
                resource_format(format, ImageUsage::DEPTH_STENCIL_ATTACHMENT),
                view,
                "{format:?} went typeless without being sampled"
            );
        }
    }

    /// The plane of one image a copy can name: an aspect, and nothing else.
    ///
    /// Every single-plane aspect the seam has, so the tables below are asked
    /// about every (format, plane) pair rather than the depth ones somebody
    /// remembered.
    const ASPECTS: &[ImageAspect] = &[ImageAspect::COLOR, ImageAspect::DEPTH, ImageAspect::STENCIL];

    /// **The one pair D3D12 has no placed footprint for, pinned as the only
    /// one.**
    ///
    /// `crate::command::plan_copy` refuses a `None` from
    /// [`copy_footprint_format`] with a single sentence naming
    /// `D24UnormS8Uint`'s depth plane, and a sentence is only true if that pair
    /// is the sole hole. So this asserts the hole is exactly one and exactly
    /// that one: a format added to the seam without a footprint entry, or an
    /// entry deleted, makes the refusal a lie and makes this go red instead.
    ///
    /// The `None`s that are *not* holes are asserted too — a plane the format
    /// does not have, and an aspect set naming two — because a table that
    /// answered those would hand a copy a footprint for a plane that is not
    /// there.
    #[test]
    fn the_depth_plane_of_d24_unorm_s8_uint_is_the_only_copy_with_no_footprint() {
        assert!(!Format::ALL.is_empty(), "nothing to check");
        let mut holes: Vec<(Format, ImageAspect)> = Vec::new();
        for &format in Format::ALL {
            for &aspect in ASPECTS {
                match (
                    format.texel_size(aspect),
                    copy_footprint_format(format, aspect),
                ) {
                    // A plane this format does not have is not a hole in the
                    // table; it is a copy nothing could ask for.
                    (None, None) => {}
                    (None, Some(dxgi)) => panic!(
                        "{format:?} has no {aspect:?} plane and copy_footprint_format offered \
                         {dxgi:?} for it"
                    ),
                    (Some(_), None) => holes.push((format, aspect)),
                    (Some(_), Some(_)) => {}
                }
            }
        }
        assert_eq!(
            holes,
            vec![(Format::D24UnormS8Uint, ImageAspect::DEPTH)],
            "the set of copies this backend has no footprint for changed, and \
             crate::command::plan_copy still refuses all of them with one sentence naming \
             D24UnormS8Uint's depth plane"
        );
        // An aspect set naming both planes is not one plane, so it has no
        // footprint however many of its planes the format has.
        assert_eq!(
            copy_footprint_format(
                Format::D32FloatS8Uint,
                ImageAspect::DEPTH | ImageAspect::STENCIL
            ),
            None
        );
        assert_eq!(
            copy_footprint_format(Format::Rgba8Unorm, ImageAspect::empty()),
            None
        );
    }

    /// **A footprint's format is as wide as the plane's texel**, which is what
    /// makes the row pitch `crate::command::plan_copy` computes from
    /// [`Format::texel_size`] describe the same rows D3D12 will read.
    ///
    /// The failure this closes is silent and is the reason the fourth column
    /// exists at all: a depth plane given `R16_UNORM` where its texel is four
    /// bytes copies at half the pitch, so every row after the first lands
    /// shifted and the image is *sheared* rather than absent. The seam's
    /// `texel_size` is the width `plan_copy` sizes the buffer against; DXGI's
    /// constant is the width the runtime reads at; nothing else compares them.
    ///
    /// Only the depth and stencil planes need the widths spelled out — a colour
    /// aspect's footprint is [`dxgi_format`] itself, which is asserted as an
    /// identity instead and cannot disagree with a width it did not choose.
    #[test]
    fn every_footprint_format_is_as_wide_as_the_plane_it_copies() {
        // The typed single-plane formats a depth or stencil footprint can name,
        // with the bytes per texel DXGI gives each.
        const PLANE_WIDTHS: &[(DXGI_FORMAT, u32)] = &[
            (DXGI_FORMAT_R8_UINT, 1),
            (DXGI_FORMAT_R16_UNORM, 2),
            (DXGI_FORMAT_R32_FLOAT, 4),
        ];
        let depth: Vec<Format> = Format::ALL
            .iter()
            .copied()
            .filter(|format| format.is_depth_stencil())
            .collect();
        assert!(!depth.is_empty(), "nothing to check");
        let mut checked = 0_usize;
        for format in depth {
            for &aspect in ASPECTS {
                let Some(texel) = format.texel_size(aspect) else {
                    continue;
                };
                let Some(dxgi) = copy_footprint_format(format, aspect) else {
                    // `D24UnormS8Uint`'s depth plane, pinned as the only one by
                    // the test above.
                    continue;
                };
                let (_, width) = PLANE_WIDTHS
                    .iter()
                    .find(|(candidate, _)| *candidate == dxgi)
                    .unwrap_or_else(|| {
                        panic!(
                            "{format:?}'s {aspect:?} plane copies through {dxgi:?}, which is not \
                             one of the typed single-plane formats this table knows a width for"
                        )
                    });
                assert_eq!(
                    *width, texel,
                    "{format:?}'s {aspect:?} plane is {texel} bytes per texel and its footprint \
                     format {dxgi:?} is {width}; a copy would read the buffer at the wrong pitch"
                );
                checked += 1;
            }
        }
        // The loop above `continue`s past every pair it cannot check, so
        // without this it would pass by checking nothing at all. The number is
        // every depth and stencil plane the seam's formats have, less the one
        // the test above pins as having no footprint.
        assert_eq!(checked, 5, "depth and stencil planes checked");

        // And a colour format's footprint is its own format, with no second
        // spelling to get wrong.
        for &format in Format::ALL {
            if format.is_depth_stencil() {
                continue;
            }
            assert_eq!(
                copy_footprint_format(format, ImageAspect::COLOR),
                Some(dxgi_format(format)),
                "{format:?}"
            );
        }
    }

    /// The depth planes' footprint formats by name, because a transposition
    /// between two of them is a copy that runs and reads the wrong bytes.
    ///
    /// These are the answers `wgpu-hal`'s `map_texture_format_for_copy` gives
    /// for the same pairs, which is the only cross-check available off a
    /// Windows machine.
    #[test]
    fn the_depth_and_stencil_planes_copy_through_the_formats_wgpu_hal_uses() {
        assert_eq!(
            copy_footprint_format(Format::D16Unorm, ImageAspect::DEPTH),
            Some(DXGI_FORMAT_R16_UNORM)
        );
        assert_eq!(
            copy_footprint_format(Format::D32Float, ImageAspect::DEPTH),
            Some(DXGI_FORMAT_R32_FLOAT)
        );
        assert_eq!(
            copy_footprint_format(Format::D32FloatS8Uint, ImageAspect::DEPTH),
            Some(DXGI_FORMAT_R32_FLOAT)
        );
        assert_eq!(
            copy_footprint_format(Format::D32FloatS8Uint, ImageAspect::STENCIL),
            Some(DXGI_FORMAT_R8_UINT)
        );
        assert_eq!(
            copy_footprint_format(Format::D24UnormS8Uint, ImageAspect::STENCIL),
            Some(DXGI_FORMAT_R8_UINT)
        );
        // The footprint is never the image's own spelling for a depth format:
        // that is either planar or typeless, and a placed footprint is neither.
        for &format in Format::ALL {
            if !format.is_depth_stencil() {
                continue;
            }
            for &aspect in ASPECTS {
                let Some(dxgi) = copy_footprint_format(format, aspect) else {
                    continue;
                };
                assert_ne!(
                    dxgi,
                    dxgi_format(format),
                    "{format:?}'s {aspect:?} plane copies through the format's own DSV spelling"
                );
                assert_ne!(
                    dxgi,
                    resource_format(format, ImageUsage::SAMPLED),
                    "{format:?}'s {aspect:?} plane copies through a typeless format"
                );
            }
        }
    }

    /// A colour format is never rewritten by usage, and has no depth spelling to
    /// reach for.
    #[test]
    fn colour_formats_have_one_spelling_whatever_the_usage() {
        let colour: Vec<Format> = Format::ALL
            .iter()
            .copied()
            .filter(|format| !format.is_depth_stencil())
            .collect();
        assert!(!colour.is_empty(), "nothing to check");
        for format in colour {
            assert_eq!(
                resource_format(format, ImageUsage::SAMPLED | ImageUsage::STORAGE),
                dxgi_format(format),
                "{format:?}"
            );
            assert_eq!(
                depth_read_format(format),
                None,
                "{format:?} is not a depth format"
            );
        }
    }

    /// The pairs, pinned to DXGI's own `_SRGB` constants.
    ///
    /// Two assertions, and the first is the one that catches a dropped encode: a
    /// linear and an sRGB format that map to the *same* DXGI format render too
    /// dark and nothing else notices.
    #[test]
    fn srgb_pairs_map_to_dxgis_srgb_formats() {
        assert!(!SRGB_PAIRS.is_empty(), "nothing to check");
        for &(linear, srgb) in SRGB_PAIRS {
            assert!(!linear.is_srgb() && srgb.is_srgb(), "{linear:?}/{srgb:?}");
            assert_ne!(
                dxgi_format(linear),
                dxgi_format(srgb),
                "the sRGB encode vanished for {linear:?}/{srgb:?}"
            );
        }
        assert_eq!(
            dxgi_format(Format::Rgba8UnormSrgb),
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        );
        assert_eq!(
            dxgi_format(Format::Bgra8UnormSrgb),
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        );
        assert_eq!(
            dxgi_format(Format::Bc7RgbaUnormSrgb),
            DXGI_FORMAT_BC7_UNORM_SRGB
        );
    }

    /// The engine's two named formats, spelled out, because every golden image
    /// depends on them.
    #[test]
    fn the_engines_hdr_and_depth_formats_are_the_dxgi_ones_expected() {
        assert_eq!(
            dxgi_format(Format::Rgba16Float),
            DXGI_FORMAT_R16G16B16A16_FLOAT
        );
        assert_eq!(dxgi_format(Format::D32Float), DXGI_FORMAT_D32_FLOAT);
        // The packed HDR intermediate, and its lookalike neighbour.
        assert_eq!(
            dxgi_format(Format::R11g11b10Float),
            DXGI_FORMAT_R11G11B10_FLOAT
        );
        assert_ne!(
            dxgi_format(Format::R11g11b10Float),
            DXGI_FORMAT_R9G9B9E5_SHAREDEXP
        );
    }

    /// The heap and the initial state move together, because D3D12 rejects the
    /// pairings this table exists to keep straight: `GENERIC_READ` is the only
    /// legal start on the upload heap and `COPY_DEST` the only one on readback.
    #[test]
    fn each_heap_gets_the_only_initial_state_d3d12_accepts_for_it() {
        assert!(!LOCATIONS.is_empty(), "nothing to check");
        let mut heaps: Vec<D3D12_HEAP_TYPE> = Vec::new();
        for &location in LOCATIONS {
            let heap = heap_type(location);
            assert!(
                !heaps.contains(&heap),
                "{location:?} shares a heap with an earlier location"
            );
            heaps.push(heap);
            // A mappable location is exactly a non-default heap, which is the
            // invariant `write_buffer` relies on to reach `Map`.
            assert_eq!(
                location.is_mappable(),
                heap != D3D12_HEAP_TYPE_DEFAULT,
                "{location:?}"
            );
        }
        assert_eq!(
            initial_state(MemoryLocation::HostUpload),
            D3D12_RESOURCE_STATE_GENERIC_READ
        );
        assert_eq!(
            initial_state(MemoryLocation::HostReadback),
            D3D12_RESOURCE_STATE_COPY_DEST
        );
        assert_eq!(
            initial_state(MemoryLocation::DeviceLocal),
            D3D12_RESOURCE_STATE_COMMON
        );
    }

    /// The UAV flag is a default-heap-only flag, and asking for it anywhere else
    /// fails resource creation outright.
    #[test]
    fn a_storage_buffer_takes_the_uav_flag_only_on_the_default_heap() {
        let storage = BufferUsage::STORAGE | BufferUsage::TRANSFER_DST;
        assert_eq!(
            buffer_flags(storage, MemoryLocation::DeviceLocal),
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS
        );
        for &location in &[MemoryLocation::HostUpload, MemoryLocation::HostReadback] {
            assert_eq!(
                buffer_flags(storage, location),
                D3D12_RESOURCE_FLAG_NONE,
                "{location:?} rejects ALLOW_UNORDERED_ACCESS"
            );
        }
        assert_eq!(
            buffer_flags(BufferUsage::INDEX, MemoryLocation::DeviceLocal),
            D3D12_RESOURCE_FLAG_NONE
        );
    }

    /// Attachment and storage usage each reach their own flag, and sampling
    /// reaches none — D3D12 states the absence, not the presence.
    #[test]
    fn image_flags_follow_the_attachment_and_storage_usages_only() {
        assert_eq!(
            image_flags(ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST),
            D3D12_RESOURCE_FLAG_NONE,
            "a sampled image needs no flag; DENY_SHADER_RESOURCE is the flag D3D12 has"
        );
        assert!(
            image_flags(ImageUsage::COLOR_ATTACHMENT)
                .contains(D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET)
        );
        assert!(
            image_flags(ImageUsage::DEPTH_STENCIL_ATTACHMENT)
                .contains(D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL)
        );
        assert!(
            image_flags(ImageUsage::STORAGE).contains(D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS)
        );
        // A colour attachment must not pick up the depth flag: D3D12 rejects a
        // resource carrying both.
        assert!(
            !image_flags(ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED)
                .contains(D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL)
        );
    }

    /// Every filter combination reaches its own constant, and the comparison
    /// twin is never the same value as the plain one.
    ///
    /// The duplicate check is what a transposed arm trips: two combinations
    /// sharing a `D3D12_FILTER` is a sampler that quietly filters the wrong way,
    /// which no later call can detect.
    #[test]
    fn every_filter_combination_reaches_its_own_constant() {
        let modes = [FilterMode::Nearest, FilterMode::Linear];
        let mut seen: Vec<D3D12_FILTER> = Vec::new();
        let mut combinations = 0usize;
        for min in modes {
            for mag in modes {
                for mip in modes {
                    for compare in [None, Some(CompareOp::Greater)] {
                        let desc = SamplerDesc {
                            min_filter: min,
                            mag_filter: mag,
                            mip_filter: mip,
                            compare,
                            ..SamplerDesc::default()
                        };
                        let value = filter(&desc)
                            .unwrap_or_else(|| panic!("{min:?}/{mag:?}/{mip:?} has no filter"));
                        assert!(
                            !seen.contains(&value),
                            "{min:?}/{mag:?}/{mip:?} compare={compare:?} duplicates {value:?}"
                        );
                        seen.push(value);
                        combinations += 1;
                    }
                }
            }
        }
        assert_eq!(seen.len(), combinations, "a combination was skipped");
        // The engine's default sampler, spelled out.
        assert_eq!(
            filter(&SamplerDesc::default()),
            Some(D3D12_FILTER_MIN_MAG_MIP_LINEAR)
        );
    }

    /// Anisotropy is a filter, so it must arrive as one — and must be refused
    /// rather than silently dropped when the other filters contradict it.
    #[test]
    fn anisotropy_is_the_anisotropic_filter_or_no_filter_at_all() {
        let trilinear = SamplerDesc {
            anisotropy: 16.0,
            ..SamplerDesc::default()
        };
        assert_eq!(filter(&trilinear), Some(D3D12_FILTER_ANISOTROPIC));
        assert_eq!(
            filter(&SamplerDesc {
                compare: Some(CompareOp::Greater),
                ..trilinear
            }),
            Some(D3D12_FILTER_COMPARISON_ANISOTROPIC)
        );

        // One point filter anywhere and D3D12 has no name for the request.
        for point in [
            SamplerDesc {
                min_filter: FilterMode::Nearest,
                ..trilinear
            },
            SamplerDesc {
                mag_filter: FilterMode::Nearest,
                ..trilinear
            },
            SamplerDesc {
                mip_filter: FilterMode::Nearest,
                ..trilinear
            },
        ] {
            assert_eq!(
                filter(&point),
                None,
                "point filtering with anisotropy must be refused, not silently dropped"
            );
        }

        // Exactly 1.0 disables anisotropy, and must not tip into the
        // anisotropic filter.
        assert_ne!(
            filter(&SamplerDesc {
                anisotropy: 1.0,
                ..SamplerDesc::default()
            }),
            Some(D3D12_FILTER_ANISOTROPIC)
        );
    }

    /// Reversed-Z is produced above this seam, so the comparison must arrive and
    /// leave with the same name.
    #[test]
    fn d3d12_comparisons_are_not_flipped_for_reversed_z() {
        assert_eq!(
            comparison_func(CompareOp::Greater),
            D3D12_COMPARISON_FUNC_GREATER,
            "a shadow test asking for Greater must not become Less"
        );
        assert_eq!(comparison_func(CompareOp::Less), D3D12_COMPARISON_FUNC_LESS);
        assert_eq!(
            comparison_func(CompareOp::GreaterOrEqual),
            D3D12_COMPARISON_FUNC_GREATER_EQUAL
        );
        assert_eq!(
            comparison_func(CompareOp::LessOrEqual),
            D3D12_COMPARISON_FUNC_LESS_EQUAL
        );
        // `CompareOp::default` is the engine's depth test; if the seam ever
        // changed it, this backend would want to know.
        assert_eq!(
            comparison_func(CompareOp::default()),
            D3D12_COMPARISON_FUNC_GREATER
        );
        // The "no comparison" value belongs to a sampler that does not compare,
        // which never reaches this function.
        for op in [
            CompareOp::Never,
            CompareOp::Less,
            CompareOp::Equal,
            CompareOp::LessOrEqual,
            CompareOp::Greater,
            CompareOp::NotEqual,
            CompareOp::GreaterOrEqual,
            CompareOp::Always,
        ] {
            assert_ne!(comparison_func(op), D3D12_COMPARISON_FUNC_NONE, "{op:?}");
        }
    }

    /// The address modes, including the two D3D12 spells almost alike.
    #[test]
    fn clamp_to_border_is_the_border_mode_not_mirror_once() {
        assert_eq!(
            address_mode(SamplerAddressMode::ClampToBorder),
            D3D12_TEXTURE_ADDRESS_MODE_BORDER
        );
        assert_eq!(
            address_mode(SamplerAddressMode::Repeat),
            D3D12_TEXTURE_ADDRESS_MODE_WRAP
        );
        assert_eq!(
            address_mode(SamplerAddressMode::MirrorRepeat),
            D3D12_TEXTURE_ADDRESS_MODE_MIRROR
        );
        assert_eq!(
            address_mode(SamplerAddressMode::ClampToEdge),
            D3D12_TEXTURE_ADDRESS_MODE_CLAMP
        );
        for mode in [
            SamplerAddressMode::Repeat,
            SamplerAddressMode::MirrorRepeat,
            SamplerAddressMode::ClampToEdge,
            SamplerAddressMode::ClampToBorder,
        ] {
            assert_ne!(
                address_mode(mode),
                D3D12_TEXTURE_ADDRESS_MODE_MIRROR_ONCE,
                "{mode:?}: mirror-once is a behaviour the seam does not have"
            );
        }
    }

    /// **A colour factor is never an alpha factor, and no two seam factors
    /// share a D3D12 one.**
    ///
    /// The colour/alpha pairs are the transposition this exists to catch:
    /// `Src`→`SRC_ALPHA` compiles, blends something plausible, and is wrong
    /// wherever alpha differs from luminance — which is most of a frame and none
    /// of an eyeballed screenshot. Injectivity is asserted over the whole table
    /// rather than pair by pair, so a *new* factor mapped onto an existing one
    /// fails here too.
    #[test]
    fn blend_factors_are_injective_and_keep_colour_apart_from_alpha() {
        const FACTORS: &[BlendFactor] = &[
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
        let mut seen = Vec::new();
        for &factor in FACTORS {
            let mapped = blend_factor(factor);
            assert!(
                !seen.contains(&mapped),
                "{factor:?} maps onto a factor another variant already claimed"
            );
            seen.push(mapped);
        }
        assert_eq!(seen.len(), FACTORS.len());

        assert_eq!(blend_factor(BlendFactor::Src), D3D12_BLEND_SRC_COLOR);
        assert_eq!(blend_factor(BlendFactor::SrcAlpha), D3D12_BLEND_SRC_ALPHA);
        assert_eq!(blend_factor(BlendFactor::Dst), D3D12_BLEND_DEST_COLOR);
        assert_eq!(blend_factor(BlendFactor::DstAlpha), D3D12_BLEND_DEST_ALPHA);
    }

    /// The write mask is built bit by bit, so it does not depend on the seam's
    /// bit order happening to match D3D12's.
    #[test]
    fn the_colour_write_mask_names_each_channel() {
        assert_eq!(color_write_mask(ColorWrites::empty()), 0);
        assert_eq!(color_write_mask(ColorWrites::ALL), 0b1111);
        for (writes, expected) in [
            (ColorWrites::R, 0b0001),
            (ColorWrites::G, 0b0010),
            (ColorWrites::B, 0b0100),
            (ColorWrites::A, 0b1000),
        ] {
            assert_eq!(color_write_mask(writes), expected, "{writes:?}");
        }
    }

    /// A topology's PSO *category* and its command-list value are two answers,
    /// and a list and a strip share the first while differing in the second.
    ///
    /// That agreement is the whole reason both functions exist: a pipeline built
    /// as `TRIANGLE` and a list told `LINELIST` assembles primitives nothing
    /// asked for, and D3D12 reports it only through the debug layer.
    #[test]
    fn a_topology_has_one_pipeline_category_and_its_own_list_value() {
        const TOPOLOGIES: &[PrimitiveTopology] = &[
            PrimitiveTopology::PointList,
            PrimitiveTopology::LineList,
            PrimitiveTopology::LineStrip,
            PrimitiveTopology::TriangleList,
            PrimitiveTopology::TriangleStrip,
        ];
        let mut seen = Vec::new();
        for &topology in TOPOLOGIES {
            let value = primitive_topology(topology);
            assert!(
                !seen.contains(&value),
                "{topology:?} shares a command-list topology with another variant"
            );
            seen.push(value);
        }
        assert_eq!(
            primitive_topology_type(PrimitiveTopology::TriangleList),
            primitive_topology_type(PrimitiveTopology::TriangleStrip),
            "a list and a strip are one pipeline category"
        );
        assert_ne!(
            primitive_topology(PrimitiveTopology::TriangleList),
            primitive_topology(PrimitiveTopology::TriangleStrip),
            "and two command-list topologies"
        );
        assert_eq!(
            primitive_topology_type(PrimitiveTopology::PointList),
            D3D12_PRIMITIVE_TOPOLOGY_TYPE_POINT
        );
        assert_eq!(
            primitive_topology_type(PrimitiveTopology::LineStrip),
            D3D12_PRIMITIVE_TOPOLOGY_TYPE_LINE
        );
    }

    /// A sampler has **no** CBV/SRV/UAV range type, which is what keeps a
    /// descriptor table from mixing heap types by construction.
    #[test]
    fn a_sampler_has_no_view_range_and_writability_picks_srv_or_uav() {
        assert_eq!(
            descriptor_range_type(BindingKind::Sampler { comparison: false }),
            None
        );
        assert_eq!(
            descriptor_range_type(BindingKind::UniformBuffer { dynamic: false }),
            Some(D3D12_DESCRIPTOR_RANGE_TYPE_CBV)
        );
        assert_eq!(
            descriptor_range_type(BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            }),
            Some(D3D12_DESCRIPTOR_RANGE_TYPE_SRV)
        );
        for (read_only, expected) in [
            (true, D3D12_DESCRIPTOR_RANGE_TYPE_SRV),
            (false, D3D12_DESCRIPTOR_RANGE_TYPE_UAV),
        ] {
            assert_eq!(
                descriptor_range_type(BindingKind::StorageBuffer {
                    read_only,
                    dynamic: false
                }),
                Some(expected),
                "storage buffer read_only={read_only}"
            );
            assert_eq!(
                descriptor_range_type(BindingKind::StorageImage {
                    read_only,
                    view_type: ImageViewType::D2,
                    format: Format::Rgba8Unorm,
                }),
                Some(expected),
                "storage image read_only={read_only}"
            );
        }
    }

    /// Visibility widens rather than narrows: one stage keeps its own value, and
    /// anything else becomes `ALL`.
    ///
    /// Narrowing is the failure that matters — a root parameter visible to the
    /// vertex stage alone makes a fragment shader's read of the same set a
    /// device removal, where a wider one only costs the driver an optimisation.
    ///
    /// **Every stage the seam has is named, and each maps to a different D3D12
    /// value.** The two mesh ones are the arms that did not exist before the
    /// mesh slice, and the falsifying edit is deleting either: the stage falls
    /// into `ALL`, the root signature still serialises, and nothing anywhere
    /// says the parameter is visible to five stages the caller did not ask for.
    #[test]
    fn shader_visibility_widens_and_never_narrows() {
        let single = [
            (ShaderStages::VERTEX, D3D12_SHADER_VISIBILITY_VERTEX),
            (ShaderStages::FRAGMENT, D3D12_SHADER_VISIBILITY_PIXEL),
            (ShaderStages::MESH, D3D12_SHADER_VISIBILITY_MESH),
            (ShaderStages::TASK, D3D12_SHADER_VISIBILITY_AMPLIFICATION),
        ];
        for (stages, expected) in single {
            assert_eq!(
                shader_visibility(stages),
                expected,
                "{stages:?} is one stage and D3D12 has a value for it"
            );
        }
        // Distinct from each other as well as from `ALL`: a table of four arms
        // that all answered the same constant would pass every assertion above.
        for (index, (_, mapped)) in single.iter().enumerate() {
            assert_ne!(*mapped, D3D12_SHADER_VISIBILITY_ALL, "{index}");
            for (_, other) in &single[..index] {
                assert_ne!(mapped, other, "two stages share one visibility: {index}");
            }
        }
        for stages in [
            ShaderStages::GRAPHICS,
            ShaderStages::ALL,
            ShaderStages::COMPUTE,
            ShaderStages::MESH | ShaderStages::TASK,
            ShaderStages::empty(),
        ] {
            assert_eq!(
                shader_visibility(stages),
                D3D12_SHADER_VISIBILITY_ALL,
                "{stages:?} is not one stage D3D12 has a value for, so it must widen to ALL"
            );
        }
    }

    /// Dimensionality, one D3D12 constant per seam variant.
    #[test]
    fn image_types_are_distinct_resource_dimensions() {
        assert_eq!(
            resource_dimension(ImageType::D1),
            D3D12_RESOURCE_DIMENSION_TEXTURE1D
        );
        assert_eq!(
            resource_dimension(ImageType::D2),
            D3D12_RESOURCE_DIMENSION_TEXTURE2D
        );
        assert_eq!(
            resource_dimension(ImageType::D3),
            D3D12_RESOURCE_DIMENSION_TEXTURE3D
        );
    }
}
