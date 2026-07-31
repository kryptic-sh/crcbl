//! Conversions between `crcbl-hal` enums and `wgpu` types.

use crcbl_hal::{
    BlendFactor, BlendOp, BufferUsage, CompareOp, CullMode, FilterMode, Format, FrontFace,
    ImageUsage, IndexFormat, LoadOp, PolygonMode, PrimitiveTopology, SamplerAddressMode,
    ShaderStages, StencilOp, StoreOp,
};

pub fn map_format(f: Format) -> wgpu::TextureFormat {
    match f {
        Format::R8Unorm => wgpu::TextureFormat::R8Unorm,
        Format::Rg8Unorm => wgpu::TextureFormat::Rg8Unorm,
        Format::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        Format::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
        Format::Bgra8Unorm => wgpu::TextureFormat::Bgra8Unorm,
        Format::Bgra8UnormSrgb => wgpu::TextureFormat::Bgra8UnormSrgb,
        Format::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
        Format::Rg32Float => wgpu::TextureFormat::Rg32Float,
        Format::Rg32Uint => wgpu::TextureFormat::Rg32Uint,
        Format::R11g11b10Float => wgpu::TextureFormat::Rgba16Float, // closest wgpu match
        Format::D32Float => wgpu::TextureFormat::Depth32Float,
        Format::D32FloatS8Uint => wgpu::TextureFormat::Depth32FloatStencil8,
        Format::D24UnormS8Uint => wgpu::TextureFormat::Depth24PlusStencil8,
        Format::D16Unorm => wgpu::TextureFormat::Depth16Unorm,
        _ => {
            log::warn!("crcbl-wgpu: unmapped format {f:?}, falling back to Rgba8Unorm");
            wgpu::TextureFormat::Rgba8Unorm
        }
    }
}

pub fn map_filter(m: FilterMode) -> wgpu::FilterMode {
    match m {
        FilterMode::Nearest => wgpu::FilterMode::Nearest,
        FilterMode::Linear => wgpu::FilterMode::Linear,
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

pub fn map_buffer_usage(u: BufferUsage) -> wgpu::BufferUsages {
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
    if u.contains(ImageUsage::COLOR_ATTACHMENT) {
        out |= wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    // DEPTH_STENCIL_ATTACHMENT and PRESENT don't map directly to wgpu usages;
    // PRESENT is handled by the swapchain surface.
    out
}

#[allow(dead_code)]
pub fn map_load_op(_op: LoadOp) -> wgpu::LoadOp<wgpu::Color> {
    // wgpu uses LoadOp::Clear(color) or LoadOp::Load
    // We always use Load for now — clear values are passed via begin_render_pass
    wgpu::LoadOp::Load
}

#[allow(dead_code)]
pub fn map_store_op(_op: StoreOp) -> wgpu::StoreOp {
    wgpu::StoreOp::Store
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
pub fn map_binding_kind(k: BindingKind) -> wgpu::BindingType {
    match k {
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
        BindingKind::StorageImage { read_only: _ } => wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::ReadWrite,
            format: wgpu::TextureFormat::Rgba8Unorm,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        BindingKind::Sampler => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
    }
}
