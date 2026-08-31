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
//!
//! # The other axis, and why it needs a different scene
//!
//! That is the *radial* artefact — how far along a slice a horizon may sit. The
//! **tangential** one is about how many slice *planes* there are, and the wall
//! and plate above cannot show it: the plate spans every row, so the blurred
//! occlusion at a pixel depends on its column alone. `ssao_blur.slang`'s window
//! is `-1..=2` on each axis, which covers each of the sixteen tile phases
//! exactly once, so a column through that scene is constant by construction and
//! a measurement along it would be a check wired to nothing.
//!
//! [`the_tangential_occlusion_line_does_not_step`] therefore tilts the occluder.
//! Its plate is a half-plane whose edge runs along [`EDGE_STEP`], and the line it
//! reads runs **parallel to that edge** — so every sample sits at exactly the
//! same perpendicular distance from the occluder and the continuous answer along
//! it is one constant. The predicate is on integers and the line steps by whole
//! pixels, so neither the scene nor the sampling rounds anything: every level of
//! variation the line carries is the pass's.
//!
//! What varies is the pairing between a blur tap's tile phase and its distance
//! from the edge. A step along the edge moves both `x & 3` and `y & 3`, so it
//! re-pairs all sixteen phases with sixteen different distances and the blur's
//! sum moves. **How far it moves is what the slice count buys**: with two slices
//! a tile spans eight plane orientations and with four it spans twelve —
//! `ssao.slang`'s `SLICE_COUNT_MAX` counts them — and the fewer orientations the
//! neighbourhood holds, the more of the answer rides on which ones this
//! particular pixel drew.

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

/// The occluder's tilt, as the whole-pixel step that runs **along** its edge.
///
/// Three across and two down. Not the frame's own diagonal, and that is the
/// whole of the choice: a 45 degree edge is exactly the one a slice turned an
/// eighth of a turn runs parallel to, so measuring the eighth turn against it
/// would say more about the alignment than about the turn. Reduced modulo a half
/// turn this direction is in neither the eight orientations `ssao.slang`'s table
/// gives at two slices nor the twelve it gives at four, so neither count gets to
/// sweep straight along the edge.
///
/// **Whole pixels is what keeps the line exact.** A step of `(3, 2)` leaves
/// [`across_edge`] unchanged by construction, so every sample sits at the same
/// perpendicular distance from the same straight edge with nothing rounded and
/// nothing interpolated — see this module's header.
const EDGE_STEP: (i64, i64) = (3, 2);

/// How far the tangential line sits from that edge, in [`across_edge`]'s units.
///
/// **In the edge's own units and not in pixels**, because that is what makes it
/// exact: the perpendicular distance is this over the length of [`EDGE_STEP`],
/// and a constant written in pixels would have to be turned back into something
/// the depth predicate could compare against.
///
/// It puts the line well inside the falloff — the test asserts the projected
/// radius at [`WALL_Z`] covers it — and clear of the few columns at the
/// silhouette where `ssao_blur.slang` starts rejecting taps, which is
/// [`SILHOUETTE_SKIP`]'s territory and a different measurement.
const DIAGONAL_OFFSET: i64 = 152;

/// The tangential line's first pixel, `(x, y)`.
///
/// On the wall side of the edge, and far enough from every border that
/// [`DIAGONAL_LENGTH`] steps of [`EDGE_STEP`] stay inside [`EXTENT`]. The test
/// asserts its distance is [`DIAGONAL_OFFSET`]'s rather than this being two
/// numbers that agree today.
const DIAGONAL_FROM: (u32, u32) = (74, 100);

/// Samples taken along the tangential line, one per step of [`EDGE_STEP`].
///
/// The tile phase this walks repeats every four steps, so this is over a hundred
/// cycles of it — enough that the largest step between neighbours is the tile's
/// worst rather than whichever phase the line happened to start on.
const DIAGONAL_LENGTH: u32 = 480;

/// The difference between two neighbouring samples that counts as a sharp edge,
/// in 8-bit levels.
///
/// Two, because one is what an `R8Unorm` channel does on its own: the true
/// answer along the line is not quite a constant — the view direction turns
/// across a 1080p frame, so the integral's geometry does change — and a
/// slowly-varying quantity in eight bits rolls over by one level wherever it
/// crosses a boundary. A step of two is not the quantiser.
const SHARP_EDGE: u8 = 2;

/// Sharp edges the tangential line may hold at the counts that ship.
///
/// **Swept before it was fixed**, at [`EXTENT`] over [`DIAGONAL_LENGTH`] samples
/// on 2026-08-31, on the two drivers this machine has. The anti-baseline is a
/// `SLICE_DIRECTIONS` whose sixteen entries are all the same vector — one plane
/// orientation across the whole tile instead of eight — which is the shortage
/// this measurement is about:
///
/// ```text
///                                          radv   lavapipe
/// every SLICE_DIRECTIONS entry the same     148        172
/// SLICE_DIRECTIONS as `ssao.slang` ships      1          1
/// ```
///
/// So this sits between them on a log scale, with room on both sides: the
/// shipping table has to step twelve times as often to go red, and a table down
/// to one orientation has to lose nine tenths of its edges to pass.
const MAX_SHARP_EDGES: usize = 12;

/// Samples of the tangential line a failure message prints.
///
/// The line is [`DIAGONAL_LENGTH`] long and the artefact repeats with the tile,
/// so a few cycles of it says everything a reader needs and a full dump would
/// bury it.
const MESSAGE_SAMPLES: usize = 32;

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

/// The prepass image's contents: a wall, with the plate standing in front of it
/// wherever `occludes` says so.
///
/// The predicate takes the pixel rather than the scene taking a shape, because
/// the two tests want two different occluders out of the same two exact depths —
/// a vertical strip for the radial line and a tilted half-plane for the
/// tangential one. See this module's header on why the second cannot be the
/// first.
fn depth_image(projection: Mat4, occludes: impl Fn(u32, u32) -> bool) -> Vec<u8> {
    let wall = depth_of(projection, -WALL_Z);
    let plate = depth_of(projection, -(WALL_Z - PLATE_LIFT));
    assert!(
        plate > wall,
        "reversed-Z puts the nearer surface at the larger depth; the plate reads {plate} and the \
         wall {wall}, so the scene has the plate behind the wall and occludes nothing"
    );
    let (width, height) = EXTENT;
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let depth = if occludes(x, y) { plate } else { wall };
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

/// The tilted occluder: the half-plane on and above the frame's leading
/// diagonal.
///
/// A predicate on two integers, so the edge is exact in the same sense the
/// upright plate's is. The diagonal is the one tilt that puts every sample of a
/// parallel line at the same distance from the edge without rounding anything:
/// a step of `(1, 1)` changes `y - x` by nothing at all, so the line this scene
/// is read along needs no interpolation and no nearest pixel.
fn occluded_diagonal(x: u32, y: u32) -> bool {
    across_edge(x, y) >= 0
}

/// How far `(x, y)` is from the tilted occluder's edge, in the edge's own units.
///
/// `EDGE_STEP.1 * x - EDGE_STEP.0 * y`, which is zero on the edge, positive
/// inside the occluder and — this is the property the whole measurement rests on
/// — **exactly unchanged by a step of [`EDGE_STEP`]**, since that step adds
/// `EDGE_STEP.1 * EDGE_STEP.0` and subtracts the same product. Divided by the
/// length of [`EDGE_STEP`] it is the perpendicular distance in pixels; the test
/// never needs that division except to check the line lies inside the falloff.
fn across_edge(x: u32, y: u32) -> i64 {
    EDGE_STEP.1 * i64::from(x) - EDGE_STEP.0 * i64::from(y)
}

/// The tangential measure: how many neighbouring pairs of `line` differ by more
/// than one level, and the largest difference any pair has.
///
/// Where [`longest_run`] is the radial one. A line whose true answer is a smooth
/// ramp has no runs worth counting and no pair worth calling a cliff — what a
/// slice count too small to cover the directions leaves is a step between one
/// tile phase and the next, and `docs/backlog.md`'s claim about the higher rung
/// is in exactly those terms: that it removes **every sharp edge**.
///
/// **The count is the number that carries the signal**, and the maximum is
/// reported beside it. An 8-bit channel holding a slowly-varying quantity steps
/// by one level wherever the quantiser rolls over, so one level is not an edge;
/// two is. See [`SHARP_EDGE`].
fn sharp_edges(line: &[u8]) -> (usize, u8) {
    let steps = line.windows(2).map(|pair| pair[0].abs_diff(pair[1]));
    steps.fold((0, 0), |(count, worst), step| {
        (count + usize::from(step >= SHARP_EDGE), worst.max(step))
    })
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

/// One run of the shipping pair over an analytic prepass image.
///
/// The whole blurred channel, and what each pass cost — the two things both
/// tests here want and neither can get from the other's readback.
struct Run {
    /// The blurred occlusion, one byte per pixel and [`EXTENT`]`.0` bytes to a
    /// row. Whole rather than the one line either test reads, because the
    /// tangential line is a diagonal and a diagonal is not a copy region.
    image: Vec<u8>,
    /// `(label, nanoseconds)` per pass in the order they ran, or empty on a
    /// device with no timestamp query.
    ///
    /// **Every pass is bracketed separately**, unlike the single bracket this
    /// suite carried when there was one variable: the slice count is paid for
    /// inside `ssao` and a second blur is a whole extra full-screen read and
    /// write, and a reader deciding whether a tier can afford the rung needs the
    /// two prices apart. See `crcbl_render::ssao`'s header.
    timings: Vec<(String, u64)>,
}

impl Run {
    /// This run's value at `(x, y)`.
    fn at(&self, x: u32, y: u32) -> u8 {
        self.image[(y * EXTENT.0 + x) as usize]
    }

    /// What one pass cost, by label.
    fn pass(&self, label: &str) -> Option<u64> {
        self.timings
            .iter()
            .find(|(name, _)| name == label)
            .map(|(_, nanos)| *nanos)
    }
}

/// One configuration of the two switches, and what the tangential line measured
/// under it.
///
/// A type rather than a tuple because the sweep looks its entries up by the pair
/// of counts and then reads two statistics off them, and `.2` against `.3` is
/// exactly the pair a reader would mix up.
struct Measured {
    slices: u8,
    blurs: u32,
    /// The line's samples, kept for the failure message.
    line: Vec<u8>,
    /// Neighbouring pairs differing by [`SHARP_EDGE`] or more — see
    /// [`sharp_edges`].
    sharp: usize,
    /// The largest difference any neighbouring pair had.
    worst: u8,
}

/// Runs `ssao` and `blurs` blur passes over `texels`, at `slices` planes per
/// pixel, and reads the last blur's output back whole.
///
/// The pipelines and both layouts are `crates/crcbl-render/src/ssao.rs`'s, and
/// the second blur is that module's `ssao-blur-2`: the same pipeline reading the
/// first one's output into a third image. Nothing here reaches into that crate —
/// `Ssao` is private to it — so the shape is copied and this comment is what
/// says where from.
fn run_passes(headless: &Headless, projection: Mat4, texels: &[u8], slices: u8, blurs: u32) -> Run {
    let device = headless.device.as_ref();
    let (width, height) = EXTENT;

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
    // The second blur's target, created only when one was asked for, on the
    // renderer's terms: a transient nothing reads or writes is an image taken
    // out for a pass that does not exist.
    let second = (blurs > 1).then(|| occlusion_image(device, "ssao-blurred-2"));

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

    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("scene-depth upload"),
            size: texels.len() as u64,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("an upload buffer");
    device.write_buffer(upload, 0, texels).expect("the upload");

    let params = SsaoParams {
        inv_proj: projection.inverse().to_cols_array(),
        proj: projection.to_cols_array(),
        radius: RADIUS,
        slices,
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
    let blur_group = |source: ImageViewHandle| {
        group(
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
                    resource: BindingResource::ImageView(source),
                },
                BindGroupEntry {
                    binding: 2,
                    array_index: 0,
                    resource: BindingResource::ImageView(depth_view),
                },
            ],
        )
    };
    // Every blur's target in order: the first one's image, and the second one's
    // where a second was asked for.
    let chain: Vec<(ImageHandle, ImageViewHandle)> = core::iter::once((blurred, blurred_view))
        .chain(second)
        .collect();
    // One group per blur, each naming the image *before* it: the raw channel for
    // the first and the previous blur's target after that.
    let blur_groups: Vec<BindGroupHandle> = core::iter::once(raw_view)
        .chain(chain.iter().map(|(_, view)| *view))
        .take(blurs as usize)
        .map(blur_group)
        .collect();
    assert_eq!(
        blur_groups.len(),
        blurs as usize,
        "one group per blur, or a pass below binds the group of the pass before it"
    );

    // The whole channel, not a row: the tangential line is a diagonal, and a
    // copy region cannot be one. At [`EXTENT`] this is two megabytes, which is
    // one readback on a suite that already uploads eight.
    let alignment = device
        .caps()
        .limits
        .optimal_buffer_copy_offset_alignment
        .max(4);
    let pitch = u64::from(width).next_multiple_of(alignment);
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("ssao image"),
            size: pitch * u64::from(height),
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    // Two queries per pass, in the order the passes run — see [`Run::timings`]
    // on why every pass is bracketed rather than the pair.
    let labels: Vec<String> = core::iter::once("ssao".to_owned())
        .chain((0..blurs).map(|index| match index {
            0 => "ssao-blur".to_owned(),
            other => format!("ssao-blur-{}", other + 1),
        }))
        .collect();
    let queries = u32::try_from(labels.len() * 2).expect("a handful of passes");
    let timestamps = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let timers = timestamps.then(|| {
        device
            .create_query_set(&QuerySetDesc {
                label: Some("ssao"),
                kind: QueryKind::Timestamp,
                count: queries,
            })
            .expect("a timestamp pair per pass on a device that reports the feature")
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("ssao measurement"),
        queue: headless.queue,
    });
    if let Some(set) = timers {
        encoder.reset_query_set(set, 0..queries);
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

    // Each blur reads the image before it and writes the next one. `source` is
    // the raw channel for the first and the previous blur's target after that,
    // which is the ping-pong `crcbl_render::ssao::Ssao::add_passes` records.
    let mut source = (raw, raw_view);
    for (index, &target) in chain.iter().take(blurs as usize).enumerate() {
        encoder.pipeline_barrier(&Barriers {
            images: &[
                ImageBarrier::new(
                    source.0,
                    colour_range(),
                    ResourceState::ColorAttachment,
                    ResourceState::ShaderRead,
                ),
                ImageBarrier::new(
                    target.0,
                    colour_range(),
                    ResourceState::Undefined,
                    ResourceState::ColorAttachment,
                ),
            ],
            ..Barriers::default()
        });
        let first = u32::try_from(index * 2 + 2).expect("a handful of passes");
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("ssao-blur"),
            color_attachments: &[ColorAttachment {
                view: target.1,
                resolve: None,
                load: LoadOp::DontCare,
                store: StoreOp::Store,
                clear: ClearValue::default(),
            }],
            depth_stencil_attachment: None,
            render_area: area,
            timestamp_writes: timers.map(|set| PassTimestampWrites {
                set,
                beginning_of_pass: first,
                end_of_pass: first + 1,
            }),
        });
        encoder.set_viewport(&viewport);
        encoder.set_scissor(&area);
        encoder.bind_graphics_pipeline(passes.blur_pipeline);
        encoder.bind_group(0, blur_groups[index], &[], passes.blur_pipeline_layout);
        encoder.draw(0..3, 0..1);
        encoder.end_render_pass();

        source = target;
    }

    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            source.0,
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
        buffer_image_height: height,
        image: source.0,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });

    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    let mut padded = poisoned((pitch * u64::from(height)) as usize);
    headless.readback(staging, pitch * u64::from(height), &mut padded);
    let mut image = Vec::with_capacity((width * height) as usize);
    for row in 0..height {
        let at = (u64::from(row) * pitch) as usize;
        image.extend_from_slice(&padded[at..at + width as usize]);
    }

    let timings = match timers {
        Some(set) => {
            let mut readings = vec![0u64; queries as usize];
            device
                .query_results(set, 0, &mut readings)
                .expect("the timestamps resolve");
            labels
                .iter()
                .enumerate()
                .map(|(index, label)| {
                    (
                        label.clone(),
                        readings[index * 2 + 1].saturating_sub(readings[index * 2]),
                    )
                })
                .collect()
        }
        None => Vec::new(),
    };

    if let Some(set) = timers {
        device.destroy_query_set(set);
    }
    for group in blur_groups {
        device.destroy_bind_group(group);
    }
    device.destroy_bind_group(occlusion_group);
    device.destroy_buffer(staging);
    device.destroy_buffer(uniforms);
    device.destroy_buffer(upload);
    device.destroy_image_view(depth_view);
    device.destroy_image(depth);
    if let Some((image, view)) = second {
        device.destroy_image_view(view);
        device.destroy_image(image);
    }
    device.destroy_image_view(blurred_view);
    device.destroy_image(blurred);
    device.destroy_image_view(raw_view);
    device.destroy_image(raw);
    passes.destroy(device);

    Run { image, timings }
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
/// **At the counts that ship**, which is the point of it: this is the radial
/// artefact and the slice count is not what fixes it. The tangential test below
/// is where the higher rung is measured.
///
/// The failure message carries the whole plateau list, because the number that
/// went red says the window terraced and only the list says *how*.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_blurred_occlusion_falloff_does_not_terrace() {
    let headless = Headless::open_at_format(EXTENT, None, Features::TIMESTAMP_QUERY);
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

    let texels = depth_image(projection, |x, _| PLATE_COLUMNS.contains(&x));
    let run = run_passes(
        &headless,
        projection,
        &texels,
        crcbl::shaders::ssao::SLICE_COUNT_DEFAULT,
        1,
    );

    let start = (PLATE_COLUMNS.end + SILHOUETTE_SKIP) as usize;
    let row: Vec<u8> = (0..WINDOW)
        .map(|column| run.at(start as u32 + column, LINE_Y))
        .collect();
    let window = row.as_slice();
    let (run_length, level) = longest_run(window);
    let low = *window.iter().min().expect("a non-empty window");
    let high = *window.iter().max().expect("a non-empty window");
    let shape = plateaus(window);

    match run.pass("ssao") {
        Some(ns) => eprintln!(
            "{suite}: the ssao pass took {ns} ns at {width}×{height}; the falloff's longest run \
             is {run_length} columns at level {level} over {low}..={high}",
            suite = crate::SUITE,
        ),
        None => eprintln!(
            "{suite}: this device has no timestamp query, so the ssao pass is untimed here; the \
             falloff's longest run is {run_length} columns at level {level} over {low}..={high}",
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
        run_length < MAX_RUN,
        "the blurred occlusion terraces: {run_length} consecutive columns at level {level} inside \
         the {WINDOW}-column falloff past the plate's edge, where {MAX_RUN} is the bound. The \
         whole window is {shape}.\n\
         \x20 What this measured against: `ssao.slang` with every `STEP_OFFSETS` entry forced to \
         one — the march that starts every pixel at the same fraction of a step — gave a longest \
         run of 16 columns on radv and 13 on lavapipe here, against 5 and 4 with the table in \
         place. A step of the march is {step:.1} pixels ({reach:.1}-pixel reach over \
         {SLICE_STEPS_HINT} steps), so a run in the tens of columns is the horizon landing on \
         {SLICE_STEPS_HINT} distances with nothing dithering them across the blur's footprint.",
        step = reach / SLICE_STEPS_HINT as f32,
    );

    headless.finish();
}

/// **A line parallel to a tilted occluder's edge should be one constant, and
/// the slice count is what decides how far it is from one.**
///
/// The scene is [`the_blurred_occlusion_falloff_does_not_terrace`]'s two depths
/// with the plate tilted to the diagonal — see this module's header on why the
/// upright plate cannot show this and why the diagonal costs no rounding. Every
/// sample on the line sits at the same perpendicular distance from the edge, so
/// the continuous answer is flat and every level of variation is the pass's own
/// tile showing through the blur.
///
/// # What is asserted, and what is only reported
///
/// Two things, and both are about the shipping configuration being *no worse
/// than it is*: the line's largest step between neighbours, and its
/// peak-to-peak. Neither is a claim that the rung is affordable — that is a
/// tier's decision and lives in `docs/backlog.md` — so the four configurations
/// are timed and printed rather than compared for speed.
///
/// The third assertion is the one the rung rests on: **four slices and two
/// blurs must measure flatter than two and one.** A change that made the extra
/// planes stop contributing — an eighth turn that landed back on the quarter
/// turn, a count the shader clamped away — leaves every other check here green.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_tangential_occlusion_line_does_not_step() {
    let headless = Headless::open_at_format(EXTENT, None, Features::TIMESTAMP_QUERY);
    let (width, height) = EXTENT;
    let size = (width as f32, height as f32);
    let projection = Projection::Perspective {
        fov_y: FOV_Y,
        near: NEAR,
    }
    .matrix(size.0 / size.1);

    let reach = reach_in_pixels(projection, size);
    // The line's perpendicular distance from the edge, which is the one place
    // this test leaves the edge's own integer units. It has to sit inside the
    // falloff, which is as deep as the projected radius and no deeper — named
    // here rather than left to the assertions below, which would go red on flat
    // unoccluded wall for a reason that is not theirs.
    let edge_length = ((EDGE_STEP.0 * EDGE_STEP.0 + EDGE_STEP.1 * EDGE_STEP.1) as f64).sqrt();
    let lift = DIAGONAL_OFFSET as f64 / edge_length;
    assert!(
        f64::from(reach) > lift,
        "the scene projects a {RADIUS} radius to {reach:.1} pixels and the line sits {lift:.1} \
         pixels from the edge — outside the falloff, where the occlusion is flat because there is \
         none rather than because the pass is smooth"
    );
    assert_eq!(
        across_edge(DIAGONAL_FROM.0, DIAGONAL_FROM.1),
        -DIAGONAL_OFFSET,
        "the line's first pixel does not sit `DIAGONAL_OFFSET` from the edge, so the distance the \
         guard above checked is not the distance the samples were taken at"
    );

    let texels = depth_image(projection, occluded_diagonal);

    // **Warm up, and throw the answer away.** Whichever configuration runs first
    // in a process pays for both pipelines' creation, which on a software
    // rasteriser is the shader's own compilation and is an order of magnitude
    // more than the draw — measured on lavapipe 2026-08-31, the first `ssao`
    // reading of a process was 17 ms against 12 ms for the same work later in
    // the same process. Nothing is asserted on the timings, but a printed number
    // that is mostly a compiler is a number someone will quote.
    let _ = run_passes(
        &headless,
        projection,
        &texels,
        crcbl::shaders::ssao::SLICE_COUNT_DEFAULT,
        1,
    );

    // Every combination of the two switches, so the two purchases can be priced
    // apart — see `crcbl_render::ssao`'s header on why they are two variables.
    // The counts are the shader's own constants rather than literals, so a
    // sweep that stopped covering the range would be a compile error.
    let low = crcbl::shaders::ssao::SLICE_COUNT_DEFAULT;
    let high = crcbl::shaders::ssao::SLICE_COUNT_MAX;
    let mut lines = Vec::new();
    for (slices, blurs) in [(low, 1u32), (high, 1), (low, 2), (high, 2)] {
        let run = run_passes(&headless, projection, &texels, slices, blurs);
        let line: Vec<u8> = (0..DIAGONAL_LENGTH)
            .map(|step| {
                let along = u32::try_from(EDGE_STEP.0).expect("a step of a few pixels") * step;
                let down = u32::try_from(EDGE_STEP.1).expect("a step of a few pixels") * step;
                run.at(DIAGONAL_FROM.0 + along, DIAGONAL_FROM.1 + down)
            })
            .collect();
        let (sharp, worst) = sharp_edges(&line);
        let low = *line.iter().min().expect("a non-empty line");
        let high = *line.iter().max().expect("a non-empty line");
        let march = run.pass("ssao");
        let blur = run.pass("ssao-blur");
        let second = run.pass("ssao-blur-2");
        eprintln!(
            "{suite}: {slices} slices, {blurs} blur: the tangential line has {sharp} sharp edges \
             in {DIAGONAL_LENGTH} samples, worst step {worst}, over {low}..={high}; ssao \
             {march:?} ns, ssao-blur {blur:?} ns, ssao-blur-2 {second:?} ns",
            suite = crate::SUITE,
        );
        lines.push(Measured {
            slices,
            blurs,
            line,
            sharp,
            worst,
        });
    }

    let at = |slices: u8, blurs: u32| {
        lines
            .iter()
            .find(|measured| measured.slices == slices && measured.blurs == blurs)
            .unwrap_or_else(|| panic!("{slices} slices and {blurs} blur(s) was measured"))
    };
    let shipped = at(low, 1);
    let rung = at(high, 2);
    // The rung's own blur count with the shipping slice count, which is what
    // isolates the eighth turn: comparing the rung against what ships would let
    // the second blur alone answer for it.
    let blurred_twice = at(low, 2);

    // Anti-vacuity, on `MIN_SWING`'s terms: a line lying outside the falloff, or
    // on a scene whose plate occludes nothing, is flat for a reason that has
    // nothing to do with the pass and would pass every assertion below.
    let level = shipped.line[0];
    assert!(
        level < UNOCCLUDED && level > 0,
        "the tangential line reads {level} at its first sample, which is neither occluded nor \
         lit — the line is outside the falloff or the plate is not in front of the wall, and a \
         flatness measured there says nothing about the pass"
    );

    assert!(
        shipped.sharp <= MAX_SHARP_EDGES,
        "the tangential line has {} sharp edges in {DIAGONAL_LENGTH} samples at the counts that \
         ship, where {MAX_SHARP_EDGES} is the bound and its worst step is {} levels. The line is \
         a straight run at a fixed distance from a straight edge, so what is stepping is \
         `ssao.slang`'s tile coming through `ssao_blur.slang` rather than anything in the scene. \
         Its first samples are {first:?}.",
        shipped.sharp,
        shipped.worst,
        first = &shipped.line[..MESSAGE_SAMPLES.min(shipped.line.len())],
    );
    assert!(
        rung.sharp < blurred_twice.sharp,
        "the eighth turn bought nothing: {high} slices left {} sharp edges against {low} slices' \
         {}, at the same two blur passes. Either `ssao.slang`'s eighth turn landed back on a \
         direction the quarter turn already gave, or the count never reached the shader — and \
         every other check here is green either way. This is the one assertion about the rung \
         itself.",
        rung.sharp,
        blurred_twice.sharp,
    );

    headless.finish();
}
