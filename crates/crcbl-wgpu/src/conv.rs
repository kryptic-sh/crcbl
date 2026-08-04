//! Conversions between `crcbl-hal` enums and `wgpu` types.
//!
//! Every mapping here is either total or fallible. Nothing falls back to a
//! "close enough" substitute: a format the backend silently swapped is a golden
//! image that differs from the Vulkan one for no reason a log line explains.

use crcbl_hal::{
    BackendKind, BlendFactor, BlendOp, BufferUsage, CompareOp, CullMode, FilterMode, Format,
    FrontFace, HalError, ImageAspect, ImageUsage, IndexFormat, MemoryLocation, PolygonMode,
    PrimitiveTopology, SamplerAddressMode, ShaderStages, StencilOp,
};

/// The HAL format set is a subset of wgpu's, so this is total — see
/// `crcbl_hal::format`'s note on why `Format` is deliberately not
/// `#[non_exhaustive]`.
///
/// [`Format::D24UnormS8Uint`] is the one inexact pair: wgpu exposes
/// `Depth24PlusStencil8`, which is *at least* 24 bits of depth. The HAL's own
/// docs already prefer [`Format::D32Float`] for that reason.
pub fn map_format(f: Format) -> wgpu::TextureFormat {
    use wgpu::TextureFormat as W;
    match f {
        Format::R8Unorm => W::R8Unorm,
        Format::Rg8Unorm => W::Rg8Unorm,
        Format::Rgba8Unorm => W::Rgba8Unorm,
        Format::Rgba8UnormSrgb => W::Rgba8UnormSrgb,
        Format::Bgra8Unorm => W::Bgra8Unorm,
        Format::Bgra8UnormSrgb => W::Bgra8UnormSrgb,
        Format::Rgb10a2Unorm => W::Rgb10a2Unorm,
        Format::R11g11b10Float => W::Rg11b10Ufloat,
        Format::R16Float => W::R16Float,
        Format::Rg16Float => W::Rg16Float,
        Format::Rgba16Float => W::Rgba16Float,
        Format::R32Float => W::R32Float,
        Format::Rg32Float => W::Rg32Float,
        Format::Rgba32Float => W::Rgba32Float,
        Format::R32Uint => W::R32Uint,
        Format::Rg32Uint => W::Rg32Uint,
        Format::D32Float => W::Depth32Float,
        Format::D32FloatS8Uint => W::Depth32FloatStencil8,
        Format::D24UnormS8Uint => W::Depth24PlusStencil8,
        Format::D16Unorm => W::Depth16Unorm,
        Format::Bc1RgbaUnorm => W::Bc1RgbaUnorm,
        Format::Bc1RgbaUnormSrgb => W::Bc1RgbaUnormSrgb,
        Format::Bc3RgbaUnorm => W::Bc3RgbaUnorm,
        Format::Bc3RgbaUnormSrgb => W::Bc3RgbaUnormSrgb,
        Format::Bc4RUnorm => W::Bc4RUnorm,
        Format::Bc5RgUnorm => W::Bc5RgUnorm,
        Format::Bc6hRgbUfloat => W::Bc6hRgbUfloat,
        Format::Bc7RgbaUnorm => W::Bc7RgbaUnorm,
        Format::Bc7RgbaUnormSrgb => W::Bc7RgbaUnormSrgb,
    }
}

/// The sRGB-encoded counterpart of a linear surface format.
///
/// `None` for everything else, which includes the sRGB formats themselves and
/// the block-compressed pairs: only a *surface* format is asked about here, and
/// the pair exists so a canvas that cannot be configured sRGB can still be
/// viewed that way. See [`WgpuInstance::surface_caps`](crate::WgpuInstance).
pub const fn srgb_counterpart(f: Format) -> Option<Format> {
    match f {
        Format::Rgba8Unorm => Some(Format::Rgba8UnormSrgb),
        Format::Bgra8Unorm => Some(Format::Bgra8UnormSrgb),
        _ => None,
    }
}

/// Reverse of [`srgb_counterpart`]: the linear format an sRGB one is the encode
/// of, and `None` for a format that is already linear.
pub const fn linear_counterpart(f: Format) -> Option<Format> {
    match f {
        Format::Rgba8UnormSrgb => Some(Format::Rgba8Unorm),
        Format::Bgra8UnormSrgb => Some(Format::Bgra8Unorm),
        _ => None,
    }
}

/// Reverse of [`map_format`]: wgpu format → HAL format.
///
/// `None` for a wgpu format the seam has no name for. Callers enumerating what
/// a surface or adapter supports drop those entries; there is nothing to be
/// gained by reporting a format nothing above the seam can ask for, and
/// substituting a different one is how a swapchain ends up configured in a
/// format the caller never chose.
pub fn unmap_format(f: wgpu::TextureFormat) -> Option<Format> {
    use wgpu::TextureFormat as W;
    Some(match f {
        W::R8Unorm => Format::R8Unorm,
        W::Rg8Unorm => Format::Rg8Unorm,
        W::Rgba8Unorm => Format::Rgba8Unorm,
        W::Rgba8UnormSrgb => Format::Rgba8UnormSrgb,
        W::Bgra8Unorm => Format::Bgra8Unorm,
        W::Bgra8UnormSrgb => Format::Bgra8UnormSrgb,
        W::Rgb10a2Unorm => Format::Rgb10a2Unorm,
        W::Rg11b10Ufloat => Format::R11g11b10Float,
        W::R16Float => Format::R16Float,
        W::Rg16Float => Format::Rg16Float,
        W::Rgba16Float => Format::Rgba16Float,
        W::R32Float => Format::R32Float,
        W::Rg32Float => Format::Rg32Float,
        W::Rgba32Float => Format::Rgba32Float,
        W::R32Uint => Format::R32Uint,
        W::Rg32Uint => Format::Rg32Uint,
        W::Depth32Float => Format::D32Float,
        W::Depth32FloatStencil8 => Format::D32FloatS8Uint,
        W::Depth24PlusStencil8 => Format::D24UnormS8Uint,
        W::Depth16Unorm => Format::D16Unorm,
        W::Bc1RgbaUnorm => Format::Bc1RgbaUnorm,
        W::Bc1RgbaUnormSrgb => Format::Bc1RgbaUnormSrgb,
        W::Bc3RgbaUnorm => Format::Bc3RgbaUnorm,
        W::Bc3RgbaUnormSrgb => Format::Bc3RgbaUnormSrgb,
        W::Bc4RUnorm => Format::Bc4RUnorm,
        W::Bc5RgUnorm => Format::Bc5RgUnorm,
        W::Bc6hRgbUfloat => Format::Bc6hRgbUfloat,
        W::Bc7RgbaUnorm => Format::Bc7RgbaUnorm,
        W::Bc7RgbaUnormSrgb => Format::Bc7RgbaUnormSrgb,
        _ => return None,
    })
}

pub fn map_filter(m: FilterMode) -> wgpu::FilterMode {
    match m {
        FilterMode::Nearest => wgpu::FilterMode::Nearest,
        FilterMode::Linear => wgpu::FilterMode::Linear,
    }
}

pub fn map_mip_filter(m: FilterMode) -> wgpu::MipmapFilterMode {
    match m {
        FilterMode::Nearest => wgpu::MipmapFilterMode::Nearest,
        FilterMode::Linear => wgpu::MipmapFilterMode::Linear,
    }
}

pub fn map_address(a: SamplerAddressMode) -> wgpu::AddressMode {
    match a {
        SamplerAddressMode::Repeat => wgpu::AddressMode::Repeat,
        SamplerAddressMode::MirrorRepeat => wgpu::AddressMode::MirrorRepeat,
        SamplerAddressMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
        SamplerAddressMode::ClampToBorder => wgpu::AddressMode::ClampToBorder,
    }
}

pub fn map_compare(op: CompareOp) -> wgpu::CompareFunction {
    match op {
        CompareOp::Never => wgpu::CompareFunction::Never,
        CompareOp::Less => wgpu::CompareFunction::Less,
        CompareOp::Equal => wgpu::CompareFunction::Equal,
        CompareOp::LessOrEqual => wgpu::CompareFunction::LessEqual,
        CompareOp::Greater => wgpu::CompareFunction::Greater,
        CompareOp::NotEqual => wgpu::CompareFunction::NotEqual,
        CompareOp::GreaterOrEqual => wgpu::CompareFunction::GreaterEqual,
        CompareOp::Always => wgpu::CompareFunction::Always,
    }
}

/// Buffer usages, plus whatever the *memory location* implies.
///
/// wgpu has no mapped-memory location: [`crcbl_hal::Device::write_buffer`] on
/// this backend is `Queue::write_buffer`, which stages through wgpu's own
/// upload heap and therefore needs `COPY_DST` on the destination. `crcbl-vk`
/// writes straight into a persistent mapping and needs no usage bit for it, so
/// the seam does not make callers declare one — which means the backend that
/// does need it has to derive it, and the honest derivation is "every buffer
/// the seam says is host-writable".
///
/// [`MemoryLocation::HostReadback`] gets `COPY_DST` for the same reason and
/// `MAP_READ` so the readback path can map it.
pub fn map_buffer_usage(u: BufferUsage, memory: MemoryLocation) -> wgpu::BufferUsages {
    let mut out = wgpu::BufferUsages::empty();
    if u.contains(BufferUsage::TRANSFER_SRC) {
        out |= wgpu::BufferUsages::COPY_SRC;
    }
    if u.contains(BufferUsage::TRANSFER_DST) {
        out |= wgpu::BufferUsages::COPY_DST;
    }
    if u.contains(BufferUsage::UNIFORM) {
        out |= wgpu::BufferUsages::UNIFORM;
    }
    if u.contains(BufferUsage::STORAGE) {
        out |= wgpu::BufferUsages::STORAGE;
    }
    if u.contains(BufferUsage::INDEX) {
        out |= wgpu::BufferUsages::INDEX;
    }
    if u.contains(BufferUsage::INDIRECT) {
        out |= wgpu::BufferUsages::INDIRECT;
    }
    if u.contains(BufferUsage::QUERY_RESOLVE) {
        out |= wgpu::BufferUsages::QUERY_RESOLVE;
    }
    match memory {
        MemoryLocation::DeviceLocal => {}
        MemoryLocation::HostUpload => out |= wgpu::BufferUsages::COPY_DST,
        MemoryLocation::HostReadback => {
            out |= wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ;
        }
    }
    out
}

pub fn map_image_usage(u: ImageUsage) -> wgpu::TextureUsages {
    let mut out = wgpu::TextureUsages::empty();
    if u.contains(ImageUsage::TRANSFER_SRC) {
        out |= wgpu::TextureUsages::COPY_SRC;
    }
    if u.contains(ImageUsage::TRANSFER_DST) {
        out |= wgpu::TextureUsages::COPY_DST;
    }
    if u.contains(ImageUsage::SAMPLED) {
        out |= wgpu::TextureUsages::TEXTURE_BINDING;
    }
    if u.contains(ImageUsage::STORAGE) {
        out |= wgpu::TextureUsages::STORAGE_BINDING;
    }
    // wgpu has one attachment usage for both planes; the format decides which
    // it means. Leaving depth out is how `TransientImageDesc::scene_depth` —
    // which sets `DEPTH_STENCIL_ATTACHMENT` and nothing else — used to ask for
    // a texture with no usages at all.
    if u.contains(ImageUsage::COLOR_ATTACHMENT) || u.contains(ImageUsage::DEPTH_STENCIL_ATTACHMENT)
    {
        out |= wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    // `PRESENT` is the surface configuration's business, not a texture usage.
    out
}

/// A copy's aspect. A copy names exactly one plane, per
/// [`crcbl_hal::ImageSubresourceLayers`].
pub fn map_aspect(aspect: ImageAspect) -> Result<wgpu::TextureAspect, HalError> {
    if aspect == ImageAspect::DEPTH {
        Ok(wgpu::TextureAspect::DepthOnly)
    } else if aspect == ImageAspect::STENCIL {
        Ok(wgpu::TextureAspect::StencilOnly)
    } else if aspect == ImageAspect::COLOR {
        Ok(wgpu::TextureAspect::All)
    } else {
        Err(HalError::InvalidDescriptor(format!(
            "a copy must name exactly one image plane, got {aspect:?}"
        )))
    }
}

pub fn map_index_format(f: IndexFormat) -> wgpu::IndexFormat {
    match f {
        IndexFormat::Uint16 => wgpu::IndexFormat::Uint16,
        IndexFormat::Uint32 => wgpu::IndexFormat::Uint32,
    }
}

pub fn map_topology(t: PrimitiveTopology) -> wgpu::PrimitiveTopology {
    match t {
        PrimitiveTopology::PointList => wgpu::PrimitiveTopology::PointList,
        PrimitiveTopology::LineList => wgpu::PrimitiveTopology::LineList,
        PrimitiveTopology::LineStrip => wgpu::PrimitiveTopology::LineStrip,
        PrimitiveTopology::TriangleList => wgpu::PrimitiveTopology::TriangleList,
        PrimitiveTopology::TriangleStrip => wgpu::PrimitiveTopology::TriangleStrip,
    }
}

pub fn map_front_face(f: FrontFace) -> wgpu::FrontFace {
    match f {
        FrontFace::Ccw => wgpu::FrontFace::Ccw,
        FrontFace::Cw => wgpu::FrontFace::Cw,
    }
}

pub fn map_cull_mode(c: CullMode) -> Option<wgpu::Face> {
    match c {
        CullMode::None => None,
        CullMode::Front => Some(wgpu::Face::Front),
        CullMode::Back => Some(wgpu::Face::Back),
    }
}

pub fn map_polygon_mode(p: PolygonMode) -> wgpu::PolygonMode {
    match p {
        PolygonMode::Fill => wgpu::PolygonMode::Fill,
        PolygonMode::Line => wgpu::PolygonMode::Line,
    }
}

pub fn map_blend_factor(bf: BlendFactor) -> wgpu::BlendFactor {
    match bf {
        BlendFactor::Zero => wgpu::BlendFactor::Zero,
        BlendFactor::One => wgpu::BlendFactor::One,
        BlendFactor::Src => wgpu::BlendFactor::Src,
        BlendFactor::OneMinusSrc => wgpu::BlendFactor::OneMinusSrc,
        BlendFactor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
        BlendFactor::OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        BlendFactor::Dst => wgpu::BlendFactor::Dst,
        BlendFactor::OneMinusDst => wgpu::BlendFactor::OneMinusDst,
        BlendFactor::DstAlpha => wgpu::BlendFactor::DstAlpha,
        BlendFactor::OneMinusDstAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
    }
}

pub fn map_blend_op(bo: BlendOp) -> wgpu::BlendOperation {
    match bo {
        BlendOp::Add => wgpu::BlendOperation::Add,
        BlendOp::Subtract => wgpu::BlendOperation::Subtract,
        BlendOp::ReverseSubtract => wgpu::BlendOperation::ReverseSubtract,
        BlendOp::Min => wgpu::BlendOperation::Min,
        BlendOp::Max => wgpu::BlendOperation::Max,
    }
}

pub fn map_stencil_op(so: StencilOp) -> wgpu::StencilOperation {
    match so {
        StencilOp::Keep => wgpu::StencilOperation::Keep,
        StencilOp::Zero => wgpu::StencilOperation::Zero,
        StencilOp::Replace => wgpu::StencilOperation::Replace,
        StencilOp::Invert => wgpu::StencilOperation::Invert,
        StencilOp::IncrementClamp => wgpu::StencilOperation::IncrementClamp,
        StencilOp::DecrementClamp => wgpu::StencilOperation::DecrementClamp,
        StencilOp::IncrementWrap => wgpu::StencilOperation::IncrementWrap,
        StencilOp::DecrementWrap => wgpu::StencilOperation::DecrementWrap,
    }
}

pub fn map_stencil_face(face: crcbl_hal::StencilFaceState) -> wgpu::StencilFaceState {
    wgpu::StencilFaceState {
        compare: map_compare(face.compare),
        fail_op: map_stencil_op(face.fail_op),
        depth_fail_op: map_stencil_op(face.depth_fail_op),
        pass_op: map_stencil_op(face.pass_op),
    }
}

pub fn map_color_writes(w: crcbl_hal::ColorWrites) -> wgpu::ColorWrites {
    let mut out = wgpu::ColorWrites::empty();
    if w.contains(crcbl_hal::ColorWrites::R) {
        out |= wgpu::ColorWrites::RED;
    }
    if w.contains(crcbl_hal::ColorWrites::G) {
        out |= wgpu::ColorWrites::GREEN;
    }
    if w.contains(crcbl_hal::ColorWrites::B) {
        out |= wgpu::ColorWrites::BLUE;
    }
    if w.contains(crcbl_hal::ColorWrites::A) {
        out |= wgpu::ColorWrites::ALPHA;
    }
    out
}

pub fn map_shader_stages(s: ShaderStages) -> wgpu::ShaderStages {
    let mut out = wgpu::ShaderStages::empty();
    if s.contains(ShaderStages::VERTEX) {
        out |= wgpu::ShaderStages::VERTEX;
    }
    if s.contains(ShaderStages::FRAGMENT) {
        out |= wgpu::ShaderStages::FRAGMENT;
    }
    if s.contains(ShaderStages::COMPUTE) {
        out |= wgpu::ShaderStages::COMPUTE;
    }
    out
}

use crcbl_hal::BindingKind;

/// Binding kinds.
///
/// Fallible because of storage images: wgpu's `BindingType::StorageTexture`
/// needs the texel format and the view dimension at *layout* creation, and
/// [`BindingKind::StorageImage`] carries neither. Inventing `Rgba8Unorm` there
/// produced a layout that silently disagreed with every shader whose storage
/// image was anything else, so this refuses instead. Sampled images are
/// assumed 2D, single-sampled and float-filterable, which is what every sampled
/// binding in the engine is; a shadow-comparison or integer sampled image would
/// need the same information the seam does not yet carry.
pub fn map_binding_kind(k: BindingKind) -> Result<wgpu::BindingType, HalError> {
    Ok(match k {
        BindingKind::UniformBuffer { dynamic } => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        BindingKind::StorageBuffer { read_only, dynamic } => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        BindingKind::SampledImage => wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        BindingKind::StorageImage { .. } => {
            return Err(HalError::Unsupported {
                backend: BackendKind::Wgpu,
                what: "storage-image bindings: wgpu needs the texel format and view dimension at \
                       bind-group-layout creation and BindingKind::StorageImage carries neither",
            });
        }
        BindingKind::Sampler => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A depth-only transient must still be usable as an attachment, or every
    /// forward pass fails at texture creation.
    #[test]
    fn depth_only_usage_reaches_render_attachment() {
        let usage = map_image_usage(ImageUsage::DEPTH_STENCIL_ATTACHMENT);
        assert_eq!(usage, wgpu::TextureUsages::RENDER_ATTACHMENT);
        assert!(!usage.is_empty());
    }

    /// `write_buffer` is `Queue::write_buffer` here, which needs `COPY_DST`
    /// even on a buffer whose declared usage is only `UNIFORM`.
    #[test]
    fn host_visible_buffers_can_be_written() {
        let usage = map_buffer_usage(BufferUsage::UNIFORM, MemoryLocation::HostUpload);
        assert!(usage.contains(wgpu::BufferUsages::COPY_DST));
        assert!(usage.contains(wgpu::BufferUsages::UNIFORM));
        // A device-local buffer gets nothing it did not ask for.
        let device_local = map_buffer_usage(BufferUsage::STORAGE, MemoryLocation::DeviceLocal);
        assert!(!device_local.contains(wgpu::BufferUsages::COPY_DST));
    }

    /// The pair must be an inverse for everything wgpu and the seam share, or a
    /// swapchain configured from `surface_caps` is configured in a format the
    /// caller never picked.
    #[test]
    fn format_mapping_round_trips() {
        for format in [
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
        ] {
            assert_eq!(
                unmap_format(map_format(format)),
                Some(format),
                "{format:?} does not survive the round trip"
            );
        }
    }

    /// A format wgpu has and the seam does not is dropped, not substituted.
    #[test]
    fn an_unnameable_wgpu_format_is_none() {
        assert_eq!(unmap_format(wgpu::TextureFormat::Rgba8Snorm), None);
    }

    #[test]
    fn a_storage_image_binding_is_refused_rather_than_guessed() {
        let error = map_binding_kind(BindingKind::StorageImage { read_only: false })
            .expect_err("wgpu cannot express this without a format");
        assert!(matches!(error, HalError::Unsupported { .. }), "{error:?}");
    }

    #[test]
    fn a_copy_names_exactly_one_plane() {
        assert_eq!(
            map_aspect(ImageAspect::DEPTH).expect("depth"),
            wgpu::TextureAspect::DepthOnly
        );
        assert!(map_aspect(ImageAspect::DEPTH | ImageAspect::STENCIL).is_err());
        assert!(map_aspect(ImageAspect::empty()).is_err());
    }
}
