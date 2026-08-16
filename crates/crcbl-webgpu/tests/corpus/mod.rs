//! The canonical stream: one of every command shape, with no two fields alike.
//!
//! Shared rather than duplicated, because every test that uses it is about the
//! *same* bytes. `stream.rs` round-trips it through this crate's own reader;
//! `fixture.rs` freezes it into `tests/fixtures/` for the JavaScript decoder to
//! meet; `reply.rs` borrows it for the one check that is about both directions
//! at once. A second copy of the corpus would be a second thing to keep in step,
//! and the whole point of the fixture is that nothing drifts unnoticed.
//!
//! The replies are a sibling module, [`replies`](crate::replies) — see its docs
//! for why they are not in here.

use crcbl_core::Handle;
use crcbl_hal::{
    AdapterId, Barriers, BindGroupDesc, BindGroupEntry, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindingFlags, BindingKind, BindingResource, BlendFactor, BlendOp, BlendState, BufferBarrier,
    BufferCopy, BufferDesc, BufferImageCopy, BufferUsage, ClearValue, ColorAttachment,
    ColorTargetState, ColorWrites, CommandEncoderDesc, CompareOp, CompositeAlpha, ComputePassDesc,
    ComputePipelineDesc, CullMode, DepthBias, DepthStencilAttachment, DepthStencilState,
    DeviceDesc, Extent3d, Features, FilterMode, Format, FrontFace, GraphicsPipelineDesc,
    ImageAspect, ImageBarrier, ImageCopy, ImageDesc, ImageSubresourceLayers, ImageSubresourceRange,
    ImageType, ImageUsage, ImageViewDesc, ImageViewType, LoadOp, MemoryLocation, MultisampleState,
    Offset3d, PipelineLayoutDesc, PolygonMode, PresentInfo, PresentMode, PrimitiveState,
    PrimitiveTopology, PushConstantRange, QueueTransfer, ReadbackDesc, Rect2d, RenderPassDesc,
    ResourceState, SampleType, SamplerAddressMode, SamplerDesc, SemaphoreSignal, SemaphoreWait,
    ShaderEntry, ShaderModuleDesc, ShaderStages, StencilFaceState, StencilOp, StencilState,
    StoreOp, SubmitInfo, SwapchainDesc, depth,
};
use crcbl_webgpu::{Command, StreamWriter};

/// A handle with distinct index and generation halves, so a field written with
/// the two swapped does not still compare equal.
pub fn handle<T>(index: u32, generation: u32) -> Handle<T> {
    Handle::from_bits((u64::from(generation) << 32) | u64::from(index))
        .expect("a non-zero generation is a real generation")
}

/// One of every command this slice encodes, with no two fields sharing a value.
///
/// Shared values are how a round-trip test passes while the encoder writes two
/// fields in the wrong order — every number here is distinct for that reason,
/// and every optional field appears both ways somewhere in the list.
pub fn every_command() -> Vec<Command> {
    vec![
        Command::CreateBuffer {
            buffer: handle(11, 12),
            label: Some("instances".into()),
            size: 4096,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        },
        // The unlabelled twin: `None` and `Some("")` are different values.
        Command::CreateBuffer {
            buffer: handle(13, 14),
            label: None,
            size: 1,
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        },
        Command::CreateBuffer {
            buffer: handle(15, 16),
            label: Some(String::new()),
            size: u64::MAX,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostReadback,
        },
        // The canvas key is the whole target, and it is `19` rather than `0` so
        // a writer that dropped the field would not still compare equal.
        Command::CreateSurface {
            surface: handle(45, 46),
            canvas_id: 19,
        },
        // **Every [`ImageType`] appears**, because the code table in
        // `web/engine/gpu-stream.js` is a hand-written list and a row for a
        // code the fixture never carries is a row nothing checks. The three
        // extent components differ from each other and from `mip_levels` and
        // `samples` in every image below, so a field written in the wrong order
        // decodes to a different number rather than to the same one.
        Command::CreateImage {
            image: handle(61, 62),
            label: Some("gbuffer albedo".into()),
            image_type: ImageType::D2,
            extent: Extent3d {
                width: 1280,
                height: 720,
                depth_or_layers: 3,
            },
            format: Format::Rgba8UnormSrgb,
            mip_levels: 11,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
        },
        // The unlabelled twin, and the volume: `depth_or_layers` is a depth
        // here and an array-layer count above, decided by nothing but the
        // `image_type` byte.
        Command::CreateImage {
            image: handle(63, 64),
            label: None,
            image_type: ImageType::D3,
            extent: Extent3d {
                width: 160,
                height: 90,
                depth_or_layers: 64,
            },
            format: Format::R16Float,
            mip_levels: 7,
            samples: 4,
            usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_SRC,
        },
        // **Zero `mip_levels` and zero `samples`, deliberately.** No device
        // accepts either, and both cross verbatim: the encoding refuses
        // malformed *streams*, not descriptors a replayer will reject through
        // `take_error`. The two images above are what pin the order of the
        // pair, since these two values are equal.
        //
        // `usage` is `ImageUsage::all()`, which is what pins the claimed-bit
        // mask the JavaScript decoder enforces — a mask narrower than the HAL's
        // refuses this very command.
        Command::CreateImage {
            image: handle(65, 66),
            label: Some(String::new()),
            image_type: ImageType::D1,
            extent: Extent3d {
                width: 256,
                height: 1,
                depth_or_layers: 1,
            },
            format: Format::R8Unorm,
            mip_levels: 0,
            samples: 0,
            usage: ImageUsage::all(),
        },
        // **Every [`ImageViewType`] appears too**, for the reason every
        // `ImageType` does. The two handles in each are distinct in both halves
        // so the id being filled in cannot be confused with the id being read,
        // and every subresource field holds its own number so a transposition
        // inside the range is visible.
        Command::CreateImageView {
            view: handle(67, 68),
            label: Some("cascade 2".into()),
            image: handle(61, 62),
            view_type: ImageViewType::D2Array,
            format: Format::D32FloatS8Uint,
            range: ImageSubresourceRange {
                aspect: ImageAspect::DEPTH | ImageAspect::STENCIL,
                base_mip: 1,
                mip_count: 2,
                base_layer: 3,
                layer_count: 4,
            },
        },
        // `ImageSubresourceRange::ALL` is `u32::MAX` and crosses as itself:
        // resolving it would need the image's own mip and layer counts, which
        // this side of the boundary does not have. One of the two counts is the
        // sentinel and the other is not, so the pair cannot be swapped
        // unnoticed.
        Command::CreateImageView {
            view: handle(69, 70),
            label: None,
            image: handle(63, 64),
            view_type: ImageViewType::D3,
            format: Format::R16Float,
            range: ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 5,
                mip_count: ImageSubresourceRange::ALL,
                base_layer: 6,
                layer_count: 7,
            },
        },
        Command::CreateImageView {
            view: handle(71, 72),
            label: Some(String::new()),
            image: handle(65, 66),
            view_type: ImageViewType::D1,
            format: Format::R8Unorm,
            range: ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 9,
                mip_count: 10,
                base_layer: 11,
                layer_count: 12,
            },
        },
        Command::CreateImageView {
            view: handle(73, 74),
            label: Some("sky cube".into()),
            image: handle(61, 62),
            view_type: ImageViewType::Cube,
            format: Format::Rgba8Unorm,
            range: ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 13,
                mip_count: 14,
                base_layer: 15,
                layer_count: 16,
            },
        },
        // The other half of each adjacent pair of view types, so `Cube` and
        // `CubeArray` are both on the wire and a table that folded one into the
        // other cannot stay green.
        Command::CreateImageView {
            view: handle(75, 76),
            label: None,
            image: handle(63, 64),
            view_type: ImageViewType::CubeArray,
            format: Format::Bgra8UnormSrgb,
            range: ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 17,
                mip_count: 8,
                base_layer: 18,
                layer_count: ImageSubresourceRange::ALL,
            },
        },
        // A stencil-only view, which is the one aspect no other command here
        // sets: with `COLOR` and `DEPTH | STENCIL` above, all three bits are
        // exercised, and the claimed-bit mask the JavaScript decoder derives is
        // held to three rather than two.
        Command::CreateImageView {
            view: handle(77, 78),
            label: Some("stencil".into()),
            image: handle(65, 66),
            view_type: ImageViewType::D2,
            format: Format::D24UnormS8Uint,
            range: ImageSubresourceRange {
                aspect: ImageAspect::STENCIL,
                base_mip: 19,
                mip_count: 20,
                base_layer: 21,
                layer_count: 22,
            },
        },
        // **Four samplers, because three fields of one kind sit in a row twice.**
        // `mag`/`min`/`mip` are three `FilterMode`s back to back and there are
        // only two variants, so no single command can make all three distinct —
        // the first three below each put the single `Linear` in a different
        // slot instead, and every pairwise transposition of the trio changes at
        // least one of them. `address_mode` needs no such trick: each command
        // spells three *different* modes, in a different rotation each time.
        //
        // The floats are all distinct within a command and across the four, and
        // one of them is deliberately not a short decimal — see `lod_min` below.
        //
        // **Every `SamplerAddressMode` appears**, for the reason every
        // `ImageType` does: the code table in `web/engine/gpu-stream.js` is a
        // hand-written list and a row the fixture never carries is a row nothing
        // checks. `ClampToBorder` is also the one WebGPU cannot express, so it
        // is what the replayer's refusal is driven through.
        Command::CreateSampler {
            sampler: handle(83, 84),
            label: Some("shadow pcf".into()),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Nearest,
            mip_filter: FilterMode::Nearest,
            address_mode: [
                SamplerAddressMode::Repeat,
                SamplerAddressMode::MirrorRepeat,
                SamplerAddressMode::ClampToEdge,
            ],
            lod_min: 0.5,
            // **The sentinel**, on the one sampler here a browser can actually
            // build: `f32::MAX` is `SamplerDesc::default`'s "no limit" and
            // crosses verbatim, and only the replayer can resolve it. Putting
            // it on a sampler the replayer refuses for some other reason would
            // leave the resolution unobserved.
            lod_max: f32::MAX,
            anisotropy: 1.0,
            // The reversed-Z shadow test. Its opposite is three commands down,
            // so a table that folded the pair cannot stay green.
            compare: Some(CompareOp::Greater),
        },
        // The unlabelled twin, with the awkward decimal and no comparison.
        Command::CreateSampler {
            sampler: handle(85, 86),
            label: None,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Nearest,
            address_mode: [
                SamplerAddressMode::ClampToBorder,
                SamplerAddressMode::Repeat,
                SamplerAddressMode::MirrorRepeat,
            ],
            // **`0.1` is not representable in binary and this is deliberate.**
            // The nearest `f32` is `0.100000001490116119384765625`, so an
            // encoding that went through a decimal string — or that widened to
            // `f64` and back — lands on a different number, and a mip clamp a
            // half-ulp out is a sampler nobody can tell is wrong.
            lod_min: 0.1,
            lod_max: 12.25,
            anisotropy: 1.0,
            compare: None,
        },
        Command::CreateSampler {
            sampler: handle(87, 88),
            label: Some(String::new()),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mip_filter: FilterMode::Linear,
            address_mode: [
                SamplerAddressMode::ClampToEdge,
                SamplerAddressMode::ClampToBorder,
                SamplerAddressMode::Repeat,
            ],
            lod_min: 2.0,
            lod_max: 3.0,
            // Past `1.0` while the filters are not all `Linear`, which WebGPU
            // forbids outright — the replayer's refusal, driven through a
            // command the Rust encoder really wrote.
            anisotropy: 16.0,
            compare: Some(CompareOp::Less),
        },
        Command::CreateSampler {
            sampler: handle(89, 90),
            label: Some("aniso".into()),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Linear,
            address_mode: [
                SamplerAddressMode::MirrorRepeat,
                SamplerAddressMode::ClampToEdge,
                SamplerAddressMode::Repeat,
            ],
            lod_min: 1.0,
            lod_max: 8.0,
            // Fractional, which WebGPU's `GPUSize32` cannot carry: the wire
            // takes it verbatim and the replayer is what narrows it.
            anisotropy: 4.5,
            compare: Some(CompareOp::Always),
        },
        // **Six layouts, because this is the first counted list of *structs*.**
        // Every command above is a fixed set of fields, or a list of scalars
        // whose stride cannot be wrong. An entry here is five fields deep and
        // carries an enum whose variants have different-length payloads, so a
        // stride out by a byte does not truncate — it decodes the next entry out
        // of the middle of this one and produces a layout that is well-formed
        // and describes different resources.
        //
        // The first is the long one: **every [`BindingKind`] WebGPU can express,
        // each with both values of every `bool` it carries**, so no payload byte
        // can be read as its neighbour's without the decode changing. Order is
        // free here — no entry sets `VARIABLE_COUNT` — which is what makes the
        // *next* one the order-sensitive case.
        Command::CreateBindGroupLayout {
            layout: handle(93, 94),
            label: Some("frame".into()),
            entries: vec![
                // The engine's own geometry binding: vertex pulling reads its
                // streams out of a read-only storage buffer.
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
                    kind: BindingKind::StorageBuffer {
                        read_only: true,
                        dynamic: false,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                // The same kind with **both** of its bools the other way round,
                // so a decoder that read one of them twice cannot stay green.
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    kind: BindingKind::StorageBuffer {
                        read_only: false,
                        dynamic: true,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                // The substitute for push constants, which WebGPU has none of.
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::VERTEX.union(ShaderStages::FRAGMENT),
                    kind: BindingKind::UniformBuffer { dynamic: true },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    kind: BindingKind::UniformBuffer { dynamic: false },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                // A `SampleType::Depth` slot beside a comparison sampler, which
                // is the pair `crcbl-hal` says WebGPU checks against each other:
                // a depth view is only bindable through a slot that says so.
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    kind: BindingKind::SampledImage {
                        view_type: ImageViewType::D2Array,
                        sample_type: SampleType::Depth,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                BindGroupLayoutEntry {
                    binding: 5,
                    visibility: ShaderStages::FRAGMENT,
                    kind: BindingKind::Sampler { comparison: true },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                // …and the ordinary pair, so both values of `sample_type` and
                // both of `comparison` are on the wire.
                BindGroupLayoutEntry {
                    binding: 6,
                    visibility: ShaderStages::FRAGMENT,
                    kind: BindingKind::SampledImage {
                        view_type: ImageViewType::Cube,
                        sample_type: SampleType::Float,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                BindGroupLayoutEntry {
                    binding: 7,
                    visibility: ShaderStages::COMPUTE,
                    kind: BindingKind::Sampler { comparison: false },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
            ],
        },
        // **The portable bindless declaration, and the unlabelled twin.** The
        // shape `BindGroupLayoutDesc::check_entries`'s own test builds: a
        // `u32::MAX` count — "as many as this device can", not four billion
        // descriptors — beside all three `BindingFlags`, on the entry that is
        // both last in the slice and highest-numbered. It crosses verbatim by
        // the sentinel rule in `docs/plan/41-webgpu-stream.md`, and **WebGPU has
        // no binding arrays at all**, so this is the layout the replayer has to
        // refuse rather than quietly build one descriptor for.
        Command::CreateBindGroupLayout {
            layout: handle(95, 96),
            label: None,
            entries: vec![
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    kind: BindingKind::StorageBuffer {
                        read_only: true,
                        dynamic: false,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    kind: BindingKind::SampledImage {
                        view_type: ImageViewType::D2,
                        sample_type: SampleType::Float,
                    },
                    count: u32::MAX,
                    flags: BindingFlags::VARIABLE_COUNT
                        .union(BindingFlags::PARTIALLY_BOUND)
                        .union(BindingFlags::UPDATE_AFTER_BIND),
                },
            ],
        },
        // Present-and-empty label, and an **empty entry list**: the counted list
        // at zero, which is the length a reader most easily mistakes for "read
        // until something stops you". The command after it is what would be
        // eaten if it did.
        Command::CreateBindGroupLayout {
            layout: handle(97, 98),
            label: Some(String::new()),
            entries: Vec::new(),
        },
        // **A fixed-size array**, which is the other half of the `count`
        // decision: `64` is neither `1` nor the sentinel, so a replayer that
        // treated "not the sentinel" as "one descriptor" would build a
        // single-slot binding for a sixty-four-slot declaration and every write
        // past the first would target a slot that does not exist.
        Command::CreateBindGroupLayout {
            layout: handle(99, 100),
            label: Some("texture page".into()),
            entries: vec![BindGroupLayoutEntry {
                binding: 8,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2Array,
                    sample_type: SampleType::Float,
                },
                count: 64,
                flags: BindingFlags::empty(),
            }],
        },
        // **The two stages WebGPU has no `GPUShaderStage` bit for.** Both are
        // real `ShaderStages` bits and both cross verbatim — the encoding
        // carries what the caller said and the replayer is what refuses it — and
        // `ShaderStages::TASK` is bit 4, which is the bit a claimed-bit mask
        // that stopped at `MESH` would drop. That mask is the JavaScript
        // decoder's, and this command is what holds it honest.
        Command::CreateBindGroupLayout {
            layout: handle(101, 102),
            label: None,
            entries: vec![
                BindGroupLayoutEntry {
                    binding: 9,
                    visibility: ShaderStages::MESH,
                    kind: BindingKind::SampledImage {
                        view_type: ImageViewType::D2Array,
                        sample_type: SampleType::Float,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                BindGroupLayoutEntry {
                    binding: 10,
                    visibility: ShaderStages::TASK,
                    kind: BindingKind::StorageBuffer {
                        read_only: true,
                        dynamic: false,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
            ],
        },
        // **Two entries differing in exactly one field, and sharing a binding
        // number.** Both halves are deliberate. The single differing field is
        // `read_only`, so a decoder that read the kind's payload once and copied
        // it fails here and nowhere else. The shared binding number is what pins
        // the rule `docs/plan/41-webgpu-stream.md` states about this command: a
        // decoder must preserve slice order rather than rebuild the list from
        // binding numbers, and one that keyed entries by binding would collapse
        // these two into one.
        //
        // `check_entries` rejects a duplicated binding number, and that is not a
        // contradiction — it is the division of labour this command is built on.
        // The seam's rules are the caller's to enforce before encoding; the
        // encoding refuses a malformed *stream* and nothing else, and a `u32`
        // claims every value it can hold. `BindingKind::StorageImage` is here
        // for a second reason of the same kind: WebGPU's
        // `GPUStorageTextureBindingLayout.format` is a required member and this
        // seam's variant carries no format, so it is the one `BindingKind` a
        // replayer cannot express — and it can only refuse what it was told.
        Command::CreateBindGroupLayout {
            layout: handle(103, 104),
            label: Some("gbuffer store".into()),
            entries: vec![
                BindGroupLayoutEntry {
                    binding: 11,
                    visibility: ShaderStages::COMPUTE,
                    kind: BindingKind::StorageImage { read_only: false },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                BindGroupLayoutEntry {
                    binding: 11,
                    visibility: ShaderStages::COMPUTE,
                    kind: BindingKind::StorageImage { read_only: true },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
            ],
        },
        // **The first command whose entries carry handles into three different
        // resource tables.** One of each [`BindingResource`] shape, and the
        // discriminant is the only thing that says which table each id indexes —
        // a buffer, a view and a sampler may legitimately hold identical bits. The
        // buffer entries carry a real `offset`/`size` pair and the `WHOLE_BUFFER`
        // sentinel, so both a numbered range and the "to the end" sentinel are on
        // the wire; `layout` names an existing layout handle. `variable_count` is
        // `None` here — the ordinary value — and `Some` on the twin below.
        Command::CreateBindGroup {
            group: handle(107, 108),
            label: Some("material".into()),
            layout: handle(93, 94),
            entries: vec![
                BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::Buffer {
                        buffer: handle(11, 12),
                        offset: 256,
                        size: 1024,
                    },
                },
                BindGroupEntry {
                    binding: 1,
                    array_index: 0,
                    resource: BindingResource::ImageView(handle(67, 68)),
                },
                BindGroupEntry {
                    binding: 2,
                    array_index: 0,
                    resource: BindingResource::Sampler(handle(83, 84)),
                },
                // **`WHOLE_BUFFER`, the sentinel**: `u64::MAX`, which crosses
                // verbatim and which only the replayer resolves — to WebGPU's
                // *absent* `GPUBufferBinding.size`, the right resolution here
                // where the view's range sentinel was absence too, and unlike
                // `lod_max` whose absence WebGPU reads as a number.
                BindGroupEntry {
                    binding: 3,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(handle(13, 14)),
                },
            ],
            variable_count: None,
        },
        // **`variable_count: Some`, and two entries differing in exactly one
        // field.** The two share binding 0 and differ only in `array_index` — the
        // bindless write path — so a decoder that keyed the list on binding would
        // collapse them, and a body that read `array_index` where `binding` goes
        // would not. Both `Some(_)` and a non-zero `array_index` are values a
        // `u32` claims, so they cross verbatim and the replayer is what refuses
        // them: WebGPU has no runtime-sized arrays and no per-element array
        // binding.
        Command::CreateBindGroup {
            group: handle(109, 110),
            label: None,
            layout: handle(95, 96),
            entries: vec![
                BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::ImageView(handle(69, 70)),
                },
                BindGroupEntry {
                    binding: 0,
                    array_index: 1,
                    resource: BindingResource::ImageView(handle(71, 72)),
                },
            ],
            variable_count: Some(2),
        },
        // **The heaviest single descriptor on the seam, carrying every artifact
        // at once.** All four formats are non-trivial, so a decoder that stopped
        // traversing one of them lands the cursor wrong for the rest and answers a
        // different module. The `spirv` words are distinct and open with SPIR-V's
        // own magic; the `wgsl` and `msl` are two different sources; and the
        // `dxil` list is two pairs whose **string lengths and container lengths
        // both differ** (`vsMain`/4 bytes against `fragment`/2), so a decoder that
        // read the wrong length for either leaf is caught rather than landing back
        // on a plausible boundary.
        Command::CreateShaderModule {
            module: handle(113, 114),
            label: Some("mesh.slang".into()),
            spirv: vec![
                0x0723_0203,
                0x0001_0600,
                0x0000_002a,
                0x0000_0007,
                0x0000_0000,
            ],
            wgsl: Some("@vertex fn vs() -> @builtin(position) vec4f { return vec4f(0.0); }".into()),
            msl: Some("#include <metal_stdlib>\nvertex float4 vs() { return 0; }".into()),
            dxil: vec![
                ("vsMain".into(), vec![0xDE, 0xAD, 0xBE, 0xEF]),
                ("fragment".into(), vec![0x01, 0x02]),
            ],
        },
        // **The first absence trap: `spirv` empty, `wgsl` `Some("")`, `msl`
        // `None`, `dxil` the empty list.** The empty WGSL is a *valid empty
        // module* — one with no entry points — and must not converge with the
        // `None` on the next module; the empty `spirv` is genuinely absent, unlike
        // the empty WGSL string; and the empty `dxil` list is absence, against the
        // pair-with-empty-container on the next module. A decoder that treated
        // `Some("")` as `None`, or an empty `dxil` container as an absent artifact,
        // goes red here.
        Command::CreateShaderModule {
            module: handle(115, 116),
            label: Some("empty.wgsl".into()),
            spirv: Vec::new(),
            wgsl: Some(String::new()),
            msl: None,
            dxil: Vec::new(),
        },
        // **The second absence trap, the mirror of the first: `wgsl` `None`, `msl`
        // `Some("")`, `dxil` a single pair whose container is empty.** The `None`
        // WGSL is absence where the module above had a present-and-empty string;
        // the `Some("")` MSL is present-and-empty where the module above had
        // `None`; and the empty *container* under a present name is a truncated
        // DXIL artifact, which is a present pair — the distinction the empty list
        // above is what carries. `spirv` is empty again, so the two empties are
        // both on the wire and neither is a sentinel.
        Command::CreateShaderModule {
            module: handle(117, 118),
            label: None,
            spirv: Vec::new(),
            wgsl: None,
            msl: Some(String::new()),
            dxil: vec![("truncated".into(), Vec::new())],
        },
        // **The last thing a pipeline is built from, and a counted list of bare
        // handles rather than of structs.** `bind_group_layouts` is in *set
        // order* — what a shader's `@group(n)` indexes — so the two handles here
        // are distinct in both halves and a decoder that reversed them answers a
        // different layout. `push_constants` is `None`, the ordinary value; the
        // twin below carries the `Some` WebGPU cannot express. Two handles rather
        // than one, because a single-element list decodes identically whether the
        // reader kept order or not.
        Command::CreatePipelineLayout {
            layout: handle(121, 122),
            label: Some("gbuffer".into()),
            bind_group_layouts: vec![handle(93, 94), handle(95, 96)],
            push_constants: None,
        },
        // **`push_constants: Some`, which WebGPU has no way to express at all.**
        // It crosses whole — `stages`, `offset`, `size` — because the writer
        // carries what the caller gives and the replayer is what refuses it,
        // exactly as a `BufferUsage::DEVICE_ADDRESS` buffer or a `VARIABLE_COUNT`
        // layout does. `stages` names two at once so the `ShaderStages` bits are
        // exercised beyond a single-bit value, and `offset` differs from `size`
        // so the pair cannot be swapped unnoticed. The single bind-group layout
        // keeps this list's length distinct from the two-entry one above.
        Command::CreatePipelineLayout {
            layout: handle(123, 124),
            label: None,
            bind_group_layouts: vec![handle(97, 98)],
            push_constants: Some(PushConstantRange {
                stages: ShaderStages::VERTEX.union(ShaderStages::FRAGMENT),
                offset: 16,
                size: 128,
            }),
        },
        // **The first command resolving handles into two *different* non-buffer
        // tables.** `layout` names an existing pipeline layout and `module` an
        // existing shader module — a handle carries no kind, so which table each
        // indexes is the wire position and nothing else. **`workgroup_size` is
        // `[8, 4, 2]`, non-uniform on purpose**: all three components differ, so a
        // transposition on the wire changes the decode rather than reproducing it,
        // and the replayer drops the field entirely because WebGPU reads the real
        // value from the module's `@workgroup_size`. `entry_point` is a real
        // string, distinct from every label here.
        Command::CreateComputePipeline {
            pipeline: handle(127, 128),
            label: Some("cull".into()),
            layout: handle(121, 122),
            module: handle(113, 114),
            entry_point: "computeMain".into(),
            workgroup_size: [8, 4, 2],
        },
        // **The largest descriptor on the seam, and the deepest.** Every field is
        // non-default and every enum differs from the ones beside it, so a
        // transposition anywhere in the tree changes the decode rather than
        // reproducing it. `layout`, `vertex_module` and the fragment module all
        // name objects created above, into three different tables; a handle
        // carries no kind, so which table each indexes is its position and the
        // opcode.
        //
        // **The depth-stencil chain is `Some(Some(..))` with distinct front and
        // back faces** — every field of `front` differs from `back` — so a
        // front/back swap goes red, and its three masks differ so a transposition
        // among them does too. The bias floats include one no short decimal names
        // (`slope_scale: 0.1`), so an encoding that went through a string lands on
        // a different number. `constant` is integer-valued (`-2.0`) so a browser
        // *can* build it: the fractional-constant refusal is driven separately.
        //
        // **Two colour targets with distinct formats, one `Some` blend and one
        // `None`**, so both the present and absent blend bodies are on the wire
        // and a target read in the other order answers a different format. MSAA
        // `samples: 4` — the one non-1 count WebGPU accepts. `polygon_mode` is
        // `Fill` and `depth_clamp` is `false` so the pipeline builds; the `Line`
        // and depth-clamp refusals are driven separately.
        Command::CreateGraphicsPipeline {
            pipeline: handle(131, 132),
            label: Some("gbuffer".into()),
            layout: handle(121, 122),
            vertex_module: handle(113, 114),
            vertex_entry_point: "vertexMain".into(),
            fragment: Some((handle(115, 116), "fragmentMain".into())),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                front_face: FrontFace::Cw,
                cull_mode: CullMode::Back,
                polygon_mode: PolygonMode::Fill,
                depth_clamp: false,
            },
            depth_stencil: Some(DepthStencilState {
                format: Format::D32FloatS8Uint,
                depth_write: false,
                depth_compare: CompareOp::GreaterOrEqual,
                stencil: Some(StencilState {
                    front: StencilFaceState {
                        compare: CompareOp::Less,
                        fail_op: StencilOp::Keep,
                        depth_fail_op: StencilOp::IncrementWrap,
                        pass_op: StencilOp::Replace,
                    },
                    back: StencilFaceState {
                        compare: CompareOp::Greater,
                        fail_op: StencilOp::Zero,
                        depth_fail_op: StencilOp::DecrementClamp,
                        pass_op: StencilOp::Invert,
                    },
                    read_mask: 0x0F,
                    write_mask: 0xF0,
                    reference: 0x2A,
                }),
                bias: DepthBias {
                    constant: -2.0,
                    slope_scale: 0.1,
                    clamp: 0.25,
                },
            }),
            multisample: MultisampleState {
                samples: 4,
                mask: 0x0000_00FF,
                alpha_to_coverage: true,
            },
            color_targets: vec![
                ColorTargetState {
                    format: Format::Rgba16Float,
                    blend: Some(BlendState {
                        color_src: BlendFactor::SrcAlpha,
                        color_dst: BlendFactor::OneMinusSrcAlpha,
                        color_op: BlendOp::Add,
                        alpha_src: BlendFactor::One,
                        alpha_dst: BlendFactor::OneMinusSrcAlpha,
                        alpha_op: BlendOp::Add,
                    }),
                    write_mask: ColorWrites::ALL,
                },
                ColorTargetState {
                    format: Format::Rg16Float,
                    blend: None,
                    write_mask: ColorWrites::R.union(ColorWrites::G),
                },
            ],
        },
        Command::DestroyBuffer {
            buffer: handle(17, 18),
        },
        Command::DestroySurface {
            surface: handle(47, 48),
        },
        // A view and the image it views are separate objects in separate
        // tables, so these are two commands rather than one that could be made
        // to stand for both.
        Command::DestroyImage {
            image: handle(79, 80),
        },
        Command::DestroyImageView {
            view: handle(81, 82),
        },
        // Its own command and its own table, for the reason the view's is: a
        // sampler's id and an image's are allowed to be the same eight bytes.
        Command::DestroySampler {
            sampler: handle(91, 92),
        },
        // Its own command and its own table again, and the destroy whose empty
        // slot is the *ordinary* case: a layout the replayer refused still has
        // its pre-allocated handle destroyed by the caller.
        Command::DestroyBindGroupLayout {
            layout: handle(105, 106),
        },
        // Its own command and its own table again: a bind group's id is allowed
        // to be the same eight bytes as anything else's.
        Command::DestroyBindGroup {
            group: handle(111, 112),
        },
        // Its own command and its own table again: a shader module's id is allowed
        // to be the same eight bytes as anything else's, and this is the destroy
        // `crcbl-render` leans on hardest — it pre-allocates the handle, destroys
        // it, and only then applies `?` to the creation.
        Command::DestroyShaderModule {
            module: handle(119, 120),
        },
        // Its own command and its own table again, and — like the bind-group
        // layout's destroy — the one whose empty slot is the *ordinary* case: a
        // pipeline layout the replayer refused (a `Some` push-constant range, an
        // unresolvable bind-group layout) still has its pre-allocated handle
        // destroyed by the caller.
        Command::DestroyPipelineLayout {
            layout: handle(125, 126),
        },
        // Its own command and its own table again, and — like the pipeline-layout
        // and bind-group-layout destroys — the one whose empty slot is the
        // *ordinary* case: a compute pipeline the replayer refused (an unresolvable
        // layout or module) still has its pre-allocated handle destroyed by the
        // caller.
        Command::DestroyComputePipeline {
            pipeline: handle(129, 130),
        },
        // Its own command and its own table again, and — like the compute-pipeline
        // destroy — the one whose empty slot is the *ordinary* case: a graphics
        // pipeline the replayer refused (a `Line` polygon mode, an unresolvable
        // layout or module, a forbidden `samples` count) still has its
        // pre-allocated handle destroyed by the caller.
        Command::DestroyGraphicsPipeline {
            pipeline: handle(133, 134),
        },
        Command::BeginDebugLabel {
            label: "gbuffer — ✱".into(),
        },
        Command::BeginRenderPass {
            label: Some("shading".into()),
            color_attachments: vec![
                ColorAttachment {
                    view: handle(21, 22),
                    resolve: Some(handle(23, 24)),
                    load: LoadOp::Clear,
                    store: StoreOp::Store,
                    clear: ClearValue {
                        color: [0.25, 0.5, 0.75, 1.0],
                        depth: depth::CLEAR,
                        stencil: 7,
                    },
                },
                ColorAttachment {
                    view: handle(25, 26),
                    resolve: None,
                    load: LoadOp::DontCare,
                    store: StoreOp::Discard,
                    clear: ClearValue::default(),
                },
            ],
            depth_stencil_attachment: Some(DepthStencilAttachment {
                view: handle(27, 28),
                read_only: true,
                depth_load: LoadOp::Load,
                depth_store: StoreOp::Discard,
                stencil_load: LoadOp::Clear,
                stencil_store: StoreOp::Store,
                clear: ClearValue {
                    color: [1.0, 2.0, 3.0, 4.0],
                    depth: depth::NEAR,
                    stencil: 9,
                },
            }),
            render_area: Rect2d {
                x: -3,
                y: -5,
                width: 1920,
                height: 1080,
            },
        },
        // The empty-and-absent twin of the pass above.
        Command::BeginRenderPass {
            label: None,
            color_attachments: Vec::new(),
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(2, 3),
        },
        Command::BindGraphicsPipeline {
            pipeline: handle(31, 32),
        },
        Command::BindGroup {
            slot: 2,
            group: handle(33, 34),
            dynamic_offsets: vec![256, 512, 768],
            layout: handle(35, 36),
        },
        Command::BindGroup {
            slot: 0,
            group: handle(37, 38),
            dynamic_offsets: Vec::new(),
            layout: handle(39, 40),
        },
        Command::PushConstants {
            stages: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
            offset: 16,
            data: vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01],
            layout: handle(41, 42),
        },
        Command::Draw {
            vertices: 6..9,
            instances: 1..5,
        },
        // The compute-pass commands. `BeginComputePass` carries only a label —
        // compute has no attachments — and its labelled form is paired with the
        // `None` twin below. The dispatch's three counts are all distinct so a
        // transposition among x/y/z is visible.
        Command::BeginComputePass {
            label: Some("cull".into()),
        },
        Command::BindComputePipeline {
            pipeline: handle(174, 175),
        },
        Command::Dispatch {
            x: 1000,
            y: 2000,
            z: 3000,
        },
        Command::EndComputePass,
        // The unlabelled twin of the pass above: `None` and `Some(_)` are
        // different values.
        Command::BeginComputePass { label: None },
        // The copy that carries a dispatch's storage-buffer output to a host
        // buffer. Distinct source and destination handles, and its two offsets and
        // the size are three different values so a transposition among the copy's
        // `u64` fields is visible.
        Command::CopyBufferToBuffer {
            copy: BufferCopy {
                src: handle(176, 177),
                src_offset: 1111,
                dst: handle(178, 179),
                dst_offset: 2222,
                size: 3333,
            },
        },
        // The buffer→image upload copy. Same `BufferImageCopy` shape as
        // `CopyImageToBuffer` above but the opposite direction, with its own
        // distinct handles and a non-zero texel pitch and image height so a
        // transposition among its many numbers is visible; the offset carries a
        // negative among three distinct signed components.
        Command::CopyBufferToImage {
            copy: BufferImageCopy {
                buffer: handle(180, 181),
                buffer_offset: 512,
                buffer_row_length: 120,
                buffer_image_height: 240,
                image: handle(182, 183),
                image_subresource: ImageSubresourceLayers {
                    aspect: ImageAspect::COLOR,
                    mip: 1,
                    base_layer: 2,
                    layer_count: 3,
                },
                image_offset: Offset3d { x: 5, y: -6, z: 7 },
                image_extent: Extent3d {
                    width: 32,
                    height: 24,
                    depth_or_layers: 1,
                },
            },
        },
        // The image→image copy, across *different* mip levels and offsets on the
        // two sides so a source/destination transposition cannot round-trip. Each
        // side has its own subresource and offset; the extent is shared.
        Command::CopyImageToImage {
            copy: ImageCopy {
                src: handle(184, 185),
                src_subresource: ImageSubresourceLayers {
                    aspect: ImageAspect::COLOR,
                    mip: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
                src_offset: Offset3d { x: 8, y: 9, z: 0 },
                dst: handle(186, 187),
                dst_subresource: ImageSubresourceLayers {
                    aspect: ImageAspect::COLOR,
                    mip: 3,
                    base_layer: 4,
                    layer_count: 1,
                },
                dst_offset: Offset3d { x: -1, y: 2, z: 3 },
                extent: Extent3d {
                    width: 16,
                    height: 12,
                    depth_or_layers: 1,
                },
            },
        },
        // The buffer fill, with a NON-ZERO value so the corpus carries the wire
        // the replayer refuses (WebGPU's `clearBuffer` is zero-only). Its offset
        // and size are distinct so a transposition among the `u64` fields shows.
        Command::FillBuffer {
            buffer: handle(188, 189),
            offset: 64,
            size: 256,
            value: 0xDEAD_BEEF,
        },
        // The pipeline barrier, the documented no-op — carried whole for wire
        // fidelity though the replayer records nothing. The empty case first: a
        // global-only barrier with no buffer or image transitions, which pins the
        // two zero counts and the `global` flag on their own.
        Command::PipelineBarrier {
            buffers: Vec::new(),
            images: Vec::new(),
            global: true,
        },
        // And the populated case: one buffer barrier carrying a `Some`
        // queue-transfer (the acquire/release WebGPU has no queue to honour), and
        // one image barrier over a NON-default subresource range with DISTINCT
        // `from`/`to` states, so a transposition among the states, handles or
        // range fields is visible. `global` is `false` here so both flag values
        // appear in the corpus.
        Command::PipelineBarrier {
            buffers: vec![BufferBarrier {
                buffer: handle(190, 191),
                from: ResourceState::ShaderWrite,
                to: ResourceState::TransferSrc,
                queue_transfer: Some(QueueTransfer {
                    from: handle(192, 193),
                    to: handle(194, 195),
                }),
            }],
            images: vec![ImageBarrier {
                image: handle(196, 197),
                range: ImageSubresourceRange {
                    aspect: ImageAspect::COLOR,
                    base_mip: 1,
                    mip_count: 2,
                    base_layer: 3,
                    layer_count: 4,
                },
                from: ResourceState::Undefined,
                to: ResourceState::ColorAttachment,
                queue_transfer: None,
            }],
            global: false,
        },
        // **Every bit of both feature words crosses**, including the ones no
        // browser can satisfy: the replayer is what refuses them, and it can
        // only refuse what it was told. `TIMELINE_SEMAPHORE` is required here
        // for exactly that reason — WebGPU has no semaphores — and a
        // `compatible_surface` is present so the optional handle appears both
        // ways round in this corpus.
        Command::RequestDevice {
            adapter: AdapterId(3),
            label: Some("device".into()),
            required_features: Features::COMPUTE.union(Features::TIMELINE_SEMAPHORE),
            optional_features: Features::TIMESTAMP_QUERY.union(Features::TEXTURE_COMPRESSION_BC),
            compatible_surface: Some(handle(43, 44)),
        },
        // Its opposite in every field that has one: no label rather than an
        // empty one, no surface, and the two feature words at their extremes —
        // `all()` is what pins the claimed-bit mask the JavaScript decoder
        // enforces, since a mask that drifted from `Features::all()` would
        // refuse this very command.
        Command::RequestDevice {
            adapter: AdapterId(0),
            label: None,
            required_features: Features::empty(),
            optional_features: Features::all(),
            compatible_surface: None,
        },
        // The readback path, in the order a frame records it. Every number is
        // distinct so a transposition among the copy's many `u32`/`u64` fields is
        // visible, and every optional field appears both ways.
        Command::CreateCommandEncoder {
            label: Some("readback encoder".into()),
            queue: handle(140, 141),
        },
        // The copy the readback reads: its buffer and image side, and the many
        // numbers `docs/plan/41-webgpu-stream.md` warns a transposition would
        // hide. `buffer_row_length` is texels and non-zero here (an explicit
        // pitch), `image_height` a different non-zero, and the offset's three
        // signed components are all distinct with a negative among them.
        Command::CopyImageToBuffer {
            buffer: handle(142, 143),
            buffer_offset: 256,
            buffer_row_length: 100,
            buffer_image_height: 200,
            image: handle(144, 145),
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 2,
                base_layer: 3,
                layer_count: 5,
            },
            image_offset: Offset3d { x: -7, y: 9, z: 11 },
            image_extent: Extent3d {
                width: 64,
                height: 48,
                depth_or_layers: 1,
            },
        },
        Command::EndRenderPass,
        Command::Finish {
            command_buffer: handle(146, 147),
        },
        // **Waits and signals non-empty**, which no browser honours but the
        // encoding must carry field for field: two command buffers, one wait and
        // one signal, each a distinct handle and a distinct `u64` value so the
        // list strides and the value halves are both pinned.
        Command::Submit {
            command_buffers: vec![handle(148, 149), handle(150, 151)],
            waits: vec![SemaphoreWait {
                semaphore: handle(152, 153),
                value: 0x0102_0304_0506_0708,
            }],
            signals: vec![SemaphoreSignal {
                semaphore: handle(154, 155),
                value: 9,
            }],
        },
        // Its empty-list twin — the only case WebGPU maps — and a single command
        // buffer, so both counted-list boundaries appear at zero as well.
        Command::Submit {
            command_buffers: vec![handle(156, 157)],
            waits: Vec::new(),
            signals: Vec::new(),
        },
        // `after: Some` — a semaphore wait the replayer refuses — with a full
        // `u64` value, distinct offset and size.
        Command::RequestReadback {
            readback: handle(158, 159),
            label: Some("stats readback".into()),
            buffer: handle(160, 161),
            offset: 32,
            size: 64,
            after: Some(SemaphoreWait {
                semaphore: handle(162, 163),
                value: 0x1122_3344_5566_7788,
            }),
        },
        // `after: None` — WebGPU's `mapAsync` — no label, and a `size` past a
        // `u32` so the field's width is pinned.
        Command::RequestReadback {
            readback: handle(164, 165),
            label: None,
            buffer: handle(166, 167),
            offset: 0,
            size: 0x0000_0001_0000_0000,
            after: None,
        },
        Command::PollReadback {
            readback: handle(168, 169),
        },
        Command::DestroyReadback {
            readback: handle(170, 171),
        },
        Command::DestroyCommandBuffer {
            command_buffer: handle(172, 173),
        },
        // The presentation family. A NON-DEFAULT present mode and composite alpha
        // and a NON-SQUARE extent, so a writer that dropped a field or swapped the
        // two enum bytes decodes to a different value rather than the same one; the
        // extent's two components differ from each other and from the image count.
        Command::CreateSwapchain {
            swapchain: handle(174, 175),
            label: Some("swapchain".into()),
            surface: handle(176, 177),
            format: Format::Bgra8UnormSrgb,
            extent: (800, 600),
            image_count: 3,
            present_mode: PresentMode::Mailbox,
            composite_alpha: CompositeAlpha::PreMultiplied,
        },
        Command::AcquireNextFrame {
            swapchain: handle(178, 179),
            image: handle(180, 181),
            view: handle(182, 183),
        },
        // A NON-EMPTY waits list and a `Some(present_id)`, so the refusal-carrying
        // wire is exercised: the wait handle is distinct and the id is a full `u64`.
        Command::Present {
            swapchain: handle(184, 185),
            waits: vec![handle(186, 187)],
            present_id: Some(0x0a0b_0c0d_0e0f_1011),
        },
        // Its empty-list twin — the only case WebGPU maps — and `present_id: None`,
        // so both the counted-list boundary at zero and the optional both ways appear.
        Command::Present {
            swapchain: handle(188, 189),
            waits: Vec::new(),
            present_id: None,
        },
        Command::DestroySwapchain {
            swapchain: handle(190, 191),
        },
        // Reconfigure in place — the same descriptor as `CreateSwapchain` above,
        // with a DIFFERENT format (`Rgba8Unorm`, not the create's `Bgra8UnormSrgb`)
        // and a different extent, present mode and composite alpha, so an arm that
        // decoded a create's fields into a reconfigure — or dropped or swapped one
        // — decodes to a different value rather than the same one.
        Command::ReconfigureSwapchain {
            swapchain: handle(192, 193),
            label: Some("reconfigured swapchain".into()),
            surface: handle(194, 195),
            format: Format::Rgba8Unorm,
            extent: (1024, 768),
            image_count: 2,
            present_mode: PresentMode::Immediate,
            composite_alpha: CompositeAlpha::Opaque,
        },
        // **Body-less, and deliberately not last.** Its whole encoding is one
        // byte, so a decoder that read a field that is no longer there would
        // consume the `EnumerateAdapters` below it and end the stream one
        // command short — which is what the pair says and neither says alone.
        Command::SurfaceCaps,
        // Last, and not for tidiness: `web/tools/stream-decode.mjs` reaches into
        // this fixture by byte offset to corrupt one field at a time, and every
        // one of those offsets is counted from the *first* command. A command
        // inserted above would move all of them.
        //
        // Body-less too, and here it is the *end* of the stream that follows the
        // tag: a decoder that read one field too many runs off the buffer rather
        // than into a neighbour, which is the other half of the shape.
        Command::EnumerateAdapters,
    ]
}

/// Encodes `command` through the writer method it came from.
///
/// The `match` is exhaustive, so a variant added to [`Command`] stops this file
/// compiling — which is the point at which the suites that use it are impossible
/// to leave un-extended.
pub fn encode(stream: &mut StreamWriter, command: &Command) -> u64 {
    match command {
        Command::CreateBuffer {
            buffer,
            label,
            size,
            usage,
            memory,
        } => stream.create_buffer(
            *buffer,
            &BufferDesc {
                label: label.as_deref(),
                size: *size,
                usage: *usage,
                memory: *memory,
            },
        ),
        Command::CreateSurface { surface, canvas_id } => {
            stream.create_surface(*surface, *canvas_id)
        }
        Command::CreateImage {
            image,
            label,
            image_type,
            extent,
            format,
            mip_levels,
            samples,
            usage,
        } => stream.create_image(
            *image,
            &ImageDesc {
                label: label.as_deref(),
                image_type: *image_type,
                extent: *extent,
                format: *format,
                mip_levels: *mip_levels,
                samples: *samples,
                usage: *usage,
            },
        ),
        Command::CreateImageView {
            view,
            label,
            image,
            view_type,
            format,
            range,
        } => stream.create_image_view(
            *view,
            &ImageViewDesc {
                label: label.as_deref(),
                image: *image,
                view_type: *view_type,
                format: *format,
                range: *range,
            },
        ),
        Command::CreateSampler {
            sampler,
            label,
            mag_filter,
            min_filter,
            mip_filter,
            address_mode,
            lod_min,
            lod_max,
            anisotropy,
            compare,
        } => stream.create_sampler(
            *sampler,
            &SamplerDesc {
                label: label.as_deref(),
                mag_filter: *mag_filter,
                min_filter: *min_filter,
                mip_filter: *mip_filter,
                address_mode: *address_mode,
                lod_min: *lod_min,
                lod_max: *lod_max,
                anisotropy: *anisotropy,
                compare: *compare,
            },
        ),
        Command::CreateBindGroupLayout {
            layout,
            label,
            entries,
        } => stream.create_bind_group_layout(
            *layout,
            &BindGroupLayoutDesc {
                label: label.as_deref(),
                entries,
            },
        ),
        Command::CreateBindGroup {
            group,
            label,
            layout,
            entries,
            variable_count,
        } => stream.create_bind_group(
            *group,
            &BindGroupDesc {
                label: label.as_deref(),
                layout: *layout,
                entries,
                variable_count: *variable_count,
            },
        ),
        Command::CreateShaderModule {
            module,
            label,
            spirv,
            wgsl,
            msl,
            dxil,
        } => {
            // The owned `(String, Vec<u8>)` pairs are borrowed back into the
            // `(&str, &[u8])` shape the descriptor takes, so the round trip goes
            // through the same encoder a real caller would.
            let dxil: Vec<(&str, &[u8])> = dxil
                .iter()
                .map(|(name, container)| (name.as_str(), container.as_slice()))
                .collect();
            stream.create_shader_module(
                *module,
                &ShaderModuleDesc {
                    label: label.as_deref(),
                    spirv,
                    wgsl: wgsl.as_deref(),
                    msl: msl.as_deref(),
                    dxil: &dxil,
                },
            )
        }
        Command::CreatePipelineLayout {
            layout,
            label,
            bind_group_layouts,
            push_constants,
        } => stream.create_pipeline_layout(
            *layout,
            &PipelineLayoutDesc {
                label: label.as_deref(),
                bind_group_layouts,
                push_constants: *push_constants,
            },
        ),
        Command::CreateComputePipeline {
            pipeline,
            label,
            layout,
            module,
            entry_point,
            workgroup_size,
        } => stream.create_compute_pipeline(
            *pipeline,
            &ComputePipelineDesc {
                label: label.as_deref(),
                layout: *layout,
                compute: ShaderEntry {
                    module: *module,
                    entry_point,
                },
                workgroup_size: *workgroup_size,
            },
        ),
        Command::CreateGraphicsPipeline {
            pipeline,
            label,
            layout,
            vertex_module,
            vertex_entry_point,
            fragment,
            primitive,
            depth_stencil,
            multisample,
            color_targets,
        } => stream.create_graphics_pipeline(
            *pipeline,
            &GraphicsPipelineDesc {
                label: label.as_deref(),
                layout: *layout,
                vertex: ShaderEntry {
                    module: *vertex_module,
                    entry_point: vertex_entry_point,
                },
                fragment: fragment.as_ref().map(|(module, entry_point)| ShaderEntry {
                    module: *module,
                    entry_point,
                }),
                primitive: *primitive,
                depth_stencil: *depth_stencil,
                multisample: *multisample,
                color_targets,
            },
        ),
        Command::DestroyComputePipeline { pipeline } => stream.destroy_compute_pipeline(*pipeline),
        Command::DestroyGraphicsPipeline { pipeline } => {
            stream.destroy_graphics_pipeline(*pipeline)
        }
        Command::DestroyPipelineLayout { layout } => stream.destroy_pipeline_layout(*layout),
        Command::DestroyBindGroupLayout { layout } => stream.destroy_bind_group_layout(*layout),
        Command::DestroyBindGroup { group } => stream.destroy_bind_group(*group),
        Command::DestroyShaderModule { module } => stream.destroy_shader_module(*module),
        Command::DestroyBuffer { buffer } => stream.destroy_buffer(*buffer),
        Command::DestroySurface { surface } => stream.destroy_surface(*surface),
        Command::DestroyImage { image } => stream.destroy_image(*image),
        Command::DestroyImageView { view } => stream.destroy_image_view(*view),
        Command::DestroySampler { sampler } => stream.destroy_sampler(*sampler),
        Command::BeginDebugLabel { label } => stream.begin_debug_label(label),
        Command::BeginRenderPass {
            label,
            color_attachments,
            depth_stencil_attachment,
            render_area,
        } => stream.begin_render_pass(&RenderPassDesc {
            label: label.as_deref(),
            color_attachments,
            depth_stencil_attachment: *depth_stencil_attachment,
            render_area: *render_area,
        }),
        Command::BindGraphicsPipeline { pipeline } => stream.bind_graphics_pipeline(*pipeline),
        Command::BindGroup {
            slot,
            group,
            dynamic_offsets,
            layout,
        } => stream.bind_group(*slot, *group, dynamic_offsets, *layout),
        Command::PushConstants {
            stages,
            offset,
            data,
            layout,
        } => stream.push_constants(*stages, *offset, data, *layout),
        Command::Draw {
            vertices,
            instances,
        } => stream.draw(vertices.clone(), instances.clone()),
        Command::BeginComputePass { label } => stream.begin_compute_pass(&ComputePassDesc {
            label: label.as_deref(),
        }),
        Command::BindComputePipeline { pipeline } => stream.bind_compute_pipeline(*pipeline),
        Command::Dispatch { x, y, z } => stream.dispatch(*x, *y, *z),
        Command::EndComputePass => stream.end_compute_pass(),
        Command::CopyBufferToBuffer { copy } => stream.copy_buffer_to_buffer(copy),
        Command::CopyBufferToImage { copy } => stream.copy_buffer_to_image(copy),
        Command::CopyImageToImage { copy } => stream.copy_image_to_image(copy),
        Command::FillBuffer {
            buffer,
            offset,
            size,
            value,
        } => stream.fill_buffer(*buffer, *offset, *size, *value),
        Command::PipelineBarrier {
            buffers,
            images,
            global,
        } => stream.pipeline_barrier(&Barriers {
            buffers,
            images,
            global: *global,
        }),
        Command::EnumerateAdapters => stream.enumerate_adapters(),
        Command::SurfaceCaps => stream.surface_caps(),
        Command::CreateSwapchain {
            swapchain,
            label,
            surface,
            format,
            extent,
            image_count,
            present_mode,
            composite_alpha,
        } => stream.create_swapchain(
            *swapchain,
            &SwapchainDesc {
                label: label.as_deref(),
                surface: *surface,
                format: *format,
                extent: *extent,
                image_count: *image_count,
                present_mode: *present_mode,
                composite_alpha: *composite_alpha,
            },
        ),
        Command::AcquireNextFrame {
            swapchain,
            image,
            view,
        } => stream.acquire_next_frame(*swapchain, *image, *view),
        Command::Present {
            swapchain,
            waits,
            present_id,
        } => stream.present(&PresentInfo {
            swapchain: *swapchain,
            waits,
            present_id: *present_id,
        }),
        Command::DestroySwapchain { swapchain } => stream.destroy_swapchain(*swapchain),
        Command::ReconfigureSwapchain {
            swapchain,
            label,
            surface,
            format,
            extent,
            image_count,
            present_mode,
            composite_alpha,
        } => stream.reconfigure_swapchain(
            *swapchain,
            &SwapchainDesc {
                label: label.as_deref(),
                surface: *surface,
                format: *format,
                extent: *extent,
                image_count: *image_count,
                present_mode: *present_mode,
                composite_alpha: *composite_alpha,
            },
        ),
        Command::CreateCommandEncoder { label, queue } => {
            stream.create_command_encoder(&CommandEncoderDesc {
                label: label.as_deref(),
                queue: *queue,
            })
        }
        Command::EndRenderPass => stream.end_render_pass(),
        Command::CopyImageToBuffer {
            buffer,
            buffer_offset,
            buffer_row_length,
            buffer_image_height,
            image,
            image_subresource,
            image_offset,
            image_extent,
        } => stream.copy_image_to_buffer(&BufferImageCopy {
            buffer: *buffer,
            buffer_offset: *buffer_offset,
            buffer_row_length: *buffer_row_length,
            buffer_image_height: *buffer_image_height,
            image: *image,
            image_subresource: *image_subresource,
            image_offset: *image_offset,
            image_extent: *image_extent,
        }),
        Command::Finish { command_buffer } => stream.finish(*command_buffer),
        Command::Submit {
            command_buffers,
            waits,
            signals,
        } => stream.submit(&SubmitInfo {
            command_buffers,
            waits,
            signals,
        }),
        Command::RequestReadback {
            readback,
            label,
            buffer,
            offset,
            size,
            after,
        } => stream.request_readback(
            *readback,
            &ReadbackDesc {
                label: label.as_deref(),
                buffer: *buffer,
                offset: *offset,
                size: *size,
                after: *after,
            },
        ),
        Command::PollReadback { readback } => stream.poll_readback(*readback),
        Command::DestroyReadback { readback } => stream.destroy_readback(*readback),
        Command::DestroyCommandBuffer { command_buffer } => {
            stream.destroy_command_buffer(*command_buffer)
        }
        Command::RequestDevice {
            adapter,
            label,
            required_features,
            optional_features,
            compatible_surface,
        } => stream.request_device(&DeviceDesc {
            label: label.as_deref(),
            adapter: *adapter,
            required_features: *required_features,
            optional_features: *optional_features,
            compatible_surface: *compatible_surface,
        }),
    }
}

/// A stream holding every command in [`every_command`], in order.
pub fn encode_all() -> (StreamWriter, Vec<Command>) {
    let commands = every_command();
    let mut stream = StreamWriter::new();
    for command in &commands {
        encode(&mut stream, command);
    }
    (stream, commands)
}
