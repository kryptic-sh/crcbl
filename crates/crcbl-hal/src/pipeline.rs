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
//! [`BindGroupLayoutEntry`] carries a `count` and [`BindingFlags`]. A backend
//! with [`Features::DESCRIPTOR_INDEXING`](crate::Features::DESCRIPTOR_INDEXING)
//! turns `count: u32::MAX` plus `VARIABLE_COUNT | PARTIALLY_BOUND |
//! UPDATE_AFTER_BIND` into a runtime-sized descriptor array; one without it sees
//! a fixed `count` and produces an ordinary texture array page. Same
//! declaration, both [`BindingModel`](crate::BindingModel)s — which is the point
//! of topic 03's "the lesser path is a constraint on data layout, not a separate
//! renderer".

use crcbl_core::Handle;

use crate::{
    BackendKind, DeviceCaps, Features, Format, HalError, ImageViewHandle, ImageViewType, Limits,
    SamplerHandle, ShaderEntry, ShaderStages, resource::BufferHandle,
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

/// What a sampled image's texels mean to the shader that reads them.
///
/// Carried on [`BindingKind::SampledImage`] for the same reason
/// [`view_type`](BindingKind::SampledImage::view_type) is: WebGPU puts it in the
/// *layout*. `wgpu::BindingType::Texture` has a `sample_type`, a depth-format
/// texture is only bindable as [`Depth`](SampleType::Depth), and a layout that
/// says otherwise is refused at pipeline creation. Vulkan, Metal and D3D12 take
/// the interpretation off the view's format and ignore this — each backend's
/// conversion says where it drops it.
///
/// Two variants and not four: integer and multisampled sampled images are
/// things no shader in this engine declares, and a variant nothing constructs is
/// a variant no backend's mapping was ever checked against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SampleType {
    /// Ordinary filterable colour texels — a `Texture2D<float4>`.
    #[default]
    Float,
    /// A depth texture read through a comparison sampler: HLSL's
    /// `Texture2D<float>` beside a `SamplerComparisonState`, WGSL's
    /// `texture_depth_2d`.
    ///
    /// The [`BindingKind::Sampler`] filtering it must set its
    /// `comparison` flag, and the sampler object
    /// bound into that slot must carry a
    /// [`SamplerDesc::compare`](crate::SamplerDesc::compare) — the layout says
    /// what the shader declared, the descriptor says what the hardware does, and
    /// WebGPU checks the pair.
    Depth,
}

/// What kind of resource a binding slot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingKind {
    /// A uniform buffer.
    UniformBuffer {
        /// Whether the binding takes a dynamic offset at bind time.
        ///
        /// The substitute for push constants where there are none: WebGPU has
        /// no root
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
    SampledImage {
        /// Dimensionality the shader declares for it — the `D2` of a
        /// `Texture2D`, the `D2Array` of a `Texture2DArray`.
        ///
        /// Carried here rather than read off the bound view because WebGPU puts
        /// it in the *layout*: `wgpu::BindingType::Texture` has a
        /// `view_dimension`, and a layout that says `D2` while the view is
        /// `D2Array` is refused at pipeline creation, not at bind time. Vulkan,
        /// Metal and D3D12 all take it from the view instead and ignore this,
        /// which each backend's conversion says where it drops it.
        ///
        /// It must match the view every [`BindingResource::ImageView`] filling
        /// this slot was created with — see
        /// [`ImageViewDesc::view_type`](crate::ImageViewDesc::view_type).
        view_type: ImageViewType,
        /// How the shader reads the texels — see [`SampleType`].
        ///
        /// It must match the format every [`BindingResource::ImageView`] filling
        /// this slot was created with: [`SampleType::Depth`] takes a depth
        /// format and nothing else.
        sample_type: SampleType,
    },
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
    Sampler {
        /// Whether the shader declared it as a comparison sampler — HLSL's
        /// `SamplerComparisonState`, WGSL's `sampler_comparison` — rather than
        /// an ordinary filtering one.
        ///
        /// Here for the same reason [`SampleType`] is: WebGPU puts it in the
        /// layout, as `wgpu::SamplerBindingType::Comparison`, and refuses a
        /// comparison sampler bound through a `Filtering` slot. The other three
        /// backends take it off the sampler object — which carries
        /// [`SamplerDesc::compare`](crate::SamplerDesc::compare) — and ignore
        /// this, saying so at the arm that drops it.
        ///
        /// The two must agree: a `true` here filtering a sampler created with
        /// `compare: None` is a layout describing a sampler that does not exist.
        comparison: bool,
    },
}

bitflags::bitflags! {
    /// Descriptor-indexing behaviour for one binding.
    ///
    /// All three require [`Features::DESCRIPTOR_INDEXING`](crate::Features::DESCRIPTOR_INDEXING).
    /// A backend without it must reject a layout that sets any of them rather than
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
    /// the upper bound. Zero is rejected: a binding holding no descriptors is
    /// one no shader can index.
    ///
    /// [`u32::MAX`] is the sentinel for **"as many as this device can"**, not a
    /// request for 4 294 967 295 descriptors — it is the number the portable
    /// bindless declaration is written with, because the caller cannot know a
    /// device's ceiling before it opens one. Every backend resolves it through
    /// [`resolved_count`](BindGroupLayoutEntry::resolved_count), and it is the
    /// one value exempt from the
    /// [`max_bindless_descriptors`](Limits::max_bindless_descriptors) check in
    /// [`BindGroupLayoutDesc::check_entries`].
    pub count: u32,
    /// Descriptor-indexing behaviour.
    pub flags: BindingFlags,
}

impl BindGroupLayoutEntry {
    /// The descriptor count this binding is actually created with.
    ///
    /// Resolves the [`count`](Self::count) sentinel: [`u32::MAX`] becomes the
    /// device's own
    /// [`max_bindless_descriptors`](Limits::max_bindless_descriptors), and any
    /// other count is left alone. The floor of one is what keeps a device
    /// reporting no bindless descriptors at all from turning the sentinel into
    /// a zero-length array, which is a layout no backend will create.
    #[must_use]
    pub fn resolved_count(&self, limits: &Limits) -> u32 {
        if self.count == u32::MAX {
            limits.max_bindless_descriptors.max(1)
        } else {
            self.count
        }
    }
}

/// Creation parameters for a bind-group layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindGroupLayoutDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Binding slots.
    ///
    /// Order is free **except** for a slot carrying
    /// [`BindingFlags::VARIABLE_COUNT`], which must be both the last element of
    /// this slice *and* the highest [`binding`](BindGroupLayoutEntry::binding)
    /// number in it. Vulkan's rule is the second half — a runtime-sized array
    /// is only legal on the highest-numbered binding of a set — and backends
    /// check the first half too, because a `VARIABLE_COUNT` entry that is
    /// highest-numbered but not last would make every "the variable binding is
    /// `entries.last()`" reading in a backend silently wrong. Violating either
    /// is [`HalError::InvalidDescriptor`].
    ///
    /// Duplicate binding numbers are rejected.
    ///
    /// [`check_entries`](BindGroupLayoutDesc::check_entries) is where all of
    /// that is enforced, once, for every backend.
    pub entries: &'a [BindGroupLayoutEntry],
}

impl BindGroupLayoutDesc<'_> {
    /// Checks [`entries`](Self::entries) against the rules that field states
    /// and against what the device can actually express.
    ///
    /// **Every backend calls this**, and none of them states the rules again.
    /// Each used to write them out for itself, and they drifted apart on which
    /// rules were present at all and on what the refusal said — so a layout
    /// that failed by name on one backend was silently built on another. That
    /// is duplicated *knowledge*: one change to the rule has to alter every
    /// enforcement of it, which is what makes this a seam function rather than
    /// a helper.
    ///
    /// A backend still refuses what only *it* cannot express, and those checks
    /// stay where they are: D3D12's root descriptors take one GPU address, so
    /// an array of dynamic-offset buffers is refused in `crcbl-dx12`; Metal
    /// binds flat argument tables of a fixed size, so `crcbl-mtl` bounds a
    /// layout by them and refuses every [`BindingFlags`] outright.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`], naming the binding, for a
    /// [`count`](BindGroupLayoutEntry::count) of zero, a binding number
    /// declared twice, a [`BindingFlags::VARIABLE_COUNT`] entry that is not
    /// both last and highest-numbered, or an explicit count above
    /// [`Limits::max_bindless_descriptors`] — which the `u32::MAX` sentinel is
    /// exempt from, being a request to clamp rather than an over-large one.
    ///
    /// [`HalError::Unsupported`] when the device lacks
    /// [`Features::DESCRIPTOR_INDEXING`] and an entry sets any
    /// [`BindingFlags`], or when an entry's
    /// [`visibility`](BindGroupLayoutEntry::visibility) names a stage the
    /// device does not report — see
    /// [`ShaderStages::check_supported`](crate::ShaderStages::check_supported).
    pub fn check_entries(&self, caps: &DeviceCaps, backend: BackendKind) -> Result<(), HalError> {
        let indexing = caps.features.contains(Features::DESCRIPTOR_INDEXING);
        for (index, entry) in self.entries.iter().enumerate() {
            // Before the driver sees it. `VK_SHADER_STAGE_MESH_BIT_EXT` in a
            // set layout on a device without `meshShader` is a validation error
            // that arrives from `vkCreateDescriptorSetLayout` naming neither
            // the binding it came from nor the capability that is missing.
            entry.visibility.check_supported(caps.features, backend)?;
            if entry.count == 0 {
                return Err(HalError::InvalidDescriptor(format!(
                    "binding {} has count 0; a binding must hold at least one descriptor",
                    entry.binding
                )));
            }
            if self.entries[..index]
                .iter()
                .any(|earlier| earlier.binding == entry.binding)
            {
                return Err(HalError::InvalidDescriptor(format!(
                    "binding {} is declared twice in one bind group layout",
                    entry.binding
                )));
            }
            if !entry.flags.is_empty() && !indexing {
                // [`BindingFlags`] is explicit that this must be loud: "a
                // bindless array quietly downgraded to a fixed one reads
                // garbage at index 4097."
                return Err(HalError::Unsupported {
                    backend,
                    what: "descriptor-indexing flags on a device without DESCRIPTOR_INDEXING",
                });
            }
            if entry.flags.contains(BindingFlags::VARIABLE_COUNT) {
                // Both halves, and each says which one it is: a layout that
                // breaks one is not the same caller bug as one that breaks the
                // other, and the reader has to know which to move.
                if index + 1 != self.entries.len() {
                    return Err(HalError::InvalidDescriptor(format!(
                        "binding {} sets VARIABLE_COUNT but is not the last entry of its layout; \
                         every \"the variable binding is `entries.last()`\" reading in a backend \
                         depends on it being last",
                        entry.binding
                    )));
                }
                if self
                    .entries
                    .iter()
                    .any(|other| other.binding > entry.binding)
                {
                    return Err(HalError::InvalidDescriptor(format!(
                        "binding {} sets VARIABLE_COUNT but is not the highest-numbered binding of \
                         its layout; a runtime-sized array is only legal on the highest-numbered \
                         binding of a set",
                        entry.binding
                    )));
                }
            }
            // `u32::MAX` is exempt: it is a request to clamp, not a request for
            // four billion descriptors, and checking it here rejected the one
            // shape the bindless model is written in — found by `crcbl-vk`'s
            // e2e suite on radv, where the ceiling is 8 388 606 and the
            // sentinel is larger. See `BindGroupLayoutEntry::resolved_count`.
            if indexing
                && entry.count != u32::MAX
                && entry.count > 1
                && entry.count > caps.limits.max_bindless_descriptors.max(1)
            {
                return Err(HalError::InvalidDescriptor(format!(
                    "binding {} asks for {} descriptors but max_bindless_descriptors is {}",
                    entry.binding, entry.count, caps.limits.max_bindless_descriptors
                )));
            }
        }
        Ok(())
    }
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
    /// Variable descriptor count. Required when the layout includes a
    /// binding with [`BindingFlags::VARIABLE_COUNT`]; tells the backend how
    /// many array elements the caller is *using* (which cannot exceed the
    /// layout's [`BindGroupLayoutEntry::count`] ceiling). `None` means
    /// "infer from entries", which matches the behaviour before this field
    /// existed and is safe for fixed-size arrays.
    pub variable_count: Option<u32>,
}

/// A push-constant / root-constant range.
///
/// Requires [`Features::PUSH_CONSTANTS`](crate::Features::PUSH_CONSTANTS).
///
/// # WebGPU has none — an obligation, not a footnote
///
/// WebGPU has no push constants at all, so a backend on it **must** report
/// [`Features::PUSH_CONSTANTS`](crate::Features::PUSH_CONSTANTS) absent and
/// `max_push_constant_size` as `0`, and **must** fail
/// [`PipelineLayoutDesc::push_constants`] loudly at layout creation rather than
/// accepting the layout and dropping every
/// [`CommandEncoder::push_constants`](crate::CommandEncoder::push_constants)
/// silently. The renderer's substitute is a dynamic-offset uniform buffer — see
/// [`BindingKind::UniformBuffer`] — which is a *data-layout* decision the
/// renderer makes up front, not something a backend can paper over per draw.
/// That is why push constants are deliberately not part of
/// [`Features::GPU_DRIVEN`](crate::Features::GPU_DRIVEN).
///
/// # Decision: the name stays
///
/// P0's seam review flagged "push constant" as Vulkan vocabulary. It is also
/// DX12's concept under the name *root constants* and Metal's under
/// `setBytes`, so it is the common cross-API term rather than a vk-only one —
/// and renaming it would not close the real gap, which is that WebGPU lacks the
/// feature entirely. That gap is a **capability fact**, and it belongs in
/// [`DeviceCaps`] where a backend declares it and a caller
/// branches on it, not in the name of a type that three of four backends
/// implement natively.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PushConstantRange {
    /// Stages that read it.
    pub stages: ShaderStages,
    /// Byte offset within the push-constant block.
    pub offset: u32,
    /// Byte size, bounded by
    /// [`Limits::max_push_constant_size`].
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

/// Creation parameters for a **mesh** pipeline — the primary geometry path.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.5 makes mesh shading the geometry
/// path the engine reaches for first, with the indirect draws in
/// [`GeometryPath`](crate::GeometryPath) as what a device without
/// [`Features::MESH_SHADER`](crate::Features::MESH_SHADER) falls back to.
///
/// # Decision: a sibling descriptor, the *same* handle type
///
/// This is a separate descriptor from [`GraphicsPipelineDesc`] but it produces
/// a [`GraphicsPipelineHandle`], is bound with
/// [`bind_graphics_pipeline`](crate::CommandEncoder::bind_graphics_pipeline)
/// and destroyed with
/// [`destroy_graphics_pipeline`](crate::Device::destroy_graphics_pipeline).
/// Both halves of that were deliberate.
///
/// **A separate descriptor, because the field sets are disjoint where it
/// matters.** A mesh pipeline has no vertex stage, and the only honest way to
/// express that on [`GraphicsPipelineDesc`] would be to make
/// [`vertex`](GraphicsPipelineDesc::vertex) an `Option` and add `mesh` and
/// `task` beside it — three optional stage slots of which exactly two
/// combinations are legal, checked at run time, in a type every existing caller
/// would have to be edited for. There is also nothing for
/// [`PrimitiveState::topology`] to mean here: the topology is fixed by the mesh
/// shader's own `[outputtopology(…)]`, and there is no primitive assembly to
/// configure because there are no input primitives to assemble.
///
/// **The same handle, because on all three native APIs a mesh pipeline is the
/// same kind of object as a raster one.** Vulkan builds it with
/// `vkCreateGraphicsPipelines` and binds it at
/// `VK_PIPELINE_BIND_POINT_GRAPHICS`; D3D12 produces an `ID3D12PipelineState`
/// bound by `SetPipelineState`; Metal produces an `MTLRenderPipelineState` from
/// an `MTLMeshRenderPipelineDescriptor`. A distinct `MeshPipelineHandle` would
/// buy one thing — making "bind a raster pipeline, then
/// [`draw_mesh_tasks`](crate::CommandEncoder::draw_mesh_tasks)" a type error —
/// and this seam does not track that kind of pairing anywhere else either
/// ([`draw`](crate::CommandEncoder::draw) after binding nothing is equally
/// unchecked, and is left to the validation layer). It would cost a second
/// handle type, a second destroy, a second bind, and a second pool in every
/// backend, all aliasing one driver object.
///
/// # No vertex input, in a stronger sense than everywhere else
///
/// The module docs above say the seam has no vertex-buffer layout because
/// vertex pulling is the only geometry path. A mesh shader takes that further:
/// there is no vertex *stage*, no `SV_VertexID`, and no index buffer in play —
/// the mesh stage writes its vertices and its index triples into output arrays
/// and says how many of each it produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshPipelineDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Resource layout.
    pub layout: PipelineLayoutHandle,
    /// Task stage, if any. Runs first and chooses how many mesh workgroups to
    /// launch, which is where per-cluster culling will live (§3.5).
    ///
    /// Requires [`Features::TASK_SHADER`](crate::Features::TASK_SHADER) — a
    /// device may have the mesh stage without it, which is why the seam reports
    /// the two separately.
    pub task: Option<ShaderEntry<'a>>,
    /// Mesh stage. Emits vertices and primitives; there is no vertex stage.
    pub mesh: ShaderEntry<'a>,
    /// Fragment stage; `None` for a depth-only pass, as on
    /// [`GraphicsPipelineDesc`].
    pub fragment: Option<ShaderEntry<'a>>,
    /// Rasteriser state. [`PrimitiveState::topology`] is **ignored**: the mesh
    /// shader's `[outputtopology(…)]` decides it, and there is no input
    /// assembly. Everything else — winding, culling, fill mode, depth clamp —
    /// applies exactly as it does to a raster pipeline.
    pub primitive: PrimitiveState,
    /// Depth/stencil state; `None` for a pass with no depth attachment.
    pub depth_stencil: Option<DepthStencilState>,
    /// Multisampling state.
    pub multisample: MultisampleState,
    /// Colour attachment formats and blend state, in attachment order.
    pub color_targets: &'a [ColorTargetState],
}

/// Creation parameters for a compute pipeline.
///
/// # Why the workgroup size is here at all
///
/// SPIR-V, DXIL and WGSL each bake it into the module — `[numthreads(…)]` in
/// the Slang source becomes `OpExecutionMode … LocalSize`, a DXIL metadata
/// entry, and `@workgroup_size(…)` respectively — so Vulkan and D3D12 need no
/// such field. **Metal has nowhere to declare it.** MSL's `[[kernel]]` carries
/// no thread count, and `dispatchThreadgroups:threadsPerThreadgroup:` takes it
/// at the *call*, which means a Metal backend with only a module and an entry
/// point has no number to pass and cannot dispatch at all. Carrying it on the
/// descriptor is what makes the compute half of this seam expressible on every
/// backend rather than on three of the four.
///
/// The cost is a second place the number lives, and
/// [`workgroup_size`](Self::workgroup_size) says where to take it from so the
/// two cannot be written independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComputePipelineDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Resource layout.
    pub layout: PipelineLayoutHandle,
    /// Compute stage.
    pub compute: ShaderEntry<'a>,
    /// Invocations per workgroup, in the three dimensions — the same numbers
    /// the shader's own `[numthreads(x, y, z)]` declares.
    ///
    /// **Take it from the crate that owns the shader source rather than
    /// writing a literal.** `crcbl-shaders` publishes a `WORKGROUP_SIZE` beside
    /// every compute shader it compiles, checked against the `[numthreads(…)]`
    /// in the `.slang` file by that module's own test, so
    /// `[crcbl_shaders::cull::WORKGROUP_SIZE, 1, 1]` is one number in one place.
    /// A hand-written value that disagrees with the shader is the failure this
    /// field introduces, and it is invisible on the backends that read the size
    /// out of the module: only Metal would launch the wrong number of threads.
    ///
    /// A backend that *can* see the declared size checks this against it and
    /// fails by name — `crcbl-vk` does, from the SPIR-V it is compiling.
    /// [`check_workgroup_size`](Self::check_workgroup_size) is the half every
    /// backend can perform.
    pub workgroup_size: [u32; 3],
}

impl ComputePipelineDesc<'_> {
    /// Checks [`workgroup_size`](Self::workgroup_size) against what the device
    /// can actually launch.
    ///
    /// Every backend calls this, because none of them gets a usable diagnostic
    /// from below: Vulkan reports an over-large size as a VUID against
    /// `vkCreateComputePipelines` naming neither the number nor the dimension,
    /// and Metal raises an Objective-C exception at the dispatch — which aborts
    /// the process rather than returning an error at all.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`], naming the dimension and the limit it
    /// exceeded.
    pub fn check_workgroup_size(&self, limits: &Limits) -> Result<(), HalError> {
        let size = self.workgroup_size;
        for (axis, (extent, max)) in size
            .into_iter()
            .zip(limits.max_compute_workgroup_size)
            .enumerate()
        {
            if extent == 0 {
                return Err(HalError::InvalidDescriptor(format!(
                    "workgroup_size {size:?} is zero in dimension {axis}: a workgroup with no \
                     invocations runs nothing, and no shader declares one"
                )));
            }
            if extent > max {
                return Err(HalError::InvalidDescriptor(format!(
                    "workgroup_size {size:?} exceeds this device's \
                     max_compute_workgroup_size {:?} in dimension {axis}",
                    limits.max_compute_workgroup_size
                )));
            }
        }
        let invocations = size[0]
            .checked_mul(size[1])
            .and_then(|product| product.checked_mul(size[2]))
            .ok_or_else(|| {
                HalError::InvalidDescriptor(format!(
                    "workgroup_size {size:?} overflows a u32 when multiplied out"
                ))
            })?;
        if invocations > limits.max_compute_invocations_per_workgroup {
            return Err(HalError::InvalidDescriptor(format!(
                "workgroup_size {size:?} is {invocations} invocations, over this device's \
                 max_compute_invocations_per_workgroup {}",
                limits.max_compute_invocations_per_workgroup
            )));
        }
        Ok(())
    }
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

    /// A workgroup size no device could launch is refused, and the message
    /// names the dimension.
    ///
    /// **What turns it red.** Dropping the zero check — a `[0, 1, 1]` pipeline
    /// builds on every backend and dispatches nothing, which is exactly the
    /// silent no-op this field exists to make impossible. Comparing against the
    /// wrong axis, or against the total only — the `z` case passes its own
    /// dimension's limit and fails on the product, and the `x` case is the
    /// reverse. Accepting the size that satisfies both limits — the last case,
    /// which is what every real shader here looks like.
    #[test]
    fn a_workgroup_size_the_device_cannot_launch_is_refused_by_dimension() {
        let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
        let layout: PipelineLayoutHandle = pool.insert(0).cast();
        let module: crate::ShaderModuleHandle = pool.insert(0).cast();
        let desc = |workgroup_size| ComputePipelineDesc {
            label: Some("guard"),
            layout,
            compute: ShaderEntry {
                module,
                entry_point: "computeMain",
            },
            workgroup_size,
        };
        let limits = Limits::minimum();

        let zero = desc([0, 1, 1])
            .check_workgroup_size(&limits)
            .expect_err("a workgroup of no invocations");
        assert!(zero.to_string().contains("zero in dimension 0"), "{zero}");

        let wide = desc([limits.max_compute_workgroup_size[0] + 1, 1, 1])
            .check_workgroup_size(&limits)
            .expect_err("over the per-dimension limit");
        assert!(
            wide.to_string().contains("dimension 0"),
            "the dimension must be named: {wide}"
        );

        // Under every per-dimension limit and still too many threads, which is
        // the case a per-axis check alone would let through.
        let deep = [8, 8, limits.max_compute_workgroup_size[2]];
        assert!(
            deep.iter()
                .zip(limits.max_compute_workgroup_size)
                .all(|(extent, max)| *extent <= max),
            "this case must be legal on every axis or it proves nothing"
        );
        let total = desc(deep)
            .check_workgroup_size(&limits)
            .expect_err("over the invocation total");
        assert!(
            total.to_string().contains("invocations"),
            "the total must be named: {total}"
        );

        desc([64, 1, 1])
            .check_workgroup_size(&limits)
            .expect("the size every compute shader in this workspace declares");
    }

    fn caps(features: Features) -> DeviceCaps {
        DeviceCaps {
            features,
            limits: Limits::desktop(),
        }
    }

    fn binding(binding: u32, flags: BindingFlags, count: u32) -> BindGroupLayoutEntry {
        BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::VERTEX,
            kind: BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count,
            flags,
        }
    }

    fn check(entries: &[BindGroupLayoutEntry], caps: &DeviceCaps) -> Result<(), HalError> {
        BindGroupLayoutDesc {
            label: Some("guard"),
            entries,
        }
        .check_entries(caps, BackendKind::Null)
    }

    /// A legal layout passes — the half without which every rejection below is
    /// satisfied by a checker that refuses everything.
    ///
    /// The triangle's own layout is one read-only storage buffer with no flags,
    /// and it must be legal on **both** tiers: vertex pulling is not a Tier A
    /// feature, it is the engine's only geometry path.
    #[test]
    fn the_vertex_pulling_layout_is_legal_on_both_tiers() {
        let entries = [binding(0, BindingFlags::empty(), 1)];
        check(&entries, &caps(Features::GPU_DRIVEN)).expect("Tier A");
        check(&entries, &caps(Features::empty())).expect("Tier B too");

        // And the shape the bindless model is actually written in, on a device
        // that reports the feature.
        let bindless = [
            binding(0, BindingFlags::empty(), 1),
            BindGroupLayoutEntry {
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                ..binding(
                    1,
                    BindingFlags::VARIABLE_COUNT
                        | BindingFlags::PARTIALLY_BOUND
                        | BindingFlags::UPDATE_AFTER_BIND,
                    u32::MAX,
                )
            },
        ];
        check(&bindless, &caps(Features::GPU_DRIVEN))
            .expect("u32::MAX is a request to clamp, not an over-large request");
    }

    /// A binding holding no descriptors is one no shader can index, and wgpu
    /// spells "no count" and "a count of one" the same way — so a zero would
    /// arrive there as an ordinary scalar binding.
    #[test]
    fn a_zero_count_is_refused_by_binding_number() {
        let error = check(
            &[binding(3, BindingFlags::empty(), 0)],
            &caps(Features::GPU_DRIVEN),
        )
        .expect_err("a binding holding nothing");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        let text = error.to_string();
        for expected in ["binding 3", "count 0"] {
            assert!(text.contains(expected), "{expected:?} missing from {text}");
        }
    }

    /// Two entries claiming one binding number is a caller bug, and which of
    /// the two wins is not something the seam decides.
    #[test]
    fn a_duplicated_binding_number_is_refused() {
        let entries = [
            binding(3, BindingFlags::empty(), 1),
            binding(3, BindingFlags::empty(), 1),
        ];
        let error = check(&entries, &caps(Features::GPU_DRIVEN)).expect_err("binding 3 twice");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(error.to_string().contains("twice"), "{error}");
    }

    /// The seam's rule, and the reason it exists: "a bindless array quietly
    /// downgraded to a fixed one reads garbage at index 4097".
    ///
    /// Every flag individually, because one standing for the others is a
    /// reading the loop would hide.
    #[test]
    fn a_device_without_descriptor_indexing_refuses_the_flags_rather_than_ignoring_them() {
        for flag in [
            BindingFlags::PARTIALLY_BOUND,
            BindingFlags::UPDATE_AFTER_BIND,
            BindingFlags::VARIABLE_COUNT,
        ] {
            assert!(!flag.is_empty(), "{flag:?} would prove nothing");
            let entries = [binding(0, flag, 4)];
            let error = check(&entries, &caps(Features::empty()))
                .expect_err("a device without the feature");
            assert!(
                matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Null),
                "{flag:?}: {error:?}"
            );
            check(&entries, &caps(Features::GPU_DRIVEN))
                .unwrap_or_else(|error| panic!("{flag:?} on a device reporting it: {error}"));
        }
    }

    /// A binding visible to a mesh stage is legal on a device that reports the
    /// stage and nowhere else. `GPU_DRIVEN` carries neither mesh flag, which is
    /// what makes the refusing half a real device rather than a contrived one.
    #[test]
    fn a_mesh_visible_binding_needs_the_matching_capability() {
        for (stage, feature) in [
            (ShaderStages::MESH, Features::MESH_SHADER),
            (ShaderStages::TASK, Features::TASK_SHADER),
        ] {
            let entries = [BindGroupLayoutEntry {
                visibility: stage,
                ..binding(0, BindingFlags::empty(), 1)
            }];
            let error = check(&entries, &caps(Features::GPU_DRIVEN))
                .expect_err("GPU_DRIVEN carries neither mesh flag");
            assert!(
                matches!(error, HalError::Unsupported { .. }),
                "{stage:?}: {error}"
            );
            check(&entries, &caps(Features::GPU_DRIVEN | feature))
                .unwrap_or_else(|error| panic!("{stage:?} on a device reporting it: {error}"));
        }
    }

    /// Both halves of the `VARIABLE_COUNT` rule, and each refusal says which
    /// half it is.
    ///
    /// **What turns it red.** Checking only "last in the slice" accepts the
    /// second case, which every driver refuses. Checking only "highest
    /// numbered" accepts the first, which leaves `entries.last()` pointing at
    /// the wrong entry in every backend that reads it that way. Checking
    /// neither accepts the layout the third case builds legally, so the passing
    /// half is what keeps the two apart.
    #[test]
    fn variable_count_must_be_both_last_and_highest_numbered() {
        let tier_a = caps(Features::GPU_DRIVEN);

        // Highest-numbered, but not last in the slice.
        let entries = [
            binding(7, BindingFlags::VARIABLE_COUNT, 8),
            binding(2, BindingFlags::empty(), 1),
        ];
        let error = check(&entries, &tier_a).expect_err("not the last entry");
        assert!(
            error.to_string().contains("not the last entry"),
            "the half that failed must be named: {error}"
        );

        // Last in the slice, but binding 2 sits below binding 7.
        let entries = [
            binding(7, BindingFlags::empty(), 1),
            binding(2, BindingFlags::VARIABLE_COUNT, 8),
        ];
        let error = check(&entries, &tier_a).expect_err("not the highest binding number");
        assert!(
            error
                .to_string()
                .contains("not the highest-numbered binding"),
            "the half that failed must be named: {error}"
        );

        // Both halves satisfied.
        let entries = [
            binding(2, BindingFlags::empty(), 1),
            binding(7, BindingFlags::VARIABLE_COUNT, 8),
        ];
        check(&entries, &tier_a).expect("last and highest");
    }

    /// The sentinel is exempt from the ceiling it is about to be clamped to;
    /// an explicit count above it is still a caller bug.
    ///
    /// **What turns it red.** Dropping the `u32::MAX` exemption — the sentinel
    /// is larger than any real device's ceiling, so the portable bindless
    /// declaration would be refused on every one of them. Dropping the ceiling
    /// check — an explicit over-large count reaches the driver as a VUID naming
    /// neither the binding nor the limit.
    #[test]
    fn the_count_ceiling_exempts_the_sentinel_and_nothing_else() {
        let tier_a = caps(Features::GPU_DRIVEN);
        let ceiling = tier_a.limits.max_bindless_descriptors;
        assert!(ceiling > 1, "a ceiling of {ceiling} would prove nothing");

        check(&[binding(0, BindingFlags::empty(), u32::MAX)], &tier_a)
            .expect("the sentinel is a request to clamp");
        check(&[binding(0, BindingFlags::empty(), ceiling)], &tier_a).expect("exactly the ceiling");

        let error = check(&[binding(0, BindingFlags::empty(), ceiling + 1)], &tier_a)
            .expect_err("one past the ceiling is not a sentinel");
        assert!(
            error.to_string().contains("max_bindless_descriptors"),
            "the limit must be named: {error}"
        );
    }

    /// The sentinel resolves to what the device can do, and never to zero.
    ///
    /// **What turns it red.** Resolving `u32::MAX` verbatim — which is what
    /// `crcbl-wgpu` did, and wgpu refused the layout outright. Clamping an
    /// ordinary count as well, which would silently shrink a fixed array.
    /// Dropping the floor of one, which turns the sentinel into a zero-length
    /// array on a device reporting no bindless descriptors at all.
    #[test]
    fn the_sentinel_resolves_to_the_devices_own_ceiling() {
        let desktop = Limits::desktop();
        assert!(desktop.max_bindless_descriptors > 1);
        assert_eq!(
            binding(0, BindingFlags::VARIABLE_COUNT, u32::MAX).resolved_count(&desktop),
            desktop.max_bindless_descriptors
        );
        assert_eq!(
            binding(0, BindingFlags::empty(), 16).resolved_count(&desktop),
            16,
            "an explicit count is left alone"
        );

        let minimum = Limits::minimum();
        assert_eq!(minimum.max_bindless_descriptors, 0);
        assert_eq!(
            binding(0, BindingFlags::empty(), u32::MAX).resolved_count(&minimum),
            1,
            "a device with no bindless ceiling still owes a creatable binding"
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
