//! The occlusion pass's own gradient, read back at native resolution.
//!
//! # Why a picture cannot show it
//!
//! `tests/render_e2e.rs` renders at 256×192, which is the whole reason this
//! suite's siblings exist: a golden at that size cannot resolve a gradient's
//! *shape*. The banding `docs/backlog.md` records was reported off a real
//! display at 1080p, where an occluder drew concentric terraces instead of a
//! falloff, and every golden in the tree was green while it did.
//!
//! # The scene is arithmetic, not geometry
//!
//! Nothing is rasterised here. The depth prepass's image is filled from the
//! host with two constants — a wall at [`WALL_Z`] and a plate [`PLATE_LIFT`] in
//! front of it over [`PLATE_COLUMNS`] — so the input to `ssao.slang` is exact
//! and the only thing that can move the output is the shader. A rendered box
//! would put its own silhouette aliasing into the measurement.
//!
//! The two passes are built here rather than driven through `ForwardRenderer`
//! because `crcbl_render::ssao::Ssao` is private to that crate, and because a
//! frame would bring the whole lighting stack into a question that is about two
//! full-screen draws. The pipelines and both bind-group layouts are
//! `crates/crcbl-render/src/ssao.rs`'s, copied field for field.
//!
//! # What "terraced" is, as a number
//!
//! Along a row crossing the plate's edge the blurred occlusion rises from its
//! darkest value back to unoccluded over about [`SLICE_STEPS_HINT`] steps of the
//! march. With every pixel starting its march at the same fraction of a step,
//! a horizon can only sit at that many distances, so the row is a staircase:
//! long runs of one 8-bit level separated by large jumps. The measure is
//! therefore the **longest run of a single level** inside the gradient, and
//! `ssao.slang`'s `STEP_OFFSETS` is what breaks it.

use crate::harness::{Headless, poisoned};
use crcbl::hal::{
    Barriers, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferImageCopy,
    BufferUsage, Capability, ClearValue, ColorAttachment, ColorTargetState, CommandEncoderDesc,
    Device, Extent3d, Features, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, ImageAspect,
    ImageBarrier, ImageDesc, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange, ImageType,
    ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType, LoadOp, MemoryLocation,
    MultisampleState, Offset3d, PassTimestampWrites, PipelineLayoutDesc, PipelineLayoutHandle,
    PrimitiveState, QueryKind, QuerySetDesc, Rect2d, RenderPassDesc, ResourceState, SampleType,
    ShaderEntry, ShaderModuleDesc, ShaderStages, StoreOp, SubmitInfo, Viewport,
};
use crcbl::math::{Mat4, Vec4};
use crcbl::render::Projection;
use crcbl::shaders::{SSAO, SSAO_BLUR, Shader, Stage, ssao::SsaoParams};

/// The frame this measures at.
///
/// 1080p rather than the golden suite's 256×192, which is the point: the
/// terraces are tens of pixels wide and a frame that small has no room for one.
/// The swapchain the fixture opens is this size too and nothing is drawn into
/// it — every image here is created below.
const EXTENT: (u32, u32) = (1920, 1080);

/// The vertical field of view the scene is projected through.
///
/// A third of a turn. It and [`WALL_Z`] together set the projected radius,
/// which is the one number that decides whether this scene can terrace at all —
/// see [`WALL_Z`].
const FOV_Y: f32 = core::f32::consts::FRAC_PI_3;

/// The near plane, and under reversed-Z the only number that sets depth
/// precision. `crcbl::render::Projection`'s own default.
const NEAR: f32 = 0.1;

/// View-space distance to the wall, in world units.
///
/// **Close, and that is what makes the measurement possible.** The wall's
/// distance sets [`reach_in_pixels`], and a step of the march is a quarter of
/// it; the plate's silhouette is a straight edge, so a march along
/// `SLICE_DIRECTIONS[i]` crosses it at that direction's own projection onto the
/// row — which already spreads the four step distances over four positions per
/// direction. At a wall ten units away the whole falloff is about forty columns
/// and those positions land within a column or two of each other, so the
/// coherent march this test is about produces no terrace to find: swept on
/// 2026-08-30, the longest run there was four columns *with the offsets forced
/// off*. Bringing the wall in to here makes a step tens of columns wide and the
/// direction spread no longer closes the gaps. See [`MAX_RUN`] for what the two
/// marches then measure.
const WALL_Z: f32 = 2.5;

/// How far in front of the wall the occluding plate stands, in world units.
///
/// Well inside [`RADIUS`], so a tap that lands on the plate is inside the
/// neighbourhood the integral counts rather than rejected by the falloff.
const PLATE_LIFT: f32 = 0.15;

/// The screen columns the plate covers, as `start..end`.
///
/// Far wider than the projected radius so the plate's own two edges never
/// interact, and clear of the frame's left border for the same reason.
const PLATE_COLUMNS: core::ops::Range<u32> = 640..960;

/// The row read back — the middle of the frame, far from every border.
const LINE_Y: u32 = 540;

/// The sampling radius, in world units.
///
/// `crcbl_render`'s `SSAO_RADIUS`, which is private to that crate. The value is
/// mirrored rather than reached for because this test is about the *shape* of
/// the falloff and would measure the same shape at any radius; what it must not
/// do is measure a radius nobody ships.
const RADIUS: f32 = 0.5;

/// `ssao.slang`'s `SLICE_STEPS`, for the failure message's arithmetic.
///
/// Not read by the shader — the shader owns its own copy — and not asserted
/// against it either: this is only how many terraces a reader should expect the
/// baseline to have, and the assertion below is on the runs themselves.
const SLICE_STEPS_HINT: u32 = 4;

/// The `R8Unorm` texel a fully unoccluded pixel carries.
const UNOCCLUDED: u8 = 0xFF;

/// Columns of the gradient skipped at the plate's silhouette.
///
/// `ssao_blur.slang` weights its kernel by view-space depth, so within its own
/// footprint of the plate's edge it rejects taps and its divisor falls towards
/// one. Those columns are a different measurement — the tile banding the
/// backlog's second hypothesis is about — and they are not what this test is
/// for.
const SILHOUETTE_SKIP: u32 = 4;

/// Columns of gradient the run is measured over, starting past
/// [`SILHOUETTE_SKIP`].
///
/// **The steep part of the falloff, deliberately not all of it.** At [`WALL_Z`]
/// the occlusion runs out over about 135 columns and its last third is nearly
/// flat, where an 8-bit channel holds one level for several columns however
/// well the march is dithered — a run this test would then be measuring the
/// quantiser with. The test also asserts the window's last column is still
/// occluded, so a change that shortened the falloff fails here rather than
/// quietly measuring flat unoccluded wall.
const WINDOW: u32 = 80;

/// The smallest spread of levels the window must hold.
///
/// Anti-vacuity. A window with no gradient in it has no runs to break either,
/// so every assertion below would pass on a pass that wrote one constant — the
/// exact shape of a check wired to nothing. The 2026-08-30 sweep measured a
/// swing of 71 levels with `STEP_OFFSETS` and 55 without, on radv and on
/// lavapipe alike; this sits well below the smaller of them.
const MIN_SWING: u8 = 40;

/// The longest run of one 8-bit level the window may hold.
///
/// **Swept before it was fixed**, at [`EXTENT`] on 2026-08-30, on the two
/// drivers this machine has:
///
/// ```text
///                                        radv   lavapipe
/// every STEP_OFFSETS entry forced to 1     16         13
/// STEP_OFFSETS as `ssao.slang` ships it     5          4
/// ```
///
/// So this sits between them with room on both sides — the fixed march has to
/// nearly double its worst run to go red, and the coherent one has to nearly
/// halve its best. The remaining runs at the shallow end of the falloff are
/// 8-bit quantisation of a ramp, not terracing, which is why the window stops
/// short of where the falloff flattens: see [`WINDOW`].
const MAX_RUN: usize = 9;

/// The projected radius at the wall, in pixels — `occlusion_at`'s `reach`.
///
/// The same arithmetic the shader does, on the host: project the two ends of a
/// world-space radius and measure the gap in pixels.
fn reach_in_pixels(projection: Mat4, size: (f32, f32)) -> f32 {
    let centre = Vec4::new(0.0, 0.0, -WALL_Z, 1.0);
    let offset = Vec4::new(RADIUS, 0.0, -WALL_Z, 1.0);
    let near = projection * centre;
    let far = projection * offset;
    (far.x / far.w - near.x / near.w).abs() * 0.5 * size.0
}

/// The reversed-Z depth of a surface at `view_z`, through `projection`.
///
/// Derived from the matrix rather than from a closed form, so the depth the
/// image holds and the `inv_proj` the shader unprojects with cannot disagree.
fn depth_of(projection: Mat4, view_z: f32) -> f32 {
    let clip = projection * Vec4::new(0.0, 0.0, view_z, 1.0);
    clip.z / clip.w
}

/// The prepass image's contents: a wall, and a plate standing in front of it.
fn analytic_depth(projection: Mat4) -> Vec<u8> {
    let wall = depth_of(projection, -WALL_Z);
    let plate = depth_of(projection, -(WALL_Z - PLATE_LIFT));
    assert!(
        plate > wall,
        "reversed-Z puts the nearer surface at the larger depth; the plate reads {plate} and the \
         wall {wall}, so the scene has the plate behind the wall and occludes nothing"
    );
    let (width, height) = EXTENT;
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for x in 0..width {
            let depth = if PLATE_COLUMNS.contains(&x) {
                plate
            } else {
                wall
            };
            bytes.extend_from_slice(&depth.to_le_bytes());
        }
    }
    bytes
}

/// The longest run of one value in `values`, as `(length, value)`.
fn longest_run(values: &[u8]) -> (usize, u8) {
    let mut best = (0, 0);
    let mut run = (0usize, 0u8);
    for &value in values {
        run = if value == run.1 {
            (run.0 + 1, value)
        } else {
            (1, value)
        };
        if run.0 > best.0 {
            best = run;
        }
    }
    best
}

/// Every plateau in `values`, as `value×length`, for the failure message.
fn plateaus(values: &[u8]) -> String {
    let mut out = Vec::new();
    for &value in values {
        match out.last_mut() {
            Some((last, count)) if *last == value => *count += 1,
            _ => out.push((value, 1usize)),
        }
    }
    out.iter()
        .map(|(value, count)| format!("{value}×{count}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The two bind-group layouts and the two pipelines, built exactly as
/// `crates/crcbl-render/src/ssao.rs` builds them.
struct Passes {
    layout: crcbl::hal::BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    blur_layout: crcbl::hal::BindGroupLayoutHandle,
    blur_pipeline_layout: PipelineLayoutHandle,
    blur_pipeline: GraphicsPipelineHandle,
}

/// A full-screen-triangle pipeline over `shader`, which is
/// `ForwardRenderer::build_fullscreen`'s shape: no depth state, no
/// multisampling, one colour target, and the module released before the result
/// is unwrapped.
fn fullscreen_pipeline(
    device: &dyn Device,
    label: &str,
    shader: &Shader,
    layout: PipelineLayoutHandle,
) -> GraphicsPipelineHandle {
    let vertex = shader
        .entry_point(Stage::Vertex)
        .expect("a vertex entry point");
    let fragment = shader
        .entry_point(Stage::Fragment)
        .expect("a fragment entry point");
    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some(label),
            spirv: shader.spirv(),
            wgsl: shader.wgsl(),
            msl: shader.msl(),
            dxil: &shader.dxil_containers(),
        })
        .expect("a shader module");
    let targets = [ColorTargetState::opaque(Format::R8Unorm)];
    let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
        label: Some(label),
        layout,
        vertex: ShaderEntry {
            module,
            entry_point: vertex,
        },
        fragment: Some(ShaderEntry {
            module,
            entry_point: fragment,
        }),
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        color_targets: &targets,
    });
    device.destroy_shader_module(module);
    pipeline.expect("a full-screen pipeline")
}

impl Passes {
    /// Both layouts and both pipelines. The entries are
    /// `crates/crcbl-render/src/ssao.rs`'s, including the `Depth` sample type
    /// that `DepthTexture2D` needs on WebGPU.
    fn new(device: &dyn Device) -> Self {
        let uniform = BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::UniformBuffer { dynamic: false },
            count: 1,
            flags: BindingFlags::empty(),
        };
        let sampled = |binding: u32, sample_type: SampleType| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type,
            },
            count: 1,
            flags: BindingFlags::empty(),
        };

        let entries = [uniform, sampled(1, SampleType::Depth)];
        let layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("ssao depth"),
                entries: &entries,
            })
            .expect("the occlusion layout");
        let set_layouts = [layout];
        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("ssao"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("the occlusion pipeline layout");

        let blur_entries = [
            uniform,
            sampled(1, SampleType::Float),
            sampled(2, SampleType::Depth),
        ];
        let blur_layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("ssao blur"),
                entries: &blur_entries,
            })
            .expect("the blur layout");
        let blur_set_layouts = [blur_layout];
        let blur_pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("ssao blur"),
                bind_group_layouts: &blur_set_layouts,
                push_constants: None,
            })
            .expect("the blur pipeline layout");

        Self {
            pipeline: fullscreen_pipeline(device, "ssao", &SSAO, pipeline_layout),
            blur_pipeline: fullscreen_pipeline(
                device,
                "ssao blur",
                &SSAO_BLUR,
                blur_pipeline_layout,
            ),
            layout,
            pipeline_layout,
            blur_layout,
            blur_pipeline_layout,
        }
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_graphics_pipeline(self.blur_pipeline);
        device.destroy_pipeline_layout(self.blur_pipeline_layout);
        device.destroy_bind_group_layout(self.blur_layout);
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
    }
}

/// One `R8Unorm` target of [`EXTENT`], sampled by the pass after it.
fn occlusion_image(device: &dyn Device, label: &str) -> (ImageHandle, ImageViewHandle) {
    let image = device
        .create_image(&ImageDesc {
            label: Some(label),
            image_type: ImageType::D2,
            extent: Extent3d::d2(EXTENT.0, EXTENT.1),
            format: Format::R8Unorm,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED | ImageUsage::TRANSFER_SRC,
        })
        .expect("an occlusion target");
    let view = device
        .create_image_view(&ImageViewDesc {
            label: Some(label),
            image,
            view_type: ImageViewType::D2,
            format: Format::R8Unorm,
            range: ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip: 0,
                mip_count: 1,
                base_layer: 0,
                layer_count: 1,
            },
        })
        .expect("an occlusion view");
    (image, view)
}

/// The whole colour range of a single-level, single-layer image.
fn colour_range() -> ImageSubresourceRange {
    ImageSubresourceRange {
        aspect: ImageAspect::COLOR,
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    }
}

/// **The blurred occlusion falls off smoothly, and a march that started every
/// pixel at the same fraction of a step made it a staircase.**
///
/// A wall with a plate in front of it, both written into the prepass image as
/// exact depths, through the shipping `ssao` and `ssao-blur` pipelines at 1080p.
/// The row through the plate's edge is read back and the longest run of one
/// 8-bit level inside the falloff is measured: the baseline shape is runs of a
/// step's width, which `ssao.slang`'s `STEP_OFFSETS` breaks into runs a blur
/// footprint wide.
///
/// The failure message carries the whole plateau list, because the number that
/// went red says the window terraced and only the list says *how*.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_blurred_occlusion_falloff_does_not_terrace() {
    let headless = Headless::open_at_format(EXTENT, None, Features::TIMESTAMP_QUERY);
    let device = headless.device.as_ref();
    let (width, height) = EXTENT;
    let size = (width as f32, height as f32);
    let projection = Projection::Perspective {
        fov_y: FOV_Y,
        near: NEAR,
    }
    .matrix(size.0 / size.1);

    // The falloff cannot be longer than the radius it is measured within, so a
    // scene whose radius no longer covers the window is one where the window
    // runs off the end into flat unoccluded wall. The assertion on the window's
    // own last column catches that too; this catches it with the scene named.
    let reach = reach_in_pixels(projection, size);
    let needed = f32::from(u16::try_from(SILHOUETTE_SKIP + WINDOW).expect("a window under a row"));
    assert!(
        reach >= needed,
        "the scene projects a {RADIUS} radius to {reach:.1} pixels at a wall {WALL_Z} away, and \
         the window past the silhouette is {needed} columns wide — so it cannot fit inside the \
         falloff. Move the wall closer, widen the field of view, or shorten the window."
    );

    // **The prepass image is filled by a copy, and not every backend can.**
    // WebGPU defines a buffer-to-image copy for `D16Unorm`'s depth plane alone
    // — `crcbl::hal::Capability::DepthImageCopy` carries the table — so a
    // backend that cannot do it would otherwise fail somewhere inside the
    // submission with nothing naming the reason. No such backend runs this
    // suite today; this is what says so if one arrives.
    assert!(
        device.supports(Capability::DepthImageCopy).is_yes(),
        "this backend cannot copy a buffer into a depth image, so the analytic prepass this test \
         measures from cannot be built on it. See `crcbl::hal::Capability::DepthImageCopy` for \
         which formats each API moves and in which direction."
    );

    let passes = Passes::new(device);
    let (raw, raw_view) = occlusion_image(device, "ssao");
    let (blurred, blurred_view) = occlusion_image(device, "ssao-blurred");

    // The prepass image, filled from the host. `SAMPLED` because both passes
    // read it, `TRANSFER_DST` because this is where it comes from.
    let depth = device
        .create_image(&ImageDesc {
            label: Some("scene-depth"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(width, height),
            format: Format::D32Float,
            mip_levels: 1,
            samples: 1,
            // `crcbl_render::TransientImageDesc::scene_depth`'s pair, plus the
            // transfer this fills it through. The attachment usage is carried
            // even though nothing renders into it here: it is what the shipping
            // image is created with, and an image created with a different
            // usage is an image a driver may lay out differently.
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT
                | ImageUsage::SAMPLED
                | ImageUsage::TRANSFER_DST,
        })
        .expect("a prepass image");
    let depth_range = ImageSubresourceRange {
        aspect: ImageAspect::DEPTH,
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    };
    let depth_view = device
        .create_image_view(&ImageViewDesc {
            label: Some("scene-depth"),
            image: depth,
            view_type: ImageViewType::D2,
            format: Format::D32Float,
            range: depth_range,
        })
        .expect("a prepass view");

    let texels = analytic_depth(projection);
    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("scene-depth upload"),
            size: texels.len() as u64,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("an upload buffer");
    device.write_buffer(upload, 0, &texels).expect("the upload");

    let params = SsaoParams {
        inv_proj: projection.inverse().to_cols_array(),
        proj: projection.to_cols_array(),
        radius: RADIUS,
    };
    let uniforms = device
        .create_buffer(&BufferDesc {
            label: Some("ssao params"),
            size: crcbl::shaders::ssao::PARAMS_SIZE as u64,
            usage: BufferUsage::UNIFORM,
            memory: MemoryLocation::HostUpload,
        })
        .expect("the uniform block");
    device
        .write_buffer(uniforms, 0, &params.to_bytes())
        .expect("the block is written");

    let group = |label: &str, layout, entries: Vec<BindGroupEntry>| -> BindGroupHandle {
        device
            .create_bind_group(&BindGroupDesc {
                label: Some(label),
                layout,
                entries: &entries,
                variable_count: None,
            })
            .expect("a bind group")
    };
    let occlusion_group = group(
        "ssao depth",
        passes.layout,
        vec![
            BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::whole_buffer(uniforms),
            },
            BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: BindingResource::ImageView(depth_view),
            },
        ],
    );
    let blur_group = group(
        "ssao blur",
        passes.blur_layout,
        vec![
            BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::whole_buffer(uniforms),
            },
            BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: BindingResource::ImageView(raw_view),
            },
            BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: BindingResource::ImageView(depth_view),
            },
        ],
    );

    // One row is copied back, not the frame: the measurement is a line across
    // the plate's edge and a whole 1080p readback would be three orders of
    // magnitude more bytes for the same answer.
    let alignment = device
        .caps()
        .limits
        .optimal_buffer_copy_offset_alignment
        .max(4);
    let pitch = u64::from(width).next_multiple_of(alignment);
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("ssao row"),
            size: pitch,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    // The `ssao` pass alone is bracketed: the blur beside it is a fixed 4×4
    // kernel this change does not touch, and a bracket around both would report
    // a cost the two share.
    let timestamps = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let timers = timestamps.then(|| {
        device
            .create_query_set(&QuerySetDesc {
                label: Some("ssao"),
                kind: QueryKind::Timestamp,
                count: 2,
            })
            .expect("a timestamp pair on a device that reports the feature")
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("ssao measurement"),
        queue: headless.queue,
    });
    if let Some(set) = timers {
        encoder.reset_query_set(set, 0..2);
    }
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            depth,
            depth_range,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_image(&BufferImageCopy {
        buffer: upload,
        buffer_offset: 0,
        buffer_row_length: width,
        buffer_image_height: height,
        image: depth,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::DEPTH,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[
            ImageBarrier::new(
                depth,
                depth_range,
                ResourceState::TransferDst,
                ResourceState::ShaderRead,
            ),
            ImageBarrier::new(
                raw,
                colour_range(),
                ResourceState::Undefined,
                ResourceState::ColorAttachment,
            ),
        ],
        ..Barriers::default()
    });

    let area = Rect2d::from_size(width, height);
    let viewport = Viewport::from_size(width, height);
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("ssao"),
        color_attachments: &[ColorAttachment {
            view: raw_view,
            resolve: None,
            load: LoadOp::DontCare,
            store: StoreOp::Store,
            clear: ClearValue::default(),
        }],
        depth_stencil_attachment: None,
        render_area: area,
        timestamp_writes: timers.map(|set| PassTimestampWrites {
            set,
            beginning_of_pass: 0,
            end_of_pass: 1,
        }),
    });
    encoder.set_viewport(&viewport);
    encoder.set_scissor(&area);
    encoder.bind_graphics_pipeline(passes.pipeline);
    encoder.bind_group(0, occlusion_group, &[], passes.pipeline_layout);
    encoder.draw(0..3, 0..1);
    encoder.end_render_pass();

    encoder.pipeline_barrier(&Barriers {
        images: &[
            ImageBarrier::new(
                raw,
                colour_range(),
                ResourceState::ColorAttachment,
                ResourceState::ShaderRead,
            ),
            ImageBarrier::new(
                blurred,
                colour_range(),
                ResourceState::Undefined,
                ResourceState::ColorAttachment,
            ),
        ],
        ..Barriers::default()
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("ssao-blur"),
        color_attachments: &[ColorAttachment {
            view: blurred_view,
            resolve: None,
            load: LoadOp::DontCare,
            store: StoreOp::Store,
            clear: ClearValue::default(),
        }],
        depth_stencil_attachment: None,
        render_area: area,
        timestamp_writes: None,
    });
    encoder.set_viewport(&viewport);
    encoder.set_scissor(&area);
    encoder.bind_graphics_pipeline(passes.blur_pipeline);
    encoder.bind_group(0, blur_group, &[], passes.blur_pipeline_layout);
    encoder.draw(0..3, 0..1);
    encoder.end_render_pass();

    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            blurred,
            colour_range(),
            ResourceState::ColorAttachment,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: u32::try_from(pitch).expect("a row of a 1080p frame"),
        buffer_image_height: 1,
        image: blurred,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d {
            x: 0,
            y: LINE_Y as i32,
            z: 0,
        },
        image_extent: Extent3d::d2(width, 1),
    });

    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    let mut padded = poisoned(pitch as usize);
    headless.readback(staging, pitch, &mut padded);
    let row = &padded[..width as usize];

    let start = (PLATE_COLUMNS.end + SILHOUETTE_SKIP) as usize;
    let window = &row[start..start + WINDOW as usize];
    let (run, level) = longest_run(window);
    let low = *window.iter().min().expect("a non-empty window");
    let high = *window.iter().max().expect("a non-empty window");
    let shape = plateaus(window);

    let elapsed = timers.map(|set| {
        let mut readings = [0u64; 2];
        device
            .query_results(set, 0, &mut readings)
            .expect("the timestamps resolve");
        readings[1].saturating_sub(readings[0])
    });
    match elapsed {
        Some(ns) => eprintln!(
            "{suite}: the ssao pass took {ns} ns at {width}×{height}; the falloff's longest run \
             is {run} columns at level {level} over {low}..={high}",
            suite = crate::SUITE,
        ),
        None => eprintln!(
            "{suite}: this device has no timestamp query, so the ssao pass is untimed here; the \
             falloff's longest run is {run} columns at level {level} over {low}..={high}",
            suite = crate::SUITE,
        ),
    }

    assert!(
        high < UNOCCLUDED,
        "the {WINDOW}-column window past the plate's edge reaches {UNOCCLUDED:#04x}, so the \
         falloff ended before the window did and the run below is measured over flat unoccluded \
         wall. The window is {shape}. Move the window or the wall — the projected radius is \
         {reach:.1} pixels."
    );
    assert!(
        high - low >= MIN_SWING,
        "the window spans {low}..={high}, a swing of {}, and this measurement needs at least \
         {MIN_SWING} levels of gradient to have anything to say about its shape. The window is \
         {shape}.",
        high - low
    );
    assert!(
        run < MAX_RUN,
        "the blurred occlusion terraces: {run} consecutive columns at level {level} inside the \
         {WINDOW}-column falloff past the plate's edge, where {MAX_RUN} is the bound. The whole \
         window is {shape}.\n\
         \x20 What this measured against: `ssao.slang` with every `STEP_OFFSETS` entry forced to \
         one — the march that starts every pixel at the same fraction of a step — gave a longest \
         run of 16 columns on radv and 13 on lavapipe here, against 5 and 4 with the table in \
         place. A step of the march is {step:.1} pixels ({reach:.1}-pixel reach over \
         {SLICE_STEPS_HINT} steps), so a run in the tens of columns is the horizon landing on \
         {SLICE_STEPS_HINT} distances with nothing dithering them across the blur's footprint.",
        step = reach / SLICE_STEPS_HINT as f32,
    );

    if let Some(set) = timers {
        device.destroy_query_set(set);
    }
    device.destroy_bind_group(blur_group);
    device.destroy_bind_group(occlusion_group);
    device.destroy_buffer(staging);
    device.destroy_buffer(uniforms);
    device.destroy_buffer(upload);
    device.destroy_image_view(depth_view);
    device.destroy_image(depth);
    device.destroy_image_view(blurred_view);
    device.destroy_image(blurred);
    device.destroy_image_view(raw_view);
    device.destroy_image(raw);
    passes.destroy(device);
    headless.finish();
}
