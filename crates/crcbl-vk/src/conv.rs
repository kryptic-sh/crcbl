//! Seam vocabulary ↔ Vulkan enums.
//!
//! Every function here is **pure**: no device, no loader, no `unsafe`. That is
//! deliberate, because it is what makes the interesting half of a backend
//! testable without a GPU in the room — format selection, present-mode choice,
//! the extent rule and the barrier expansion are all decisions, and decisions
//! deserve unit tests. The parts that genuinely need a driver live in
//! `tests/vk_e2e.rs` behind the `vk-e2e` feature.
//!
//! # `ResourceState` is expanded here, once
//!
//! `crcbl-hal`'s [`ResourceState`] is a small closed set of *uses*; Vulkan wants
//! a `sync2` stage mask, an access mask and an image layout. The seam's docs
//! call the resulting over-synchronisation "the known place this seam gives up
//! performance", and this module is where that trade is actually made — one
//! table, not a decision scattered across the encoder.

use ash::vk;

use crcbl_hal::{
    CompositeAlpha, Format, ImageAspect, ImageSubresourceRange, PresentMode, ResourceState,
};

/// Maps a seam format onto a Vulkan format.
///
/// Total, because [`Format`] is deliberately not `#[non_exhaustive]` — adding a
/// format must break this `match` at compile time rather than fall into a
/// `_ => UNDEFINED` arm three backends deep.
#[must_use]
pub fn format(format: Format) -> vk::Format {
    match format {
        Format::R8Unorm => vk::Format::R8_UNORM,
        Format::Rg8Unorm => vk::Format::R8G8_UNORM,
        Format::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
        Format::Rgba8UnormSrgb => vk::Format::R8G8B8A8_SRGB,
        Format::Bgra8Unorm => vk::Format::B8G8R8A8_UNORM,
        Format::Bgra8UnormSrgb => vk::Format::B8G8R8A8_SRGB,
        Format::Rgb10a2Unorm => vk::Format::A2B10G10R10_UNORM_PACK32,
        Format::R11g11b10Float => vk::Format::B10G11R11_UFLOAT_PACK32,
        Format::R16Float => vk::Format::R16_SFLOAT,
        Format::Rg16Float => vk::Format::R16G16_SFLOAT,
        Format::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
        Format::R32Float => vk::Format::R32_SFLOAT,
        Format::Rg32Float => vk::Format::R32G32_SFLOAT,
        Format::Rgba32Float => vk::Format::R32G32B32A32_SFLOAT,
        Format::R32Uint => vk::Format::R32_UINT,
        Format::Rg32Uint => vk::Format::R32G32_UINT,
        Format::D32Float => vk::Format::D32_SFLOAT,
        Format::D32FloatS8Uint => vk::Format::D32_SFLOAT_S8_UINT,
        Format::D24UnormS8Uint => vk::Format::D24_UNORM_S8_UINT,
        Format::D16Unorm => vk::Format::D16_UNORM,
        Format::Bc1RgbaUnorm => vk::Format::BC1_RGBA_UNORM_BLOCK,
        Format::Bc1RgbaUnormSrgb => vk::Format::BC1_RGBA_SRGB_BLOCK,
        Format::Bc3RgbaUnorm => vk::Format::BC3_UNORM_BLOCK,
        Format::Bc3RgbaUnormSrgb => vk::Format::BC3_SRGB_BLOCK,
        Format::Bc4RUnorm => vk::Format::BC4_UNORM_BLOCK,
        Format::Bc5RgUnorm => vk::Format::BC5_UNORM_BLOCK,
        Format::Bc6hRgbUfloat => vk::Format::BC6H_UFLOAT_BLOCK,
        Format::Bc7RgbaUnorm => vk::Format::BC7_UNORM_BLOCK,
        Format::Bc7RgbaUnormSrgb => vk::Format::BC7_SRGB_BLOCK,
    }
}

/// Maps a Vulkan format back onto a seam format, if the seam has one.
///
/// `None` is the honest answer for the long tail a surface may offer and the
/// engine does not model; `VkInstance`'s `surface_caps` filters those out
/// rather than inventing a nearest match, because a silently substituted format
/// is a colour-space bug that shows up as "slightly wrong on one backend".
#[must_use]
pub fn format_from_vk(raw: vk::Format) -> Option<Format> {
    let mapped = match raw {
        vk::Format::R8_UNORM => Format::R8Unorm,
        vk::Format::R8G8_UNORM => Format::Rg8Unorm,
        vk::Format::R8G8B8A8_UNORM => Format::Rgba8Unorm,
        vk::Format::R8G8B8A8_SRGB => Format::Rgba8UnormSrgb,
        vk::Format::B8G8R8A8_UNORM => Format::Bgra8Unorm,
        vk::Format::B8G8R8A8_SRGB => Format::Bgra8UnormSrgb,
        vk::Format::A2B10G10R10_UNORM_PACK32 => Format::Rgb10a2Unorm,
        vk::Format::B10G11R11_UFLOAT_PACK32 => Format::R11g11b10Float,
        vk::Format::R16_SFLOAT => Format::R16Float,
        vk::Format::R16G16_SFLOAT => Format::Rg16Float,
        vk::Format::R16G16B16A16_SFLOAT => Format::Rgba16Float,
        vk::Format::R32_SFLOAT => Format::R32Float,
        vk::Format::R32G32_SFLOAT => Format::Rg32Float,
        vk::Format::R32G32B32A32_SFLOAT => Format::Rgba32Float,
        vk::Format::R32_UINT => Format::R32Uint,
        vk::Format::R32G32_UINT => Format::Rg32Uint,
        vk::Format::D32_SFLOAT => Format::D32Float,
        vk::Format::D32_SFLOAT_S8_UINT => Format::D32FloatS8Uint,
        vk::Format::D24_UNORM_S8_UINT => Format::D24UnormS8Uint,
        vk::Format::D16_UNORM => Format::D16Unorm,
        vk::Format::BC1_RGBA_UNORM_BLOCK => Format::Bc1RgbaUnorm,
        vk::Format::BC1_RGBA_SRGB_BLOCK => Format::Bc1RgbaUnormSrgb,
        vk::Format::BC3_UNORM_BLOCK => Format::Bc3RgbaUnorm,
        vk::Format::BC3_SRGB_BLOCK => Format::Bc3RgbaUnormSrgb,
        vk::Format::BC4_UNORM_BLOCK => Format::Bc4RUnorm,
        vk::Format::BC5_UNORM_BLOCK => Format::Bc5RgUnorm,
        vk::Format::BC6H_UFLOAT_BLOCK => Format::Bc6hRgbUfloat,
        vk::Format::BC7_UNORM_BLOCK => Format::Bc7RgbaUnorm,
        vk::Format::BC7_SRGB_BLOCK => Format::Bc7RgbaUnormSrgb,
        _ => return None,
    };
    Some(mapped)
}

/// Maps a seam present mode onto a Vulkan one.
#[must_use]
pub fn present_mode(mode: PresentMode) -> vk::PresentModeKHR {
    match mode {
        PresentMode::Fifo => vk::PresentModeKHR::FIFO,
        PresentMode::FifoRelaxed => vk::PresentModeKHR::FIFO_RELAXED,
        PresentMode::Mailbox => vk::PresentModeKHR::MAILBOX,
        PresentMode::Immediate => vk::PresentModeKHR::IMMEDIATE,
    }
}

/// Maps a Vulkan present mode back, if the seam models it.
#[must_use]
pub fn present_mode_from_vk(raw: vk::PresentModeKHR) -> Option<PresentMode> {
    match raw {
        vk::PresentModeKHR::FIFO => Some(PresentMode::Fifo),
        vk::PresentModeKHR::FIFO_RELAXED => Some(PresentMode::FifoRelaxed),
        vk::PresentModeKHR::MAILBOX => Some(PresentMode::Mailbox),
        vk::PresentModeKHR::IMMEDIATE => Some(PresentMode::Immediate),
        // The shared-present modes from `VK_KHR_shared_presentable_image`; the
        // seam has no vocabulary for "the compositor reads while you write".
        _ => None,
    }
}

/// Maps a seam composite-alpha mode onto a Vulkan flag.
#[must_use]
pub fn composite_alpha(alpha: CompositeAlpha) -> vk::CompositeAlphaFlagsKHR {
    match alpha {
        CompositeAlpha::Opaque => vk::CompositeAlphaFlagsKHR::OPAQUE,
        CompositeAlpha::PreMultiplied => vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        CompositeAlpha::PostMultiplied => vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        CompositeAlpha::Inherit => vk::CompositeAlphaFlagsKHR::INHERIT,
    }
}

/// Every seam composite-alpha mode a Vulkan flag set contains.
#[must_use]
pub fn composite_alpha_from_vk(flags: vk::CompositeAlphaFlagsKHR) -> Vec<CompositeAlpha> {
    [
        CompositeAlpha::Opaque,
        CompositeAlpha::PreMultiplied,
        CompositeAlpha::PostMultiplied,
        CompositeAlpha::Inherit,
    ]
    .into_iter()
    .filter(|mode| flags.contains(composite_alpha(*mode)))
    .collect()
}

/// Maps seam aspect flags onto Vulkan ones.
#[must_use]
pub fn aspect(aspect: ImageAspect) -> vk::ImageAspectFlags {
    let mut flags = vk::ImageAspectFlags::empty();
    if aspect.contains(ImageAspect::COLOR) {
        flags |= vk::ImageAspectFlags::COLOR;
    }
    if aspect.contains(ImageAspect::DEPTH) {
        flags |= vk::ImageAspectFlags::DEPTH;
    }
    if aspect.contains(ImageAspect::STENCIL) {
        flags |= vk::ImageAspectFlags::STENCIL;
    }
    flags
}

/// Maps a seam subresource range onto a Vulkan one.
///
/// [`ImageSubresourceRange::ALL`] is `u32::MAX`, which is exactly Vulkan's
/// `VK_REMAINING_MIP_LEVELS` / `VK_REMAINING_ARRAY_LAYERS`, so the sentinel
/// passes through unchanged — one of the few places the two vocabularies agree
/// bit for bit.
pub fn subresource_range(range: ImageSubresourceRange) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: aspect(range.aspect),
        base_mip_level: range.base_mip,
        level_count: if range.mip_count == ImageSubresourceRange::ALL {
            vk::REMAINING_MIP_LEVELS
        } else {
            range.mip_count
        },
        base_array_layer: range.base_layer,
        layer_count: if range.layer_count == ImageSubresourceRange::ALL {
            vk::REMAINING_ARRAY_LAYERS
        } else {
            range.layer_count
        },
    }
}

/// Maps seam buffer usage onto Vulkan usage flags.
///
/// `TRANSFER_DST` is added unconditionally: every buffer in this engine is
/// eventually written by a staging copy or a `fill_buffer`, and the flag costs
/// nothing on any driver. That is also what covers
/// [`BufferUsage::QUERY_RESOLVE`](crcbl_hal::BufferUsage::QUERY_RESOLVE), which
/// has no arm of its own below — `vkCmdCopyQueryPoolResults` requires exactly
/// `TRANSFER_DST` and nothing more, so the flag is already present. Stated
/// rather than left as a silently-ignored variant.
///
/// `DEVICE_ADDRESS` additionally implies `SHADER_DEVICE_ADDRESS`, which the
/// device must have enabled.
#[must_use]
pub fn buffer_usage(usage: crcbl_hal::BufferUsage) -> vk::BufferUsageFlags {
    use crcbl_hal::BufferUsage as U;
    let mut flags = vk::BufferUsageFlags::TRANSFER_DST;
    if usage.contains(U::TRANSFER_SRC) {
        flags |= vk::BufferUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(U::UNIFORM) {
        flags |= vk::BufferUsageFlags::UNIFORM_BUFFER;
    }
    if usage.contains(U::STORAGE) {
        flags |= vk::BufferUsageFlags::STORAGE_BUFFER;
    }
    if usage.contains(U::INDEX) {
        flags |= vk::BufferUsageFlags::INDEX_BUFFER;
    }
    if usage.contains(U::INDIRECT) {
        flags |= vk::BufferUsageFlags::INDIRECT_BUFFER;
    }
    if usage.contains(U::DEVICE_ADDRESS) {
        flags |= vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;
    }
    flags
}

/// Maps seam image usage onto Vulkan usage flags.
#[must_use]
pub fn image_usage(usage: crcbl_hal::ImageUsage) -> vk::ImageUsageFlags {
    use crcbl_hal::ImageUsage as U;
    let mut flags = vk::ImageUsageFlags::empty();
    if usage.contains(U::TRANSFER_SRC) {
        flags |= vk::ImageUsageFlags::TRANSFER_SRC;
    }
    if usage.contains(U::TRANSFER_DST) {
        flags |= vk::ImageUsageFlags::TRANSFER_DST;
    }
    if usage.contains(U::SAMPLED) {
        flags |= vk::ImageUsageFlags::SAMPLED;
    }
    if usage.contains(U::STORAGE) {
        flags |= vk::ImageUsageFlags::STORAGE;
    }
    if usage.contains(U::COLOR_ATTACHMENT) {
        flags |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
    }
    if usage.contains(U::DEPTH_STENCIL_ATTACHMENT) {
        flags |= vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT;
    }
    // `PRESENT` has no Vulkan usage flag: presentability is a property of the
    // swapchain that owns the image, not of the image's usage mask.
    flags
}

/// Maps a seam image type onto a Vulkan one.
#[must_use]
pub fn image_type(kind: crcbl_hal::ImageType) -> vk::ImageType {
    match kind {
        crcbl_hal::ImageType::D1 => vk::ImageType::TYPE_1D,
        crcbl_hal::ImageType::D2 => vk::ImageType::TYPE_2D,
        crcbl_hal::ImageType::D3 => vk::ImageType::TYPE_3D,
    }
}

/// Maps a seam view type onto a Vulkan one.
#[must_use]
pub fn image_view_type(kind: crcbl_hal::ImageViewType) -> vk::ImageViewType {
    match kind {
        crcbl_hal::ImageViewType::D1 => vk::ImageViewType::TYPE_1D,
        crcbl_hal::ImageViewType::D2 => vk::ImageViewType::TYPE_2D,
        crcbl_hal::ImageViewType::D2Array => vk::ImageViewType::TYPE_2D_ARRAY,
        crcbl_hal::ImageViewType::Cube => vk::ImageViewType::CUBE,
        crcbl_hal::ImageViewType::CubeArray => vk::ImageViewType::CUBE_ARRAY,
        crcbl_hal::ImageViewType::D3 => vk::ImageViewType::TYPE_3D,
    }
}

/// Maps a seam load op onto a Vulkan attachment load op.
#[must_use]
pub fn load_op(op: crcbl_hal::LoadOp) -> vk::AttachmentLoadOp {
    match op {
        crcbl_hal::LoadOp::Load => vk::AttachmentLoadOp::LOAD,
        crcbl_hal::LoadOp::Clear => vk::AttachmentLoadOp::CLEAR,
        crcbl_hal::LoadOp::DontCare => vk::AttachmentLoadOp::DONT_CARE,
    }
}

/// Maps a seam store op onto a Vulkan attachment store op.
#[must_use]
pub fn store_op(op: crcbl_hal::StoreOp) -> vk::AttachmentStoreOp {
    match op {
        crcbl_hal::StoreOp::Store => vk::AttachmentStoreOp::STORE,
        crcbl_hal::StoreOp::Discard => vk::AttachmentStoreOp::DONT_CARE,
    }
}

/// Maps a seam subresource-layers selection onto a Vulkan one.
pub fn subresource_layers(layers: crcbl_hal::ImageSubresourceLayers) -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers {
        aspect_mask: aspect(layers.aspect),
        mip_level: layers.mip,
        base_array_layer: layers.base_layer,
        layer_count: layers.layer_count,
    }
}

// --- pipeline state ------------------------------------------------------
//
// Everything below is milestone 2's vocabulary. It is here rather than in
// `pipeline.rs` for the reason the module docs give: these are *decisions*, and
// keeping them beside the other decisions is what keeps them all unit-tested
// against a table rather than spot-checked against whichever value a test
// happened to use.

/// Maps a seam comparison onto a Vulkan one.
///
/// The seam names these by the comparison performed, never by what it means for
/// visibility, so this is the one mapping in the file that is a pure rename —
/// and it must stay one. `CompareOp::Greater` is the engine's reversed-Z depth
/// test; turning it into anything but `GREATER` here would silently invert the
/// depth convention for every pipeline at once.
#[must_use]
pub fn compare_op(op: crcbl_hal::CompareOp) -> vk::CompareOp {
    use crcbl_hal::CompareOp as C;
    match op {
        C::Never => vk::CompareOp::NEVER,
        C::Less => vk::CompareOp::LESS,
        C::Equal => vk::CompareOp::EQUAL,
        C::LessOrEqual => vk::CompareOp::LESS_OR_EQUAL,
        C::Greater => vk::CompareOp::GREATER,
        C::NotEqual => vk::CompareOp::NOT_EQUAL,
        C::GreaterOrEqual => vk::CompareOp::GREATER_OR_EQUAL,
        C::Always => vk::CompareOp::ALWAYS,
    }
}

/// Maps a seam stencil operation onto a Vulkan one.
#[must_use]
pub fn stencil_op(op: crcbl_hal::StencilOp) -> vk::StencilOp {
    use crcbl_hal::StencilOp as S;
    match op {
        S::Keep => vk::StencilOp::KEEP,
        S::Zero => vk::StencilOp::ZERO,
        S::Replace => vk::StencilOp::REPLACE,
        S::Invert => vk::StencilOp::INVERT,
        S::IncrementClamp => vk::StencilOp::INCREMENT_AND_CLAMP,
        S::DecrementClamp => vk::StencilOp::DECREMENT_AND_CLAMP,
        S::IncrementWrap => vk::StencilOp::INCREMENT_AND_WRAP,
        S::DecrementWrap => vk::StencilOp::DECREMENT_AND_WRAP,
    }
}

/// Builds one facing's Vulkan stencil state.
///
/// The masks and the reference live on [`StencilState`](crcbl_hal::StencilState)
/// rather than per face, because no API this engine targets lets the two facings
/// disagree about them.
pub fn stencil_face(
    face: crcbl_hal::StencilFaceState,
    state: crcbl_hal::StencilState,
) -> vk::StencilOpState {
    vk::StencilOpState {
        fail_op: stencil_op(face.fail_op),
        pass_op: stencil_op(face.pass_op),
        depth_fail_op: stencil_op(face.depth_fail_op),
        compare_op: compare_op(face.compare),
        compare_mask: state.read_mask,
        write_mask: state.write_mask,
        // The seam has no pipeline-side reference — it is pass state, and every
        // pipeline this backend builds declares
        // `VK_DYNAMIC_STATE_STENCIL_REFERENCE`, so whatever is written here is
        // replaced before the first draw. Zero rather than a sentinel: a value
        // that never reaches the GPU should not read as a choice.
        reference: 0,
    }
}

/// Maps a seam blend factor onto a Vulkan one.
#[must_use]
pub fn blend_factor(factor: crcbl_hal::BlendFactor) -> vk::BlendFactor {
    use crcbl_hal::BlendFactor as B;
    match factor {
        B::Zero => vk::BlendFactor::ZERO,
        B::One => vk::BlendFactor::ONE,
        B::Src => vk::BlendFactor::SRC_COLOR,
        B::OneMinusSrc => vk::BlendFactor::ONE_MINUS_SRC_COLOR,
        B::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
        B::OneMinusSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        B::Dst => vk::BlendFactor::DST_COLOR,
        B::OneMinusDst => vk::BlendFactor::ONE_MINUS_DST_COLOR,
        B::DstAlpha => vk::BlendFactor::DST_ALPHA,
        B::OneMinusDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
    }
}

/// Maps a seam blend operation onto a Vulkan one.
#[must_use]
pub fn blend_op(op: crcbl_hal::BlendOp) -> vk::BlendOp {
    use crcbl_hal::BlendOp as B;
    match op {
        B::Add => vk::BlendOp::ADD,
        B::Subtract => vk::BlendOp::SUBTRACT,
        B::ReverseSubtract => vk::BlendOp::REVERSE_SUBTRACT,
        B::Min => vk::BlendOp::MIN,
        B::Max => vk::BlendOp::MAX,
    }
}

/// Maps seam colour-write flags onto Vulkan colour components.
#[must_use]
pub fn color_writes(writes: crcbl_hal::ColorWrites) -> vk::ColorComponentFlags {
    use crcbl_hal::ColorWrites as W;
    let mut flags = vk::ColorComponentFlags::empty();
    if writes.contains(W::R) {
        flags |= vk::ColorComponentFlags::R;
    }
    if writes.contains(W::G) {
        flags |= vk::ColorComponentFlags::G;
    }
    if writes.contains(W::B) {
        flags |= vk::ColorComponentFlags::B;
    }
    if writes.contains(W::A) {
        flags |= vk::ColorComponentFlags::A;
    }
    flags
}

/// Maps a seam topology onto a Vulkan one.
#[must_use]
pub fn topology(topology: crcbl_hal::PrimitiveTopology) -> vk::PrimitiveTopology {
    use crcbl_hal::PrimitiveTopology as T;
    match topology {
        T::PointList => vk::PrimitiveTopology::POINT_LIST,
        T::LineList => vk::PrimitiveTopology::LINE_LIST,
        T::LineStrip => vk::PrimitiveTopology::LINE_STRIP,
        T::TriangleList => vk::PrimitiveTopology::TRIANGLE_LIST,
        T::TriangleStrip => vk::PrimitiveTopology::TRIANGLE_STRIP,
    }
}

/// Maps a seam winding onto a Vulkan front face.
///
/// # The viewport flip changes what this means, and it is not this function's
/// job to compensate
///
/// The encoder submits a **negative-height viewport** so the seam's Y-up
/// convention survives Vulkan's inverted clip space. Front-facing determination
/// happens in framebuffer space, *after* that transform, so a negative height
/// reverses the observed winding of every triangle. `Ccw` therefore reaches the
/// rasteriser as `COUNTER_CLOCKWISE` and means "counter-clockwise **in the
/// seam's coordinate system**", which is the only definition a caller can
/// author geometry against.
///
/// Flipping it here instead would make `FrontFace::Ccw` mean one thing on
/// Vulkan and another everywhere else. It costs nothing at P1.2, where
/// [`CullMode::None`](crcbl_hal::CullMode) is the default and nothing is
/// discarded; it is the first thing to check when P1.3's depth-tested mesh
/// renders inside-out.
#[must_use]
pub fn front_face(face: crcbl_hal::FrontFace) -> vk::FrontFace {
    match face {
        crcbl_hal::FrontFace::Ccw => vk::FrontFace::COUNTER_CLOCKWISE,
        crcbl_hal::FrontFace::Cw => vk::FrontFace::CLOCKWISE,
    }
}

/// Maps a seam cull mode onto Vulkan cull flags.
#[must_use]
pub fn cull_mode(mode: crcbl_hal::CullMode) -> vk::CullModeFlags {
    match mode {
        crcbl_hal::CullMode::None => vk::CullModeFlags::NONE,
        crcbl_hal::CullMode::Front => vk::CullModeFlags::FRONT,
        crcbl_hal::CullMode::Back => vk::CullModeFlags::BACK,
    }
}

/// Maps a seam fill mode onto a Vulkan polygon mode.
#[must_use]
pub fn polygon_mode(mode: crcbl_hal::PolygonMode) -> vk::PolygonMode {
    match mode {
        crcbl_hal::PolygonMode::Fill => vk::PolygonMode::FILL,
        crcbl_hal::PolygonMode::Line => vk::PolygonMode::LINE,
    }
}

/// Maps a seam filter onto a Vulkan one.
#[must_use]
pub fn filter(mode: crcbl_hal::FilterMode) -> vk::Filter {
    match mode {
        crcbl_hal::FilterMode::Nearest => vk::Filter::NEAREST,
        crcbl_hal::FilterMode::Linear => vk::Filter::LINEAR,
    }
}

/// Maps a seam mip filter onto a Vulkan sampler mipmap mode.
#[must_use]
pub fn mipmap_mode(mode: crcbl_hal::FilterMode) -> vk::SamplerMipmapMode {
    match mode {
        crcbl_hal::FilterMode::Nearest => vk::SamplerMipmapMode::NEAREST,
        crcbl_hal::FilterMode::Linear => vk::SamplerMipmapMode::LINEAR,
    }
}

/// Maps a seam address mode onto a Vulkan one.
///
/// `ClampToBorder` picks **transparent black**, which is what the seam's docs
/// promise; Vulkan makes the border colour a separate enum, and opaque black is
/// the value a shadow atlas must never get.
#[must_use]
pub fn address_mode(mode: crcbl_hal::SamplerAddressMode) -> vk::SamplerAddressMode {
    use crcbl_hal::SamplerAddressMode as A;
    match mode {
        A::Repeat => vk::SamplerAddressMode::REPEAT,
        A::MirrorRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
        A::ClampToEdge => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        A::ClampToBorder => vk::SamplerAddressMode::CLAMP_TO_BORDER,
    }
}

/// Maps seam stage flags onto Vulkan shader stages.
#[must_use]
pub fn shader_stages(stages: crcbl_hal::ShaderStages) -> vk::ShaderStageFlags {
    use crcbl_hal::ShaderStages as S;
    let mut flags = vk::ShaderStageFlags::empty();
    if stages.contains(S::VERTEX) {
        flags |= vk::ShaderStageFlags::VERTEX;
    }
    if stages.contains(S::FRAGMENT) {
        flags |= vk::ShaderStageFlags::FRAGMENT;
    }
    if stages.contains(S::COMPUTE) {
        flags |= vk::ShaderStageFlags::COMPUTE;
    }
    // These two are legal only on a device with `meshShader` / `taskShader`
    // enabled, which is why the seam keeps them out of `GRAPHICS` and `ALL` —
    // see `crcbl_hal::ShaderStages`. Nothing is dropped here: a caller that
    // names them on a device without the capability gets the driver's refusal
    // rather than a silently narrower stage mask.
    if stages.contains(S::MESH) {
        flags |= vk::ShaderStageFlags::MESH_EXT;
    }
    if stages.contains(S::TASK) {
        flags |= vk::ShaderStageFlags::TASK_EXT;
    }
    flags
}

/// Maps a seam binding kind onto a Vulkan descriptor type.
///
/// `read_only` does **not** change the descriptor type: Vulkan has one
/// `STORAGE_BUFFER` and expresses read-only-ness with the shader's
/// `NonWritable` decoration, which comes from the shader rather than from here.
/// The flag still matters — it is what the render graph reads to decide a
/// read→read pair needs no barrier — so it is carried on the layout record and
/// simply has no descriptor-type consequence.
#[must_use]
pub fn descriptor_type(kind: crcbl_hal::BindingKind) -> vk::DescriptorType {
    use crcbl_hal::BindingKind as K;
    match kind {
        K::UniformBuffer { dynamic: false } => vk::DescriptorType::UNIFORM_BUFFER,
        K::UniformBuffer { dynamic: true } => vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
        K::StorageBuffer { dynamic: false, .. } => vk::DescriptorType::STORAGE_BUFFER,
        K::StorageBuffer { dynamic: true, .. } => vk::DescriptorType::STORAGE_BUFFER_DYNAMIC,
        // The `view_type` and the `sample_type` are both dropped: a
        // `VkDescriptorSetLayoutBinding` has neither a dimension field nor a
        // format one, because a `VkImageView` was created with its own
        // `viewType` and `format` and those are what the shader's `OpTypeImage`
        // is matched against. Only WebGPU wants either in the layout.
        K::SampledImage { .. } => vk::DescriptorType::SAMPLED_IMAGE,
        // The `view_type` and the `format` are dropped for the same reason, and
        // the format is the one WebGPU cannot do without: a Vulkan storage image
        // takes its format from the `VkImageView`'s own `format`, which is what
        // the shader's `OpTypeImage` format operand is matched against, and a
        // `VkDescriptorSetLayoutBinding` has nowhere to put either.
        K::StorageImage { .. } => vk::DescriptorType::STORAGE_IMAGE,
        // `comparison` is dropped for the same reason: a Vulkan sampler decides
        // whether it compares at `vkCreateSampler` time, through
        // `compareEnable`/`compareOp` — see `sampler` below, which is where
        // `SamplerDesc::compare` lands. There is one descriptor type either way.
        K::Sampler { .. } => vk::DescriptorType::SAMPLER,
    }
}

/// Maps seam binding flags onto Vulkan descriptor-binding flags.
#[must_use]
pub fn binding_flags(flags: crcbl_hal::BindingFlags) -> vk::DescriptorBindingFlags {
    use crcbl_hal::BindingFlags as F;
    let mut out = vk::DescriptorBindingFlags::empty();
    if flags.contains(F::PARTIALLY_BOUND) {
        out |= vk::DescriptorBindingFlags::PARTIALLY_BOUND;
    }
    if flags.contains(F::UPDATE_AFTER_BIND) {
        out |= vk::DescriptorBindingFlags::UPDATE_AFTER_BIND;
    }
    if flags.contains(F::VARIABLE_COUNT) {
        out |= vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT;
    }
    out
}

/// Maps a sample count onto a Vulkan flag, rejecting a non-power-of-two.
///
/// `SampleCountFlags` is a bitmask whose bits *are* the counts, so
/// `from_raw(n)` is correct for 1, 2, 4, … and silently wrong for 3 — it would
/// produce `1 | 2`, which a driver reads as a different count than the one
/// asked for. `None` is what makes that an `InvalidDescriptor` instead.
pub fn sample_count(samples: u32) -> Option<vk::SampleCountFlags> {
    if samples == 0 || !samples.is_power_of_two() || samples > 64 {
        return None;
    }
    Some(vk::SampleCountFlags::from_raw(samples))
}

/// How a [`ResourceState`] is expressed to a `sync2` barrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateMasks {
    /// Pipeline stages the use happens in.
    pub stage: vk::PipelineStageFlags2,
    /// Memory accesses the use performs.
    pub access: vk::AccessFlags2,
    /// Image layout the use requires.
    pub layout: vk::ImageLayout,
}

/// Expands a seam resource state into the `sync2` triple Vulkan wants.
///
/// The masks are deliberately broad — `ShaderRead` covers every shader stage,
/// because the seam does not carry which stage reads. See the module docs.
#[must_use]
pub fn state_masks(state: ResourceState) -> StateMasks {
    use vk::AccessFlags2 as A;
    use vk::ImageLayout as L;
    use vk::PipelineStageFlags2 as S;

    let (stage, access, layout) = match state {
        // The only legal source for a fresh or freshly acquired image.
        // `NONE`/`NONE` is what makes the transition free: nothing to flush,
        // nothing to wait for, contents discarded.
        ResourceState::Undefined => (S::NONE, A::NONE, L::UNDEFINED),
        ResourceState::ShaderRead => (
            S::ALL_COMMANDS,
            A::SHADER_SAMPLED_READ | A::SHADER_STORAGE_READ | A::UNIFORM_READ,
            L::SHADER_READ_ONLY_OPTIMAL,
        ),
        ResourceState::ShaderWrite => (
            S::ALL_COMMANDS,
            A::SHADER_STORAGE_WRITE,
            L::GENERAL, // A storage image is never `SHADER_READ_ONLY_OPTIMAL`.
        ),
        ResourceState::ShaderReadWrite => (
            S::ALL_COMMANDS,
            A::SHADER_STORAGE_READ | A::SHADER_STORAGE_WRITE,
            L::GENERAL,
        ),
        ResourceState::ColorAttachment => (
            S::COLOR_ATTACHMENT_OUTPUT,
            A::COLOR_ATTACHMENT_READ | A::COLOR_ATTACHMENT_WRITE,
            L::COLOR_ATTACHMENT_OPTIMAL,
        ),
        ResourceState::DepthStencilWrite => (
            S::EARLY_FRAGMENT_TESTS | S::LATE_FRAGMENT_TESTS,
            A::DEPTH_STENCIL_ATTACHMENT_READ | A::DEPTH_STENCIL_ATTACHMENT_WRITE,
            L::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        ),
        // `FRAGMENT_SHADER` used to be in this mask with no shader access bit
        // beside it, which synchronises against a stage that cannot touch the
        // resource: harmless over-sync, and wrong the day P7 samples the depth
        // buffer, because the *right* answer then is `ShaderRead`, not a
        // depth-attachment state with a shader stage bolted on.
        // The write bit is not a mistake: "read-only" describes the depth
        // *test*, and an attachment in this state still performs its store op
        // at `vkCmdEndRendering`, which syncval accounts as
        // `SYNC_LATE_FRAGMENT_TESTS_DEPTH_STENCIL_ATTACHMENT_WRITE`. Without
        // it the next barrier over that image has a read-only source scope and
        // cannot order against the store — a write-after-write with
        // `write_barriers: 0`.
        ResourceState::DepthStencilRead => (
            S::EARLY_FRAGMENT_TESTS | S::LATE_FRAGMENT_TESTS,
            A::DEPTH_STENCIL_ATTACHMENT_READ | A::DEPTH_STENCIL_ATTACHMENT_WRITE,
            L::DEPTH_STENCIL_READ_ONLY_OPTIMAL,
        ),
        ResourceState::TransferSrc => (S::ALL_TRANSFER, A::TRANSFER_READ, L::TRANSFER_SRC_OPTIMAL),
        ResourceState::TransferDst => (S::ALL_TRANSFER, A::TRANSFER_WRITE, L::TRANSFER_DST_OPTIMAL),
        // The barrier whose absence produces "sometimes nothing draws".
        ResourceState::IndirectArgument => (
            S::DRAW_INDIRECT,
            A::INDIRECT_COMMAND_READ,
            L::SHADER_READ_ONLY_OPTIMAL,
        ),
        ResourceState::IndexBuffer => (
            S::INDEX_INPUT,
            A::INDEX_READ,
            L::SHADER_READ_ONLY_OPTIMAL, // Buffers ignore the layout entirely.
        ),
        ResourceState::HostRead => (S::HOST, A::HOST_READ, L::GENERAL),
        // A present is not a pipeline stage: the swapchain's own semaphore
        // carries the dependency, so the barrier only has to reach the layout.
        // Naming an access mask here is the classic over-sync that validation's
        // sync layer flags.
        ResourceState::Present => (S::NONE, A::NONE, L::PRESENT_SRC_KHR),
    };
    StateMasks {
        stage,
        access,
        layout,
    }
}

/// Fractional bits [`timestamp_nanos`] holds `timestampPeriod` in.
///
/// Thirty-two puts the period's rounding error below 2⁻³² ns — far under any
/// clock's accuracy — while leaving a `u128` product room for the whole `u64`
/// tick range times any period a driver has ever reported.
const TIMESTAMP_FRACTION_BITS: u32 = 32;

/// A raw Vulkan timestamp in nanoseconds, which is what
/// [`Device::query_results`](crcbl_hal::Device::query_results) reports.
///
/// `period_ns` is `VkPhysicalDeviceLimits::timestampPeriod`, nanoseconds per
/// tick — 10.0 on radv's Navi 31, 1.0 on NVIDIA, 83.333 on Intel — so the
/// arithmetic is a multiply and the only real question is what to do with the
/// fraction.
///
/// # Fixed point rather than `f64`, and the size of the difference
///
/// The obvious spelling is `(ticks as f64 * period).round() as u64`, and it is
/// wrong in a way that only shows up on a machine that has been running a
/// while: an `f64` carries 53 bits of mantissa and a Vulkan timestamp is a full
/// `u64` off a free-running counter, so once a product passes 2⁵³ ns — about
/// 104 days of GPU uptime at a 1 ns period — the result is quantised to 2 ns,
/// then 4, and a delta of a few hundred nanoseconds starts arriving in steps.
/// The fix is to never leave the integers: the period is rounded once into a
/// [`TIMESTAMP_FRACTION_BITS`]-bit fixed-point multiplier and the scaling is
/// `u128` arithmetic, which is exact across the whole input range. This is the
/// mult/shift form the Linux kernel scales a clocksource's cycles with, for the
/// same reason.
///
/// Rounding is to nearest, applied once here rather than by each caller: the
/// seam's contract is a nanosecond count, so the fraction has to go somewhere,
/// and one rule at the source is the only way two readers agree.
///
/// A `period_ns` of zero — the device has no timestamps, see
/// `adapter::timestamp_period_of` — converts every tick to zero, which is the
/// same answer the seam already documents for a device that cannot time
/// anything.
#[must_use]
pub fn timestamp_nanos(ticks: u64, period_ns: f32) -> u64 {
    // A period is a small positive number, so the scaled value is at most ~2^40
    // for any clock that exists and the cast is exact well before that. A
    // negative or NaN period is not a period; `max(0.0)` makes it zero ticks
    // rather than a wrapped multiplier.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let multiplier =
        (f64::from(period_ns).max(0.0) * f64::exp2(f64::from(TIMESTAMP_FRACTION_BITS))) as u128;
    let half = 1u128 << (TIMESTAMP_FRACTION_BITS - 1);
    let nanos = (u128::from(ticks) * multiplier + half) >> TIMESTAMP_FRACTION_BITS;
    // Saturating rather than wrapping: a count so large it leaves `u64` is a
    // broken clock, and `u64::MAX` says "past anything" where a wrap would say
    // "just after boot".
    u64::try_from(nanos).unwrap_or(u64::MAX)
}

/// Turns a `VkResult` failure into the seam's error vocabulary.
///
/// Out-of-memory and device-lost get their own variants because callers act on
/// them differently; everything else keeps the driver's own spelling in a
/// string rather than leaking `ash::vk::Result` across the seam.
#[must_use]
pub fn hal_error(what: &str, result: vk::Result) -> crcbl_hal::HalError {
    use crcbl_hal::HalError;
    match result {
        vk::Result::ERROR_OUT_OF_DEVICE_MEMORY => HalError::OutOfDeviceMemory,
        vk::Result::ERROR_OUT_OF_HOST_MEMORY => HalError::OutOfHostMemory,
        vk::Result::ERROR_DEVICE_LOST => HalError::DeviceLost(format!("{what}: {result:?}")),
        other => HalError::Backend(format!("{what}: {other:?}")),
    }
}

/// Turns a `VkResult` failure from a surface or swapchain call into the seam's
/// presentation vocabulary.
///
/// `OUT_OF_DATE` and `SUBOPTIMAL` are **expected traffic** during a resize, not
/// errors, which is exactly why [`crcbl_hal::SurfaceError`] exists separately.
#[must_use]
pub fn surface_error(what: &str, result: vk::Result) -> crcbl_hal::SurfaceError {
    use crcbl_hal::SurfaceError;
    match result {
        vk::Result::ERROR_OUT_OF_DATE_KHR => SurfaceError::OutOfDate,
        vk::Result::ERROR_SURFACE_LOST_KHR | vk::Result::ERROR_NATIVE_WINDOW_IN_USE_KHR => {
            SurfaceError::Lost
        }
        vk::Result::TIMEOUT | vk::Result::NOT_READY => SurfaceError::Timeout,
        other => SurfaceError::Hal(hal_error(what, other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATES: &[ResourceState] = &[
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

    /// A format that survives a round trip is one a surface query can report
    /// back to the seam without inventing anything. Every seam format must,
    /// or `surface_caps` would silently drop a format the engine can render to.
    ///
    /// Driven off [`Format::ALL`] — the seam's list, not a second copy kept
    /// here. The copy that used to sit in this module was tied to nothing: a
    /// format added to `Format` and forgotten in [`format_from_vk`]'s
    /// `_ => None` tail could equally be forgotten here, and this test would
    /// have stayed green while `surface_caps` dropped it.
    #[test]
    fn every_seam_format_round_trips_through_vulkan() {
        for &f in Format::ALL {
            assert_eq!(format_from_vk(format(f)), Some(f), "{f:?}");
        }
        // Injective as well as total: two seam formats sharing one Vulkan
        // format would make the reverse mapping pick one of them arbitrarily.
        let mut raw: Vec<i32> = Format::ALL.iter().map(|&f| format(f).as_raw()).collect();
        raw.sort_unstable();
        raw.dedup();
        assert_eq!(raw.len(), Format::ALL.len(), "two formats map to one");
        // The long tail is honestly `None` rather than a nearest match.
        assert_eq!(format_from_vk(vk::Format::R5G6B5_UNORM_PACK16), None);
        assert_eq!(format_from_vk(vk::Format::UNDEFINED), None);
    }

    #[test]
    fn every_seam_present_mode_round_trips() {
        for mode in [
            PresentMode::Fifo,
            PresentMode::FifoRelaxed,
            PresentMode::Mailbox,
            PresentMode::Immediate,
        ] {
            assert_eq!(present_mode_from_vk(present_mode(mode)), Some(mode));
        }
        assert_eq!(
            present_mode_from_vk(vk::PresentModeKHR::SHARED_DEMAND_REFRESH),
            None,
            "the seam has no vocabulary for shared-presentable images"
        );
    }

    #[test]
    fn composite_alpha_flags_decompose_into_seam_modes() {
        let flags = vk::CompositeAlphaFlagsKHR::OPAQUE | vk::CompositeAlphaFlagsKHR::INHERIT;
        assert_eq!(
            composite_alpha_from_vk(flags),
            vec![CompositeAlpha::Opaque, CompositeAlpha::Inherit]
        );
        assert!(composite_alpha_from_vk(vk::CompositeAlphaFlagsKHR::empty()).is_empty());
    }

    /// `buffer_usage` is a chain of `if contains { flags |= }`, which is the
    /// shape that drops a bit silently: the seam flag stays legal, the Vulkan
    /// flag never appears, and the failure is a validation error inside
    /// whatever draw eventually needs it.
    ///
    /// Both halves are checked per bit — the right flag arrives, and nothing
    /// else does — because an arm pasted from the line above passes a
    /// "contains" test and fails this one.
    #[test]
    fn every_buffer_usage_bit_maps_to_its_own_vulkan_flag() {
        use crcbl_hal::BufferUsage as U;

        // Not part of the table: it is added unconditionally, which is also
        // what covers `QUERY_RESOLVE`. See `buffer_usage`.
        const ALWAYS: vk::BufferUsageFlags = vk::BufferUsageFlags::TRANSFER_DST;
        let table = [
            (U::TRANSFER_SRC, vk::BufferUsageFlags::TRANSFER_SRC),
            (U::TRANSFER_DST, vk::BufferUsageFlags::empty()),
            (U::UNIFORM, vk::BufferUsageFlags::UNIFORM_BUFFER),
            (U::STORAGE, vk::BufferUsageFlags::STORAGE_BUFFER),
            (U::INDEX, vk::BufferUsageFlags::INDEX_BUFFER),
            (U::INDIRECT, vk::BufferUsageFlags::INDIRECT_BUFFER),
            (
                U::DEVICE_ADDRESS,
                vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            ),
            (U::QUERY_RESOLVE, vk::BufferUsageFlags::empty()),
        ];

        assert_eq!(
            buffer_usage(U::empty()),
            ALWAYS,
            "every buffer is a copy dst"
        );
        for (seam, expected) in table {
            assert_eq!(
                buffer_usage(seam),
                ALWAYS | expected,
                "{seam:?} maps to exactly one flag beyond the unconditional one"
            );
        }
        // Nothing is dropped when they arrive together, which a per-bit table
        // alone cannot show.
        let every = table
            .iter()
            .fold(ALWAYS, |flags, (_, expected)| flags | *expected);
        assert_eq!(buffer_usage(U::all()), every);
        assert_eq!(
            table.len(),
            U::all().bits().count_ones() as usize,
            "a usage bit was added to the seam and not to this table"
        );
    }

    /// As above. `PRESENT` is the one bit with no Vulkan usage flag — a
    /// swapchain image's presentability belongs to the swapchain — so it maps
    /// to nothing *on purpose*, which is worth pinning rather than leaving as
    /// an omission indistinguishable from a forgotten arm.
    #[test]
    fn every_image_usage_bit_maps_to_its_own_vulkan_flag() {
        use crcbl_hal::ImageUsage as U;

        let table = [
            (U::TRANSFER_SRC, vk::ImageUsageFlags::TRANSFER_SRC),
            (U::TRANSFER_DST, vk::ImageUsageFlags::TRANSFER_DST),
            (U::SAMPLED, vk::ImageUsageFlags::SAMPLED),
            (U::STORAGE, vk::ImageUsageFlags::STORAGE),
            (U::COLOR_ATTACHMENT, vk::ImageUsageFlags::COLOR_ATTACHMENT),
            (
                U::DEPTH_STENCIL_ATTACHMENT,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            ),
            (U::PRESENT, vk::ImageUsageFlags::empty()),
        ];

        assert!(image_usage(U::empty()).is_empty());
        for (seam, expected) in table {
            assert_eq!(image_usage(seam), expected, "{seam:?}");
        }
        let every = table
            .iter()
            .fold(vk::ImageUsageFlags::empty(), |flags, (_, expected)| {
                flags | *expected
            });
        assert_eq!(image_usage(U::all()), every);
        assert_eq!(
            table.len(),
            U::all().bits().count_ones() as usize,
            "a usage bit was added to the seam and not to this table"
        );
    }

    /// The same shape once more, for the aspect mask a copy and every barrier
    /// is built from.
    #[test]
    fn every_aspect_bit_maps_and_an_empty_set_stays_empty() {
        use crcbl_hal::ImageAspect as A;

        let table = [
            (A::COLOR, vk::ImageAspectFlags::COLOR),
            (A::DEPTH, vk::ImageAspectFlags::DEPTH),
            (A::STENCIL, vk::ImageAspectFlags::STENCIL),
        ];
        assert!(aspect(A::empty()).is_empty());
        for (seam, expected) in table {
            assert_eq!(aspect(seam), expected, "{seam:?}");
        }
        assert_eq!(
            aspect(A::DEPTH | A::STENCIL),
            vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
        );
        assert_eq!(
            table.len(),
            A::all().bits().count_ones() as usize,
            "an aspect bit was added to the seam and not to this table"
        );
    }

    /// `LoadOp::Load` reaches no golden test at all — every one of them clears
    /// — so this is the only thing standing between it and `DONT_CARE`, which
    /// would discard the previous contents of an accumulation target and look
    /// like a flickering pass rather than a mapping bug.
    #[test]
    fn attachment_ops_map_to_distinct_vulkan_ops() {
        use crcbl_hal::{LoadOp, StoreOp};

        assert_eq!(load_op(LoadOp::Load), vk::AttachmentLoadOp::LOAD);
        assert_eq!(load_op(LoadOp::Clear), vk::AttachmentLoadOp::CLEAR);
        assert_eq!(load_op(LoadOp::DontCare), vk::AttachmentLoadOp::DONT_CARE);
        let mut raw: Vec<i32> = [LoadOp::Load, LoadOp::Clear, LoadOp::DontCare]
            .iter()
            .map(|op| load_op(*op).as_raw())
            .collect();
        raw.sort_unstable();
        raw.dedup();
        assert_eq!(raw.len(), 3, "two load ops collapsed into one");

        assert_eq!(store_op(StoreOp::Store), vk::AttachmentStoreOp::STORE);
        assert_eq!(
            store_op(StoreOp::Discard),
            vk::AttachmentStoreOp::DONT_CARE,
            "the seam calls it Discard; Vulkan spells it DONT_CARE, and \
             swapping it for STORE would make every transient attachment live"
        );
    }

    /// Dimensionality is two enums in Vulkan and two in the seam, and they do
    /// not line up: `D2Array` and `Cube` are view types with no image type, so
    /// a shared mapping would have to invent one.
    #[test]
    fn image_and_view_types_map_to_distinct_vulkan_enums() {
        use crcbl_hal::{ImageType, ImageViewType};

        assert_eq!(image_type(ImageType::D1), vk::ImageType::TYPE_1D);
        assert_eq!(image_type(ImageType::D2), vk::ImageType::TYPE_2D);
        assert_eq!(image_type(ImageType::D3), vk::ImageType::TYPE_3D);

        let views = [
            (ImageViewType::D1, vk::ImageViewType::TYPE_1D),
            (ImageViewType::D2, vk::ImageViewType::TYPE_2D),
            (ImageViewType::D2Array, vk::ImageViewType::TYPE_2D_ARRAY),
            (ImageViewType::Cube, vk::ImageViewType::CUBE),
            (ImageViewType::CubeArray, vk::ImageViewType::CUBE_ARRAY),
            (ImageViewType::D3, vk::ImageViewType::TYPE_3D),
        ];
        for (seam, expected) in views {
            assert_eq!(image_view_type(seam), expected, "{seam:?}");
        }
        let mut raw: Vec<i32> = views
            .iter()
            .map(|(seam, _)| image_view_type(*seam).as_raw())
            .collect();
        raw.sort_unstable();
        raw.dedup();
        assert_eq!(raw.len(), views.len(), "two view types collapsed into one");
    }

    /// A copy names one mip level and a layer range, and `layer_count` passes
    /// through unchanged — unlike `subresource_range`'s, which has a sentinel.
    /// Confusing the two is how a whole-image copy becomes a one-layer copy.
    #[test]
    fn subresource_layers_pass_through_without_the_range_sentinel() {
        use crcbl_hal::{ImageAspect, ImageSubresourceLayers};

        let layers = subresource_layers(ImageSubresourceLayers {
            aspect: ImageAspect::DEPTH,
            mip: 3,
            base_layer: 2,
            layer_count: 4,
        });
        assert_eq!(layers.aspect_mask, vk::ImageAspectFlags::DEPTH);
        assert_eq!(layers.mip_level, 3);
        assert_eq!(layers.base_array_layer, 2);
        assert_eq!(layers.layer_count, 4);
        assert_ne!(
            layers.layer_count,
            vk::REMAINING_ARRAY_LAYERS,
            "a copy region has no 'remaining' sentinel"
        );
    }

    /// The forward direction of the composite-alpha mapping; the reverse one is
    /// already covered, and a test of the reverse alone would pass on a forward
    /// mapping that had every mode wrong in the same way.
    #[test]
    fn every_composite_alpha_mode_maps_to_its_own_flag() {
        let modes = [
            (CompositeAlpha::Opaque, vk::CompositeAlphaFlagsKHR::OPAQUE),
            (
                CompositeAlpha::PreMultiplied,
                vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
            ),
            (
                CompositeAlpha::PostMultiplied,
                vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
            ),
            (CompositeAlpha::Inherit, vk::CompositeAlphaFlagsKHR::INHERIT),
        ];
        for (seam, expected) in modes {
            assert_eq!(composite_alpha(seam), expected, "{seam:?}");
            assert_eq!(
                composite_alpha_from_vk(expected),
                vec![seam],
                "and back again, alone"
            );
        }
    }

    /// The masks live on `StencilState`, not per face, so the two facings must
    /// receive the same ones while keeping their own ops. A mapping that took
    /// the ops from one face for both is invisible until a two-sided stencil
    /// effect.
    ///
    /// The reference is the other half: the seam has none to map, and every
    /// pipeline here declares `VK_DYNAMIC_STATE_STENCIL_REFERENCE`, so the
    /// baked field must stay at zero rather than acquire a meaning.
    #[test]
    fn stencil_faces_keep_their_own_ops_and_share_the_masks() {
        use crcbl_hal::{CompareOp, StencilFaceState, StencilOp, StencilState};

        let state = StencilState {
            read_mask: 0x0F,
            write_mask: 0xF0,
            ..StencilState::default()
        };
        let face = StencilFaceState {
            fail_op: StencilOp::Zero,
            depth_fail_op: StencilOp::IncrementClamp,
            pass_op: StencilOp::Replace,
            compare: CompareOp::Equal,
        };
        let mapped = stencil_face(face, state);
        assert_eq!(mapped.fail_op, vk::StencilOp::ZERO);
        assert_eq!(mapped.depth_fail_op, vk::StencilOp::INCREMENT_AND_CLAMP);
        assert_eq!(mapped.pass_op, vk::StencilOp::REPLACE);
        assert_eq!(mapped.compare_op, vk::CompareOp::EQUAL);
        assert_eq!(mapped.compare_mask, state.read_mask);
        assert_eq!(mapped.write_mask, state.write_mask);
        assert_eq!(
            mapped.reference, 0,
            "the reference is dynamic state, so nothing may bake one into the pipeline"
        );
        assert_ne!(
            mapped.compare_mask, mapped.write_mask,
            "the two masks are distinct fields, not one read twice"
        );
    }

    /// The seam's `ALL` sentinel and Vulkan's `REMAINING_*` are the same bits,
    /// and this is the test that stops someone "fixing" that by hand.
    #[test]
    fn the_whole_image_sentinel_maps_to_vulkans_remaining() {
        let range = subresource_range(ImageSubresourceRange::all(Format::Bgra8UnormSrgb));
        assert_eq!(range.aspect_mask, vk::ImageAspectFlags::COLOR);
        assert_eq!(range.level_count, vk::REMAINING_MIP_LEVELS);
        assert_eq!(range.layer_count, vk::REMAINING_ARRAY_LAYERS);

        let depth = subresource_range(ImageSubresourceRange::all(Format::D32FloatS8Uint));
        assert_eq!(
            depth.aspect_mask,
            vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
        );

        // An explicit count is *not* rewritten to the sentinel.
        let one = subresource_range(ImageSubresourceRange {
            mip_count: 1,
            layer_count: 1,
            ..ImageSubresourceRange::all(Format::D32Float)
        });
        assert_eq!((one.level_count, one.layer_count), (1, 1));
    }

    /// `Undefined` and `Present` must expand to empty masks: the first because
    /// discarding contents is free, the second because the swapchain's own
    /// semaphore carries the dependency. Getting either wrong is a validation
    /// warning on the very first frame — which is precisely what P1's "zero
    /// validation errors" gate is there to catch.
    #[test]
    fn the_two_free_states_carry_no_stages_or_accesses() {
        let undefined = state_masks(ResourceState::Undefined);
        assert_eq!(undefined.stage, vk::PipelineStageFlags2::NONE);
        assert_eq!(undefined.access, vk::AccessFlags2::NONE);
        assert_eq!(undefined.layout, vk::ImageLayout::UNDEFINED);

        let present = state_masks(ResourceState::Present);
        assert_eq!(present.stage, vk::PipelineStageFlags2::NONE);
        assert_eq!(present.access, vk::AccessFlags2::NONE);
        assert_eq!(present.layout, vk::ImageLayout::PRESENT_SRC_KHR);
    }

    /// Every other state must name at least one stage, or a barrier into it
    /// synchronises nothing at all and the bug is invisible until a race.
    #[test]
    fn every_working_state_names_a_stage_and_an_access() {
        for &state in ALL_STATES {
            let masks = state_masks(state);
            if matches!(state, ResourceState::Undefined | ResourceState::Present) {
                continue;
            }
            assert_ne!(masks.stage, vk::PipelineStageFlags2::NONE, "{state:?}");
            assert_ne!(masks.access, vk::AccessFlags2::NONE, "{state:?}");
        }
    }

    /// The seam classifies states as reads or writes; the Vulkan expansion must
    /// agree, or the graph's "read→read needs no barrier" optimisation would
    /// skip a barrier that actually flushes a write.
    #[test]
    fn write_states_expand_to_write_accesses() {
        const WRITES: vk::AccessFlags2 = vk::AccessFlags2::from_raw(
            vk::AccessFlags2::SHADER_STORAGE_WRITE.as_raw()
                | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE.as_raw()
                | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE.as_raw()
                | vk::AccessFlags2::TRANSFER_WRITE.as_raw(),
        );
        for &state in ALL_STATES {
            let masks = state_masks(state);
            assert_eq!(
                masks.access.intersects(WRITES),
                state.is_write(),
                "{state:?} disagrees with the seam's own is_write()"
            );
        }
    }

    /// Storage-image states must be `GENERAL`: Vulkan has no read-only-optimal
    /// layout that permits a storage write, and picking one is the mistake that
    /// produces a black compute output with no error message.
    #[test]
    fn storage_states_use_the_general_layout() {
        assert_eq!(
            state_masks(ResourceState::ShaderWrite).layout,
            vk::ImageLayout::GENERAL
        );
        assert_eq!(
            state_masks(ResourceState::ShaderReadWrite).layout,
            vk::ImageLayout::GENERAL
        );
        assert_eq!(
            state_masks(ResourceState::ShaderRead).layout,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
        );
    }

    /// `OutOfDate` must never be wrapped into a `HalError`: the frame loop
    /// matches on it directly, and a resize would silently become fatal.
    #[test]
    fn resize_traffic_stays_out_of_the_generic_error() {
        use crcbl_hal::SurfaceError;
        assert!(matches!(
            surface_error("acquire", vk::Result::ERROR_OUT_OF_DATE_KHR),
            SurfaceError::OutOfDate
        ));
        assert!(matches!(
            surface_error("acquire", vk::Result::ERROR_SURFACE_LOST_KHR),
            SurfaceError::Lost
        ));
        assert!(matches!(
            surface_error("acquire", vk::Result::TIMEOUT),
            SurfaceError::Timeout
        ));
        assert!(matches!(
            surface_error("create", vk::Result::ERROR_OUT_OF_DEVICE_MEMORY),
            SurfaceError::Hal(crcbl_hal::HalError::OutOfDeviceMemory)
        ));
    }

    #[test]
    fn driver_errors_keep_their_own_spelling() {
        let error = hal_error("vkCreateImage", vk::Result::ERROR_FORMAT_NOT_SUPPORTED);
        assert!(error.to_string().contains("vkCreateImage"), "{error}");
        assert!(
            error.to_string().contains("FORMAT_NOT_SUPPORTED"),
            "{error}"
        );
        assert!(matches!(
            hal_error("x", vk::Result::ERROR_DEVICE_LOST),
            crcbl_hal::HalError::DeviceLost(_)
        ));
    }
    /// The locked decision, restated where it is actually consumed:
    /// `docs/plan/02-vulkan-backend.md` fixes reversed-Z with compare op
    /// `GREATER`, and `crcbl-hal` makes it the default. If this mapping ever
    /// becomes anything else, every pipeline in the engine inverts its depth
    /// test at once and every golden image needs re-blessing.
    #[test]
    fn the_reversed_z_compare_op_survives_the_mapping() {
        assert_eq!(
            compare_op(crcbl_hal::CompareOp::Greater),
            vk::CompareOp::GREATER
        );
        assert_eq!(
            compare_op(crcbl_hal::CompareOp::default()),
            vk::CompareOp::GREATER,
            "the seam's default is the engine's depth test"
        );
        assert_eq!(
            compare_op(crcbl_hal::DepthStencilState::default().depth_compare),
            vk::CompareOp::GREATER
        );
        // And the prepass-matching pair, which is the other half of the
        // convention.
        assert_eq!(
            compare_op(crcbl_hal::CompareOp::GreaterOrEqual),
            vk::CompareOp::GREATER_OR_EQUAL
        );
    }

    /// Every comparison must map to a *distinct* Vulkan op. A duplicated arm
    /// would make two different depth tests behave identically, which is
    /// invisible until a shadow pass.
    #[test]
    fn every_comparison_maps_somewhere_distinct() {
        use crcbl_hal::CompareOp as C;
        let all = [
            C::Never,
            C::Less,
            C::Equal,
            C::LessOrEqual,
            C::Greater,
            C::NotEqual,
            C::GreaterOrEqual,
            C::Always,
        ];
        let mut mapped: Vec<i32> = all.iter().map(|op| compare_op(*op).as_raw()).collect();
        mapped.sort_unstable();
        mapped.dedup();
        assert_eq!(mapped.len(), all.len());
    }

    #[test]
    fn every_stencil_and_blend_operation_maps_somewhere_distinct() {
        use crcbl_hal::{BlendFactor as B, BlendOp as O, StencilOp as S};
        let stencils = [
            S::Keep,
            S::Zero,
            S::Replace,
            S::Invert,
            S::IncrementClamp,
            S::DecrementClamp,
            S::IncrementWrap,
            S::DecrementWrap,
        ];
        let mut raw: Vec<i32> = stencils.iter().map(|op| stencil_op(*op).as_raw()).collect();
        raw.sort_unstable();
        raw.dedup();
        assert_eq!(raw.len(), stencils.len());

        let factors = [
            B::Zero,
            B::One,
            B::Src,
            B::OneMinusSrc,
            B::SrcAlpha,
            B::OneMinusSrcAlpha,
            B::Dst,
            B::OneMinusDst,
            B::DstAlpha,
            B::OneMinusDstAlpha,
        ];
        let mut raw: Vec<i32> = factors
            .iter()
            .map(|factor| blend_factor(*factor).as_raw())
            .collect();
        raw.sort_unstable();
        raw.dedup();
        assert_eq!(raw.len(), factors.len());

        let ops = [O::Add, O::Subtract, O::ReverseSubtract, O::Min, O::Max];
        let mut raw: Vec<i32> = ops.iter().map(|op| blend_op(*op).as_raw()).collect();
        raw.sort_unstable();
        raw.dedup();
        assert_eq!(raw.len(), ops.len());
    }

    /// The seam's blend presets must survive into the values that actually
    /// produce them: straight alpha and premultiplied differ in exactly one
    /// factor, and swapping them is the classic "UI has dark fringes" bug.
    #[test]
    fn the_blend_presets_map_to_the_factors_that_define_them() {
        let alpha = crcbl_hal::BlendState::alpha();
        assert_eq!(blend_factor(alpha.color_src), vk::BlendFactor::SRC_ALPHA);
        assert_eq!(
            blend_factor(alpha.color_dst),
            vk::BlendFactor::ONE_MINUS_SRC_ALPHA
        );

        let premultiplied = crcbl_hal::BlendState::premultiplied();
        assert_eq!(blend_factor(premultiplied.color_src), vk::BlendFactor::ONE);
        assert_eq!(
            blend_factor(premultiplied.color_dst),
            vk::BlendFactor::ONE_MINUS_SRC_ALPHA
        );
    }

    #[test]
    fn colour_writes_decompose_into_components() {
        assert_eq!(
            color_writes(crcbl_hal::ColorWrites::ALL),
            vk::ColorComponentFlags::RGBA
        );
        assert_eq!(
            color_writes(crcbl_hal::ColorWrites::R | crcbl_hal::ColorWrites::A),
            vk::ColorComponentFlags::R | vk::ColorComponentFlags::A
        );
        assert!(
            color_writes(crcbl_hal::ColorWrites::empty()).is_empty(),
            "an empty mask must stay empty, not default to RGBA"
        );
    }

    /// The seam's winding reaches the rasteriser unflipped; the negative-height
    /// viewport is what accounts for Vulkan's inverted clip space. See
    /// [`front_face`] for why compensating here would be wrong.
    #[test]
    fn winding_is_passed_through_rather_than_compensated() {
        assert_eq!(
            front_face(crcbl_hal::FrontFace::Ccw),
            vk::FrontFace::COUNTER_CLOCKWISE
        );
        assert_eq!(
            front_face(crcbl_hal::FrontFace::Cw),
            vk::FrontFace::CLOCKWISE
        );
        assert_eq!(
            front_face(crcbl_hal::FrontFace::default()),
            vk::FrontFace::COUNTER_CLOCKWISE
        );
        // The default must discard nothing: a caller cannot know the winding of
        // imported geometry, and silently culling half of it is the worst
        // possible default.
        assert_eq!(
            cull_mode(crcbl_hal::CullMode::default()),
            vk::CullModeFlags::NONE
        );
    }

    /// A dynamic binding is a *different descriptor type* in Vulkan, not a flag
    /// on the same one. Getting this wrong makes every `bind_group` with a
    /// dynamic offset a validation error.
    #[test]
    fn dynamic_bindings_select_the_dynamic_descriptor_type() {
        use crcbl_hal::BindingKind as K;
        assert_eq!(
            descriptor_type(K::UniformBuffer { dynamic: false }),
            vk::DescriptorType::UNIFORM_BUFFER
        );
        assert_eq!(
            descriptor_type(K::UniformBuffer { dynamic: true }),
            vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC
        );
        assert_eq!(
            descriptor_type(K::StorageBuffer {
                read_only: true,
                dynamic: true
            }),
            vk::DescriptorType::STORAGE_BUFFER_DYNAMIC
        );
        // `read_only` is a graph hint, not a descriptor type: Vulkan expresses
        // it with the shader's `NonWritable` decoration.
        assert_eq!(
            descriptor_type(K::StorageBuffer {
                read_only: true,
                dynamic: false
            }),
            descriptor_type(K::StorageBuffer {
                read_only: false,
                dynamic: false
            })
        );
        assert_eq!(
            descriptor_type(K::Sampler { comparison: false }),
            vk::DescriptorType::SAMPLER,
            "separate samplers, never a combined image-sampler"
        );
    }

    #[test]
    fn every_binding_flag_maps_and_an_empty_set_stays_empty() {
        use crcbl_hal::BindingFlags as F;
        assert_eq!(
            binding_flags(F::PARTIALLY_BOUND),
            vk::DescriptorBindingFlags::PARTIALLY_BOUND
        );
        assert_eq!(
            binding_flags(F::UPDATE_AFTER_BIND),
            vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
        );
        assert_eq!(
            binding_flags(F::VARIABLE_COUNT),
            vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT
        );
        assert_eq!(
            binding_flags(F::all()),
            vk::DescriptorBindingFlags::PARTIALLY_BOUND
                | vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
                | vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT
        );
        assert!(binding_flags(F::empty()).is_empty());
    }

    /// `SampleCountFlags`' bits *are* the counts, so a non-power-of-two would
    /// silently become a different count rather than an error.
    #[test]
    fn sample_counts_must_be_powers_of_two() {
        assert_eq!(sample_count(1), Some(vk::SampleCountFlags::TYPE_1));
        assert_eq!(sample_count(4), Some(vk::SampleCountFlags::TYPE_4));
        assert_eq!(sample_count(64), Some(vk::SampleCountFlags::TYPE_64));
        assert_eq!(sample_count(3), None, "3 would decode as 1|2");
        assert_eq!(sample_count(0), None);
        assert_eq!(sample_count(128), None);
    }

    /// The seam's default sampler is trilinear and repeating, and the mip
    /// filter is a *different Vulkan enum* from the min/mag filter — the one
    /// place a copy-paste produces a sampler that is bilinear per mip and
    /// point-sampled between them.
    #[test]
    fn sampler_state_maps_including_the_separate_mip_enum() {
        let sampler = crcbl_hal::SamplerDesc::default();
        assert_eq!(filter(sampler.mag_filter), vk::Filter::LINEAR);
        assert_eq!(
            mipmap_mode(sampler.mip_filter),
            vk::SamplerMipmapMode::LINEAR
        );
        assert_eq!(
            mipmap_mode(crcbl_hal::FilterMode::Nearest),
            vk::SamplerMipmapMode::NEAREST
        );
        assert_eq!(
            address_mode(sampler.address_mode[0]),
            vk::SamplerAddressMode::REPEAT
        );
        assert_eq!(
            address_mode(crcbl_hal::SamplerAddressMode::ClampToBorder),
            vk::SamplerAddressMode::CLAMP_TO_BORDER
        );
    }

    #[test]
    fn shader_stage_groups_expand_to_every_member() {
        assert_eq!(
            shader_stages(crcbl_hal::ShaderStages::GRAPHICS),
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT
        );
        assert_eq!(
            shader_stages(crcbl_hal::ShaderStages::ALL),
            vk::ShaderStageFlags::VERTEX
                | vk::ShaderStageFlags::FRAGMENT
                | vk::ShaderStageFlags::COMPUTE
        );
        assert!(shader_stages(crcbl_hal::ShaderStages::empty()).is_empty());
    }

    /// The mesh stages map, and neither composite drags one in.
    ///
    /// The second half is the load-bearing one: `VK_SHADER_STAGE_MESH_BIT_EXT`
    /// in a descriptor-set layout on a device without `meshShader` is a
    /// validation error, so an `ALL` that expanded to include it would break
    /// every layout in the engine on every device that lacks the extension.
    #[test]
    fn the_mesh_stages_map_and_stay_out_of_the_composites() {
        use crcbl_hal::ShaderStages as S;
        assert_eq!(shader_stages(S::MESH), vk::ShaderStageFlags::MESH_EXT);
        assert_eq!(shader_stages(S::TASK), vk::ShaderStageFlags::TASK_EXT);
        assert_eq!(
            shader_stages(S::MESH | S::TASK),
            vk::ShaderStageFlags::MESH_EXT | vk::ShaderStageFlags::TASK_EXT
        );
        for composite in [S::GRAPHICS, S::ALL] {
            assert!(
                !shader_stages(composite)
                    .intersects(vk::ShaderStageFlags::MESH_EXT | vk::ShaderStageFlags::TASK_EXT)
            );
        }
    }

    #[test]
    fn topology_and_polygon_mode_map() {
        assert_eq!(
            topology(crcbl_hal::PrimitiveTopology::default()),
            vk::PrimitiveTopology::TRIANGLE_LIST
        );
        assert_eq!(
            topology(crcbl_hal::PrimitiveTopology::LineStrip),
            vk::PrimitiveTopology::LINE_STRIP
        );
        assert_eq!(
            polygon_mode(crcbl_hal::PolygonMode::default()),
            vk::PolygonMode::FILL
        );
        assert_eq!(
            polygon_mode(crcbl_hal::PolygonMode::Line),
            vk::PolygonMode::LINE
        );
    }

    /// **The seam reports nanoseconds, so the period has to be spent here.**
    ///
    /// Red if `timestamp_nanos` returns its ticks unscaled, which is the
    /// mistake that reads correctly on the 1.0 ns period NVIDIA reports and is
    /// out by a factor of ten on the radv Navi 31 this workspace tests against.
    #[test]
    fn a_timestamp_is_scaled_by_the_devices_period() {
        // radv on Navi 31: a 100 MHz counter, measured.
        assert_eq!(timestamp_nanos(744, 10.0), 7_440);
        // NVIDIA's, where the conversion is the identity and therefore proves
        // nothing on its own — which is why it is not the only case here.
        assert_eq!(timestamp_nanos(744, 1.0), 744);
        assert_eq!(timestamp_nanos(0, 10.0), 0);
        // No timestamps, no duration: the seam's documented degrade.
        assert_eq!(timestamp_nanos(744, 0.0), 0);
    }

    /// The fraction is rounded to nearest, once, rather than truncated.
    ///
    /// Red on a `>>` with no rounding term: Intel's 83.333 ns period turns
    /// three ticks into 249 rather than 250, and every reader that re-derived
    /// the number would land somewhere else.
    #[test]
    fn a_fractional_period_rounds_to_nearest() {
        assert_eq!(timestamp_nanos(1, 2.5), 3, "half rounds up, not down");
        assert_eq!(timestamp_nanos(3, 2.5), 8, "7.5 rounds up too");
        assert_eq!(timestamp_nanos(1, 2.4), 2);
        // 1/12 GHz, the period Intel's drivers report.
        assert_eq!(timestamp_nanos(3, 1_000.0 / 12.0), 250);
    }

    /// **The whole `u64` tick range converts without the `f64` cliff**, which
    /// is the reason this is fixed-point arithmetic rather than a multiply.
    ///
    /// Red on `(ticks as f64 * period).round() as u64`: past 2⁵³ an `f64`
    /// cannot hold consecutive integers, so a tick counter that has been
    /// running for months reports two different readings as the same instant
    /// and a delta of a few hundred nanoseconds arrives as zero.
    #[test]
    fn a_counter_past_the_f64_mantissa_still_resolves_single_nanoseconds() {
        let past_the_mantissa = (1u64 << 53) + 8;
        assert_eq!(timestamp_nanos(past_the_mantissa, 1.0), past_the_mantissa);
        assert_eq!(
            timestamp_nanos(past_the_mantissa + 1, 1.0) - timestamp_nanos(past_the_mantissa, 1.0),
            1,
            "one tick of a 1 ns clock is one nanosecond however long the GPU has been up"
        );
        // And a count no clock could reach saturates rather than wrapping into
        // a plausible-looking instant just after boot.
        assert_eq!(timestamp_nanos(u64::MAX, 10.0), u64::MAX);
    }
}
