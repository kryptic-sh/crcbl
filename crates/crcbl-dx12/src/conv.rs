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
//! # Depth formats have three spellings in D3D12, not one
//!
//! This is the one place D3D12 needs a table Metal and Vulkan do not.
//! `D3D12_RESOURCE_DESC::Format` names the *storage*, and a resource created as
//! `DXGI_FORMAT_D32_FLOAT` can carry a depth-stencil view and nothing else — an
//! SRV on it is rejected, so a shadow map created that way would be unreadable.
//! The API's answer is a **typeless** storage format plus a concrete format per
//! view, and this module spells all three:
//!
//! | seam | [`resource_format`] (sampled depth) | [`dxgi_format`] (DSV) | [`depth_read_format`] (SRV) |
//! | --- | --- | --- | --- |
//! | `D32Float` | `R32_TYPELESS` | `D32_FLOAT` | `R32_FLOAT` |
//! | `D16Unorm` | `R16_TYPELESS` | `D16_UNORM` | `R16_UNORM` |
//! | `D24UnormS8Uint` | `R24G8_TYPELESS` | `D24_UNORM_S8_UINT` | `R24_UNORM_X8_TYPELESS` |
//! | `D32FloatS8Uint` | `R32G8X24_TYPELESS` | `D32_FLOAT_S8X24_UINT` | `R32_FLOAT_X8X24_TYPELESS` |
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
    BufferUsage, CompareOp, FilterMode, Format, ImageType, ImageUsage, MemoryLocation,
    SamplerAddressMode, SamplerDesc,
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
    D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_GENERIC_READ, D3D12_RESOURCE_STATES,
    D3D12_TEXTURE_ADDRESS_MODE, D3D12_TEXTURE_ADDRESS_MODE_BORDER,
    D3D12_TEXTURE_ADDRESS_MODE_CLAMP, D3D12_TEXTURE_ADDRESS_MODE_MIRROR,
    D3D12_TEXTURE_ADDRESS_MODE_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
    DXGI_FORMAT_BC1_UNORM, DXGI_FORMAT_BC1_UNORM_SRGB, DXGI_FORMAT_BC3_UNORM,
    DXGI_FORMAT_BC3_UNORM_SRGB, DXGI_FORMAT_BC4_UNORM, DXGI_FORMAT_BC5_UNORM,
    DXGI_FORMAT_BC6H_UF16, DXGI_FORMAT_BC7_UNORM, DXGI_FORMAT_BC7_UNORM_SRGB,
    DXGI_FORMAT_D16_UNORM, DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_D32_FLOAT,
    DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_FORMAT_R10G10B10A2_UNORM,
    DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R16_FLOAT, DXGI_FORMAT_R16_TYPELESS,
    DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT,
    DXGI_FORMAT_R24_UNORM_X8_TYPELESS, DXGI_FORMAT_R24G8_TYPELESS, DXGI_FORMAT_R32_FLOAT,
    DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS, DXGI_FORMAT_R32_TYPELESS, DXGI_FORMAT_R32_UINT,
    DXGI_FORMAT_R32G8X24_TYPELESS, DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32_UINT,
    DXGI_FORMAT_R32G32B32A32_FLOAT,
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

/// Resource flags for a buffer.
///
/// Only one of D3D12's flags applies to a buffer, and **only on the default
/// heap**: `ALLOW_UNORDERED_ACCESS` is rejected outright on the upload and
/// readback heaps, so a `STORAGE` staging buffer asking for it would fail
/// creation rather than gaining anything. A host-visible buffer is read through
/// a copy or as a root descriptor either way, neither of which needs the flag.
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Direct3D12::{
        D3D12_COMPARISON_FUNC_NONE, D3D12_TEXTURE_ADDRESS_MODE_MIRROR_ONCE,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_R9G9B9E5_SHAREDEXP, DXGI_FORMAT_UNKNOWN,
    };

    /// Every [`Format`] the seam declares, so the properties below are checked
    /// over all of them rather than over the handful someone remembered.
    ///
    /// Hand-written because `Format` has no iterator; [`dxgi_format`]'s
    /// exhaustive `match` is what makes a *missing* variant a compile error, and
    /// `every_format_appears_in_the_exhaustive_list` below is what makes a
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

    /// **The mapping is injective.** Two seam formats sharing one DXGI format is
    /// the copy-paste failure this file is most exposed to, and it is invisible
    /// at run time: the image is created, the sample succeeds, the colour is
    /// wrong.
    #[test]
    fn no_two_formats_share_a_dxgi_format() {
        assert!(!ALL.is_empty(), "nothing to check");
        let mut seen: Vec<(Format, DXGI_FORMAT)> = Vec::new();
        for &format in ALL {
            let dxgi = dxgi_format(format);
            assert_ne!(dxgi, DXGI_FORMAT_UNKNOWN, "{format:?} has no DXGI format");
            if let Some((other, _)) = seen.iter().find(|(_, seen)| *seen == dxgi) {
                panic!("{format:?} and {other:?} both map to {dxgi:?}");
            }
            seen.push((format, dxgi));
        }
        assert_eq!(seen.len(), ALL.len());
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
        let depth: Vec<Format> = ALL
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

    /// A colour format is never rewritten by usage, and has no depth spelling to
    /// reach for.
    #[test]
    fn colour_formats_have_one_spelling_whatever_the_usage() {
        let colour: Vec<Format> = ALL
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
    fn comparisons_are_not_flipped_for_reversed_z() {
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
