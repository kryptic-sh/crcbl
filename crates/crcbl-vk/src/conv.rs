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
/// nothing on any driver. `DEVICE_ADDRESS` additionally implies
/// `SHADER_DEVICE_ADDRESS`, which the device must have enabled.
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
        ResourceState::DepthStencilRead => (
            S::EARLY_FRAGMENT_TESTS | S::LATE_FRAGMENT_TESTS | S::FRAGMENT_SHADER,
            A::DEPTH_STENCIL_ATTACHMENT_READ,
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

    const ALL_FORMATS: &[Format] = &[
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

    /// A format that survives a round trip is one a surface query can report
    /// back to the seam without inventing anything. Every seam format must,
    /// or `surface_caps` would silently drop a format the engine can render to.
    #[test]
    fn every_seam_format_round_trips_through_vulkan() {
        for &f in ALL_FORMATS {
            assert_eq!(format_from_vk(format(f)), Some(f), "{f:?}");
        }
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
}
