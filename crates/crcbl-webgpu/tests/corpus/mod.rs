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
    AdapterId, BufferDesc, BufferUsage, ClearValue, ColorAttachment, DepthStencilAttachment,
    DeviceDesc, Extent3d, Features, Format, ImageAspect, ImageDesc, ImageSubresourceRange,
    ImageType, ImageUsage, ImageViewDesc, ImageViewType, LoadOp, MemoryLocation, Rect2d,
    RenderPassDesc, ShaderStages, StoreOp, depth,
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
        Command::DestroyBuffer { buffer } => stream.destroy_buffer(*buffer),
        Command::DestroySurface { surface } => stream.destroy_surface(*surface),
        Command::DestroyImage { image } => stream.destroy_image(*image),
        Command::DestroyImageView { view } => stream.destroy_image_view(*view),
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
        Command::EnumerateAdapters => stream.enumerate_adapters(),
        Command::SurfaceCaps => stream.surface_caps(),
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
