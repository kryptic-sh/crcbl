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
    DeviceDesc, Features, LoadOp, MemoryLocation, Rect2d, RenderPassDesc, ShaderStages, StoreOp,
    depth,
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
        Command::DestroyBuffer {
            buffer: handle(17, 18),
        },
        Command::DestroySurface {
            surface: handle(47, 48),
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
        // **The surface and the adapter id have nothing in common.** They are
        // different widths on the wire, so reading them the other way round
        // runs off into the next command rather than swapping two values — but
        // a *second* decoder is free to choose the opposite order, and these
        // numbers are what make that a visible difference rather than a
        // coincidence: `65` is neither half of `handle(63, 64)`.
        Command::SurfaceCaps {
            surface: handle(63, 64),
            adapter: AdapterId(65),
        },
        // Last, and not for tidiness: `web/tools/stream-decode.mjs` reaches into
        // this fixture by byte offset to corrupt one field at a time, and every
        // one of those offsets is counted from the *first* command. A command
        // inserted above would move all of them.
        //
        // The only body-less command in the corpus, which is a shape of its own:
        // the byte after this tag is the next command's tag, so a decoder that
        // read one field too many here would decode the rest of the stream as
        // garbage — and there is nothing after it, so it is also the case where
        // that shows up as a clean end rather than as an error.
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
        Command::DestroyBuffer { buffer } => stream.destroy_buffer(*buffer),
        Command::DestroySurface { surface } => stream.destroy_surface(*surface),
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
        Command::SurfaceCaps { surface, adapter } => stream.surface_caps(*surface, *adapter),
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
