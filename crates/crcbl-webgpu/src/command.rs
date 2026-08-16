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
    BufferHandle, BufferUsage, ColorAttachment, CompareOp, ComputePipelineHandle,
    DepthStencilAttachment, Extent3d, Features, FilterMode, Format, GraphicsPipelineHandle,
    ImageHandle, ImageSubresourceRange, ImageType, ImageUsage, ImageViewHandle, ImageViewType,
    MemoryLocation, PipelineLayoutHandle, PushConstantRange, Rect2d, SamplerAddressMode,
    SamplerHandle, ShaderModuleHandle, ShaderStages, SurfaceHandle,
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
    /// [`begin_debug_label`](crcbl_hal::CommandEncoder::begin_debug_label).
    BeginDebugLabel {
        /// Region name.
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
    /// [`draw`](crcbl_hal::CommandEncoder::draw).
    Draw {
        /// Vertex range.
        vertices: Range<u32>,
        /// Instance range.
        instances: Range<u32>,
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
            Self::CreateImage { .. } => "CreateImage",
            Self::CreateImageView { .. } => "CreateImageView",
            Self::CreateSampler { .. } => "CreateSampler",
            Self::CreateBindGroupLayout { .. } => "CreateBindGroupLayout",
            Self::CreateBindGroup { .. } => "CreateBindGroup",
            Self::CreateShaderModule { .. } => "CreateShaderModule",
            Self::CreatePipelineLayout { .. } => "CreatePipelineLayout",
            Self::CreateComputePipeline { .. } => "CreateComputePipeline",
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
            Self::BeginDebugLabel { .. } => "BeginDebugLabel",
            Self::BeginRenderPass { .. } => "BeginRenderPass",
            Self::BindGraphicsPipeline { .. } => "BindGraphicsPipeline",
            Self::BindGroup { .. } => "BindGroup",
            Self::PushConstants { .. } => "PushConstants",
            Self::Draw { .. } => "Draw",
            Self::EnumerateAdapters => "EnumerateAdapters",
            Self::RequestDevice { .. } => "RequestDevice",
            Self::SurfaceCaps => "SurfaceCaps",
        }
    }
}
