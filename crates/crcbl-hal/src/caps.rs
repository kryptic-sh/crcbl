//! Adapter identification and device capability reporting.
//!
//! This module is what makes topic 39's **capability-driven degradation**
//! expressible. The renderer never asks "am I on Vulkan?" — it asks
//! [`Features::contains`] or one of the derived path selectors, exactly as
//! `crcbl-shell` exposes `ShellCaps` instead of letting anyone sniff for
//! Wayland.
//!
//! ```text
//! MESH_SHADER, DRAW_INDIRECT_COUNT   -> GeometryPath
//! DESCRIPTOR_INDEXING                -> BindingModel
//! RAY_QUERY, ACCELERATION_STRUCTURE  -> LightingPath
//! ```
//!
//! Per topic 39, **a missing feature degrades by default**: each selector is
//! ordered best-first and resolves downward, every value consumes the same
//! geometry pools, instance buffers and material tables, and only the
//! draw-emission tail differs. A caller that genuinely cannot run without a
//! feature names it in
//! [`DeviceDesc::required_features`](crate::DeviceDesc::required_features), and
//! its absence is then a loud, named failure at device creation rather than a
//! picture that is quietly a different game. There is deliberately no third
//! behaviour and no single tier value: two buckets cannot hold three
//! independent axes.

use core::fmt;

/// Which backend implementation is behind the seam.
///
/// For logs, capture-tool selection and bug reports only. Renderer *behaviour*
/// must key off [`Features`] and the selectors derived from it, never off this
/// — that is the difference between a capability system and platform sniffing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// `crcbl-vk` — Vulkan 1.3.
    Vulkan,
    /// `crcbl-wgpu` — wgpu, native or WebGPU.
    Wgpu,
    /// `crcbl-webgpu` — WebGPU reached directly from wasm, without wgpu.
    ///
    /// Distinct from [`Wgpu`](Self::Wgpu) on purpose: the two are meant to be
    /// selectable apart while `crcbl-webgpu` grows into the browser backend
    /// `crcbl-wgpu` currently is there, and a shared name would make a log line
    /// unable to say which one drew the frame.
    WebGpu,
    /// `crcbl-mtl` — Metal (P14).
    Metal,
    /// `crcbl-dx12` — Direct3D 12 (P14).
    Dx12,
    /// The in-crate recording no-op backend. See [`crate::null`].
    Null,
}

impl BackendKind {
    /// Whether this backend actually drives a GPU.
    ///
    /// [`Null`](Self::Null) records commands instead of executing them, so it is
    /// outside the parity model in [`crate::capability`]: a recorder accepting
    /// everything is not evidence that anything works, and listing it beside the
    /// five real backends would make a parity sweep read one wider than it is.
    ///
    /// A `match` rather than `!matches!(self, Self::Null)`, so a backend added
    /// later has to say which side it is on.
    #[must_use]
    pub const fn is_gpu(self) -> bool {
        match self {
            Self::Vulkan | Self::Wgpu | Self::WebGpu | Self::Metal | Self::Dx12 => true,
            Self::Null => false,
        }
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Vulkan => "vulkan",
            Self::Wgpu => "wgpu",
            Self::WebGpu => "webgpu",
            Self::Metal => "metal",
            Self::Dx12 => "dx12",
            Self::Null => "null",
        };
        f.write_str(name)
    }
}

/// Broad class of physical device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceType {
    /// Software rasteriser — lavapipe, WARP, swiftshader. Correct, slow; the
    /// P1 golden-image CI target.
    Cpu,
    /// A GPU integrated with the CPU package.
    Integrated,
    /// A separate GPU card.
    Discrete,
    /// A virtualised or para-virtualised GPU.
    Virtual,
    /// The backend declined to say.
    Other,
}

/// Index of an adapter in [`Instance::adapters`](crate::Instance::adapters).
///
/// A newtype rather than a bare `u32` so it cannot be confused with a queue
/// index or an image index at a call site — those are all `u32` too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AdapterId(pub u32);

/// What an adapter says about itself, before a device exists.
///
/// Cloneable and owned: adapter enumeration happens once at startup, so the two
/// `String`s are not worth avoiding, and the alternative (borrowing from the
/// instance) would infect every consumer with a lifetime.
#[derive(Clone, Debug, PartialEq)]
pub struct AdapterInfo {
    /// Position in the enumeration; pass to [`DeviceDesc`](crate::DeviceDesc).
    pub id: AdapterId,
    /// Human-readable device name, e.g. `"AMD Radeon RX 7900 XTX (RADV)"`.
    pub name: String,
    /// PCI vendor id, or `0` if unknown.
    pub vendor_id: u32,
    /// PCI device id, or `0` if unknown.
    pub device_id: u32,
    /// Device class.
    pub device_type: DeviceType,
    /// Driver name and version, as the backend reports them.
    pub driver: String,
    /// Which backend enumerated it.
    pub backend: BackendKind,
    /// Everything this adapter can do. Available *before* device creation so
    /// selection logic can reject an adapter without paying for a device.
    pub caps: DeviceCaps,
}

bitflags::bitflags! {
    /// Optional device capabilities.
    ///
    /// A caller asks for what it needs rather than for a bundle: a flag in
    /// [`DeviceDesc::required_features`](crate::DeviceDesc::required_features)
    /// that the adapter lacks is a named failure at device creation, and
    /// everything else degrades. They are flags rather than a tier because
    /// devices genuinely vary along independent axes — WebGPU has no bindless,
    /// no multi-draw-indirect, no buffer device address and no push constants,
    /// timestamp queries are browser-dependent, and Metal has
    /// multi-draw-indirect with no GPU-side count at all.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Features: u64 {
        /// Runtime-sized descriptor arrays with partial binding and
        /// update-after-bind — the bindless model [`BindingModel::Bindless`]
        /// selects. Vulkan `descriptorIndexing`, DX12 SM6.6 dynamic resources,
        /// Metal argument buffers tier 2. Absent on WebGPU.
        const DESCRIPTOR_INDEXING = 1 << 0;
        /// Shaders can dereference raw GPU pointers. Vulkan
        /// `bufferDeviceAddress`, DX12 GPU virtual addresses, Metal 3
        /// `gpuAddress`. Absent on WebGPU, which uses indexed SSBO lookups
        /// instead.
        const BUFFER_DEVICE_ADDRESS = 1 << 1;
        /// [`draw_indirect_count`](crate::CommandEncoder::draw_indirect_count)
        /// and its indexed sibling are usable: the draw count itself comes from
        /// a GPU buffer, so the CPU never learns how many draws happen.
        /// Absent on WebGPU.
        const DRAW_INDIRECT_COUNT = 1 << 2;
        /// One indirect call may emit more than one draw (`draw_count > 1` in
        /// [`DrawIndirect`](crate::DrawIndirect)). Distinct from
        /// `DRAW_INDIRECT_COUNT`: WebGPU has neither, but a backend could
        /// plausibly have this without the count buffer.
        const MULTI_DRAW_INDIRECT = 1 << 3;
        /// `first_instance` in an indirect draw argument is honoured rather
        /// than required to be zero. WebGPU requires zero.
        const INDIRECT_FIRST_INSTANCE = 1 << 4;
        /// Timestamp queries are supported. Optional and browser-dependent on
        /// WebGPU, which is why the profiler HUD must degrade rather than
        /// break (topic 10 risk list).
        const TIMESTAMP_QUERY = 1 << 5;
        /// Pipeline statistics queries (primitives generated, invocations).
        const PIPELINE_STATISTICS_QUERY = 1 << 6;
        /// Occlusion queries.
        const OCCLUSION_QUERY = 1 << 7;
        /// Compute shaders. Effectively universal — WebGPU has them, which is
        /// why GPU culling stays GPU-side on every [`GeometryPath`] — but a
        /// GL-ES2-class wgpu fallback would not, and the renderer needs
        /// somewhere to say so.
        const COMPUTE = 1 << 8;
        /// Timeline semaphores: monotonic counters usable for both GPU-GPU and
        /// CPU-GPU waits. Vulkan `timelineSemaphore`, DX12 `ID3D12Fence`,
        /// Metal `MTLSharedEvent`. WebGPU has no semaphores at all; see
        /// [`crate::sync`].
        const TIMELINE_SEMAPHORE = 1 << 9;
        /// A queue family that supports compute independently of graphics, so
        /// async compute is possible. Async compute is post-MVP, but the seam
        /// models queues plural from the start.
        const ASYNC_COMPUTE_QUEUE = 1 << 10;
        /// A transfer-only queue family exists. MVP uploads share the
        /// graphics+compute queue; this flag is what a later dedicated
        /// transfer queue keys off.
        const TRANSFER_QUEUE = 1 << 11;
        /// Push constants / root constants are available.
        /// [`Limits::max_push_constant_size`] gives the budget. **Absent on
        /// WebGPU**, which substitutes a dynamic-offset uniform buffer — a
        /// data-layout consequence the renderer must plan for.
        const PUSH_CONSTANTS = 1 << 12;
        /// Depth clamping (as opposed to clipping) — shadow-map rendering
        /// wants it so caster geometry behind the near plane still writes
        /// depth.
        const DEPTH_CLAMP = 1 << 13;
        /// A non-zero [`DepthBias::clamp`](crate::DepthBias::clamp) is honoured.
        const DEPTH_BIAS_CLAMP = 1 << 14;
        /// Fill-mode `Line` in [`PolygonMode`](crate::PolygonMode) — wireframe
        /// debug views. Absent on WebGPU and Metal-on-some-hardware.
        const POLYGON_MODE_LINE = 1 << 15;
        /// BC/DXT compressed texture formats are sampleable.
        const TEXTURE_COMPRESSION_BC = 1 << 16;
        /// Anisotropic sampling; [`Limits::max_sampler_anisotropy`] gives the
        /// cap.
        const SAMPLER_ANISOTROPY = 1 << 17;
        /// Debug labels and markers reach the capture tool
        /// (`VK_EXT_debug_utils`, PIX events, Metal debug groups). Without it
        /// the encoder's label calls are accepted and dropped.
        const DEBUG_MARKERS = 1 << 18;
        /// GPU-side shader printf / validation messages are routed into `log`.
        const SHADER_DEBUG_PRINTF = 1 << 19;
        /// The device reports back when a present has actually completed, so a
        /// frame loop can pace on the display instead of on a clock —
        /// [`Device::wait_until_presented`](crate::Device::wait_until_presented).
        ///
        /// Named for the capability rather than for any one API's spelling of
        /// it, because the three that have it do not agree on the shape: one
        /// numbers each present and blocks on the number, one hands out a
        /// waitable object with no number at all, and one only calls back once
        /// a drawable has been shown. What they share is that the CPU can find
        /// out, which is the whole of what this flag promises. Optional, and
        /// deliberately **not** part of [`GPU_DRIVEN`](Self::GPU_DRIVEN): a
        /// device without it renders exactly the same frames, it just cannot be
        /// paced by them.
        const PRESENT_FEEDBACK = 1 << 20;
        /// The device can say what the display is **doing** with the frames we
        /// present — fixed refresh, adaptive sync, or neither reported —
        /// through [`Device::display_timing`](crate::Device::display_timing).
        ///
        /// Distinct from [`PRESENT_FEEDBACK`](Self::PRESENT_FEEDBACK), which
        /// says only that a present *finished*. This one is about the display's
        /// own cadence, which nothing else in the seam can observe: a
        /// [`PresentMode`](crate::PresentMode) is a request, and a frame time
        /// measured over a paced loop is the request coming back at you. Also
        /// optional and also **not** part of [`GPU_DRIVEN`](Self::GPU_DRIVEN):
        /// a device without it renders the same frames and simply cannot be
        /// asked.
        const PRESENT_TIMING = 1 << 21;
        /// Mesh shaders: a compute-shaped stage that emits primitives straight
        /// into the rasteriser, replacing the vertex-fetch and input-assembly
        /// front end entirely. Vulkan `VK_EXT_mesh_shader`, DX12 mesh shader
        /// tier 1, Metal mesh functions. Absent on WebGPU.
        ///
        /// The best [`GeometryPath`], because a meshlet cull can run in the
        /// same dispatch that emits the geometry rather than writing indirect
        /// arguments for a separate draw to read back.
        const MESH_SHADER = 1 << 22;
        /// The amplification stage in front of a mesh shader, which decides how
        /// many mesh workgroups to launch. Vulkan's task shader, DX12's
        /// amplification shader, Metal's object function.
        ///
        /// Reported separately from [`MESH_SHADER`](Self::MESH_SHADER) because
        /// the two are separately optional: a mesh shader works on its own with
        /// a workgroup count computed on the CPU or by an earlier dispatch, and
        /// this stage is only what moves that decision into the same draw.
        const TASK_SHADER = 1 << 23;
        /// Inline ray queries from any shader stage — `RayQuery` in Slang,
        /// `VK_KHR_ray_query`, DXR 1.1 inline ray tracing. Traversal happens
        /// inside an otherwise ordinary fragment or compute shader, with no
        /// separate hit or miss stage and no shader binding table.
        ///
        /// Half of what [`LightingPath::RayTraced`] needs; the rays still want
        /// an [`ACCELERATION_STRUCTURE`](Self::ACCELERATION_STRUCTURE) to
        /// traverse, which is why neither flag alone selects that path.
        const RAY_QUERY = 1 << 24;
        /// Ray generation, closest-hit, any-hit, intersection and miss shader
        /// stages, dispatched as a pipeline of their own against a shader
        /// binding table. Vulkan `VK_KHR_ray_tracing_pipeline`, DXR 1.0.
        ///
        /// Strictly more than [`RAY_QUERY`](Self::RAY_QUERY) and reported apart
        /// from it: a device may offer inline queries without the pipeline
        /// stages, and nothing in the engine's own lighting needs the stages.
        const RAY_TRACING_PIPELINE = 1 << 25;
        /// Bottom- and top-level acceleration structures can be built, refit
        /// and traversed. Vulkan `VK_KHR_acceleration_structure`, DXR
        /// BLAS/TLAS, Metal `MTLAccelerationStructure`.
        ///
        /// The other half of [`LightingPath::RayTraced`], and reported apart
        /// from the two ray flags because it is the half that costs memory and
        /// build time whether or not anything traces against it.
        const ACCELERATION_STRUCTURE = 1 << 26;

        /// The feature set the full GPU-driven path uses.
        ///
        /// A **named bundle**, not a tier and not a gate. A device holding all
        /// of it can run one bindless descriptor array for every texture, GPU
        /// pointers for per-draw data, and a single indirect-count call per
        /// pass.
        ///
        /// Useful as an
        /// [`optional_features`](crate::DeviceDesc::optional_features) bundle,
        /// so a caller enables whatever of it the adapter turns out to have.
        /// **Never a requirement of code that has to run on whatever device it
        /// finds.** Demanding the whole set there is what topic 39 removed — it
        /// refuses a device over one absent flag while it has the rest, which
        /// is the opposite of degrading. What such a caller genuinely cannot
        /// run without goes in
        /// [`required_features`](crate::DeviceDesc::required_features) by name.
        ///
        /// A test that opens
        /// [`NullInstance::gpu_driven`](crate::null::NullInstance::gpu_driven)
        /// is the exception, and the reason the rule above names the caller it
        /// binds rather than saying "never": that preset holds this bundle by
        /// construction, so there is no hardware to refuse and the requirement
        /// is a precondition — it asserts the preset asked for is the preset
        /// that opened. It also grants it, since a null device is opened with
        /// the adapter's features intersected against
        /// `required_features | optional_features`. Move it to the optional
        /// half and a preset that lost a flag opens as a lesser path with those
        /// tests still green, which is the untested-fallback failure topic 39
        /// names as its likeliest risk.
        const GPU_DRIVEN = Self::DESCRIPTOR_INDEXING.bits()
            | Self::BUFFER_DEVICE_ADDRESS.bits()
            | Self::DRAW_INDIRECT_COUNT.bits()
            | Self::MULTI_DRAW_INDIRECT.bits()
            | Self::COMPUTE.bits()
            | Self::TIMELINE_SEMAPHORE.bits();
    }
}

/// Numeric device limits.
///
/// Every field is a hard ceiling the backend guarantees; exceeding one is a
/// [`HalError`](crate::HalError), not undefined behaviour. Fields are named
/// after the thing they bound, not after any one API's spelling of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Largest width/height of a 2D image.
    pub max_image_2d: u32,
    /// Largest edge of a 3D image.
    pub max_image_3d: u32,
    /// Largest layer count of an array image.
    pub max_image_array_layers: u32,
    /// Largest bindable range of a storage buffer, in bytes.
    pub max_storage_buffer_range: u64,
    /// Largest bindable range of a uniform buffer, in bytes.
    pub max_uniform_buffer_range: u64,
    /// Bind groups (descriptor sets) bindable simultaneously.
    pub max_bind_groups: u32,
    /// Descriptors in one bindless array. `0` when
    /// [`Features::DESCRIPTOR_INDEXING`] is absent.
    pub max_bindless_descriptors: u32,
    /// Push-constant bytes. `0` when [`Features::PUSH_CONSTANTS`] is absent.
    pub max_push_constant_size: u32,
    /// Colour attachments in one render pass.
    pub max_color_attachments: u32,
    /// Highest sample count usable for a multisampled image or pipeline.
    ///
    /// Always a power of two in `1..=64`: a sample count is a *mask* in every
    /// API underneath, so `3` is not "three samples", it is `TYPE_1 | TYPE_2`.
    /// Backends reject a request that is not a power of two, or that exceeds
    /// this, with [`HalError::InvalidDescriptor`](crate::HalError::InvalidDescriptor)
    /// — which is the error [`Device::create_image`](crate::Device::create_image)
    /// documents and which had nowhere to be expressed before this field.
    pub max_sample_count: u32,
    /// Draws one [`draw_indirect_count`](crate::CommandEncoder::draw_indirect_count)
    /// may emit.
    pub max_draw_indirect_count: u32,
    /// Per-dimension workgroup size limit for compute dispatches.
    pub max_compute_workgroup_size: [u32; 3],
    /// Total invocations in one compute workgroup.
    pub max_compute_invocations_per_workgroup: u32,
    /// Per-dimension workgroup *count* limit for
    /// [`dispatch`](crate::CommandEncoder::dispatch).
    pub max_compute_workgroups_per_dimension: u32,
    /// Required alignment for a uniform buffer binding offset.
    pub min_uniform_buffer_offset_alignment: u64,
    /// Required alignment for a storage buffer binding offset.
    pub min_storage_buffer_offset_alignment: u64,
    /// Preferred alignment for buffer↔image copy offsets.
    pub optimal_buffer_copy_offset_alignment: u64,
    /// Maximum anisotropy in [`SamplerDesc`](crate::SamplerDesc).
    pub max_sampler_anisotropy: f32,
    /// Nanoseconds per timestamp-query tick. Multiply a timestamp delta by this
    /// to get nanoseconds. `0.0` when [`Features::TIMESTAMP_QUERY`] is absent.
    pub timestamp_period_ns: f32,
}

impl Limits {
    /// The floor every backend must clear.
    ///
    /// Chosen to sit at or below WebGPU's guaranteed `downlevel` limits, so a
    /// renderer written against these numbers runs on the weakest device the
    /// engine targets. It is a *contract*, not a description of any real
    /// device — [`NullInstance::portable`](crate::null::NullInstance::portable)
    /// reports exactly this.
    #[must_use]
    pub const fn minimum() -> Self {
        Self {
            max_image_2d: 8192,
            max_image_3d: 2048,
            max_image_array_layers: 256,
            max_storage_buffer_range: 128 << 20,
            max_uniform_buffer_range: 64 << 10,
            max_bind_groups: 4,
            max_bindless_descriptors: 0,
            max_push_constant_size: 0,
            max_color_attachments: 4,
            // WebGPU's downlevel floor: 1x and 4x, and nothing above.
            max_sample_count: 4,
            max_draw_indirect_count: 1,
            max_compute_workgroup_size: [256, 256, 64],
            max_compute_invocations_per_workgroup: 256,
            max_compute_workgroups_per_dimension: 65535,
            min_uniform_buffer_offset_alignment: 256,
            min_storage_buffer_offset_alignment: 256,
            optimal_buffer_copy_offset_alignment: 256,
            max_sampler_anisotropy: 1.0,
            timestamp_period_ns: 0.0,
        }
    }

    /// A representative desktop device with headroom on every axis.
    ///
    /// Used by [`NullInstance::gpu_driven`](crate::null::NullInstance::gpu_driven)
    /// and by tests that need plausible ceilings. Not a promise about any
    /// specific GPU.
    #[must_use]
    pub const fn desktop() -> Self {
        Self {
            max_image_2d: 16384,
            max_image_3d: 2048,
            max_image_array_layers: 2048,
            max_storage_buffer_range: u32::MAX as u64,
            max_uniform_buffer_range: 64 << 10,
            max_bind_groups: 8,
            max_bindless_descriptors: 500_000,
            max_push_constant_size: 128,
            max_color_attachments: 8,
            max_sample_count: 8,
            max_draw_indirect_count: 1 << 20,
            max_compute_workgroup_size: [1024, 1024, 64],
            max_compute_invocations_per_workgroup: 1024,
            max_compute_workgroups_per_dimension: 65535,
            min_uniform_buffer_offset_alignment: 64,
            min_storage_buffer_offset_alignment: 16,
            optimal_buffer_copy_offset_alignment: 4,
            max_sampler_anisotropy: 16.0,
            timestamp_period_ns: 1.0,
        }
    }
}

/// How the renderer gets geometry in front of the rasteriser.
///
/// One shader-permutation axis and one golden-image axis, declared best-first
/// and resolved downward: every value draws the same meshlets out of the same
/// pools, and only the submission tail differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GeometryPath {
    /// A mesh shader emits primitives directly, so the meshlet cull and the
    /// geometry it survives live in one dispatch.
    MeshShader,
    /// Indirect draws whose *count* is itself read from a GPU buffer, so the
    /// CPU never learns how many draws a pass emits.
    IndirectCount,
    /// Indirect draws with a CPU-known count: one argument per batch, each
    /// batch's instance count still decided on the GPU. The floor, and what
    /// WebGPU gets.
    IndirectPerBatch,
}

impl GeometryPath {
    /// The features this selector reads, and the only ones it may read.
    ///
    /// Stated as data because [`downgrades`] has to answer "which selector did
    /// this absence move", and a second hand-written table would drift from
    /// [`from_features`](Self::from_features) the first time an axis gains a
    /// flag. `selector_inputs_are_the_only_features_a_selector_reads` pins the
    /// two together.
    pub const INPUTS: Features = Features::MESH_SHADER.union(Features::DRAW_INDIRECT_COUNT);

    /// The best path `features` can run.
    #[must_use]
    pub const fn from_features(features: Features) -> Self {
        if features.contains(Features::MESH_SHADER) {
            Self::MeshShader
        } else if features.contains(Features::DRAW_INDIRECT_COUNT) {
            Self::IndirectCount
        } else {
            Self::IndirectPerBatch
        }
    }
}

/// How the renderer addresses textures and per-draw data from a shader.
///
/// The data-layout axis: it decides what a material index *means* in shader
/// source, which is why it is a permutation rather than a branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingModel {
    /// One runtime-sized descriptor array holding every texture, indexed by a
    /// number that travels in the instance data.
    Bindless,
    /// Fixed-size texture arrays, bound a page at a time, with draws sorted so
    /// one page serves a run of them.
    ArrayPages,
}

impl BindingModel {
    /// The features this selector reads — see [`GeometryPath::INPUTS`].
    pub const INPUTS: Features = Features::DESCRIPTOR_INDEXING;

    /// The best model `features` can run.
    #[must_use]
    pub const fn from_features(features: Features) -> Self {
        if features.contains(Features::DESCRIPTOR_INDEXING) {
            Self::Bindless
        } else {
            Self::ArrayPages
        }
    }
}

/// How the renderer resolves indirect lighting, shadows and reflections.
///
/// The raster path exists on every device and is what the browser and Apple
/// targets get; see topic 39's worked example for why that is a capability
/// answer and not a platform branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LightingPath {
    /// Inline ray queries traced against an acceleration structure.
    RayTraced,
    /// Shadow maps, screen-space reflections and a probe-based ambient term.
    Rasterised,
}

impl LightingPath {
    /// The features this selector reads — see [`GeometryPath::INPUTS`].
    pub const INPUTS: Features = Features::RAY_QUERY.union(Features::ACCELERATION_STRUCTURE);

    /// The best path `features` can run.
    ///
    /// Ray tracing needs **both** halves: a device reporting only one of
    /// [`Features::RAY_QUERY`] and [`Features::ACCELERATION_STRUCTURE`] has
    /// nothing to trace, or nothing to trace with, and rasterises.
    #[must_use]
    pub const fn from_features(features: Features) -> Self {
        if features.contains(Features::RAY_QUERY.union(Features::ACCELERATION_STRUCTURE)) {
            Self::RayTraced
        } else {
            Self::Rasterised
        }
    }
}

/// Everything the renderer needs to know about a device's abilities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceCaps {
    /// Optional capabilities present.
    pub features: Features,
    /// Numeric ceilings.
    pub limits: Limits,
}

impl DeviceCaps {
    /// Which [`GeometryPath`] this device selects.
    ///
    /// Derived rather than stored, here and for the other two selectors: a
    /// backend cannot claim a path while missing what that path is built on —
    /// the one lie that would silently produce a renderer taking the bindless
    /// path on a device without bindless.
    #[must_use]
    pub const fn geometry_path(&self) -> GeometryPath {
        GeometryPath::from_features(self.features)
    }

    /// Which [`BindingModel`] this device selects.
    #[must_use]
    pub const fn binding_model(&self) -> BindingModel {
        BindingModel::from_features(self.features)
    }

    /// Which [`LightingPath`] this device selects.
    #[must_use]
    pub const fn lighting_path(&self) -> LightingPath {
        LightingPath::from_features(self.features)
    }

    /// Which of `required` this device is missing — empty if it satisfies them.
    ///
    /// The shape `Instance::create_device`
    /// uses to build [`HalError::UnsupportedFeatures`](crate::HalError::UnsupportedFeatures),
    /// so the error names the gap instead of saying "unsupported".
    #[must_use]
    pub fn missing(&self, required: Features) -> Features {
        required.difference(self.features)
    }

    /// Whether this device satisfies `required`.
    #[must_use]
    pub fn supports(&self, required: Features) -> bool {
        self.features.contains(required)
    }
}

/// Which derived selector an absent feature moved, and the value it moved it to.
///
/// Only the flags the three selectors are derived from have one. A device that
/// declines a timestamp query or a debug marker has still downgraded — the
/// caller asked and did not get — but nothing about which path the renderer
/// compiles changed, and [`Downgrade::selected`] says so by being `None` rather
/// than by naming a path that was never in question.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectedPath {
    /// A [`GeometryPath`], from [`GeometryPath::INPUTS`].
    Geometry(GeometryPath),
    /// A [`BindingModel`], from [`BindingModel::INPUTS`].
    Binding(BindingModel),
    /// A [`LightingPath`], from [`LightingPath::INPUTS`].
    Lighting(LightingPath),
}

impl fmt::Display for SelectedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry(path) => write!(f, "geometry {path:?}"),
            Self::Binding(model) => write!(f, "binding {model:?}"),
            Self::Lighting(path) => write!(f, "lighting {path:?}"),
        }
    }
}

/// One optional feature a device was asked for and did not grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Downgrade {
    /// The absent feature — always exactly one flag.
    pub feature: Features,
    /// How the flag is spelled in source, e.g. `"MESH_SHADER"`. Taken from the
    /// flag table rather than written out, so it cannot disagree with the flag.
    pub name: &'static str,
    /// The selector this absence moved, or `None` when no selector reads it.
    pub selected: Option<SelectedPath>,
}

impl fmt::Display for Downgrade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.selected {
            Some(selected) => write!(f, "{} -> {selected}", self.name),
            None => f.write_str(self.name),
        }
    }
}

/// Every optional feature a device declined, with the path each absence chose.
///
/// From [`downgrades`], and **empty when the device granted everything asked
/// for**. That is the property worth having: topic 39 wants one line at device
/// creation and no line at all when nothing degraded, so silence means no
/// downgrade rather than meaning nobody looked.
///
/// [`Display`](fmt::Display) renders the whole set as one comma-separated
/// clause — `MESH_SHADER -> geometry IndirectCount, TIMESTAMP_QUERY` — which is
/// the log line's payload. It renders empty when the set is, so a caller must
/// gate on [`is_empty`](Self::is_empty) rather than logging unconditionally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Downgrades {
    absent: Features,
    geometry: GeometryPath,
    binding: BindingModel,
    lighting: LightingPath,
}

impl Downgrades {
    /// The absent features as one flag set. Empty means nothing degraded.
    #[must_use]
    pub const fn absent(&self) -> Features {
        self.absent
    }

    /// Whether the device granted everything it was asked for.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.absent.is_empty()
    }

    /// Each absent feature in flag-declaration order, with what it cost.
    pub fn iter(&self) -> impl Iterator<Item = Downgrade> + '_ {
        use bitflags::Flags;

        Features::FLAGS
            .iter()
            // `GPU_DRIVEN` is a named union of other flags, not a bit of its
            // own; reporting it would name the same absence twice.
            .filter(|flag| flag.value().bits().is_power_of_two())
            .filter(|flag| self.absent.contains(*flag.value()))
            .map(|flag| Downgrade {
                feature: *flag.value(),
                name: flag.name(),
                selected: self.selected_for(*flag.value()),
            })
    }

    /// Which selector `feature`'s absence moved, from the selectors' own
    /// declared inputs rather than from a table repeated here.
    fn selected_for(&self, feature: Features) -> Option<SelectedPath> {
        if GeometryPath::INPUTS.contains(feature) {
            Some(SelectedPath::Geometry(self.geometry))
        } else if BindingModel::INPUTS.contains(feature) {
            Some(SelectedPath::Binding(self.binding))
        } else if LightingPath::INPUTS.contains(feature) {
            Some(SelectedPath::Lighting(self.lighting))
        } else {
            None
        }
    }
}

impl fmt::Display for Downgrades {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, downgrade) in self.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{downgrade}")?;
        }
        Ok(())
    }
}

/// What `granted` did not deliver of the optional features `requested`.
///
/// Topic 39: *"Every downgrade is logged once, at device creation, naming the
/// feature and the path it selected."* This is that line's content, computed
/// away from the call site so it can be checked without a device — the call
/// site's only job is to log the result when it is not
/// [`empty`](Downgrades::is_empty).
///
/// `requested` is the **optional** set and nothing else. A missing *required*
/// feature is not a downgrade: it is
/// [`HalError::UnsupportedFeatures`](crate::HalError::UnsupportedFeatures) and
/// there is no device to describe.
///
/// ```
/// use crcbl_hal::{DeviceCaps, Features, Limits, downgrades};
///
/// let webgpu_shaped = DeviceCaps {
///     features: Features::COMPUTE,
///     limits: Limits::minimum(),
/// };
/// let report = downgrades(Features::COMPUTE | Features::DESCRIPTOR_INDEXING, &webgpu_shaped);
/// assert_eq!(report.to_string(), "DESCRIPTOR_INDEXING -> binding ArrayPages");
///
/// let asked_for_what_it_has = downgrades(Features::COMPUTE, &webgpu_shaped);
/// assert!(asked_for_what_it_has.is_empty());
/// ```
#[must_use]
pub fn downgrades(requested: Features, granted: &DeviceCaps) -> Downgrades {
    Downgrades {
        absent: granted.missing(requested),
        geometry: granted.geometry_path(),
        binding: granted.binding_model(),
        lighting: granted.lighting_path(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HalError;

    /// A device with mesh shaders and no GPU-side draw count must still take
    /// the mesh path. The natural way to write the mapping wrong is to test the
    /// cheaper flag first, or to require both, and either produces a device
    /// that has the best path available and does not use it.
    #[test]
    fn mesh_shading_wins_without_a_draw_indirect_count() {
        let mesh_only = GeometryPath::from_features(Features::MESH_SHADER);
        assert_eq!(mesh_only, GeometryPath::MeshShader);
        assert_eq!(
            GeometryPath::from_features(Features::MESH_SHADER | Features::DRAW_INDIRECT_COUNT),
            mesh_only,
            "a draw count must not displace the mesh path"
        );
        assert_eq!(
            GeometryPath::from_features(Features::DRAW_INDIRECT_COUNT),
            GeometryPath::IndirectCount,
            "the count alone is the middle path, not the floor"
        );
    }

    /// Ray-traced lighting needs a structure to trace *and* a way to trace it.
    /// Written as a truth table because the mistake is an `or`, which reads
    /// identically and lets a device select a path it cannot execute.
    #[test]
    fn ray_traced_lighting_needs_both_halves() {
        for (features, expected) in [
            (Features::empty(), LightingPath::Rasterised),
            (Features::RAY_QUERY, LightingPath::Rasterised),
            (Features::ACCELERATION_STRUCTURE, LightingPath::Rasterised),
            (
                Features::RAY_TRACING_PIPELINE | Features::ACCELERATION_STRUCTURE,
                LightingPath::Rasterised,
            ),
            (
                Features::RAY_QUERY | Features::ACCELERATION_STRUCTURE,
                LightingPath::RayTraced,
            ),
        ] {
            assert_eq!(
                LightingPath::from_features(features),
                expected,
                "{features:?}"
            );
        }
    }

    /// The null-device case: a backend reporting nothing at all must still
    /// select a runnable path on every axis, because there is no fourth answer
    /// and no `Option` for the renderer to handle.
    #[test]
    fn an_empty_feature_set_lands_on_the_floor_of_every_selector() {
        let caps = DeviceCaps {
            features: Features::empty(),
            limits: Limits::minimum(),
        };
        assert_eq!(caps.geometry_path(), GeometryPath::IndirectPerBatch);
        assert_eq!(caps.binding_model(), BindingModel::ArrayPages);
        assert_eq!(caps.lighting_path(), LightingPath::Rasterised);
    }

    /// The accessors must read the device's own features and nothing else — a
    /// selector wired to a constant would satisfy every assertion above.
    #[test]
    fn the_accessors_follow_the_devices_features() {
        let caps = DeviceCaps {
            features: Features::MESH_SHADER
                | Features::DESCRIPTOR_INDEXING
                | Features::RAY_QUERY
                | Features::ACCELERATION_STRUCTURE,
            limits: Limits::desktop(),
        };
        assert_eq!(caps.geometry_path(), GeometryPath::MeshShader);
        assert_eq!(caps.binding_model(), BindingModel::Bindless);
        assert_eq!(caps.lighting_path(), LightingPath::RayTraced);
    }

    /// Topic 39's monotonicity rule: **no capability's arrival may select a
    /// path that needs more than the one it replaced.** Ranked by a table
    /// written out here rather than by an `Ord` derived from declaration order,
    /// which would make the assertion circular with the thing it checks.
    #[test]
    fn adding_a_feature_never_selects_a_path_that_needs_more() {
        const AXES: [Features; 5] = [
            Features::MESH_SHADER,
            Features::DRAW_INDIRECT_COUNT,
            Features::DESCRIPTOR_INDEXING,
            Features::RAY_QUERY,
            Features::ACCELERATION_STRUCTURE,
        ];

        fn rank(features: Features) -> [u8; 3] {
            [
                match GeometryPath::from_features(features) {
                    GeometryPath::IndirectPerBatch => 0,
                    GeometryPath::IndirectCount => 1,
                    GeometryPath::MeshShader => 2,
                },
                match BindingModel::from_features(features) {
                    BindingModel::ArrayPages => 0,
                    BindingModel::Bindless => 1,
                },
                match LightingPath::from_features(features) {
                    LightingPath::Rasterised => 0,
                    LightingPath::RayTraced => 1,
                },
            ]
        }

        for subset in 0..(1u32 << AXES.len()) {
            let base = AXES
                .iter()
                .enumerate()
                .filter(|(bit, _)| subset & (1 << bit) != 0)
                .fold(Features::empty(), |acc, (_, axis)| acc.union(*axis));
            let before = rank(base);
            for axis in AXES {
                let after = rank(base.union(axis));
                assert!(
                    after.iter().zip(&before).all(|(now, then)| now >= then),
                    "gaining {axis:?} on top of {base:?} went backwards: \
                     {before:?} -> {after:?}"
                );
            }
        }
    }

    #[test]
    fn gpu_driven_is_exactly_the_documented_bundle() {
        // Guards the flag list against silent growth. Named exhaustively, and
        // checked for equality rather than containment: a feature added to
        // `GPU_DRIVEN` without a doc update is the thing this catches, and a
        // `contains` list of a subset never would.
        let documented = Features::DESCRIPTOR_INDEXING
            | Features::BUFFER_DEVICE_ADDRESS
            | Features::DRAW_INDIRECT_COUNT
            | Features::MULTI_DRAW_INDIRECT
            | Features::COMPUTE
            | Features::TIMELINE_SEMAPHORE;
        assert_eq!(Features::GPU_DRIVEN, documented);
        // Push constants are NOT in the bundle: WebGPU lacks them, but so do
        // some otherwise fully GPU-driven configurations, and the renderer must
        // have a uniform-buffer path regardless.
        assert!(!Features::GPU_DRIVEN.contains(Features::PUSH_CONSTANTS));
        // Neither present capability is in it, for the same reason as each
        // other: a device without them renders identical frames and only loses
        // the ability to be *asked* about the display.
        assert!(!Features::GPU_DRIVEN.contains(Features::PRESENT_FEEDBACK));
        assert!(!Features::GPU_DRIVEN.contains(Features::PRESENT_TIMING));
        // Nor is anything the selectors branch on beyond the draw count: mesh
        // shading and ray tracing are their own axes, and folding them in here
        // would make the bundle a tier again.
        assert!(!Features::GPU_DRIVEN.contains(Features::MESH_SHADER));
        assert!(!Features::GPU_DRIVEN.contains(Features::RAY_QUERY));
        assert!(!Features::GPU_DRIVEN.contains(Features::ACCELERATION_STRUCTURE));
    }

    #[test]
    fn missing_names_the_gap() {
        let caps = DeviceCaps {
            features: Features::COMPUTE | Features::TIMELINE_SEMAPHORE,
            limits: Limits::minimum(),
        };
        let missing = caps.missing(Features::GPU_DRIVEN);
        assert!(missing.contains(Features::DESCRIPTOR_INDEXING));
        assert!(missing.contains(Features::BUFFER_DEVICE_ADDRESS));
        assert!(!missing.contains(Features::COMPUTE));
        assert!(!caps.supports(Features::GPU_DRIVEN));
        assert!(caps.supports(Features::COMPUTE));
        assert!(caps.missing(Features::COMPUTE).is_empty());
    }

    /// Strictly below, not merely "not above". The regression worth guarding is
    /// the desktop preset being pasted into `minimum()`, and a table of `<=`
    /// passes on two byte-identical presets while the name says otherwise.
    #[test]
    fn minimum_limits_are_below_desktop_limits() {
        let min = Limits::minimum();
        let desktop = Limits::desktop();
        assert!(min.max_image_2d < desktop.max_image_2d);
        assert!(min.max_image_array_layers < desktop.max_image_array_layers);
        assert!(min.max_storage_buffer_range < desktop.max_storage_buffer_range);
        assert!(min.max_bind_groups < desktop.max_bind_groups);
        assert!(min.max_bindless_descriptors < desktop.max_bindless_descriptors);
        assert!(min.max_push_constant_size < desktop.max_push_constant_size);
        assert!(min.max_color_attachments < desktop.max_color_attachments);
        assert!(min.max_sample_count < desktop.max_sample_count);
        assert!(min.max_draw_indirect_count < desktop.max_draw_indirect_count);
        assert!(min.max_compute_workgroup_size[0] < desktop.max_compute_workgroup_size[0]);
        assert!(min.max_compute_workgroup_size[1] < desktop.max_compute_workgroup_size[1]);
        assert!(
            min.max_compute_invocations_per_workgroup
                < desktop.max_compute_invocations_per_workgroup
        );
        assert!(min.max_sampler_anisotropy < desktop.max_sampler_anisotropy);
        assert!(min.timestamp_period_ns < desktop.timestamp_period_ns);

        // The fields the two presets legitimately agree on. Listed rather than
        // omitted, so "the floor already matches a desktop device here" is a
        // stated fact instead of a gap in the table above.
        assert!(min.max_image_3d <= desktop.max_image_3d);
        assert!(min.max_uniform_buffer_range <= desktop.max_uniform_buffer_range);
        assert!(min.max_compute_workgroup_size[2] <= desktop.max_compute_workgroup_size[2]);
        assert!(
            min.max_compute_workgroups_per_dimension
                <= desktop.max_compute_workgroups_per_dimension
        );

        // A sample count is a bit in a mask underneath, so a preset reporting a
        // non-power-of-two ceiling would make every backend's check nonsense.
        for limits in [min, desktop] {
            assert!(limits.max_sample_count.is_power_of_two());
            assert!(limits.max_sample_count <= 64);
        }
        // Alignment limits run the other way: the floor is the *coarser* one,
        // and strictly so for the same reason.
        assert!(
            min.min_uniform_buffer_offset_alignment > desktop.min_uniform_buffer_offset_alignment
        );
        assert!(
            min.min_storage_buffer_offset_alignment > desktop.min_storage_buffer_offset_alignment
        );
        assert!(
            min.optimal_buffer_copy_offset_alignment > desktop.optimal_buffer_copy_offset_alignment
        );
    }

    /// Every flag is a hand-written `1 << N`, and nothing checked the shifts
    /// were distinct. A repeated shift makes [`Features::contains`] vacuously
    /// true for the shadowed flag — it is never absent, so no test that asks
    /// "is X missing" can ever see it.
    ///
    /// The count comes from the flag table rather than a literal, so the check
    /// grows with the enum instead of needing an edit that would not happen.
    #[test]
    fn every_feature_owns_a_distinct_bit() {
        use bitflags::Flags;

        // `GPU_DRIVEN` is a named union of other flags, not a bit of its own.
        let single_bit: Vec<&'static str> = Features::FLAGS
            .iter()
            .filter(|flag| flag.value().bits().is_power_of_two())
            .map(bitflags::Flag::name)
            .collect();
        assert_eq!(
            Features::all().bits().count_ones() as usize,
            single_bit.len(),
            "two of {single_bit:?} share a bit"
        );
    }

    /// Every optional feature must be individually observable: `missing` names
    /// exactly it, and a device without it refuses it.
    ///
    /// Driven off the flag table, because a hand-written list structurally
    /// cannot keep up — nine flags had no mention in any test at all when this
    /// was written, including the two the queue selection keys off.
    #[test]
    fn every_feature_is_reported_one_at_a_time() {
        use bitflags::Flags;

        for flag in Features::FLAGS {
            let feature = *flag.value();
            if !feature.bits().is_power_of_two() {
                continue;
            }
            let name = flag.name();
            let without = DeviceCaps {
                features: Features::all().difference(feature),
                limits: Limits::desktop(),
            };
            assert_eq!(without.missing(feature), feature, "{name}");
            assert!(!without.supports(feature), "{name}");

            let only = DeviceCaps {
                features: feature,
                limits: Limits::minimum(),
            };
            assert!(only.supports(feature), "{name}");
            assert!(only.missing(feature).is_empty(), "{name}");
            assert!(
                HalError::UnsupportedFeatures {
                    missing: without.missing(feature)
                }
                .to_string()
                .contains(name),
                "the error a backend owes must name {name}"
            );
        }
    }

    #[test]
    fn minimum_limits_report_no_bindless_or_push_constants() {
        let min = Limits::minimum();
        assert_eq!(min.max_bindless_descriptors, 0);
        assert_eq!(min.max_push_constant_size, 0);
        assert_eq!(
            min.max_draw_indirect_count, 1,
            "the floor emits one draw per indirect call"
        );
    }

    /// The mapping [`Downgrades::selected_for`] reads must be the whole of what
    /// a selector reads, or the log line attributes an absence to a path that
    /// did not move — and, worse, stays silent about the one that did.
    ///
    /// Checked by exhaustion rather than by inspection: every flag outside a
    /// selector's `INPUTS` is toggled against every subset of the flags inside
    /// it, and the selector's answer must not budge.
    #[test]
    fn selector_inputs_are_the_only_features_a_selector_reads() {
        use bitflags::Flags;

        let single_bits: Vec<Features> = Features::FLAGS
            .iter()
            .map(|flag| *flag.value())
            .filter(|feature| feature.bits().is_power_of_two())
            .collect();

        // One function per selector, each returning a rank so the three can be
        // compared through one code path.
        type Select = fn(Features) -> u8;
        let selectors: [(Features, Select); 3] = [
            (GeometryPath::INPUTS, |features| {
                GeometryPath::from_features(features) as u8
            }),
            (BindingModel::INPUTS, |features| {
                BindingModel::from_features(features) as u8
            }),
            (LightingPath::INPUTS, |features| {
                LightingPath::from_features(features) as u8
            }),
        ];

        for (inputs, select) in selectors {
            let members: Vec<Features> = single_bits
                .iter()
                .copied()
                .filter(|feature| inputs.contains(*feature))
                .collect();
            assert!(
                !members.is_empty(),
                "a selector reading nothing would pass this vacuously"
            );

            for subset in 0..(1u32 << members.len()) {
                let base = members
                    .iter()
                    .enumerate()
                    .filter(|(bit, _)| subset & (1 << bit) != 0)
                    .fold(Features::empty(), |acc, (_, feature)| acc.union(*feature));
                let expected = select(base);
                for other in &single_bits {
                    if inputs.contains(*other) {
                        continue;
                    }
                    assert_eq!(
                        select(base.union(*other)),
                        expected,
                        "{other:?} moved a selector that does not list it in INPUTS"
                    );
                }
            }

            // The other half: each input must be *able* to move it, or it is
            // listed in `INPUTS` and read by nothing.
            for member in &members {
                assert!(
                    (0..(1u32 << members.len())).any(|subset| {
                        let base = members
                            .iter()
                            .enumerate()
                            .filter(|(bit, _)| subset & (1 << bit) != 0)
                            .fold(Features::empty(), |acc, (_, feature)| acc.union(*feature));
                        select(base.union(*member)) != select(base.difference(*member))
                    }),
                    "{member:?} is listed in INPUTS but changes nothing"
                );
            }
        }
    }

    /// **Silence is the property.** A device that granted everything asked for
    /// must produce nothing to log, and the empty rendering is what the call
    /// site's `is_empty` guard stands in for.
    #[test]
    fn a_device_that_granted_everything_reports_no_downgrade() {
        let caps = DeviceCaps {
            features: Features::all(),
            limits: Limits::desktop(),
        };
        for requested in [
            Features::empty(),
            Features::COMPUTE,
            Features::GPU_DRIVEN,
            Features::all(),
        ] {
            let report = downgrades(requested, &caps);
            assert!(report.is_empty(), "{requested:?}");
            assert!(report.absent().is_empty(), "{requested:?}");
            assert_eq!(report.iter().count(), 0, "{requested:?}");
            assert_eq!(report.to_string(), "", "{requested:?}");
        }
    }

    /// One absent feature names *that* feature and the selector it moved —
    /// which is the whole of what topic 39 asks the line to say.
    ///
    /// Driven off the selectors' own inputs so every axis is covered, and the
    /// expected value is computed from the device rather than written out, so
    /// the assertion is about the wiring rather than about three literals.
    #[test]
    fn one_absent_feature_names_itself_and_the_path_it_moved() {
        for (feature, name, expected) in [
            (
                Features::MESH_SHADER,
                "MESH_SHADER",
                SelectedPath::Geometry(GeometryPath::IndirectCount),
            ),
            (
                Features::DRAW_INDIRECT_COUNT,
                "DRAW_INDIRECT_COUNT",
                SelectedPath::Geometry(GeometryPath::MeshShader),
            ),
            (
                Features::DESCRIPTOR_INDEXING,
                "DESCRIPTOR_INDEXING",
                SelectedPath::Binding(BindingModel::ArrayPages),
            ),
            (
                Features::RAY_QUERY,
                "RAY_QUERY",
                SelectedPath::Lighting(LightingPath::Rasterised),
            ),
            (
                Features::ACCELERATION_STRUCTURE,
                "ACCELERATION_STRUCTURE",
                SelectedPath::Lighting(LightingPath::Rasterised),
            ),
        ] {
            let caps = DeviceCaps {
                features: Features::all().difference(feature),
                limits: Limits::desktop(),
            };
            let report = downgrades(Features::all(), &caps);
            assert!(!report.is_empty(), "{name}");
            assert_eq!(report.absent(), feature, "{name}");

            let entries: Vec<Downgrade> = report.iter().collect();
            assert_eq!(entries.len(), 1, "{name}");
            assert_eq!(entries[0].feature, feature, "{name}");
            assert_eq!(entries[0].name, name);
            assert_eq!(entries[0].selected, Some(expected), "{name}");

            let line = report.to_string();
            assert!(line.starts_with(name), "{line}");
            assert!(line.contains(&expected.to_string()), "{line}");
        }
    }

    /// A feature no selector is derived from is still a downgrade — the caller
    /// asked and did not get — but it must not claim to have moved a path.
    #[test]
    fn an_absence_no_selector_reads_names_no_path() {
        let caps = DeviceCaps {
            features: Features::all().difference(Features::TIMESTAMP_QUERY),
            limits: Limits::desktop(),
        };
        let report = downgrades(Features::all(), &caps);
        let entries: Vec<Downgrade> = report.iter().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].selected, None);
        assert_eq!(report.to_string(), "TIMESTAMP_QUERY");
    }

    /// Several absences must *all* be named. The failure this guards is a line
    /// that reports the first gap and stops, which reads like a complete
    /// account and is not.
    #[test]
    fn every_absent_feature_is_named() {
        // The browser's shape: compute and nothing else, asked for the full
        // GPU-driven bundle plus the two selector axes above it.
        let caps = DeviceCaps {
            features: Features::COMPUTE | Features::TIMELINE_SEMAPHORE,
            limits: Limits::minimum(),
        };
        let requested = Features::GPU_DRIVEN
            | Features::MESH_SHADER
            | Features::RAY_QUERY
            | Features::ACCELERATION_STRUCTURE
            | Features::TIMESTAMP_QUERY;
        let report = downgrades(requested, &caps);

        let named: Vec<&'static str> = report.iter().map(|downgrade| downgrade.name).collect();
        assert_eq!(
            named,
            [
                "DESCRIPTOR_INDEXING",
                "BUFFER_DEVICE_ADDRESS",
                "DRAW_INDIRECT_COUNT",
                "MULTI_DRAW_INDIRECT",
                "TIMESTAMP_QUERY",
                "MESH_SHADER",
                "RAY_QUERY",
                "ACCELERATION_STRUCTURE",
            ],
            "in flag-declaration order, and `GPU_DRIVEN` itself never named"
        );

        let line = report.to_string();
        for name in &named {
            assert!(line.contains(name), "{line} omits {name}");
        }
        assert!(line.contains("binding ArrayPages"), "{line}");
        assert!(line.contains("geometry IndirectPerBatch"), "{line}");
        assert!(line.contains("lighting Rasterised"), "{line}");

        // What the device *did* grant is never named, and neither is anything
        // nobody asked for: the line is about the gap, not about the device.
        assert!(!line.contains("COMPUTE"), "{line}");
        assert!(!line.contains("TIMELINE_SEMAPHORE"), "{line}");
        assert!(!line.contains("PRESENT_FEEDBACK"), "{line}");
    }

    #[test]
    fn backend_kind_displays_lowercase() {
        assert_eq!(BackendKind::Vulkan.to_string(), "vulkan");
        assert_eq!(BackendKind::Null.to_string(), "null");
        // `crcbl-wgpu` and `crcbl-webgpu` are two backends that both reach
        // WebGPU in a browser, and the whole point of the second variant is
        // that a log line can say which one drew. An `as_str` arm copied from
        // its neighbour would make them indistinguishable.
        assert_eq!(BackendKind::Wgpu.to_string(), "wgpu");
        assert_eq!(BackendKind::WebGpu.to_string(), "webgpu");
        // Exhaustive on purpose: a variant added without a name of its own
        // would otherwise share one silently.
        let every = [
            BackendKind::Vulkan,
            BackendKind::Wgpu,
            BackendKind::WebGpu,
            BackendKind::Metal,
            BackendKind::Dx12,
            BackendKind::Null,
        ];
        let names: std::collections::HashSet<String> =
            every.iter().map(ToString::to_string).collect();
        assert_eq!(names.len(), every.len(), "two backend kinds share a name");
    }
}
