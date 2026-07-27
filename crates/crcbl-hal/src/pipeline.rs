//! Pipeline layouts, bind groups, and graphics/compute pipeline state.
//!
//! # No vertex input state
//!
//! [`GraphicsPipelineDesc`] has no vertex-buffer layout, and there is no
//! `bind_vertex_buffer`. **Vertex pulling is the only geometry path**: position
//! and attribute streams live in storage buffers and the vertex shader indexes
//! them (`docs/plan/02-vulkan-backend.md` §2.5, `03-gpu-driven-rendering.md`
//! §3.1). That is what makes a mesh three integers and lets 10 objects and
//! 10 000 objects record the same commands.
//!
//! Index buffers *do* exist ([`bind_index_buffer`](crate::CommandEncoder::bind_index_buffer)),
//! because the post-transform vertex cache is real hardware the shader cannot
//! reach.
//!
//! # Bindless-capable descriptors
//!
//! [`BindGroupLayoutEntry`] carries a `count` and [`BindingFlags`]. A Tier A
//! backend turns `count: u32::MAX` plus `VARIABLE_COUNT | PARTIALLY_BOUND |
//! UPDATE_AFTER_BIND` into a runtime-sized descriptor array; a Tier B backend
//! sees a fixed `count` and produces an ordinary texture array page. Same
//! declaration, both tiers — which is the point of topic 03's "Tier B is a
//! constraint on data layout, not a separate renderer".

use crcbl_core::Handle;

use crate::{
    Format, ImageViewHandle, SamplerHandle, ShaderEntry, ShaderStages, resource::BufferHandle,
};

/// Marker type for pipeline-layout handles. Uninhabited.
#[derive(Debug)]
pub enum PipelineLayout {}
/// Marker type for bind-group-layout handles. Uninhabited.
#[derive(Debug)]
pub enum BindGroupLayout {}
/// Marker type for bind-group handles. Uninhabited.
#[derive(Debug)]
pub enum BindGroup {}
/// Marker type for graphics-pipeline handles. Uninhabited.
#[derive(Debug)]
pub enum GraphicsPipeline {}
/// Marker type for compute-pipeline handles. Uninhabited.
#[derive(Debug)]
pub enum ComputePipeline {}

/// A pipeline layout.
pub type PipelineLayoutHandle = Handle<PipelineLayout>;
/// A bind-group (descriptor-set) layout.
pub type BindGroupLayoutHandle = Handle<BindGroupLayout>;
/// A bind group (descriptor set).
pub type BindGroupHandle = Handle<BindGroup>;
/// A graphics pipeline.
pub type GraphicsPipelineHandle = Handle<GraphicsPipeline>;
/// A compute pipeline.
pub type ComputePipelineHandle = Handle<ComputePipeline>;

// --- descriptors -----------------------------------------------------------

/// What kind of resource a binding slot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingKind {
    /// A uniform buffer.
    UniformBuffer {
        /// Whether the binding takes a dynamic offset at bind time.
        ///
        /// The Tier B substitute for push constants: WebGPU has no root
        /// constants, so per-draw data becomes a dynamic offset into one large
        /// uniform buffer.
        dynamic: bool,
    },
    /// A storage buffer.
    StorageBuffer {
        /// Whether the shader only reads it. Read-only lets a backend place it
        /// more cheaply and lets the graph merge more barriers.
        read_only: bool,
        /// Whether the binding takes a dynamic offset at bind time.
        dynamic: bool,
    },
    /// A sampled image.
    SampledImage,
    /// A read/write storage image.
    StorageImage {
        /// Whether the shader only reads it.
        read_only: bool,
    },
    /// A sampler, separate from the image it filters.
    ///
    /// Separate samplers rather than combined image-samplers: the bindless
    /// model wants one big image array and a handful of samplers, not a
    /// combinatorial product of the two, and DX12/WebGPU have no combined type
    /// at all.
    Sampler,
}

bitflags::bitflags! {
    /// Descriptor-indexing behaviour for one binding.
    ///
    /// All three require [`Features::DESCRIPTOR_INDEXING`](crate::Features::DESCRIPTOR_INDEXING).
    /// A Tier B backend must reject a layout that sets any of them rather than
    /// silently ignoring it — a bindless array quietly downgraded to a fixed
    /// one reads garbage at index 4097.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct BindingFlags: u32 {
        /// Not every descriptor in the array has to be written before use, as
        /// long as the shader does not read the unwritten ones.
        const PARTIALLY_BOUND = 1 << 0;
        /// Descriptors may be written while command buffers referencing the set
        /// are pending — the flag that removes per-object descriptor updates
        /// from the frame loop.
        const UPDATE_AFTER_BIND = 1 << 1;
        /// The array's length is chosen when the bind group is created, up to
        /// the layout's `count`. Only legal on the last binding of a layout.
        const VARIABLE_COUNT = 1 << 2;
    }
}

/// One binding slot in a bind-group layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindGroupLayoutEntry {
    /// Binding number, matching the shader's `[[binding(n)]]`.
    pub binding: u32,
    /// Stages that may access it.
    pub visibility: ShaderStages,
    /// What it holds.
    pub kind: BindingKind,
    /// Array length. `1` for a scalar binding; larger for an array; combined
    /// with [`BindingFlags::VARIABLE_COUNT`] for a bindless array, where it is
    /// the upper bound.
    pub count: u32,
    /// Descriptor-indexing behaviour.
    pub flags: BindingFlags,
}

/// Creation parameters for a bind-group layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindGroupLayoutDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Binding slots, in any order.
    pub entries: &'a [BindGroupLayoutEntry],
}

/// A resource being bound into a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingResource {
    /// A buffer range.
    Buffer {
        /// Buffer to bind.
        buffer: BufferHandle,
        /// Offset in bytes, respecting the relevant
        /// `min_*_buffer_offset_alignment` limit.
        offset: u64,
        /// Length in bytes. [`u64::MAX`] means "to the end of the buffer".
        size: u64,
    },
    /// An image view. The view's layout at use time is the graph's
    /// responsibility, not the bind group's.
    ImageView(ImageViewHandle),
    /// A sampler.
    Sampler(SamplerHandle),
}

impl BindingResource {
    /// Sentinel for [`BindingResource::Buffer::size`] meaning "to the end".
    pub const WHOLE_BUFFER: u64 = u64::MAX;

    /// Binds a whole buffer.
    #[must_use]
    pub const fn whole_buffer(buffer: BufferHandle) -> Self {
        Self::Buffer {
            buffer,
            offset: 0,
            size: Self::WHOLE_BUFFER,
        }
    }
}

/// One resource assignment in a bind group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindGroupEntry {
    /// Binding number this fills.
    pub binding: u32,
    /// Index within the binding's array; `0` for a scalar binding. This is the
    /// bindless write path: one entry per texture slot updated.
    pub array_index: u32,
    /// What to bind.
    pub resource: BindingResource,
}

/// Creation parameters for a bind group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindGroupDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Layout this group conforms to.
    pub layout: BindGroupLayoutHandle,
    /// Assignments. With [`BindingFlags::PARTIALLY_BOUND`] this may cover only
    /// part of an array.
    pub entries: &'a [BindGroupEntry],
}

/// A push-constant / root-constant range.
///
/// Requires [`Features::PUSH_CONSTANTS`](crate::Features::PUSH_CONSTANTS).
/// WebGPU has none; Tier B pipelines declare a dynamic-offset uniform buffer
/// instead — see [`BindingKind::UniformBuffer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PushConstantRange {
    /// Stages that read it.
    pub stages: ShaderStages,
    /// Byte offset within the push-constant block.
    pub offset: u32,
    /// Byte size, bounded by
    /// [`Limits::max_push_constant_size`](crate::Limits::max_push_constant_size).
    pub size: u32,
}

/// Creation parameters for a pipeline layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipelineLayoutDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Bind-group layouts, in set order.
    pub bind_group_layouts: &'a [BindGroupLayoutHandle],
    /// Push-constant range, if any.
    pub push_constants: Option<PushConstantRange>,
}

// --- fixed-function state --------------------------------------------------

/// How vertices assemble into primitives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PrimitiveTopology {
    /// Independent points.
    PointList,
    /// Independent line segments.
    LineList,
    /// Connected line segments.
    LineStrip,
    /// Independent triangles. The engine's default and, with vertex pulling,
    /// effectively the only one used for geometry.
    #[default]
    TriangleList,
    /// Connected triangles.
    TriangleStrip,
}

/// Which winding is front-facing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FrontFace {
    /// Counter-clockwise in framebuffer space.
    #[default]
    Ccw,
    /// Clockwise in framebuffer space.
    Cw,
}

/// Which faces to discard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CullMode {
    /// Keep everything.
    #[default]
    None,
    /// Discard front faces.
    Front,
    /// Discard back faces.
    Back,
}

/// Fill mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PolygonMode {
    /// Solid.
    #[default]
    Fill,
    /// Wireframe. Requires
    /// [`Features::POLYGON_MODE_LINE`](crate::Features::POLYGON_MODE_LINE).
    Line,
}

/// Rasteriser and primitive-assembly state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PrimitiveState {
    /// Assembly mode.
    pub topology: PrimitiveTopology,
    /// Winding treated as front-facing.
    pub front_face: FrontFace,
    /// Faces discarded.
    pub cull_mode: CullMode,
    /// Fill mode.
    pub polygon_mode: PolygonMode,
    /// Clamp depth instead of clipping at the near/far planes. Requires
    /// [`Features::DEPTH_CLAMP`](crate::Features::DEPTH_CLAMP); shadow passes
    /// want it so casters behind the near plane still write depth.
    pub depth_clamp: bool,
}

/// A comparison function.
///
/// Named by the **comparison performed**, never by what it means for
/// visibility: `Greater` means `Greater`. Under this engine's reversed-Z that
/// happens to be the "closer" test, but a name like `CloserOrEqual` would bake a
/// depth convention into the vocabulary and become a lie the day someone writes
/// a stencil or shadow comparison. See [`crate::depth`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompareOp {
    /// Always fails.
    Never,
    /// Passes when the incoming value is less than the stored one.
    Less,
    /// Passes on equality.
    Equal,
    /// Passes when less than or equal.
    LessOrEqual,
    /// Passes when greater. **The engine's depth test** under reversed-Z.
    #[default]
    Greater,
    /// Passes on inequality.
    NotEqual,
    /// Passes when greater than or equal. The depth test for a second pass that
    /// must match a prepass exactly under reversed-Z.
    GreaterOrEqual,
    /// Always passes.
    Always,
}

/// Stencil operation on a test outcome.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StencilOp {
    /// Leave the value alone.
    #[default]
    Keep,
    /// Write zero.
    Zero,
    /// Write the reference value.
    Replace,
    /// Bitwise invert.
    Invert,
    /// Increment, clamping at the maximum.
    IncrementClamp,
    /// Decrement, clamping at zero.
    DecrementClamp,
    /// Increment, wrapping.
    IncrementWrap,
    /// Decrement, wrapping.
    DecrementWrap,
}

/// Stencil state for one facing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StencilFaceState {
    /// Comparison against the reference value.
    pub compare: CompareOp,
    /// Applied when the stencil test fails.
    pub fail_op: StencilOp,
    /// Applied when the stencil test passes but depth fails.
    pub depth_fail_op: StencilOp,
    /// Applied when both pass.
    pub pass_op: StencilOp,
}

/// Stencil state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StencilState {
    /// Front-facing primitives.
    pub front: StencilFaceState,
    /// Back-facing primitives.
    pub back: StencilFaceState,
    /// Bits read by the comparison.
    pub read_mask: u32,
    /// Bits the operations may write.
    pub write_mask: u32,
    /// Value compared against.
    pub reference: u32,
}

/// Polygon depth offset.
///
/// # Reversed-Z changes the sign
///
/// With depth increasing towards the viewer, pushing a polygon *away* from the
/// camera — the usual shadow-acne fix — needs a **negative** `constant`, the
/// opposite of every conventional-Z code sample. The magnitude also differs: a
/// float depth buffer's precision varies across the range, so a constant bias
/// tuned for a unorm buffer is meaningless here. Tune against the reversed-Z
/// buffer, not against a tutorial.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DepthBias {
    /// Constant offset, in units of the depth buffer's minimum resolvable
    /// difference.
    pub constant: f32,
    /// Offset scaled by the polygon's depth slope.
    pub slope_scale: f32,
    /// Maximum magnitude. Requires
    /// [`Features::DEPTH_BIAS_CLAMP`](crate::Features::DEPTH_BIAS_CLAMP);
    /// `0.0` disables clamping.
    pub clamp: f32,
}

/// Depth and stencil state.
///
/// [`Default`] is **reversed-Z**: [`CompareOp::Greater`] against a
/// [`Format::D32Float`] buffer cleared to [`depth::CLEAR`](crate::depth::CLEAR)
/// = `0.0`, with depth writes on. Getting this default wrong is exactly the
/// class of mistake that is free to prevent now and costs a re-bless of every
/// golden image later.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthStencilState {
    /// Depth attachment format.
    pub format: Format,
    /// Whether depth is written.
    pub depth_write: bool,
    /// Depth comparison. `Greater` under reversed-Z.
    pub depth_compare: CompareOp,
    /// Stencil state, if the attachment has a stencil plane.
    pub stencil: Option<StencilState>,
    /// Polygon depth offset; see [`DepthBias`] on the sign.
    pub bias: DepthBias,
}

impl Default for DepthStencilState {
    /// Reversed-Z: `D32_SFLOAT`, depth writes on, `GREATER`, no stencil.
    fn default() -> Self {
        Self {
            format: Format::D32Float,
            depth_write: true,
            depth_compare: CompareOp::Greater,
            stencil: None,
            bias: DepthBias::default(),
        }
    }
}

impl DepthStencilState {
    /// Depth-test-only state for a pass drawing against an existing prepass.
    ///
    /// [`CompareOp::GreaterOrEqual`] with writes off, which is the correct pair
    /// under reversed-Z: the fragment's depth must *equal* the prepass value to
    /// survive, and equality is only reachable through the `OrEqual`.
    #[must_use]
    pub fn equal_depth_read_only(format: Format) -> Self {
        Self {
            format,
            depth_write: false,
            depth_compare: CompareOp::GreaterOrEqual,
            stencil: None,
            bias: DepthBias::default(),
        }
    }
}

/// Multisampling state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MultisampleState {
    /// Samples per pixel; `1` disables MSAA.
    pub samples: u32,
    /// Sample coverage mask.
    pub mask: u32,
    /// Derive coverage from the fragment shader's alpha output.
    pub alpha_to_coverage: bool,
}

impl Default for MultisampleState {
    /// No multisampling. The engine's AA plan is FXAA at P7 and TAA post-MVP
    /// (topic 18), so MSAA is available but never the default.
    fn default() -> Self {
        Self {
            samples: 1,
            mask: !0,
            alpha_to_coverage: false,
        }
    }
}

/// A blend factor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendFactor {
    /// `0`.
    Zero,
    /// `1`.
    One,
    /// Source colour.
    Src,
    /// `1 - source colour`.
    OneMinusSrc,
    /// Source alpha.
    SrcAlpha,
    /// `1 - source alpha`.
    OneMinusSrcAlpha,
    /// Destination colour.
    Dst,
    /// `1 - destination colour`.
    OneMinusDst,
    /// Destination alpha.
    DstAlpha,
    /// `1 - destination alpha`.
    OneMinusDstAlpha,
}

/// How blended terms combine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlendOp {
    /// `src * src_factor + dst * dst_factor`.
    #[default]
    Add,
    /// `src * src_factor - dst * dst_factor`.
    Subtract,
    /// `dst * dst_factor - src * src_factor`.
    ReverseSubtract,
    /// Component-wise minimum; factors are ignored.
    Min,
    /// Component-wise maximum; factors are ignored.
    Max,
}

/// Blend state for one colour target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlendState {
    /// Colour source factor.
    pub color_src: BlendFactor,
    /// Colour destination factor.
    pub color_dst: BlendFactor,
    /// Colour combine operation.
    pub color_op: BlendOp,
    /// Alpha source factor.
    pub alpha_src: BlendFactor,
    /// Alpha destination factor.
    pub alpha_dst: BlendFactor,
    /// Alpha combine operation.
    pub alpha_op: BlendOp,
}

impl BlendState {
    /// Straight alpha blending: `src * a + dst * (1 - a)`.
    #[must_use]
    pub const fn alpha() -> Self {
        Self {
            color_src: BlendFactor::SrcAlpha,
            color_dst: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOp::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOp::Add,
        }
    }

    /// Premultiplied alpha: `src + dst * (1 - a)`. The correct mode for
    /// composited UI and for particle atlases authored premultiplied.
    #[must_use]
    pub const fn premultiplied() -> Self {
        Self {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOp::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOp::Add,
        }
    }

    /// Additive: `src + dst`.
    #[must_use]
    pub const fn additive() -> Self {
        Self {
            color_src: BlendFactor::One,
            color_dst: BlendFactor::One,
            color_op: BlendOp::Add,
            alpha_src: BlendFactor::One,
            alpha_dst: BlendFactor::One,
            alpha_op: BlendOp::Add,
        }
    }
}

bitflags::bitflags! {
    /// Which colour channels a target writes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ColorWrites: u32 {
        /// Red.
        const R = 1 << 0;
        /// Green.
        const G = 1 << 1;
        /// Blue.
        const B = 1 << 2;
        /// Alpha.
        const A = 1 << 3;
        /// All four.
        const ALL = Self::R.bits() | Self::G.bits() | Self::B.bits() | Self::A.bits();
    }
}

/// One colour attachment's pipeline state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorTargetState {
    /// Attachment format. For the engine's main pass this is
    /// [`Format::Rgba16Float`] — HDR from P1.
    pub format: Format,
    /// Blend state; `None` disables blending.
    pub blend: Option<BlendState>,
    /// Channels written.
    pub write_mask: ColorWrites,
}

impl ColorTargetState {
    /// An opaque target with all channels written and no blending.
    #[must_use]
    pub const fn opaque(format: Format) -> Self {
        Self {
            format,
            blend: None,
            write_mask: ColorWrites::ALL,
        }
    }
}

/// The viewport transform.
///
/// # `depth` is a range, not a direction
///
/// [`Default`] keeps `depth_min: 0.0, depth_max: 1.0` even though the engine is
/// reversed-Z. Reversed depth is produced by the **projection matrix** (swap the
/// near and far terms, infinite far plane), *not* by setting `depth_min: 1.0,
/// depth_max: 0.0`. Flipping the viewport instead is a well-known trap: it
/// produces visually similar output while throwing away the entire precision
/// benefit, because the float depth buffer's dense exponent range near 0.0 ends
/// up mapped to the far plane again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// Left edge in pixels.
    pub x: f32,
    /// Top edge in pixels.
    pub y: f32,
    /// Width in pixels.
    pub width: f32,
    /// Height in pixels.
    pub height: f32,
    /// Near end of the depth range. See the type docs before changing it.
    pub depth_min: f32,
    /// Far end of the depth range. See the type docs before changing it.
    pub depth_max: f32,
}

impl Viewport {
    /// A viewport covering a `width` × `height` target with the standard
    /// `0.0..=1.0` depth range.
    #[must_use]
    pub fn from_size(width: u32, height: u32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
            depth_min: 0.0,
            depth_max: 1.0,
        }
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::from_size(0, 0)
    }
}

// --- pipelines -------------------------------------------------------------

/// Creation parameters for a graphics pipeline.
///
/// Dynamic-rendering shaped: attachment *formats* are declared here, actual
/// attachments at [`begin_render_pass`](crate::CommandEncoder::begin_render_pass).
/// There is no render-pass or framebuffer object to keep compatible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphicsPipelineDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Resource layout.
    pub layout: PipelineLayoutHandle,
    /// Vertex stage. Reads geometry from storage buffers — see the module docs
    /// on vertex pulling.
    pub vertex: ShaderEntry<'a>,
    /// Fragment stage; `None` for a depth-only pass (shadow maps, prepass).
    pub fragment: Option<ShaderEntry<'a>>,
    /// Rasteriser state.
    pub primitive: PrimitiveState,
    /// Depth/stencil state; `None` for a pass with no depth attachment.
    /// Defaults to reversed-Z when present — see [`DepthStencilState`].
    pub depth_stencil: Option<DepthStencilState>,
    /// Multisampling state.
    pub multisample: MultisampleState,
    /// Colour attachment formats and blend state, in attachment order.
    pub color_targets: &'a [ColorTargetState],
}

/// Creation parameters for a compute pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComputePipelineDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Resource layout.
    pub layout: PipelineLayoutHandle,
    /// Compute stage.
    pub compute: ShaderEntry<'a>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::depth;

    /// The locked decision from `docs/plan/02-vulkan-backend.md`. If this test
    /// fails, someone changed the depth convention — that invalidates every
    /// golden image in the repo, so it needs a deliberate re-bless, not a
    /// test edit.
    #[test]
    fn depth_defaults_are_reversed_z() {
        let state = DepthStencilState::default();
        assert_eq!(state.depth_compare, CompareOp::Greater);
        assert_eq!(state.format, Format::D32Float);
        assert!(state.depth_write);
        assert_eq!(CompareOp::default(), CompareOp::Greater);
    }

    #[test]
    fn equal_depth_pass_uses_greater_or_equal_with_writes_off() {
        let state = DepthStencilState::equal_depth_read_only(Format::D32Float);
        assert_eq!(state.depth_compare, CompareOp::GreaterOrEqual);
        assert!(!state.depth_write);
    }

    /// Reversed-Z lives in the projection matrix; the viewport depth range
    /// stays 0..1. See [`Viewport`] for why conflating them is a trap.
    #[test]
    fn viewport_depth_range_is_not_inverted() {
        let viewport = Viewport::from_size(1920, 1080);
        assert_eq!(viewport.depth_min, 0.0);
        assert_eq!(viewport.depth_max, 1.0);
        assert!(viewport.depth_min < viewport.depth_max);
        assert_eq!(Viewport::default().depth_min, 0.0);
    }

    /// The clear value and the compare op have to agree: with `Greater`, the
    /// cleared buffer must hold the *failing* extreme, or nothing draws.
    #[test]
    fn clear_depth_agrees_with_the_default_compare_op() {
        assert_eq!(depth::CLEAR, 0.0);
        assert_eq!(depth::NEAR, 1.0);
        let state = DepthStencilState::default();
        assert_eq!(state.depth_compare, CompareOp::Greater);
        const {
            assert!(
                depth::NEAR > depth::CLEAR,
                "geometry at the near plane must beat the cleared far value"
            );
        }
    }

    #[test]
    fn blend_presets_differ_only_where_they_should() {
        let alpha = BlendState::alpha();
        let premul = BlendState::premultiplied();
        assert_eq!(alpha.color_src, BlendFactor::SrcAlpha);
        assert_eq!(premul.color_src, BlendFactor::One);
        assert_eq!(alpha.color_dst, premul.color_dst);
        assert_eq!(BlendState::additive().color_dst, BlendFactor::One);
    }

    #[test]
    fn color_writes_all_covers_four_channels() {
        assert_eq!(ColorWrites::ALL.bits().count_ones(), 4);
        assert!(ColorWrites::ALL.contains(ColorWrites::A));
    }

    #[test]
    fn opaque_target_has_no_blending() {
        let target = ColorTargetState::opaque(Format::Rgba16Float);
        assert!(target.blend.is_none());
        assert_eq!(target.write_mask, ColorWrites::ALL);
        assert!(target.format.is_hdr_capable(), "P1 renders to RGBA16F");
    }

    #[test]
    fn primitive_defaults_are_the_common_case() {
        let primitive = PrimitiveState::default();
        assert_eq!(primitive.topology, PrimitiveTopology::TriangleList);
        assert_eq!(primitive.polygon_mode, PolygonMode::Fill);
        assert_eq!(
            primitive.cull_mode,
            CullMode::None,
            "culling must be asked for: the default cannot know the winding of \
             imported geometry"
        );
    }

    #[test]
    fn whole_buffer_binding_uses_the_sentinel() {
        let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
        let buffer: BufferHandle = pool.insert(0).cast();
        let BindingResource::Buffer { offset, size, .. } = BindingResource::whole_buffer(buffer)
        else {
            panic!("wrong variant");
        };
        assert_eq!(offset, 0);
        assert_eq!(size, BindingResource::WHOLE_BUFFER);
    }
}
