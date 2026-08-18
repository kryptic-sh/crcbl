//! One decoded command, with every borrowed field owned.
//!
//! The counterpart of [`crcbl_hal::null::Command`], and owned for the same
//! reason: the stream outlives the descriptors that produced it. Two things
//! differ from that type, both deliberate.
//!
//! * **Creation and destruction are commands here.** A stream cannot answer
//!   during the call, so the caller allocates the handle itself and the id
//!   travels with the descriptor — see [`crate::StreamWriter::create_buffer`].
//! * **[`Command::PushConstants`] carries the bytes.** The null backend keeps
//!   only their length, because a test asserting on push-constant contents is
//!   asserting on shader ABI. A replayer needs the bytes.
//!
//! Descriptors are flattened into named fields rather than nested, which is what
//! the null backend's `BeginRenderPass` does and for the same reason: the
//! descriptor's lifetime is gone by the time the command exists.

use core::ops::Range;

use crcbl_hal::{
    AdapterId, BindGroupEntry, BindGroupHandle, BindGroupLayoutEntry, BindGroupLayoutHandle,
    BufferBarrier, BufferCopy, BufferHandle, BufferImageCopy, BufferUsage, ColorAttachment,
    ColorTargetState, CommandBufferHandle, CompareOp, CompositeAlpha, ComputePipelineHandle,
    DepthStencilAttachment, DepthStencilState, Extent3d, Features, FilterMode, Format,
    GraphicsPipelineHandle, ImageBarrier, ImageCopy, ImageHandle, ImageSubresourceLayers,
    ImageSubresourceRange, ImageType, ImageUsage, ImageViewHandle, ImageViewType, IndexFormat,
    MemoryLocation, MultisampleState, Offset3d, PipelineLayoutHandle, PresentMode, PrimitiveState,
    PushConstantRange, QueryKind, QuerySetHandle, QueueHandle, ReadbackHandle, Rect2d,
    SamplerAddressMode, SamplerHandle, SemaphoreHandle, SemaphoreSignal, SemaphoreWait,
    ShaderModuleHandle, ShaderStages, SurfaceHandle, SwapchainHandle, Viewport,
};

/// A command decoded out of a stream buffer.
///
/// The variants are a representative subset — see the [crate docs](crate) for
/// which shapes they cover and why the rest are not here yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// [`Device::create_buffer`](crcbl_hal::Device::create_buffer), with the
    /// handle the caller allocated for the object JS is about to create.
    CreateBuffer {
        /// Id the replayer stores the new object at.
        buffer: BufferHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Size in bytes.
        size: u64,
        /// Permitted uses.
        usage: BufferUsage,
        /// Where the memory lives.
        memory: MemoryLocation,
    },
    /// [`Instance::create_surface`](crcbl_hal::Instance::create_surface), with
    /// the handle the caller allocated for it.
    ///
    /// **Only the canvas key crosses, not a [`SurfaceTarget`](crcbl_core::SurfaceTarget).**
    /// Four of that enum's six variants carry `NonNull` pointers to platform
    /// objects, and a pointer must never be transmitted as an integer. So the
    /// encoder takes the `u32` that `Web { canvas_id }` carries and the pointer
    /// variants have nothing here to be encoded into at all.
    ///
    /// **`Offscreen` is the other browser-reachable variant and it is not this
    /// command.** It has no canvas key, and a reserved id standing in for one
    /// would be a magic number two decoders had to agree on. It gets its own
    /// command when the parity gate needs to read frames back, because the
    /// replayer's two jobs genuinely differ: this one resolves a canvas out of
    /// the shim's registry and takes its `webgpu` context, and that one has no
    /// canvas to resolve and must allocate a ring of textures nothing presents.
    CreateSurface {
        /// Id the replayer stores the new object at.
        surface: SurfaceHandle,
        /// Registry key the shell's JS shim assigned the canvas — the whole of
        /// [`SurfaceTarget::Web`](crcbl_core::SurfaceTarget::Web), and a number
        /// rather than a name so no string crosses the boundary.
        canvas_id: u32,
    },
    /// [`Instance::create_surface`](crcbl_hal::Instance::create_surface) for
    /// [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen), with
    /// the handle the caller allocated for it.
    ///
    /// **The offscreen counterpart of [`Command::CreateSurface`], and it carries
    /// only the handle.** Where the canvas command carries a `canvas_id`, this one
    /// carries nothing more: [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen)
    /// names no platform object and — deliberately — no size, and a surface is
    /// created before any device exists and before any swapchain is described. So
    /// there is no context to resolve, no extent to size a ring by, and no format
    /// to configure with at this point; all three arrive with
    /// [`Command::CreateSwapchain`], which is where the replayer allocates the
    /// ring of textures the canvas path would take from `context.getCurrentTexture()`.
    /// This command's whole job is to mark the surface id as offscreen, so that a
    /// later [`Command::CreateSwapchain`] naming it builds an owned ring rather
    /// than configuring a canvas context. See `web/engine/gpu-replay.js`, whose
    /// `#createSurface` documents at length why nothing is configured at
    /// surface-creation time on either path.
    CreateOffscreenSurface {
        /// Id the replayer stores the offscreen surface marker at.
        surface: SurfaceHandle,
    },
    /// [`Device::create_image`](crcbl_hal::Device::create_image), with the
    /// handle the caller allocated for it.
    ///
    /// The whole of [`ImageDesc`](crcbl_hal::ImageDesc), flattened as every
    /// other descriptor here is. **There is no memory location**, and that is
    /// the descriptor's own shape rather than a field dropped in the encoding:
    /// every image the seam creates is
    /// [`MemoryLocation::DeviceLocal`],
    /// so there is nothing for a caller to pass.
    ///
    /// **[`mip_levels`](Self::CreateImage::mip_levels) and
    /// [`samples`](Self::CreateImage::samples) cross verbatim, zero included.**
    /// Zero is meaningless for both, and refusing it here is not this layer's
    /// job: the wire form of a `u32` claims every value, so a decoder that
    /// refused zero would refuse a buffer this crate's own writer produced —
    /// and the writer asserts only what the reader enforces. An invalid
    /// descriptor is a creation failure, and creation failures arrive out of
    /// band through [`Device::take_error`](crcbl_hal::Device::take_error), not
    /// as a panic in the middle of a frame's recording.
    CreateImage {
        /// Id the replayer stores the new object at.
        image: ImageHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Dimensionality.
        image_type: ImageType,
        /// Size, and depth or array-layer count — which of the two
        /// [`Extent3d::depth_or_layers`] means is decided by `image_type`.
        extent: Extent3d,
        /// Texel format.
        format: Format,
        /// Mip levels. Carried through even when zero; see the variant docs.
        mip_levels: u32,
        /// Samples per texel. Carried through even when zero; see the variant
        /// docs.
        samples: u32,
        /// Permitted uses.
        usage: ImageUsage,
    },
    /// [`Device::create_image_view`](crcbl_hal::Device::create_image_view), with
    /// the handle the caller allocated for it.
    ///
    /// The whole of [`ImageViewDesc`](crcbl_hal::ImageViewDesc). **Two handles
    /// cross and they are not interchangeable**: `view` is the id the replayer
    /// stores the new object at, and `image` is the id it looks the viewed
    /// object up by. The opcode is what says which table each indexes, since a
    /// handle carries no kind.
    CreateImageView {
        /// Id the replayer stores the new object at.
        view: ImageViewHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Image being viewed.
        image: ImageHandle,
        /// Dimensionality of the view — the only field saying how
        /// [`range`](Self::CreateImageView::range)'s layers are to be read.
        view_type: ImageViewType,
        /// Format seen through the view, which may differ from the image's.
        format: Format,
        /// Subrange covered. [`ImageSubresourceRange::ALL`] crosses as the
        /// [`u32::MAX`] it is, rather than being resolved here: resolving it
        /// would need the image's own mip and layer counts, which this side of
        /// the boundary does not have.
        range: ImageSubresourceRange,
    },
    /// [`Device::create_sampler`](crcbl_hal::Device::create_sampler), with the
    /// handle the caller allocated for it.
    ///
    /// The whole of [`SamplerDesc`](crcbl_hal::SamplerDesc), flattened as every
    /// other descriptor here is. **The first command on this stream whose body
    /// is mostly floats**, and the first carrying an optional *enum*; both are
    /// spelled out below because each has a value that is easy to get wrong in a
    /// way nothing downstream reports.
    ///
    /// **[`lod_max`](Self::CreateSampler::lod_max) crosses verbatim, sentinel
    /// included.** [`SamplerDesc::default`](crcbl_hal::SamplerDesc) sets it to
    /// [`f32::MAX`], meaning "no limit", and that is the sentinel rule
    /// `docs/plan/41-webgpu-stream.md` states for `WHOLE_BUFFER` and
    /// [`ImageSubresourceRange::ALL`]: a sentinel is a value the seam defines,
    /// and an encoder that resolved one would be answering a question only the
    /// replayer has the information to answer. Here the replayer's answer is
    /// **not** an absent member — WebGPU's `lodMaxClamp` defaults to a *number*
    /// rather than to "the rest", so omitting it would silently substitute that
    /// number for the caller's "no limit". See `web/engine/gpu-replay.js`.
    ///
    /// **[`anisotropy`](Self::CreateSampler::anisotropy) crosses verbatim too**,
    /// fractional and out-of-range values included, for
    /// [`Command::CreateImage`]'s reason: every `f32` bit pattern is a value the
    /// wire form claims, so a decoder that refused one would refuse a buffer
    /// this crate's own writer produced. WebGPU's `maxAnisotropy` is an integer
    /// and the narrowing is the replayer's.
    CreateSampler {
        /// Id the replayer stores the new object at.
        sampler: SamplerHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Filter when magnifying.
        mag_filter: FilterMode,
        /// Filter when minifying.
        min_filter: FilterMode,
        /// Filter between mip levels. Third of three identically typed fields,
        /// which is what makes the order worth pinning: two of them swapped
        /// still decodes, and still builds a sampler.
        mip_filter: FilterMode,
        /// Addressing on U, V and W, in that order.
        address_mode: [SamplerAddressMode; 3],
        /// Lowest mip level sampled.
        lod_min: f32,
        /// Highest mip level sampled. [`f32::MAX`] is the "no limit" sentinel
        /// and crosses as itself; see the variant docs.
        lod_max: f32,
        /// Anisotropy; `1.0` disables. Carried through whatever it is; see the
        /// variant docs.
        anisotropy: f32,
        /// The comparison a hardware-PCF sampler performs, if it is one.
        ///
        /// **Reversed-Z decides which one.** With depth 1.0 at the near plane a
        /// shadow test is [`CompareOp::Greater`], not `Less` —
        /// [`SamplerDesc::compare`](crcbl_hal::SamplerDesc::compare) says so and
        /// [`tag::compare_op_from_code`](crate::tag::compare_op_from_code) says
        /// what folding the pair would cost.
        compare: Option<CompareOp>,
    },
    /// [`Device::create_bind_group_layout`](crcbl_hal::Device::create_bind_group_layout),
    /// with the handle the caller allocated for it.
    ///
    /// The whole of [`BindGroupLayoutDesc`](crcbl_hal::BindGroupLayoutDesc), and
    /// **the first command on this stream carrying a counted list of structs**
    /// rather than of scalars. Two things about that list are the command's
    /// shape rather than the encoding's convenience.
    ///
    /// **Slice order is preserved exactly, and is not rebuilt from binding
    /// numbers.** `docs/plan/41-webgpu-stream.md` says why:
    /// [`BindGroupLayoutDesc::entries`](crcbl_hal::BindGroupLayoutDesc::entries)
    /// is order-sensitive, because a
    /// [`BindingFlags::VARIABLE_COUNT`](crcbl_hal::BindingFlags::VARIABLE_COUNT)
    /// entry must be both last in the slice *and* highest-numbered, and every
    /// "the variable binding is `entries.last()`" reading in a backend depends
    /// on the first half. A decoder that sorted would be sorting away the one
    /// property the slice carries beyond its contents.
    ///
    /// **Nothing in the descriptor is validated here**, which is
    /// [`Command::CreateImage`]'s rule met by a much larger descriptor.
    /// [`BindGroupLayoutDesc::check_entries`](crcbl_hal::BindGroupLayoutDesc::check_entries)
    /// is where the seam's rules live — a zero `count`, a binding number
    /// declared twice, the `VARIABLE_COUNT` ordering, a count past
    /// [`Limits::max_bindless_descriptors`](crcbl_hal::Limits::max_bindless_descriptors),
    /// a visibility naming a stage the device has not got — and **every one of
    /// them is a value the wire form claims**, so none of them is a malformed
    /// stream. The `impl Device` that calls this is what runs `check_entries`;
    /// this crate's decoder refuses only codes and bits no variant claims. See
    /// the [crate docs](crate) and `web/engine/gpu-replay.js`, which states what
    /// the replayer re-checks and what it leaves to the browser.
    CreateBindGroupLayout {
        /// Id the replayer stores the new object at.
        layout: BindGroupLayoutHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Binding slots, **in the descriptor's own order**.
        entries: Vec<BindGroupLayoutEntry>,
    },
    /// [`Device::create_bind_group`](crcbl_hal::Device::create_bind_group), with
    /// the handle the caller allocated for it.
    ///
    /// The whole of [`BindGroupDesc`](crcbl_hal::BindGroupDesc), and **the first
    /// command whose entries carry handles into three different resource
    /// tables**. Each [`BindGroupEntry`]'s
    /// [`resource`](crcbl_hal::BindGroupEntry::resource) is one of
    /// [`BindingResource::Buffer`](crcbl_hal::BindingResource::Buffer),
    /// [`ImageView`](crcbl_hal::BindingResource::ImageView) or
    /// [`Sampler`](crcbl_hal::BindingResource::Sampler), and its discriminant is
    /// the only thing that says which table the replayer resolves the handle
    /// against — a handle carries no kind, so a sampler id and a view id may hold
    /// identical bits and folding one into the other would resolve the wrong
    /// object. See [`tag::binding_resource_code`](crate::tag::binding_resource_code).
    ///
    /// **Slice order is preserved exactly.** Entries are the bindless write path
    /// through their [`array_index`](crcbl_hal::BindGroupEntry::array_index), so
    /// two entries may share a [`binding`](crcbl_hal::BindGroupEntry::binding) and
    /// differ only there; a decoder that rebuilt the list from binding numbers
    /// would lose one.
    ///
    /// **Two sentinels cross verbatim**, as everywhere else on this stream.
    /// [`BindingResource::WHOLE_BUFFER`](crcbl_hal::BindingResource::WHOLE_BUFFER)
    /// in a buffer entry's `size` is `u64::MAX` and is the replayer's to resolve —
    /// here to WebGPU's *absent* `GPUBufferBinding.size`, which means the same "to
    /// the end", unlike `lod_max` whose absence WebGPU reads as a number. And
    /// nothing in the descriptor is validated: a non-zero `array_index` or a
    /// `Some` [`variable_count`](crcbl_hal::BindGroupDesc::variable_count) is a
    /// value the wire form claims, so refusing it is the replayer's job, not this
    /// decoder's. See `web/engine/gpu-replay.js`.
    CreateBindGroup {
        /// Id the replayer stores the new object at.
        group: BindGroupHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Layout the group conforms to — an id the replayer looks up, not one it
        /// fills in.
        layout: BindGroupLayoutHandle,
        /// Assignments, **in the descriptor's own order**.
        entries: Vec<BindGroupEntry>,
        /// Variable descriptor count. `None` is the ordinary value; `Some` names a
        /// runtime-sized array, which WebGPU cannot express.
        variable_count: Option<u32>,
    },
    /// [`Device::create_shader_module`](crcbl_hal::Device::create_shader_module),
    /// with the handle the caller allocated for it.
    ///
    /// The whole of [`ShaderModuleDesc`](crcbl_hal::ShaderModuleDesc), and **the
    /// heaviest single descriptor on the seam**: it carries one field per artifact
    /// format — SPIR-V words, WGSL source, MSL source, DXIL containers — because
    /// the encoding is the seam's rather than WebGPU's, so a decoder must traverse
    /// every field even though a browser consumes only [`wgsl`](Self::CreateShaderModule::wgsl).
    ///
    /// **The four artifacts do not share an absence convention, and the difference
    /// is load-bearing** — [`ShaderModuleDesc`](crcbl_hal::ShaderModuleDesc) spells
    /// each one out, and every one crosses verbatim:
    ///
    /// * [`spirv`](Self::CreateShaderModule::spirv) is words, and empty means
    ///   absent — a zero-word SPIR-V module is not a thing that exists, so the two
    ///   states cannot be confused.
    /// * [`wgsl`](Self::CreateShaderModule::wgsl) and
    ///   [`msl`](Self::CreateShaderModule::msl) are `Option<String>`, because
    ///   `Some("")` is a *valid empty module* — one with no entry points — and is
    ///   not `None`. Conflating them would turn a truncated file into "this backend
    ///   does not get WGSL".
    /// * [`dxil`](Self::CreateShaderModule::dxil) is the list, and its absence is
    ///   the *empty list*; a pair whose container is empty is a truncated artifact,
    ///   not an absent one — the distinction a zero-byte container would lose.
    ///
    /// **Nothing is validated here**, which is [`Command::CreateImage`]'s rule:
    /// [`ShaderModuleDesc::unusable`](crcbl_hal::ShaderModuleDesc::unusable) — the
    /// error a backend owes a module carrying nothing it can compile, WGSL-less
    /// modules included — is the replayer's to raise, because only it knows which
    /// formats this backend accepts. See `web/engine/gpu-replay.js`.
    CreateShaderModule {
        /// Id the replayer stores the new object at.
        module: ShaderModuleHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// SPIR-V words. Empty when there is no SPIR-V artifact — a browser never
        /// consumes these, but a decoder still traverses them to reach the fields
        /// after.
        spirv: Vec<u32>,
        /// WGSL source. `None` is absent and `Some(String::new())` is a valid
        /// empty module; the two stay distinct.
        wgsl: Option<String>,
        /// MSL source, on [`wgsl`](Self::CreateShaderModule::wgsl)'s terms.
        msl: Option<String>,
        /// DXIL containers, each paired with the entry point it was compiled for,
        /// **in the descriptor's own order**. Empty is absent; a pair with an
        /// empty container is a truncated artifact.
        dxil: Vec<(String, Vec<u8>)>,
    },
    /// [`Device::create_pipeline_layout`](crcbl_hal::Device::create_pipeline_layout),
    /// with the handle the caller allocated for it.
    ///
    /// The whole of [`PipelineLayoutDesc`](crcbl_hal::PipelineLayoutDesc), and
    /// the last thing a pipeline is built from that this seam did not already
    /// carry — shader modules and bind-group layouts both ship. Two of its three
    /// fields are the command's shape rather than the encoding's convenience.
    ///
    /// **[`bind_group_layouts`](Self::CreatePipelineLayout::bind_group_layouts)
    /// is a counted list of bare handles, in set order, and is not sorted or
    /// rebuilt.** `GPUPipelineLayoutDescriptor.bindGroupLayouts` is an array in
    /// set order, and set order is what a shader's `@group(n)` indexes — a
    /// decoder that reordered it would build a layout binding the wrong set to
    /// the wrong slot. Each handle resolves against the **bind-group-layout
    /// table** the layout command fills, and a handle that is stale, never
    /// created, or the wrong kind is a failure the replayer routes to the error
    /// queue naming which set index could not be resolved.
    ///
    /// **[`push_constants`](Self::CreatePipelineLayout::push_constants) crosses
    /// verbatim, and the replayer refuses a `Some`.** WebGPU has no push
    /// constants at all, so a `Some(_)` is the "WebGPU cannot express it" case
    /// [`Device::create_pipeline_layout`](crcbl_hal::Device::create_pipeline_layout)'s
    /// doc requires to fail *loudly rather than dropping the writes later* — the
    /// same judgement `web/engine/gpu-replay.js` already makes for
    /// [`BufferUsage::DEVICE_ADDRESS`](crcbl_hal::BufferUsage::DEVICE_ADDRESS)
    /// and a `VARIABLE_COUNT` layout. The whole
    /// [`PushConstantRange`] is on the wire anyway
    /// — the writer carries what the caller gives, the replayer refuses what
    /// WebGPU can't do — so it round-trips in Rust even though no browser builds
    /// it. A `None` proceeds, and an empty `bind_group_layouts` list with `None`
    /// is the empty pipeline layout, which must build.
    ///
    /// **Nothing is validated here**, which is [`Command::CreateImage`]'s rule:
    /// the errors [`Device::create_pipeline_layout`](crcbl_hal::Device::create_pipeline_layout)'s
    /// doc lists — push constants without
    /// [`Features::PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS), a
    /// mesh/task stage the device does not report — are the replayer's to raise,
    /// because only it faces WebGPU. See `web/engine/gpu-replay.js`.
    CreatePipelineLayout {
        /// Id the replayer stores the new object at.
        layout: PipelineLayoutHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Bind-group layouts, **in set order** — an id the replayer looks up,
        /// not one it fills in.
        bind_group_layouts: Vec<BindGroupLayoutHandle>,
        /// Push-constant range, if any. `None` is the ordinary value; `Some`
        /// names a range WebGPU cannot express, which the replayer refuses.
        push_constants: Option<PushConstantRange>,
    },
    /// [`Device::create_compute_pipeline`](crcbl_hal::Device::create_compute_pipeline),
    /// with the handle the caller allocated for it.
    ///
    /// The whole of [`ComputePipelineDesc`](crcbl_hal::ComputePipelineDesc),
    /// flattened as every other descriptor here is — and **the first command
    /// resolving handles into two *different* non-buffer tables**:
    /// [`layout`](Self::CreateComputePipeline::layout) is a
    /// [`PipelineLayoutHandle`] and [`module`](Self::CreateComputePipeline::module)
    /// a [`ShaderModuleHandle`], so the replayer looks each up in a different table
    /// and a miss on either is a failure naming *which* — the layout or the module —
    /// could not be resolved. A handle carries no kind, so the two could hold
    /// identical bits.
    ///
    /// **[`workgroup_size`](Self::CreateComputePipeline::workgroup_size) is on the
    /// wire and the replayer IGNORES it, and that is not a refusal.** WebGPU — like
    /// Vulkan — reads the workgroup size from the shader's `@workgroup_size(x, y, z)`
    /// attribute, not from the pipeline descriptor: `GPUComputePipelineDescriptor`
    /// has no member for it. Only Metal reads it from the descriptor, which is why
    /// [`ComputePipelineDesc`](crcbl_hal::ComputePipelineDesc) carries it at all.
    /// So it crosses because the HAL descriptor has it and Metal needs it, and the
    /// replayer drops it because the authoritative copy is in the WGSL the module
    /// already carries — dropping it changes nothing, unlike dropping a
    /// push-constant range, which would lose data. It still round-trips in Rust:
    /// all three components cross so a transposition is visible. See
    /// `web/engine/gpu-replay.js`.
    ///
    /// **[`entry_point`](Self::CreateComputePipeline::entry_point) is a bare
    /// string**, always present — the seam does not lean on WebGPU's rule that it
    /// may be omitted when a module has a single entry point — and becomes
    /// `GPUProgrammableStage.entryPoint`.
    ///
    /// **Nothing is validated here**, which is [`Command::CreateImage`]'s rule:
    /// [`ComputePipelineDesc::check_workgroup_size`](crcbl_hal::ComputePipelineDesc::check_workgroup_size)
    /// and the errors an unresolvable handle raises are the replayer's, because
    /// only it faces WebGPU. See `web/engine/gpu-replay.js`.
    CreateComputePipeline {
        /// Id the replayer stores the new object at.
        pipeline: ComputePipelineHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Resource layout — an id the replayer looks up in the pipeline-layout
        /// table, not one it fills in.
        layout: PipelineLayoutHandle,
        /// Compute stage's module — an id the replayer looks up in the
        /// shader-module table, a *different* table from `layout`'s.
        module: ShaderModuleHandle,
        /// Compute stage's entry point, as it appears in the module.
        entry_point: String,
        /// Invocations per workgroup, in the three dimensions. Carried because
        /// the HAL descriptor has it and Metal needs it; the WebGPU replayer
        /// drops it, reading the real value from the module — see the variant
        /// docs.
        workgroup_size: [u32; 3],
    },
    /// [`Device::create_graphics_pipeline`](crcbl_hal::Device::create_graphics_pipeline),
    /// with the handle the caller allocated for it.
    ///
    /// The whole of [`GraphicsPipelineDesc`](crcbl_hal::GraphicsPipelineDesc),
    /// flattened as every other descriptor here is — and **the largest descriptor
    /// on the seam**, the one `docs/plan/41-webgpu-stream.md` singles out for its
    /// depth-stencil chain: an `Option<DepthStencilState>` holding an
    /// `Option<StencilState>` of two `StencilFaceState`s of leaf enums, three
    /// levels of nesting where a decoder written to a shallower assumption is
    /// wrong first.
    ///
    /// **Two handles and two shader modules cross, into two tables.**
    /// [`layout`](Self::CreateGraphicsPipeline::layout) is a
    /// [`PipelineLayoutHandle`] the replayer looks up in the pipeline-layout
    /// table; [`vertex_module`](Self::CreateGraphicsPipeline::vertex_module) and
    /// the module inside
    /// [`fragment`](Self::CreateGraphicsPipeline::fragment) — when present — are
    /// [`ShaderModuleHandle`]s it looks up in the shader-module table. A handle
    /// carries no kind, so a miss on any one is a failure the replayer names —
    /// layout, vertex module, or fragment module — three distinct messages.
    ///
    /// **The vertex stage carries no buffer layout**, and that is the descriptor's
    /// own shape rather than a field dropped here: the engine pulls geometry from
    /// storage buffers, so `GPUVertexState.buffers` is always the empty array and
    /// there is nothing on the wire for it. See
    /// [`GraphicsPipelineDesc`](crcbl_hal::GraphicsPipelineDesc)'s module docs.
    ///
    /// **[`fragment: None`](Self::CreateGraphicsPipeline::fragment) is a
    /// depth-only pass** — a shadow map or a prepass — and the replayer omits the
    /// `GPUFragmentState` member entirely; a `Some` becomes one with its own
    /// `targets`.
    ///
    /// **Nothing is validated here**, which is [`Command::CreateImage`]'s rule met
    /// by the deepest descriptor: the "WebGPU cannot express it" refusals —
    /// [`polygon_mode: Line`](crcbl_hal::PolygonMode::Line),
    /// [`depth_clamp`](crcbl_hal::PrimitiveState::depth_clamp) without
    /// `depth-clip-control`, a
    /// [`samples`](crcbl_hal::MultisampleState::samples) count that is neither `1`
    /// nor `4`, a fractional
    /// [`DepthBias.constant`](crcbl_hal::DepthBias::constant), a feature-gated
    /// [`format`](crcbl_hal::ColorTargetState) — are the replayer's, because only
    /// it faces WebGPU, and each is a value the wire form claims. The stencil
    /// [`reference`](crcbl_hal::StencilState::reference) crosses too and the
    /// replayer drops it: it is not a pipeline field in WebGPU but a per-pass one,
    /// so it is dropped like `workgroup_size` rather than lost, and
    /// [`SetStencilReference`](Self::SetStencilReference) is the command that
    /// carries the value the draws actually compare against. See
    /// `web/engine/gpu-replay.js`.
    CreateGraphicsPipeline {
        /// Id the replayer stores the new object at.
        pipeline: GraphicsPipelineHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Resource layout — an id the replayer looks up in the pipeline-layout
        /// table, not one it fills in.
        layout: PipelineLayoutHandle,
        /// Vertex stage's module — an id the replayer looks up in the
        /// shader-module table.
        vertex_module: ShaderModuleHandle,
        /// Vertex stage's entry point, as it appears in the module.
        vertex_entry_point: String,
        /// Fragment stage's module and entry point, `None` for a depth-only pass.
        /// The two ride together because they are present or absent together;
        /// `Some` names a module the replayer looks up in the same table as
        /// [`vertex_module`](Self::CreateGraphicsPipeline::vertex_module).
        fragment: Option<(ShaderModuleHandle, String)>,
        /// Rasteriser and primitive-assembly state.
        primitive: PrimitiveState,
        /// Depth/stencil state; `None` for a pass with no depth attachment. The
        /// deepest optional chain on the seam — see the variant docs.
        depth_stencil: Option<DepthStencilState>,
        /// Multisampling state.
        multisample: MultisampleState,
        /// Colour attachment formats and blend state, **in attachment order**.
        /// Empty is a real depth-only-ish case a fragment that writes nothing
        /// still builds.
        color_targets: Vec<ColorTargetState>,
    },
    /// [`Device::create_query_set`](crcbl_hal::Device::create_query_set), with
    /// the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`CreateBuffer`](Self::CreateBuffer)'s
    /// reason, and it is a *creation* rather than a query-family command for the
    /// same one: it names a handle and a descriptor. See
    /// [`CREATE_QUERY_SET_TAG`](crate::tag::CREATE_QUERY_SET_TAG).
    ///
    /// **Only [`QueryKind::Occlusion`] ever
    /// reaches a browser through this backend.** `GPUQueryType` is exactly
    /// `'occlusion'` and `'timestamp'`, and the timestamp one is refused at the
    /// seam before it is encoded — WebGPU takes timestamps only through a pass's
    /// `timestampWrites`, so a set this backend created could never be written.
    /// The other two kinds are still carried on the wire so that a fold between
    /// them is a decode failure rather than the wrong pool, and the replayer
    /// refuses each by name.
    CreateQuerySet {
        /// Id the replayer stores the new object at.
        set: QuerySetHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// What it measures.
        kind: QueryKind,
        /// How many queries it holds. Query indices are `0..count`.
        count: u32,
    },
    /// [`Device::destroy_buffer`](crcbl_hal::Device::destroy_buffer).
    ///
    /// A destroy naming an id whose slot holds nothing is a **no-op for the
    /// replayer, not an error** — see the [crate docs](crate#destroying-what-was-never-created).
    DestroyBuffer {
        /// Id to release.
        buffer: BufferHandle,
    },
    /// [`Instance::destroy_surface`](crcbl_hal::Instance::destroy_surface).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is.
    DestroySurface {
        /// Id to release.
        surface: SurfaceHandle,
    },
    /// [`Device::destroy_image`](crcbl_hal::Device::destroy_image).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is.
    DestroyImage {
        /// Id to release.
        image: ImageHandle,
    },
    /// [`Device::destroy_image_view`](crcbl_hal::Device::destroy_image_view).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is. **A view's id is its own**, from a table
    /// the image's id never indexes, so destroying a view is not destroying
    /// what it views.
    DestroyImageView {
        /// Id to release.
        view: ImageViewHandle,
    },
    /// [`Device::destroy_sampler`](crcbl_hal::Device::destroy_sampler).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is.
    DestroySampler {
        /// Id to release.
        sampler: SamplerHandle,
    },
    /// [`Device::destroy_bind_group_layout`](crcbl_hal::Device::destroy_bind_group_layout).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is — and here the empty slot is the *ordinary*
    /// case rather than an edge one, because a layout this backend refused to
    /// build still has its handle destroyed by the caller that pre-allocated it.
    DestroyBindGroupLayout {
        /// Id to release.
        layout: BindGroupLayoutHandle,
    },
    /// [`Device::destroy_bind_group`](crcbl_hal::Device::destroy_bind_group).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is.
    DestroyBindGroup {
        /// Id to release.
        group: BindGroupHandle,
    },
    /// [`Device::destroy_shader_module`](crcbl_hal::Device::destroy_shader_module).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is — and this destroy is the one `crcbl-render`
    /// leans on hardest, since it pre-allocates a module handle, destroys it, and
    /// only then applies `?` to the creation `Result`, so the id may name a
    /// creation that turned out to fail.
    DestroyShaderModule {
        /// Id to release.
        module: ShaderModuleHandle,
    },
    /// [`Device::destroy_pipeline_layout`](crcbl_hal::Device::destroy_pipeline_layout).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is — and, like
    /// [`Command::DestroyBindGroupLayout`], the empty slot is the *ordinary*
    /// case rather than an edge one: a layout the replayer refused (a `Some`
    /// push-constant range, an unresolvable bind-group layout) still has its
    /// handle destroyed by the caller that pre-allocated it.
    DestroyPipelineLayout {
        /// Id to release.
        layout: PipelineLayoutHandle,
    },
    /// [`Device::destroy_compute_pipeline`](crcbl_hal::Device::destroy_compute_pipeline).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is — and, like the pipeline-layout and
    /// bind-group-layout destroys, the empty slot is an *ordinary* case rather
    /// than an edge one: a pipeline the replayer refused (an unresolvable layout
    /// or module) still has its handle destroyed by the caller that pre-allocated
    /// it.
    DestroyComputePipeline {
        /// Id to release.
        pipeline: ComputePipelineHandle,
    },
    /// [`Device::destroy_graphics_pipeline`](crcbl_hal::Device::destroy_graphics_pipeline).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is — and, like the compute-pipeline destroy, the
    /// empty slot is an *ordinary* case rather than an edge one: a pipeline the
    /// replayer refused (a `Line` polygon mode, an unresolvable layout or module,
    /// a `samples` count WebGPU forbids) still has its handle destroyed by the
    /// caller that pre-allocated it.
    DestroyGraphicsPipeline {
        /// Id to release.
        pipeline: GraphicsPipelineHandle,
    },
    /// [`Device::destroy_query_set`](crcbl_hal::Device::destroy_query_set).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is — and, as with the pipelines, the empty slot
    /// is ordinary rather than an edge case: a caller that asked for a timestamp
    /// set got an `Err` and still destroys the handle it pre-allocated.
    DestroyQuerySet {
        /// Id to release.
        set: QuerySetHandle,
    },
    /// [`begin_debug_label`](crcbl_hal::CommandEncoder::begin_debug_label) —
    /// opens a named region, which becomes `pushDebugGroup(label)`.
    ///
    /// **On whichever scope is open**, and that is the whole subtlety of the
    /// three debug commands: `pushDebugGroup` exists on the command encoder, on
    /// a render pass encoder and on a compute pass encoder, and they are
    /// different objects with independent group stacks. The replayer pushes onto
    /// the innermost open one and remembers which, so
    /// [`EndDebugLabel`](Self::EndDebugLabel) pops the object that pushed rather
    /// than whatever happens to be open when it arrives.
    BeginDebugLabel {
        /// Region name.
        label: String,
    },
    /// [`end_debug_label`](crcbl_hal::CommandEncoder::end_debug_label) — closes
    /// the region [`BeginDebugLabel`](Self::BeginDebugLabel) opened, which
    /// becomes `popDebugGroup()`.
    ///
    /// Body-less: the region it closes is the innermost open one, exactly as
    /// [`EndRenderPass`](Self::EndRenderPass) closes the open pass without
    /// naming it. Popping with no region open is a malformed stream the replayer
    /// routes to the error queue rather than throwing on.
    EndDebugLabel,
    /// [`insert_debug_marker`](crcbl_hal::CommandEncoder::insert_debug_marker) —
    /// a point-in-time marker, which becomes `insertDebugMarker(label)` on
    /// whichever scope is open.
    ///
    /// Its own command rather than a flag on
    /// [`BeginDebugLabel`](Self::BeginDebugLabel): a marker opens no region, so
    /// folding the two would leave an unbalanced `pushDebugGroup` behind every
    /// marker — and WebGPU refuses the whole `finish()` over an unbalanced
    /// group.
    InsertDebugMarker {
        /// Marker name.
        label: String,
    },
    /// [`begin_render_pass`](crcbl_hal::CommandEncoder::begin_render_pass).
    BeginRenderPass {
        /// Pass label, if the caller gave one.
        label: Option<String>,
        /// Colour attachments, in shader output order.
        color_attachments: Vec<ColorAttachment>,
        /// Depth/stencil attachment.
        depth_stencil_attachment: Option<DepthStencilAttachment>,
        /// Region rendered.
        render_area: Rect2d,
    },
    /// [`bind_graphics_pipeline`](crcbl_hal::CommandEncoder::bind_graphics_pipeline).
    BindGraphicsPipeline {
        /// Pipeline bound.
        pipeline: GraphicsPipelineHandle,
    },
    /// [`bind_group`](crcbl_hal::CommandEncoder::bind_group).
    BindGroup {
        /// Set index.
        slot: u32,
        /// Group bound.
        group: BindGroupHandle,
        /// Dynamic offsets supplied, in binding order.
        dynamic_offsets: Vec<u32>,
        /// Layout the binding is against — the last parameter of the HAL call,
        /// and the one most easily dropped when writing an encoder by hand.
        layout: PipelineLayoutHandle,
    },
    /// [`push_constants`](crcbl_hal::CommandEncoder::push_constants).
    PushConstants {
        /// Stages written.
        stages: ShaderStages,
        /// Byte offset within the block.
        offset: u32,
        /// Bytes written.
        data: Vec<u8>,
        /// Layout the write is against; see [`Command::BindGroup::layout`].
        layout: PipelineLayoutHandle,
    },
    /// [`set_viewport`](crcbl_hal::CommandEncoder::set_viewport) — dynamic
    /// viewport state on the open render pass.
    ///
    /// **Set by the render graph on every pass**, not baked into a pipeline, so a
    /// resize never rebuilds one. It becomes WebGPU's `setViewport(x, y, w, h,
    /// minDepth, maxDepth)` on the current pass encoder. The whole of
    /// [`Viewport`] crosses — the six floats in declaration order — so a
    /// transposition among them is visible; the corpus picks distinct values for
    /// each.
    SetViewport {
        /// Viewport rectangle and depth range.
        viewport: Viewport,
    },
    /// [`set_scissor`](crcbl_hal::CommandEncoder::set_scissor) — dynamic scissor
    /// state on the open render pass.
    ///
    /// The companion of [`SetViewport`](Self::SetViewport), recorded by the graph
    /// alongside it, and it becomes `setScissorRect(x, y, w, h)`. The whole of
    /// [`Rect2d`] crosses; `x`/`y` are signed on the wire — WebGPU's
    /// `setScissorRect` takes unsigned coordinates, so the narrowing (and any
    /// refusal of a negative) is the replayer's.
    SetScissor {
        /// Scissor rectangle.
        rect: Rect2d,
    },
    /// [`set_stencil_reference`](crcbl_hal::CommandEncoder::set_stencil_reference)
    /// — the value subsequent draws compare the stencil plane against, on the
    /// open render pass.
    ///
    /// [`SetScissor`](Self::SetScissor)'s shape with a `u32` instead of a
    /// rectangle, and it becomes `setStencilReference(reference)`. **The whole
    /// `u32` crosses and nothing narrows it**: WebGPU takes a `GPUStencilValue`,
    /// which is a `u32` too, and the pipeline's own
    /// [`StencilState::read_mask`](crcbl_hal::StencilState::read_mask) is what
    /// decides which of its bits the comparison sees.
    ///
    /// It is a *pass* state and not a pipeline field, which is why
    /// [`CreateGraphicsPipeline`](Self::CreateGraphicsPipeline) carries a
    /// `reference` the replayer drops: a pipeline built once serves passes that
    /// compare against different values, so the value has to arrive per pass.
    /// WebGPU's initial value for a fresh pass is `0`, so a pass that never
    /// records this compares against zero rather than against whatever the last
    /// pass set.
    SetStencilReference {
        /// Value subsequent draws compare against.
        reference: u32,
    },
    /// [`bind_index_buffer`](crcbl_hal::CommandEncoder::bind_index_buffer) — the
    /// index buffer for subsequent indexed draws, on the open render pass.
    ///
    /// It becomes WebGPU's `setIndexBuffer(buffer, format, offset)`. The
    /// [`IndexFormat`] is a byte on the wire — folding its two values is not a
    /// refusal but an index buffer read at the wrong stride, so it crosses as its
    /// own code. See [`Command::BindIndexBuffer`](Self::BindIndexBuffer) and
    /// [`tag::index_format_from_code`](crate::tag::index_format_from_code).
    BindIndexBuffer {
        /// Index buffer bound.
        buffer: BufferHandle,
        /// Byte offset the indices start at.
        offset: u64,
        /// Index width.
        format: IndexFormat,
    },
    /// [`draw`](crcbl_hal::CommandEncoder::draw).
    Draw {
        /// Vertex range.
        vertices: Range<u32>,
        /// Instance range.
        instances: Range<u32>,
    },
    /// [`draw_indexed`](crcbl_hal::CommandEncoder::draw_indexed) — an indexed
    /// draw on the open render pass, the path the UI pass takes.
    ///
    /// It becomes `drawIndexed(indexCount, instanceCount, firstIndex, baseVertex,
    /// firstInstance)`: the index range's span and start, the instance range's
    /// span and start, and the `base_vertex` between them. **`base_vertex` is
    /// signed** — an index buffer shared by meshes at different vertex offsets
    /// subtracts — so it crosses as `i32`, and the corpus picks distinct values
    /// for every field so a transposition among the five is visible.
    DrawIndexed {
        /// Index range: `indexCount` is its span, `firstIndex` its start.
        indices: Range<u32>,
        /// Value added to each index before the vertex fetch. Signed.
        base_vertex: i32,
        /// Instance range: `instanceCount` is its span, `firstInstance` its start.
        instances: Range<u32>,
    },
    /// [`draw_indirect`](crcbl_hal::CommandEncoder::draw_indirect) — an indirect
    /// non-indexed draw with a CPU-known count, on the open render pass.
    ///
    /// **Unrolled on replay.** WebGPU's `drawIndirect(indirectBuffer,
    /// indirectOffset)` is a SINGLE draw — multi-draw-indirect is not core WebGPU
    /// — so a HAL multi-draw becomes `draw_count` calls, the `i`th reading its
    /// argument structure at `offset + i * stride`. `draw_count == 1` is the
    /// common case and is one call; a `draw_count > 1` is that many WebGPU calls.
    /// The argument structure WebGPU reads is `[vertexCount, instanceCount,
    /// firstVertex, firstInstance]` (four `u32`), the standard indirect layout.
    /// `buffer`, `offset`, `draw_count` and `stride` cross as distinct values so a
    /// transposition among them is visible.
    DrawIndirect {
        /// Buffer holding the argument structures.
        buffer: BufferHandle,
        /// Byte offset of the first argument structure.
        offset: u64,
        /// Number of argument structures to read — the unroll count on replay.
        draw_count: u32,
        /// Bytes between consecutive argument structures.
        stride: u32,
    },
    /// [`draw_indexed_indirect`](crcbl_hal::CommandEncoder::draw_indexed_indirect)
    /// — an indirect indexed draw with a CPU-known count, on the open render pass;
    /// [`GeometryPath::IndirectPerBatch`](crcbl_hal::GeometryPath::IndirectPerBatch)'s
    /// draw path.
    ///
    /// **Unrolled on replay**, exactly as [`DrawIndirect`](Self::DrawIndirect) is:
    /// WebGPU's `drawIndexedIndirect(indirectBuffer, indirectOffset)` is a single
    /// draw, so `draw_count` calls are emitted, the `i`th reading `offset + i *
    /// stride`. The argument structure WebGPU reads here is `[indexCount,
    /// instanceCount, firstIndex, baseVertex, firstInstance]` (five `u32`), the
    /// standard indexed-indirect layout. Fields cross distinct so a transposition
    /// among them is visible.
    DrawIndexedIndirect {
        /// Buffer holding the argument structures.
        buffer: BufferHandle,
        /// Byte offset of the first argument structure.
        offset: u64,
        /// Number of argument structures to read — the unroll count on replay.
        draw_count: u32,
        /// Bytes between consecutive argument structures.
        stride: u32,
    },
    /// [`begin_compute_pass`](crcbl_hal::CommandEncoder::begin_compute_pass).
    ///
    /// The whole of [`ComputePassDesc`](crcbl_hal::ComputePassDesc), which is
    /// **only a label** — compute has no attachments, so the pass exists to name
    /// a timestamp boundary and nothing more. On the implicit-current encoder,
    /// exactly as [`BeginRenderPass`](Self::BeginRenderPass) is.
    BeginComputePass {
        /// Pass label, if the caller gave one.
        label: Option<String>,
    },
    /// [`bind_compute_pipeline`](crcbl_hal::CommandEncoder::bind_compute_pipeline).
    BindComputePipeline {
        /// Pipeline bound.
        pipeline: ComputePipelineHandle,
    },
    /// [`dispatch`](crcbl_hal::CommandEncoder::dispatch) — the workgroup counts
    /// for a compute dispatch on the open compute pass.
    Dispatch {
        /// Workgroups in x.
        x: u32,
        /// Workgroups in y.
        y: u32,
        /// Workgroups in z.
        z: u32,
    },
    /// [`dispatch_indirect`](crcbl_hal::CommandEncoder::dispatch_indirect) — a
    /// compute dispatch whose three workgroup counts are read out of a buffer,
    /// on the open compute pass.
    ///
    /// **Not unrolled**, unlike the indirect draws: WebGPU's
    /// `dispatchWorkgroupsIndirect(indirectBuffer, indirectOffset)` is a single
    /// dispatch and so is the HAL call, so the two map one to one and the
    /// command carries no count or stride. The argument structure WebGPU reads
    /// is `[workgroupCountX, workgroupCountY, workgroupCountZ]` (three `u32`).
    ///
    /// **The counts come from buffer contents at replay time**, so observing one
    /// end to end needs those bytes written first — this seam's
    /// [`WriteBuffer`](Self::WriteBuffer) is the upload that can do it, since
    /// [`FillBuffer`](Self::FillBuffer) only clears to zero on WebGPU.
    DispatchIndirect {
        /// Buffer holding the three workgroup counts.
        buffer: BufferHandle,
        /// Byte offset of the argument structure.
        offset: u64,
    },
    /// [`end_compute_pass`](crcbl_hal::CommandEncoder::end_compute_pass).
    ///
    /// Body-less, and on the implicit-current encoder: it closes the pass
    /// [`BeginComputePass`](Self::BeginComputePass) opened. Ending with no open
    /// pass is a malformed stream the replayer routes to the error queue rather
    /// than throwing on — see `web/engine/gpu-replay.js`.
    EndComputePass,
    /// [`reset_query_set`](crcbl_hal::CommandEncoder::reset_query_set) — **the
    /// documented no-op**, on the implicit-current encoder.
    ///
    /// WebGPU has no reset. A `GPUQuerySet` is not a pool a caller re-arms, and
    /// the specification defines an unwritten query to resolve as zero, so there
    /// is nothing for a reset to do and nothing it could break. The seam still
    /// requires every caller to record one — Vulkan forbids reading a pool that
    /// was never reset, and making everybody call it keeps the Vulkan path from
    /// being the special case — so the command crosses whole and the replayer
    /// records nothing, exactly as [`PipelineBarrier`](Self::PipelineBarrier)
    /// does.
    ///
    /// **Carried rather than dropped at the seam**, because
    /// `CommandEncoder::reset_query_set` returns `()` and so has no way to report
    /// anything: a range naming a set the replayer does not hold, or one running
    /// past that set's `count`, reaches the device error queue by name instead of
    /// vanishing. That is the only observable this command has.
    ResetQuerySet {
        /// Which query set.
        set: QuerySetHandle,
        /// First query of the range to reset.
        first_query: u32,
        /// How many queries the range covers. The seam passes a
        /// [`Range<u32>`](core::ops::Range) and this is its length, because a
        /// count is what WebGPU's own resolve takes and an empty range is then
        /// `0` rather than a pair a decoder has to check for inversion.
        query_count: u32,
    },
    /// [`resolve_query_set`](crcbl_hal::CommandEncoder::resolve_query_set) — the
    /// copy of a query range into a buffer, on the implicit-current encoder.
    ///
    /// Becomes `resolveQuerySet(querySet, firstQuery, queryCount, destination,
    /// destinationOffset)`, which is a `GPUCommandEncoder` method and so is
    /// refused inside an open pass rather than recorded.
    ///
    /// **Two validation rules the replayer refuses by name**, both of them
    /// WebGPU's rather than this stream's, and both decidable there rather than
    /// here:
    ///
    ///   * `dst_offset` must be a multiple of 256 — `wgpu-types`'
    ///     `QUERY_RESOLVE_BUFFER_ALIGNMENT`, which `wgpu-core`'s
    ///     `command::query::resolve_query_set` checks first of all.
    ///   * `dst` must carry [`BufferUsage::QUERY_RESOLVE`](crcbl_hal::BufferUsage::QUERY_RESOLVE)
    ///     — `wgpu-core` checks `BufferUsages::QUERY_RESOLVE` on the same call.
    ///
    /// Either one is a WebGPU validation error the browser would report out of
    /// band, a frame later and attributed to nothing; refusing them by name at
    /// the command that carried them is what makes the diagnosis the command
    /// rather than the device.
    ResolveQuerySet {
        /// Which query set.
        set: QuerySetHandle,
        /// First query of the range to resolve.
        first_query: u32,
        /// How many queries the range covers — see
        /// [`ResetQuerySet`](Self::ResetQuerySet) on why a count and not a pair.
        query_count: u32,
        /// Buffer the results are written into.
        dst: BufferHandle,
        /// Byte offset within `dst`. A multiple of 256 — see the variant docs.
        dst_offset: u64,
    },
    /// [`Device::query_results`](crcbl_hal::Device::query_results) — the direct
    /// read, without a resolve-to-buffer round trip of the caller's own.
    ///
    /// **Answered**, by a [`Reply::QueryResults`](crate::Reply::QueryResults)
    /// naming this command's sequence, and answered a frame or more later:
    /// WebGPU has no way to read a `GPUQuerySet` at all, so the replayer serves
    /// this by resolving into a scratch buffer of its own, copying that into a
    /// mappable one and mapping it — which is a promise, exactly as
    /// [`PollReadback`](Self::PollReadback)'s map is. It goes through
    /// [`StreamChannel::encode_awaited`](crate::web::StreamChannel::encode_awaited)
    /// for that reason.
    ///
    /// **One reply, whatever happens.** There is no failed counterpart to
    /// `Reply::QueryResults`, so a read the replayer cannot serve is answered
    /// with an *empty* `values` list and the reason goes on the device error
    /// queue. Empty is unambiguous because the seam side never asks for zero
    /// values — `query_results` with an empty `out` is answered without touching
    /// the wire.
    QueryResults {
        /// Which query set.
        set: QuerySetHandle,
        /// First query to read.
        first_query: u32,
        /// How many queries to read — `out.len()` at the seam.
        query_count: u32,
    },
    /// [`Device::create_command_encoder`](crcbl_hal::Device::create_command_encoder).
    ///
    /// **It carries no handle**, unlike every other creation on this stream, and
    /// that is the encoder's shape rather than an omission. `crcbl-hal`'s
    /// recording methods — [`begin_render_pass`](crcbl_hal::CommandEncoder::begin_render_pass),
    /// [`draw`](crcbl_hal::CommandEncoder::draw), the rest — name no encoder,
    /// because a `Box<dyn CommandEncoder>` records into itself; this stream keeps
    /// that model, so the replayer holds a single *implicit-current* encoder that
    /// this command opens and [`Finish`](Self::Finish) consumes. The command
    /// buffer the encoder eventually yields is what carries a handle, and
    /// [`Finish`](Self::Finish) is where it is named.
    ///
    /// **`queue` is carried and not used to select a queue.** WebGPU has one
    /// implicit queue — `device.queue` — so there is no queue for a handle to
    /// pick; it crosses because [`CommandEncoderDesc`](crcbl_hal::CommandEncoderDesc)
    /// has it (Vulkan command pools are per-queue-family, and the seam names the
    /// queue at creation so the backend need not copy at submit time) and so a
    /// transposition is visible, and the replayer ignores it the way it ignores
    /// [`workgroup_size`](Self::CreateComputePipeline::workgroup_size).
    CreateCommandEncoder {
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Queue the resulting command buffer will be submitted to. Carried, not
        /// used to select a queue — see the variant docs.
        queue: QueueHandle,
    },
    /// [`end_render_pass`](crcbl_hal::CommandEncoder::end_render_pass).
    ///
    /// Body-less, and on the implicit-current encoder: it closes the pass
    /// [`BeginRenderPass`](Self::BeginRenderPass) opened. Ending with no open
    /// pass is a malformed stream the replayer routes to the error queue rather
    /// than throwing on — see `web/engine/gpu-replay.js`.
    EndRenderPass,
    /// [`copy_image_to_buffer`](crcbl_hal::CommandEncoder::copy_image_to_buffer)
    /// — the readback path's image→buffer copy, recorded on the
    /// implicit-current encoder.
    ///
    /// The whole of [`BufferImageCopy`], flattened as every descriptor here is,
    /// and **the direction is the variant's name and
    /// never a field** — the HAL uses one struct for both directions and the
    /// method called is what says which, so the two can never disagree.
    ///
    /// **[`buffer_row_length`](Self::CopyImageToBuffer::buffer_row_length) is in
    /// texels, `0` meaning tightly packed, and crosses verbatim.** WebGPU's
    /// `bytesPerRow` is in *bytes* and must be a multiple of 256, so the replayer
    /// is where texels become bytes and where a misalignment surfaces — this
    /// decoder does not resolve it, for the sentinel rule's reason. The
    /// [`u32`](Self::CopyImageToBuffer::buffer_offset)/[`u64`](Self::CopyImageToBuffer::buffer_offset)
    /// fields are chosen distinct in the corpus so a transposition among the many
    /// numbers is visible.
    CopyImageToBuffer {
        /// Buffer side of the copy — the readback destination.
        buffer: BufferHandle,
        /// Byte offset in the buffer.
        buffer_offset: u64,
        /// Texels per row in the buffer; `0` is tightly packed. In *texels*, not
        /// bytes — see the variant docs on the 256-byte trap.
        buffer_row_length: u32,
        /// Rows per layer in the buffer; `0` is tightly packed.
        buffer_image_height: u32,
        /// Image side of the copy — the readback source.
        image: ImageHandle,
        /// Mip level and layers touched.
        image_subresource: ImageSubresourceLayers,
        /// Texel offset within the image.
        image_offset: Offset3d,
        /// Region size in texels.
        image_extent: Extent3d,
    },
    /// [`copy_buffer_to_buffer`](crcbl_hal::CommandEncoder::copy_buffer_to_buffer)
    /// — the buffer→buffer copy, recorded on the implicit-current encoder.
    ///
    /// **The only way to observe a dispatch.** A compute shader writes a storage
    /// buffer, and a storage buffer cannot be `MAP_READ`, so its output is copied
    /// into a host-readable buffer here before the readback maps it. The whole of
    /// [`BufferCopy`] crosses as one nested value rather
    /// than flattened — a small self-contained struct, unlike
    /// [`CopyImageToBuffer`](Self::CopyImageToBuffer)'s eight scattered fields —
    /// and the two `u64` offsets and the size are chosen distinct in the corpus so
    /// a transposition among them is visible.
    CopyBufferToBuffer {
        /// Source, destination, their byte offsets, and the size copied.
        copy: BufferCopy,
    },
    /// [`copy_buffer_to_image`](crcbl_hal::CommandEncoder::copy_buffer_to_image)
    /// — the buffer→image copy, recorded on the implicit-current encoder and the
    /// upload counterpart of [`CopyImageToBuffer`](Self::CopyImageToBuffer).
    ///
    /// The whole of [`BufferImageCopy`], carried as one nested value — the same
    /// struct [`CopyImageToBuffer`](Self::CopyImageToBuffer) flattens, and **the
    /// direction is the variant's name, never a field**: the HAL uses one struct
    /// for both directions and the method called is what says which. Its
    /// [`buffer_row_length`](crcbl_hal::BufferImageCopy::buffer_row_length) is in
    /// *texels*, `0` meaning tightly packed, and crosses verbatim; WebGPU's
    /// `bytesPerRow` is bytes and must be 256-aligned, so the texel→byte
    /// conversion is the replayer's — it maps to `copyBufferToTexture`, whose
    /// argument order is the reverse of `copyTextureToBuffer`. See
    /// `web/engine/gpu-replay.js`.
    CopyBufferToImage {
        /// Buffer side, image side, and the region copied.
        copy: BufferImageCopy,
    },
    /// [`copy_image_to_image`](crcbl_hal::CommandEncoder::copy_image_to_image) —
    /// the image→image copy, recorded on the implicit-current encoder.
    ///
    /// The whole of [`ImageCopy`], carried as one nested value: source and
    /// destination each with their own mip level, layer range and texel offset,
    /// then the shared extent. It maps to WebGPU's `copyTextureToTexture`. The
    /// corpus copies across *different* mip levels and offsets so a source/dest
    /// transposition is visible.
    CopyImageToImage {
        /// Source, destination, their subresources and offsets, and the extent.
        copy: ImageCopy,
    },
    /// [`fill_buffer`](crcbl_hal::CommandEncoder::fill_buffer) — a value fill
    /// over a buffer range, recorded on the implicit-current encoder.
    ///
    /// **WebGPU has no valued device-side fill.** Its `clearBuffer` zeroes and
    /// nothing else, so `value == 0` maps to `clearBuffer(buffer, offset, size)`
    /// and any other value is a write the replayer refuses to the error queue —
    /// the same judgement the `crcbl-wgpu` backend makes. The value crosses
    /// verbatim so the refusal happens where the target is, not at the encoder.
    FillBuffer {
        /// Buffer filled.
        buffer: BufferHandle,
        /// Byte offset the fill starts at.
        offset: u64,
        /// Bytes filled.
        size: u64,
        /// The `u32` written into each word; only `0` is expressible on WebGPU.
        value: u32,
    },
    /// [`Device::write_buffer`](crcbl_hal::Device::write_buffer) — a host→buffer
    /// upload, which becomes WebGPU's `queue.writeBuffer(buffer, offset, data)`.
    ///
    /// **Not an encoder command**: `write_buffer` is a [`Device`](crcbl_hal::Device)
    /// method, not a [`CommandEncoder`](crcbl_hal::CommandEncoder) one, and the
    /// replayer submits it straight to the queue rather than recording it on the
    /// implicit-current encoder. It sits in the copy-and-fill family because it is
    /// a queue-side data transfer, the upload counterpart of the copies.
    ///
    /// **The bytes cross whole**, exactly as [`PushConstants`](Self::PushConstants)'
    /// do — a replayer needs them — and the `buffer` and `offset` are chosen
    /// distinct from the payload in the corpus so a transposition is visible.
    WriteBuffer {
        /// Buffer written into. Must be
        /// [`MemoryLocation::HostUpload`], but this decoder does not enforce it —
        /// the replayer faces WebGPU and is where such a refusal belongs.
        buffer: BufferHandle,
        /// Byte offset the write starts at.
        offset: u64,
        /// Bytes uploaded.
        data: Vec<u8>,
    },
    /// [`pipeline_barrier`](crcbl_hal::CommandEncoder::pipeline_barrier) — the
    /// documented no-op, recorded on the implicit-current encoder.
    ///
    /// **It carries the whole [`Barriers`](crcbl_hal::Barriers) batch for wire
    /// fidelity, and the replayer records nothing.** WebGPU serialises its single
    /// queue and inserts hazard barriers itself — the same reason a `Submit`'s
    /// `waits`/`signals` have no home (see [`Submit`](Self::Submit)) — so there is
    /// no transition for the encoder to record. `barrier()` was called out of
    /// scope for exactly this on the seam's first rendered frame
    /// (`docs/plan/ROADMAP.md`, group S), and this is where it lands: a faithful
    /// transposition whose replay arm is empty on purpose. See
    /// `web/engine/gpu-replay.js`.
    ///
    /// The [`Barriers`](crcbl_hal::Barriers) slices are owned as `Vec`s here, the
    /// way [`BeginRenderPass`](Self::BeginRenderPass) owns its `color_attachments`:
    /// the HAL passes them by borrow, and a decoded command outlives the buffer it
    /// came from. Each [`BufferBarrier`] and [`ImageBarrier`] crosses whole, its
    /// [`from`](crcbl_hal::BufferBarrier::from)/[`to`](crcbl_hal::BufferBarrier::to)
    /// states and its optional
    /// [`queue_transfer`](crcbl_hal::BufferBarrier::queue_transfer) included, so
    /// the batch round-trips in Rust even though nothing acts on it.
    PipelineBarrier {
        /// Buffer transitions in the batch.
        buffers: Vec<BufferBarrier>,
        /// Image transitions in the batch.
        images: Vec<ImageBarrier>,
        /// The global execution+memory barrier flag.
        global: bool,
    },
    /// [`finish`](crcbl_hal::CommandEncoder::finish) — consumes the
    /// implicit-current encoder and produces the command buffer.
    ///
    /// **The handle is the caller's, positional, as every creation's is**: the
    /// stream cannot answer during the call, so wasm allocates the
    /// [`CommandBufferHandle`] and the replayer
    /// files `encoder.finish()` under it. It is an encoder command rather than a
    /// creation because it makes nothing from a descriptor — it seals what the
    /// encoder recorded — which is why its tag is in the encoder family.
    Finish {
        /// Id the replayer stores the finished command buffer at.
        command_buffer: CommandBufferHandle,
    },
    /// [`Device::submit`](crcbl_hal::Device::submit).
    ///
    /// The whole of [`SubmitInfo`](crcbl_hal::SubmitInfo): the command buffers
    /// executed in order, and the waits and signals.
    ///
    /// **The counted `waits`/`signals` lists cross even though empty is the only
    /// case WebGPU maps.** WebGPU serialises its single queue and inserts hazard
    /// barriers itself, so it has no semaphores at all — a non-empty `waits` or
    /// `signals` names one, which the replayer refuses to the error queue rather
    /// than dropping. Silently dropping a wait is a synchronisation bug, and the
    /// engine's real frame *will* carry them, so the refusal must be loud and by
    /// name — the same judgement a `Some` push-constant range gets. The lists are
    /// carried whole so they round-trip in Rust and so the replayer can name what
    /// it refuses; see `web/engine/gpu-replay.js`.
    Submit {
        /// Command buffers to execute, in order.
        command_buffers: Vec<CommandBufferHandle>,
        /// Semaphore waits before execution. Empty is the only case WebGPU maps;
        /// a non-empty list is refused — see the variant docs.
        waits: Vec<SemaphoreWait>,
        /// Semaphore signals on completion. Empty for the same reason `waits` is.
        signals: Vec<SemaphoreSignal>,
    },
    /// [`Device::request_readback`](crcbl_hal::Device::request_readback), with the
    /// handle the caller allocated for it.
    ///
    /// The whole of [`ReadbackDesc`](crcbl_hal::ReadbackDesc). Identity is
    /// positional here for [`CreateBuffer`](Self::CreateBuffer)'s reason: the
    /// [`ReadbackHandle`] is caller-allocated, and the
    /// in-flight readback is filed under it by the replayer.
    ///
    /// **`after` crosses whole, and the replayer refuses a `Some`.** `None` maps
    /// to WebGPU's `mapAsync` — "everything submitted so far" — which is exactly
    /// what [`ReadbackDesc::after`](crcbl_hal::ReadbackDesc::after)'s `None`
    /// means. `Some(wait)` names a timeline value WebGPU has no semaphore to
    /// express, so it is refused by name rather than ignored, for
    /// [`Submit`](Self::Submit)'s reason. It rides a presence byte — the house
    /// rule for an optional field that is not a handle — and round-trips in Rust
    /// so a `Some` is testable even though no browser honours it.
    RequestReadback {
        /// Id the replayer stores the in-flight readback at.
        readback: ReadbackHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Buffer to read. Must be
        /// [`MemoryLocation::HostReadback`].
        buffer: BufferHandle,
        /// Byte offset within the buffer.
        offset: u64,
        /// Bytes to read.
        size: u64,
        /// Completion point to wait for. `None` is `mapAsync`; `Some` is a
        /// semaphore wait the replayer refuses — see the variant docs.
        after: Option<SemaphoreWait>,
    },
    /// [`Device::poll_readback`](crcbl_hal::Device::poll_readback) — names the
    /// in-flight readback and asks whether its bytes are ready.
    ///
    /// **The one command in this slice that is answered on the reply channel.**
    /// The replayer maps the buffer asynchronously when
    /// [`RequestReadback`](Self::RequestReadback) is replayed, and this command
    /// reports where that has got to: a [`Reply::ReadbackReady`](crate::Reply::ReadbackReady)
    /// carrying the bytes once the map has resolved, a
    /// [`Reply::ReadbackPending`](crate::Reply::ReadbackPending) until then. Both
    /// name this command's sequence, exactly as the adapter and device polls do —
    /// see [`crate::instance`] and `web/engine/gpu-replay.js`.
    PollReadback {
        /// Which in-flight readback to poll.
        readback: ReadbackHandle,
    },
    /// [`Device::take_error`](crcbl_hal::Device::take_error) — asks for the
    /// errors the browser reported out of band since the last time it was asked.
    ///
    /// **A body-less command**, as [`Command::SurfaceCaps`] is: the HAL call
    /// takes nothing and this seam holds one device, so there is nothing to name.
    ///
    /// **Answered exactly once, by a
    /// [`Reply::DeviceErrors`](crate::Reply::DeviceErrors) naming this
    /// command's sequence — and answered even when there is nothing to report**,
    /// with an empty list. Silence is not an option here: a command nobody
    /// answers leaves its sequence registered for ever, and this one is issued
    /// again every time the caller's queue runs dry, so a replayer that stayed
    /// quiet on a healthy page would fill
    /// [`MAX_WAITING_REPLIES`](crate::tag::MAX_WAITING_REPLIES) and then stop the
    /// engine from asking anything at all.
    ///
    /// **Why the errors travel this way and not on their own.** WebGPU reports
    /// them on `uncapturederror`, out of band and long after the command that
    /// caused one has returned, so there is no sequence to attribute them to —
    /// and a reply naming a sequence nobody waits on is
    /// [`DecodeError::UnexpectedSequence`](crate::DecodeError::UnexpectedSequence),
    /// which refuses the whole buffer and takes every other answer in the frame
    /// with it. A command that *asks* is what gives the answer a sequence to
    /// name.
    TakeError,
    /// [`Device::destroy_readback`](crcbl_hal::Device::destroy_readback).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is. On WebGPU this is the `unmap` — abandoning
    /// a readback without it leaks a mapped buffer, which is why the seam has an
    /// explicit destroy.
    DestroyReadback {
        /// Id to release.
        readback: ReadbackHandle,
    },
    /// [`Device::destroy_command_buffer`](crcbl_hal::Device::destroy_command_buffer).
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroyBuffer`] is.
    DestroyCommandBuffer {
        /// Id to release.
        command_buffer: CommandBufferHandle,
    },
    /// [`Instance::adapters`](crcbl_hal::Instance::adapters) — enumerate what
    /// the browser will grant.
    ///
    /// **A body-less command**, as [`Command::SurfaceCaps`] is: the HAL call
    /// takes nothing. The enumeration cannot be handed back during the call, so
    /// the replayer queues a [`Reply::Adapter`](crate::Reply::Adapter) or a
    /// [`Reply::NoAdapter`](crate::Reply::NoAdapter) naming this command's
    /// sequence, and it arrives a frame or more later. See
    /// [`crate::instance`] for the side that waits for it.
    EnumerateAdapters,
    /// [`Instance::request_device`](crcbl_hal::Instance::request_device) — open
    /// the adapter the enumeration granted.
    ///
    /// The whole of [`DeviceDesc`](crcbl_hal::DeviceDesc), flattened as every
    /// other descriptor here is. Answered by a [`Reply::Device`](crate::Reply::Device)
    /// or a [`Reply::DeviceFailed`](crate::Reply::DeviceFailed) naming this
    /// command's sequence; see [`crate::device`] for the side that waits.
    ///
    /// **The feature words cross as [`Features`] bits, not as WebGPU names.**
    /// The replayer owns that vocabulary in both directions — it is the half
    /// that faces WebGPU — so the wire speaks the seam's language here exactly
    /// as it does for load ops and handles.
    RequestDevice {
        /// Which adapter, as [`Instance::adapters`](crcbl_hal::Instance::adapters)
        /// numbered it. Always `0` from a browser: `requestAdapter()` grants one
        /// adapter or none.
        adapter: AdapterId,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Features the caller cannot run without. A bit with no WebGPU name is
        /// unsatisfiable and **fails the request**; it is never quietly dropped.
        required_features: Features,
        /// Features to enable if the adapter has them. Bits with no WebGPU name
        /// are simply not asked for, which is what optional means.
        optional_features: Features,
        /// A surface the device must be able to present to.
        compatible_surface: Option<SurfaceHandle>,
    },
    /// [`Instance::surface_caps`](crcbl_hal::Instance::surface_caps) — what a
    /// canvas surface on this instance will accept.
    ///
    /// The third command whose answer comes back, and it is answered by a
    /// [`Reply::SurfaceCaps`](crate::Reply::SurfaceCaps) or a
    /// [`Reply::SurfaceCapsFailed`](crate::Reply::SurfaceCapsFailed) naming this
    /// command's sequence.
    ///
    /// **Body-less, though the HAL call takes a surface and an adapter.** The
    /// record depends on neither: `getPreferredCanvasFormat()` is a method on
    /// `GPU` and takes no canvas, and the rest of
    /// [`SurfaceCaps`](crcbl_hal::SurfaceCaps) is fixed for a canvas. So the two
    /// ids are exactly what an `impl Instance` validates against its own tables
    /// without asking anyone — a refusal, not a question — and sending them
    /// would be quoting arguments the answer never reads.
    SurfaceCaps,
    /// [`Device::create_swapchain`](crcbl_hal::Device::create_swapchain), with the
    /// handle the caller allocated for it.
    ///
    /// The whole of [`SwapchainDesc`](crcbl_hal::SwapchainDesc), flattened as every
    /// other descriptor here is, behind the caller-allocated handle. The replayer
    /// resolves [`surface`](Self::CreateSwapchain::surface) to its
    /// `GPUCanvasContext` and calls `configure` — swapchain creation on WebGPU is
    /// a canvas configure, not an allocation.
    ///
    /// **[`image_count`](Self::CreateSwapchain::image_count) and
    /// [`present_mode`](Self::CreateSwapchain::present_mode) cross and the replayer
    /// drops them**, the way [`CreateComputePipeline`](Self::CreateComputePipeline)'s
    /// `workgroup_size` crosses whole: a browser only offers `fifo` and manages its
    /// own buffering, so neither has a `configure` knob. [`extent`](Self::CreateSwapchain::extent)
    /// is informational — the canvas owns its size — but crosses whole for wire
    /// fidelity. See `web/engine/gpu-replay.js`.
    CreateSwapchain {
        /// Id the replayer stores the configured swapchain at.
        swapchain: SwapchainHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Surface to present to — resolved to the `GPUCanvasContext` that is
        /// configured.
        surface: SurfaceHandle,
        /// Display format the canvas is configured with.
        format: Format,
        /// Requested size in pixels. Informational; the canvas owns its size.
        extent: (u32, u32),
        /// Requested image count. Carried and dropped — a browser manages its own
        /// buffering.
        image_count: u32,
        /// Requested pacing. Carried and dropped — a browser only offers `fifo`.
        present_mode: PresentMode,
        /// Desktop compositing mode, mapped to the canvas `alphaMode`.
        composite_alpha: CompositeAlpha,
    },
    /// [`Device::acquire_next_frame`](crcbl_hal::Device::acquire_next_frame) — the
    /// frame `getCurrentTexture()` hands back, bound under the caller-allocated
    /// image and view handles.
    ///
    /// **The image and view handles are the caller's, positional, as every
    /// creation's are**: acquire is synchronous and deterministic on WebGPU —
    /// `getCurrentTexture()` answers in the call — so the stream needs no reply,
    /// and wasm allocates the two handles the acquired texture and its view are
    /// filed under, exactly as [`create_surface`](Self::CreateSurface) takes a
    /// caller-allocated handle. The replayer resolves the swapchain's context,
    /// files the texture under [`image`](Self::AcquireNextFrame::image) and its
    /// `createView()` under [`view`](Self::AcquireNextFrame::view).
    AcquireNextFrame {
        /// Which swapchain to acquire from.
        swapchain: SwapchainHandle,
        /// Id the replayer files the acquired texture under.
        image: ImageHandle,
        /// Id the replayer files the acquired texture's view under.
        view: ImageViewHandle,
    },
    /// [`Device::present`](crcbl_hal::Device::present) — replayed as a **no-op**.
    ///
    /// The whole of [`PresentInfo`](crcbl_hal::PresentInfo). WebGPU has no explicit
    /// present: the browser composites the configured canvas on its own
    /// `requestAnimationFrame`, so there is nothing for the replayer to call and
    /// the present touches nothing.
    ///
    /// **A non-empty [`waits`](Self::Present::waits) is refused**, exactly as a
    /// [`Submit`](Self::Submit)'s `waits`/`signals` are: WebGPU has no semaphores,
    /// so there is nothing to wait on, and silently dropping a wait is a
    /// synchronisation bug. The list is carried whole so it round-trips in Rust
    /// and so the replayer can name what it refuses.
    /// [`present_id`](Self::Present::present_id) crosses whole and is dropped —
    /// WebGPU has no present-completion query to number.
    Present {
        /// Swapchain to present.
        swapchain: SwapchainHandle,
        /// Semaphore waits before the present. Empty is the only case WebGPU maps;
        /// a non-empty list is refused — see the variant docs.
        waits: Vec<SemaphoreHandle>,
        /// A number for this present. Carried and dropped — WebGPU has no
        /// present-completion query.
        present_id: Option<u64>,
    },
    /// [`Device::destroy_swapchain`](crcbl_hal::Device::destroy_swapchain) —
    /// unconfigures the canvas context and releases the handle.
    ///
    /// A no-op for an id whose slot holds nothing, exactly as
    /// [`Command::DestroySurface`] is. On WebGPU this is the `unconfigure` — the
    /// counterpart of the `configure` [`CreateSwapchain`](Self::CreateSwapchain)
    /// made.
    DestroySwapchain {
        /// Id to release.
        swapchain: SwapchainHandle,
    },
    /// [`Device::reconfigure_swapchain`](crcbl_hal::Device::reconfigure_swapchain)
    /// — re-configures an already-configured swapchain in place.
    ///
    /// **The same descriptor as [`CreateSwapchain`](Self::CreateSwapchain)**,
    /// flattened identically behind the swapchain handle; the only difference is
    /// the semantic — [`swapchain`](Self::ReconfigureSwapchain::swapchain) names an
    /// *existing* configured swapchain rather than an id to file a new one under.
    /// The handle stays valid across the reconfigure, so callers above do not
    /// re-fetch it across a resize storm.
    ///
    /// On WebGPU this is `context.configure(...)` called a second time. Like
    /// `CreateSwapchain`, [`extent`](Self::ReconfigureSwapchain::extent) is
    /// informational — the canvas owns its size — so a reconfigure changes the
    /// format, usage and alpha mode, not the size; and
    /// [`image_count`](Self::ReconfigureSwapchain::image_count) and
    /// [`present_mode`](Self::ReconfigureSwapchain::present_mode) cross whole and
    /// the replayer drops them. See `web/engine/gpu-replay.js`.
    ReconfigureSwapchain {
        /// The already-configured swapchain to re-configure.
        swapchain: SwapchainHandle,
        /// Debug name, if the descriptor carried one.
        label: Option<String>,
        /// Surface the swapchain presents to.
        surface: SurfaceHandle,
        /// Display format the canvas is re-configured with.
        format: Format,
        /// Requested size in pixels. Informational; the canvas owns its size.
        extent: (u32, u32),
        /// Requested image count. Carried and dropped — a browser manages its own
        /// buffering.
        image_count: u32,
        /// Requested pacing. Carried and dropped — a browser only offers `fifo`.
        present_mode: PresentMode,
        /// Desktop compositing mode, mapped to the canvas `alphaMode`.
        composite_alpha: CompositeAlpha,
    },
}

impl Command {
    /// A stable variant name.
    ///
    /// What a stream dump prints, and what lets a test assert the *shape* of a
    /// buffer without spelling out every handle and descriptor. Same role as
    /// [`crcbl_hal::null::Command::name`].
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::CreateBuffer { .. } => "CreateBuffer",
            Self::CreateSurface { .. } => "CreateSurface",
            Self::CreateOffscreenSurface { .. } => "CreateOffscreenSurface",
            Self::CreateImage { .. } => "CreateImage",
            Self::CreateImageView { .. } => "CreateImageView",
            Self::CreateSampler { .. } => "CreateSampler",
            Self::CreateBindGroupLayout { .. } => "CreateBindGroupLayout",
            Self::CreateBindGroup { .. } => "CreateBindGroup",
            Self::CreateShaderModule { .. } => "CreateShaderModule",
            Self::CreatePipelineLayout { .. } => "CreatePipelineLayout",
            Self::CreateComputePipeline { .. } => "CreateComputePipeline",
            Self::CreateGraphicsPipeline { .. } => "CreateGraphicsPipeline",
            Self::CreateQuerySet { .. } => "CreateQuerySet",
            Self::DestroyBuffer { .. } => "DestroyBuffer",
            Self::DestroySurface { .. } => "DestroySurface",
            Self::DestroyImage { .. } => "DestroyImage",
            Self::DestroyImageView { .. } => "DestroyImageView",
            Self::DestroySampler { .. } => "DestroySampler",
            Self::DestroyBindGroupLayout { .. } => "DestroyBindGroupLayout",
            Self::DestroyBindGroup { .. } => "DestroyBindGroup",
            Self::DestroyShaderModule { .. } => "DestroyShaderModule",
            Self::DestroyPipelineLayout { .. } => "DestroyPipelineLayout",
            Self::DestroyComputePipeline { .. } => "DestroyComputePipeline",
            Self::DestroyGraphicsPipeline { .. } => "DestroyGraphicsPipeline",
            Self::DestroyQuerySet { .. } => "DestroyQuerySet",
            Self::BeginDebugLabel { .. } => "BeginDebugLabel",
            Self::EndDebugLabel => "EndDebugLabel",
            Self::InsertDebugMarker { .. } => "InsertDebugMarker",
            Self::BeginRenderPass { .. } => "BeginRenderPass",
            Self::BindGraphicsPipeline { .. } => "BindGraphicsPipeline",
            Self::BindGroup { .. } => "BindGroup",
            Self::PushConstants { .. } => "PushConstants",
            Self::SetViewport { .. } => "SetViewport",
            Self::SetScissor { .. } => "SetScissor",
            Self::SetStencilReference { .. } => "SetStencilReference",
            Self::BindIndexBuffer { .. } => "BindIndexBuffer",
            Self::Draw { .. } => "Draw",
            Self::DrawIndexed { .. } => "DrawIndexed",
            Self::DrawIndirect { .. } => "DrawIndirect",
            Self::DrawIndexedIndirect { .. } => "DrawIndexedIndirect",
            Self::BeginComputePass { .. } => "BeginComputePass",
            Self::BindComputePipeline { .. } => "BindComputePipeline",
            Self::Dispatch { .. } => "Dispatch",
            Self::DispatchIndirect { .. } => "DispatchIndirect",
            Self::EndComputePass => "EndComputePass",
            Self::ResetQuerySet { .. } => "ResetQuerySet",
            Self::ResolveQuerySet { .. } => "ResolveQuerySet",
            Self::QueryResults { .. } => "QueryResults",
            Self::CreateCommandEncoder { .. } => "CreateCommandEncoder",
            Self::EndRenderPass => "EndRenderPass",
            Self::CopyImageToBuffer { .. } => "CopyImageToBuffer",
            Self::CopyBufferToBuffer { .. } => "CopyBufferToBuffer",
            Self::CopyBufferToImage { .. } => "CopyBufferToImage",
            Self::CopyImageToImage { .. } => "CopyImageToImage",
            Self::FillBuffer { .. } => "FillBuffer",
            Self::WriteBuffer { .. } => "WriteBuffer",
            Self::PipelineBarrier { .. } => "PipelineBarrier",
            Self::Finish { .. } => "Finish",
            Self::Submit { .. } => "Submit",
            Self::RequestReadback { .. } => "RequestReadback",
            Self::PollReadback { .. } => "PollReadback",
            Self::TakeError => "TakeError",
            Self::DestroyReadback { .. } => "DestroyReadback",
            Self::DestroyCommandBuffer { .. } => "DestroyCommandBuffer",
            Self::EnumerateAdapters => "EnumerateAdapters",
            Self::RequestDevice { .. } => "RequestDevice",
            Self::SurfaceCaps => "SurfaceCaps",
            Self::CreateSwapchain { .. } => "CreateSwapchain",
            Self::AcquireNextFrame { .. } => "AcquireNextFrame",
            Self::Present { .. } => "Present",
            Self::DestroySwapchain { .. } => "DestroySwapchain",
            Self::ReconfigureSwapchain { .. } => "ReconfigureSwapchain",
        }
    }
}
