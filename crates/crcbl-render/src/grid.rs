//! The infinite ground grid: one full-screen pass that ray-casts the plane
//! `y = 0` and blends its cell lines over the scene.
//!
//! ```text
//! scene-colour ───────────────────────────┐
//!                                         ▼
//! scene-depth ──(tested, never written)──▶ grid ──▶ scene-colour
//!                                         ▲
//!                       inverse view-projection
//! ```
//!
//! `docs/backlog.md` settled both halves of this before it was written. The
//! **technique** is the screen-space one — Blender's overlay grid, Godot 4's
//! editor grid and Unity's — because a grid drawn as geometry has lines whose
//! screen width changes with zoom, so it needs a density LOD, and it either
//! stops at its own edges or is drawn absurdly large. And the **place** is this
//! crate rather than a sample, because every `.slang` in the workspace lives in
//! `crcbl-shaders` and because the depth image the grid has to be occluded by
//! is here.
//!
//! # What this module is, and what it is not
//!
//! It is the pass and its pipeline, and nothing here reads a camera type: the
//! pass takes the two matrices it needs and no more.
//!
//! [`crate::forward`] is what puts it in a frame, behind an opt-in that is off
//! by default — see [`ForwardRenderer::set_ground_grid`] — and it adds this pass
//! **after** the tonemap rather than before it. The grid is reference chrome, so
//! it is drawn in display space where its colour is the same at any exposure,
//! which is what Blender's overlays do; drawn into the HDR target it would be
//! tonemapped like geometry and shift with scene brightness. It still tests
//! against the scene depth, so real geometry occludes it.
//!
//! [`ForwardRenderer::set_ground_grid`]: crate::forward::ForwardRenderer::set_ground_grid
//!
//! [`Grid`] is deliberately **not** re-exported at the crate root, unlike most
//! of this crate's types: [`crate::light_grid::Grid`] already holds that name at
//! the root and means the froxel grid, and two `Grid`s one `use` apart is worse
//! than one path segment.
//!
//! # The maths lives twice, on purpose
//!
//! `shaders/grid.slang` owns the real implementation, and nothing in this
//! workspace can execute a fragment shader: there is no CPU path that runs one,
//! and the golden suite compares whole images rather than the rule that
//! produced them. So the coverage rule, the fade and the plane intersection are
//! written a second time here — [`plane_hit`], [`line_coverage`],
//! [`resolvable`], [`scale_coverage`], [`distance_fade`] and [`composite`] —
//! and it is that copy the tests hold.
//!
//! Two of them cannot be a literal transcription and say so in their own docs:
//! `fwidth` has no meaning off a rasteriser, so the Rust half takes the
//! derivative as an argument where the shader computes it. Everything else is
//! the same expression in the same order.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BlendState, BufferDesc,
    BufferHandle, BufferUsage, ColorTargetState, ColorWrites, CompareOp, DepthBias,
    DepthStencilState, Device, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError,
    LoadOp, MemoryLocation, MultisampleState, PipelineLayoutDesc, PipelineLayoutHandle,
    PrimitiveState, ShaderEntry, ShaderModuleDesc, ShaderStages, StoreOp,
    check_portable_storage_buffers,
};
use crcbl_shaders::{GRID, Stage};
use glam::{Mat4, Vec2, Vec3};

use crate::graph::{ImageId, RenderGraph};

/// Vertices in the over-sized full-screen triangle `grid.slang` generates from
/// `SV_VertexID`. No geometry is bound anywhere — see `shaders/tonemap.slang`,
/// which carries the shape's *why*.
const FULLSCREEN_VERTICES: u32 = 3;

/// Bytes of the uniform block: two `float4x4` and three `float4` rows.
///
/// `std140` gives a `float4x4` four sixteen-byte columns and a `float4` one
/// row, and the total is already a multiple of sixteen, so there is no tail
/// padding to write.
const PARAMS_SIZE: usize = 64 + 64 + 16 + 16 + 16;

/// How nearly parallel to the ground a ray may be before [`plane_hit`] treats
/// it as missing the plane, matching `static const float PARALLEL_EPSILON` in
/// `shaders/grid.slang`.
///
/// The direction is a unit vector, so this is the sine of the angle to the
/// ground.
pub const PARALLEL_EPSILON: f32 = 1e-7;

/// The floor put under a derivative before [`line_coverage`] divides by it,
/// matching `static const float DERIVATIVE_FLOOR` in `shaders/grid.slang`.
///
/// A derivative of exactly zero is what a perfectly axis-aligned top-down view
/// produces on one of the two axes, and the division would otherwise return an
/// infinity that clamps to a full-coverage line across the whole frame.
pub const DERIVATIVE_FLOOR: f32 = 1e-8;

/// How the grid is drawn.
///
/// Every field lands in the shader's `params`, `fine_color` and `coarse_color`
/// rows — see `shaders/grid.slang` for what each one does to a fragment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridStyle {
    /// The fine cell size, in world units.
    pub spacing: f32,
    /// How many fine cells make one coarse cell.
    pub coarse_multiple: f32,
    /// Line width, in **pixels**. Constant at any zoom, which is the whole
    /// point of taking coverage from a screen-space derivative.
    pub line_width: f32,
    /// The distance, in world units, at which the grid has faded to nothing.
    pub fade_distance: f32,
    /// The fine lines' linear RGB, with their peak opacity in `a`.
    pub fine_color: [f32; 4],
    /// The coarse lines' linear RGB, with their peak opacity in `a`.
    /// Composited over the fine lines.
    pub coarse_color: [f32; 4],
}

impl Default for GridStyle {
    /// One-metre cells with a heavier line every ten, fading out at a hundred
    /// metres.
    ///
    /// A metre because that is the unit the rest of the engine's 3D content is
    /// authored in — `crcbl_greybox::primitives` sizes every prototype solid in
    /// metres — so the fine cell is the size of the smallest thing anyone
    /// places, and the coarse cell is the ten-metre block a room is laid out
    /// on.
    fn default() -> Self {
        Self {
            spacing: 1.0,
            coarse_multiple: 10.0,
            line_width: 1.0,
            fade_distance: 100.0,
            fine_color: [0.32, 0.32, 0.32, 0.55],
            coarse_color: [0.55, 0.55, 0.55, 0.8],
        }
    }
}

impl GridStyle {
    /// Roughly how many fine cells should span the thing being looked at.
    ///
    /// Ten is the count that makes a cell read as a *unit of the model* rather
    /// than as texture: fewer and the grid stops giving a sense of scale, more
    /// and the fine lines merge before the model fills the view.
    const CELLS_ACROSS: f32 = 10.0;

    /// How far past the subject the grid runs before it has faded out.
    ///
    /// The grid has to reach the horizon at the framing distance or the ground
    /// visibly ends in mid-air behind the model.
    const FADE_MULTIPLE: f32 = 20.0;

    /// The narrowest and widest fine cell this will choose, in world units.
    ///
    /// A degenerate or absurd extent otherwise picks a spacing that makes the
    /// whole grid one line or none, and `f32` runs out of precision either way.
    const MIN_SPACING: f32 = 1e-4;
    const MAX_SPACING: f32 = 1e6;

    /// A style scaled to a subject `extent` world units across.
    ///
    /// [`Default`] is a metre grid fading at a hundred, which suits content
    /// authored at human scale and nothing else: a five-centimetre model sits
    /// inside a single cell, and a five-hundred-metre one is entirely past the
    /// fade. A viewer opening arbitrary documents cannot assume either.
    ///
    /// **The spacing snaps to a power of ten.** A cell of `0.1`, `1` or `10`
    /// units is a number a person can count in and do arithmetic with, which is
    /// the grid's whole job; a cell of `3.7` gives the same line density and
    /// tells the eye nothing. Blender's adaptive grid steps by powers of ten
    /// for the same reason. So this picks the power of ten nearest to
    /// `extent / CELLS_ACROSS` rather than that value itself.
    ///
    /// A non-finite or non-positive `extent` returns [`Default`] — there is no
    /// scale to derive one from, and a `log10` of zero is `-inf`.
    #[must_use]
    pub fn for_extent(extent: f32) -> Self {
        if !extent.is_finite() || extent <= 0.0 {
            return Self::default();
        }
        let ideal = (extent / Self::CELLS_ACROSS).clamp(Self::MIN_SPACING, Self::MAX_SPACING);
        let spacing = 10.0f32
            .powf(ideal.log10().round())
            .clamp(Self::MIN_SPACING, Self::MAX_SPACING);
        Self {
            spacing,
            fade_distance: extent * Self::FADE_MULTIPLE,
            ..Self::default()
        }
    }
}

/// Distance along `direction` from `origin` to the ground plane `y = 0`, or
/// [`None`] when the ray does not reach it.
///
/// The engine is right-handed with **+Y up and −Z forward**, so the ground is
/// the `XZ` plane and the intersection is one division. The two ways it has no
/// answer are the two `grid.slang` drops a fragment for:
///
/// * **Parallel.** A `direction.y` within [`PARALLEL_EPSILON`] of zero never
///   reaches the plane, however far the ray runs.
/// * **Behind.** A non-positive distance is a plane the ray is travelling away
///   from — the camera is below the ground looking up, or above it looking up.
///
/// `direction` is expected normalised, which is what makes the result a
/// distance in world units and what [`distance_fade`] then reads.
#[must_use]
pub fn plane_hit(origin: Vec3, direction: Vec3) -> Option<f32> {
    if direction.y.abs() <= PARALLEL_EPSILON {
        return None;
    }
    let distance = -origin.y / direction.y;
    (distance > 0.0).then_some(distance)
}

/// Coverage of the nearest line along one axis, in `0..=1`.
///
/// `cell` is the world coordinate **in cells** — the shader's `plane / spacing`
/// — and `derivative` is how much of it one pixel spans, which is what the
/// shader gets from `fwidth` and this function has to be handed: a derivative
/// is a property of a rasterised quad and does not exist off one.
///
/// Their quotient is therefore a distance in **pixels**, and that is the whole
/// technique: the line is the same width on screen at any zoom and needs no
/// density LOD.
///
/// # It is the exact box filter, not a ramp
///
/// `line_width / 2 + 0.5 - distance` is how much of a `line_width`-wide band
/// centred on the line falls inside a one-pixel box `distance` pixels away from
/// it. The extra half over a ramp from the line to half a width out is what
/// keeps a hairline that lands on a pixel *boundary* from disappearing: a ramp
/// gives it zero coverage on both sides, so a one-pixel grid drops whole lines
/// depending on where the camera happens to sit. This form gives that line a
/// half on each side, and puts the same total ink on screen wherever it lands.
///
/// A real GPU is what found that: drawn with a ramp, the line through world
/// `x = 0` was missing from a top-down frame while the one through `x = 2` was
/// not.
#[must_use]
pub fn line_coverage(cell: f32, derivative: f32, line_width: f32) -> f32 {
    // `fract` in the shader's sense — `x - floor(x)`, so this is the distance
    // to the nearest integer cell boundary in `0..=0.5` for a negative
    // coordinate too.
    let offset = cell - 0.5;
    let to_line = (offset - offset.floor() - 0.5).abs() / derivative.max(DERIVATIVE_FLOOR);
    (line_width * 0.5 + 0.5 - to_line).clamp(0.0, 1.0)
}

/// How much of one scale survives once its lines start to merge, in `0..=1`.
///
/// A line `line_width` pixels across covers `line_width * derivative` of every
/// cell, so once that reaches one the lines have joined into a sheet. The scale
/// is faded out over the upper half of that range, which is what keeps the
/// horizon from turning into aliased grey mush.
#[must_use]
pub fn resolvable(derivative: f32, line_width: f32) -> f32 {
    (2.0 * (1.0 - line_width * derivative)).clamp(0.0, 1.0)
}

/// Coverage of one grid scale, in `0..=1`.
///
/// `cell` and `derivative` are per-axis, in the units [`line_coverage`]
/// documents. The two axes are combined with `max` rather than added: a pixel
/// on a crossing is on one line, not on two, and adding would draw a bright dot
/// at every intersection.
#[must_use]
pub fn scale_coverage(cell: Vec2, derivative: Vec2, line_width: f32) -> f32 {
    let coverage = line_coverage(cell.x, derivative.x, line_width).max(line_coverage(
        cell.y,
        derivative.y,
        line_width,
    ));
    coverage * resolvable(derivative.x.max(derivative.y), line_width)
}

/// How much of the grid is left at `distance`, in `0..=1`.
///
/// Linear to zero at [`GridStyle::fade_distance`], so the grid has no edge for
/// a camera to fly past and the horizon — where [`resolvable`] is already
/// falling — has nothing left to alias.
#[must_use]
pub fn distance_fade(distance: f32, fade_distance: f32) -> f32 {
    (1.0 - distance / fade_distance).clamp(0.0, 1.0)
}

/// The two scales composited into one **premultiplied** RGBA fragment.
///
/// The coarse lines go over the fine ones, and the pair is resolved here rather
/// than by two draws — so what the blender receives is already `colour * alpha`
/// and `crcbl_hal::BlendState::premultiplied` has nothing left to multiply.
#[must_use]
pub fn composite(style: &GridStyle, fine: f32, coarse: f32, fade: f32) -> [f32; 4] {
    let coarse_alpha = style.coarse_color[3] * coarse * fade;
    let fine_alpha = style.fine_color[3] * fine * fade * (1.0 - coarse_alpha);
    [
        style.coarse_color[0] * coarse_alpha + style.fine_color[0] * fine_alpha,
        style.coarse_color[1] * coarse_alpha + style.fine_color[1] * fine_alpha,
        style.coarse_color[2] * coarse_alpha + style.fine_color[2] * fine_alpha,
        coarse_alpha + fine_alpha,
    ]
}

/// The uniform block `grid.slang` declares, as the bytes its buffer holds.
///
/// Both matrices are **column-major**, the order `glam::Mat4::to_cols_array`
/// produces and the order every other block this crate writes is in — see
/// `crcbl_shaders::ssao::SsaoParams`, whose `proj` pair these two are siblings
/// of.
fn params_bytes(inv_view_proj: Mat4, view_proj: Mat4, style: &GridStyle) -> [u8; PARAMS_SIZE] {
    let mut bytes = [0u8; PARAMS_SIZE];
    let mut at = 0;
    let rows = [
        style.spacing,
        style.coarse_multiple,
        style.line_width,
        style.fade_distance,
    ];
    let values = inv_view_proj
        .to_cols_array()
        .into_iter()
        .chain(view_proj.to_cols_array())
        .chain(rows)
        .chain(style.fine_color)
        .chain(style.coarse_color);
    for value in values {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        at += 4;
    }
    debug_assert_eq!(at, PARAMS_SIZE, "the block is written whole");
    bytes
}

/// Everything the grid pass owns.
///
/// Built once by [`Grid::new`] and released by [`Grid::destroy`], which is the
/// shape every other resource group in this crate has — see
/// [`crate::ui_pass::UiRenderer`].
#[derive(Debug)]
pub struct Grid {
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the uniform block. One per frame in flight for the frame
    /// uniforms' reason exactly — the previous frame may still be reading last
    /// frame's while this one is written.
    uniforms: Vec<BufferHandle>,
    /// `[frame]`: the group naming [`Grid::uniforms`], and nothing else.
    ///
    /// Built here rather than in the pass body, unlike [`crate::ssao`]'s: this
    /// pass binds no graph transient, so there is no view that only exists once
    /// a pass is running.
    groups: Vec<BindGroupHandle>,
    style: GridStyle,
}

impl Grid {
    /// Builds the pipeline and the uniform ring.
    ///
    /// `target_format` is the colour attachment the grid blends over and
    /// `depth_format` the depth attachment it is tested against; both are
    /// pipeline state, so a caller that renders into a different pair needs a
    /// different [`Grid`].
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. Everything created before the failure
    /// is released, so a failed build leaks nothing.
    pub fn new(
        device: &dyn Device,
        frames: usize,
        target_format: Format,
        depth_format: Format,
    ) -> Result<Self, HalError> {
        let mut rollback = Rollback::default();
        match Self::build(device, frames, target_format, depth_format, &mut rollback) {
            Ok(grid) => Ok(grid),
            Err(error) => {
                rollback.run(device);
                Err(error)
            }
        }
    }

    /// The body of [`Grid::new`], recording what it has created into `rollback`
    /// as it goes.
    fn build(
        device: &dyn Device,
        frames: usize,
        target_format: Format,
        depth_format: Format,
        rollback: &mut Rollback,
    ) -> Result<Self, HalError> {
        // One binding, and it is the whole interface: the pass samples nothing.
        // The plane it draws is reconstructed per fragment out of the matrices
        // in this block, which is what makes the grid infinite without a vertex
        // buffer, a texture or a scene resource of any kind.
        let entries = [BindGroupLayoutEntry {
            binding: 0,
            // `FRAGMENT` alone, which is where every read of the block is: the
            // vertex stage generates three corners out of `SV_VertexID` and
            // looks at nothing. Slang's Metal target still lists the buffer in
            // the vertex entry point's signature, and `ssao.slang` — the other
            // full-screen pass with a block — ships exactly that pairing.
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::UniformBuffer { dynamic: false },
            count: 1,
            flags: BindingFlags::empty(),
        }];
        let desc = BindGroupLayoutDesc {
            label: Some("grid"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("grid"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        rollback.bind_group_layouts.push(layout);

        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("grid"),
            bind_group_layouts: &set_layouts,
            // No range at all, for [`crate::ui_pass`]'s reason: the block
            // arrives through the bind group so that there is one code path on
            // every backend, including the one with no push constants.
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(pipeline_layout);

        let mut uniforms = Vec::with_capacity(frames);
        let mut groups = Vec::with_capacity(frames);
        for _ in 0..frames {
            let buffer = device.create_buffer(&BufferDesc {
                label: Some("grid params"),
                size: PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(buffer);
            uniforms.push(buffer);
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("grid params"),
                layout,
                entries: &[BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(buffer),
                }],
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            groups.push(group);
        }

        // Entry points resolved before the module exists, for
        // [`crate::ui_pass`]'s reason: a manifest that disagreed with the
        // artifact would otherwise fail inside the descriptor literal, with the
        // module already created and nothing holding it.
        let vertex = entry(Stage::Vertex)?;
        let fragment = entry(Stage::Fragment)?;
        let module = device.create_shader_module(&ShaderModuleDesc {
            label: Some(GRID.source()),
            spirv: GRID.spirv(),
            wgsl: GRID.wgsl(),
            msl: GRID.msl(),
            // A container per entry point, for the reason
            // [`crate::forward`]'s mesh module gives.
            dxil: &GRID.dxil_containers(),
        })?;
        let targets = [ColorTargetState {
            format: target_format,
            // **Premultiplied, not straight alpha.** `grid.slang` composites
            // its two scales itself and emits `colour * alpha`, so the blender
            // must not multiply again — see that file's header.
            blend: Some(BlendState::premultiplied()),
            write_mask: ColorWrites::ALL,
        }];
        let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("grid"),
            layout: pipeline_layout,
            vertex: ShaderEntry {
                module,
                entry_point: vertex,
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: fragment,
            }),
            // The triangle is deliberately oversized, so two of its vertices
            // are outside the viewport and its winding is not worth reasoning
            // about.
            primitive: PrimitiveState::default(),
            depth_stencil: Some(DepthStencilState {
                format: depth_format,
                // **Tested, never written.** The grid is chrome: geometry in
                // front of it must hide it, and it must not hide anything drawn
                // after it — which is what a depth write would do.
                depth_write: false,
                // `Greater` and not `GreaterOrEqual`, unlike a pass drawing
                // against its own prepass: this pass has no prepass entry, so
                // equality here is the grid landing exactly on a surface the
                // scene already drew, and the scene wins.
                depth_compare: CompareOp::Greater,
                stencil: None,
                bias: DepthBias::default(),
            }),
            multisample: MultisampleState::default(),
            color_targets: &targets,
        });
        device.destroy_shader_module(module);
        let pipeline = pipeline?;
        rollback.pipelines.push(pipeline);

        Ok(Self {
            layout,
            pipeline_layout,
            pipeline,
            uniforms,
            groups,
            style: GridStyle::default(),
        })
    }

    /// How the grid is drawn.
    #[must_use]
    pub const fn style(&self) -> &GridStyle {
        &self.style
    }

    /// Replaces [`Grid::style`]. Takes effect on the next
    /// [`Grid::add_pass`], which is what writes the block.
    pub const fn set_style(&mut self, style: GridStyle) {
        self.style = style;
    }

    /// Adds the `grid` pass to `graph`, blending over `target` and tested
    /// against `depth`.
    ///
    /// `depth` must be the depth the scene was drawn into and must still hold
    /// it — the pass attaches it read-only, so it is neither cleared nor
    /// written and a later pass sees exactly what the scene left. `target` is
    /// loaded rather than cleared for the same reason: the grid blends over
    /// what is already there.
    ///
    /// `inv_view_proj` is the camera's inverse view-projection, which is the
    /// camera fact the fragment stage builds its ray from: it unprojects two
    /// clip-space points per pixel to get that pixel's view ray. `view_proj` is
    /// the ordinary one, and it travels in the block beside it because the
    /// fragment stage has to project its plane hit back to the `SV_Depth` it is
    /// tested at — and no shader can invert a matrix.
    ///
    /// **Both are taken rather than one derived from the other.** Every caller
    /// is drawing a frame it already has a view-projection for, and a
    /// [`Mat4::inverse`] per frame to recover a matrix the caller was holding is
    /// arithmetic for nothing.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub fn add_pass<'a>(
        &'a self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        target: ImageId,
        depth: ImageId,
        view_proj: Mat4,
        inv_view_proj: Mat4,
    ) {
        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let group = self.groups[frame];
        let uniforms = self.uniforms[frame];
        let bytes = params_bytes(inv_view_proj, view_proj, &self.style);

        graph
            .add_render_pass("grid")
            .color(target, LoadOp::Load, StoreOp::Store, Default::default())
            // Read-only: attached so the hardware depth test runs, and declared
            // this way so the graph moves it to a depth-read state rather than
            // to one that would let this pass write it.
            .depth_read(depth)
            .execute(move |ctx| {
                // A host-visible write, not a command: it lands before this
                // frame is submitted, and the buffer is one of the ring, so the
                // rotation that makes any other per-frame block safe makes this
                // safe.
                if let Err(error) = ctx.device().write_buffer(uniforms, 0, &bytes) {
                    // Recording a pass that draws nothing beats aborting the
                    // frame, as everywhere else in this crate: the grid
                    // vanishes for a frame, the log says why, the next frame
                    // retries.
                    crcbl_core::log::error!("graph: grid params write failed: {error}");
                    return;
                }
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        for group in self.groups {
            device.destroy_bind_group(group);
        }
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
    }
}

/// `grid.slang`'s entry point for `stage`.
fn entry(stage: Stage) -> Result<&'static str, HalError> {
    GRID.entry_point(stage).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "grid.slang exposes no unambiguous {stage:?} entry point; the committed SPIR-V and \
             its manifest disagree, which crates/crcbl-shaders/tools/compile-shaders.sh would fix"
        ))
    })
}

/// What a partly-built [`Grid`] has to give back.
///
/// [`Grid::build`] creates a handful of objects with `?` between them and the
/// seam's `destroy_*` is explicit, so a failure half way through would
/// otherwise leak everything created before it. [`crate::ui_pass`] carries the
/// same shape for the same reason.
#[derive(Default)]
struct Rollback {
    buffers: Vec<BufferHandle>,
    bind_groups: Vec<BindGroupHandle>,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    pipelines: Vec<GraphicsPipelineHandle>,
}

impl Rollback {
    /// Releases everything recorded, in dependency order.
    fn run(self, device: &dyn Device) {
        for group in self.bind_groups {
            device.destroy_bind_group(group);
        }
        for buffer in self.buffers {
            device.destroy_buffer(buffer);
        }
        for pipeline in self.pipelines {
            device.destroy_graphics_pipeline(pipeline);
        }
        for layout in self.pipeline_layouts {
            device.destroy_pipeline_layout(layout);
        }
        for layout in self.bind_group_layouts {
            device.destroy_bind_group_layout(layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A style derived for a subject keeps a countable cell and reaches past it.
    ///
    /// The two properties that matter, at four scales spanning ten orders of
    /// magnitude: the cell is a power of ten (so the grid is something a person
    /// can count in), and the model spans a sane number of cells rather than
    /// sitting inside one or drowning in them.
    #[test]
    fn a_derived_style_keeps_a_power_of_ten_cell_at_any_scale() {
        for extent in [0.05f32, 1.0, 12.0, 500.0, 20_000.0] {
            let style = GridStyle::for_extent(extent);

            let decades = style.spacing.log10();
            assert!(
                (decades - decades.round()).abs() < 1e-4,
                "a {extent} m subject chose a {} m cell, which is not a power of ten and so is \
                 not a number anyone can count in",
                style.spacing
            );

            let across = extent / style.spacing;
            assert!(
                (1.0..=100.0).contains(&across),
                "a {extent} m subject spans {across} cells of {} m — outside the range where the \
                 grid says anything about its scale",
                style.spacing
            );
            assert!(
                style.fade_distance > extent,
                "a {extent} m subject fades at {}, so the ground would end inside the model",
                style.fade_distance
            );
        }
    }

    /// A scale-less subject falls back rather than computing a `log10` of zero.
    ///
    /// `for_extent` is fed a bounding box, and a document can produce a
    /// degenerate one — a single point, or the empty scene the viewer refuses
    /// only *after* it has framed. Zero would give `-inf` and a `NaN` spacing,
    /// which is a grid that never draws and never says why.
    #[test]
    fn a_degenerate_extent_falls_back_to_the_default_style() {
        for extent in [0.0f32, -3.0, f32::NAN, f32::INFINITY] {
            let style = GridStyle::for_extent(extent);
            assert_eq!(
                style.spacing,
                GridStyle::default().spacing,
                "an extent of {extent} should fall back, not derive"
            );
            assert!(style.spacing.is_finite() && style.fade_distance.is_finite());
        }
    }

    use crcbl_hal::null::{NullInstance, Recorder};
    use crcbl_hal::{
        CommandEncoderDesc, DeviceDesc, Features, ImageUsage, Instance, QueueHandle, QueueKind,
        ResourceState,
    };

    use crate::transient::{TransientImageDesc, TransientPool};

    /// The source the artifacts in `crcbl-shaders` were built from — the
    /// manifest hash-pins it, so this is the same file, and reading it is how
    /// the mirrors below are checked at all.
    const SOURCE: &str = include_str!("../../crcbl-shaders/shaders/grid.slang");

    fn open() -> (Recorder, Box<dyn Device>, QueueHandle) {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::GPU_DRIVEN,
                optional_features: Features::PUSH_CONSTANTS,
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (recorder, device, queue)
    }

    // -----------------------------------------------------------------------
    // The plane
    // -----------------------------------------------------------------------

    /// Straight down from two metres up is two metres of ray.
    #[test]
    fn the_hit_is_the_distance_along_a_unit_direction() {
        let hit = plane_hit(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(hit, Some(2.0));

        // Forty-five degrees down, looking along −Z: the hypotenuse of a
        // one-metre drop.
        let diagonal = Vec3::new(0.0, -1.0, -1.0).normalize();
        let hit = plane_hit(Vec3::new(0.0, 1.0, 0.0), diagonal).expect("it crosses");
        assert!(
            (hit - 2.0f32.sqrt()).abs() < 1e-6,
            "a 45° ray from 1 m up travels √2 m to the ground, not {hit}"
        );
    }

    /// A ray with no vertical component never reaches the ground.
    #[test]
    fn a_ray_parallel_to_the_ground_never_hits_it() {
        assert_eq!(
            plane_hit(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, 0.0, -1.0)),
            None
        );
        // Just inside the epsilon, which is the boundary the shader shares.
        assert_eq!(
            plane_hit(
                Vec3::new(0.0, 5.0, 0.0),
                Vec3::new(0.0, PARALLEL_EPSILON * 0.5, -1.0)
            ),
            None
        );
    }

    /// A camera above the ground looking up is looking away from it.
    #[test]
    fn a_plane_behind_the_camera_is_not_a_hit() {
        assert_eq!(
            plane_hit(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            None
        );
        // And a camera standing exactly on the plane: the hit is at zero
        // distance, which is the fragment's own pixel and not a surface.
        assert_eq!(
            plane_hit(Vec3::ZERO, Vec3::new(0.0, -1.0, 0.0)),
            None,
            "a zero-distance hit is the camera itself"
        );
    }

    // -----------------------------------------------------------------------
    // Coverage
    // -----------------------------------------------------------------------

    /// **The claim the whole technique rests on.** Two cameras a hundred times
    /// apart in zoom see the same line width, because coverage is a function of
    /// the distance to the line *in pixels*.
    #[test]
    fn a_line_is_the_same_pixel_width_at_any_zoom() {
        let width = 2.0;
        for derivative in [0.001f32, 0.01, 0.1] {
            // A pixel box centred half a width plus half a pixel out overlaps
            // the band by nothing: that is the last pixel the line reaches.
            let edge = derivative * (width * 0.5 + 0.5);
            let coverage = line_coverage(edge, derivative, width);
            assert!(
                coverage < 1e-4,
                "at {derivative} cells/px the line's edge read {coverage}"
            );
            // Half a width out, the box straddles the band's edge and takes
            // half of it — whatever the zoom.
            let half = derivative * width * 0.5;
            let coverage = line_coverage(half, derivative, width);
            assert!(
                (coverage - 0.5).abs() < 1e-4,
                "at {derivative} cells/px the half-covered point read {coverage}"
            );
            // And the line itself is solid.
            assert_eq!(line_coverage(0.0, derivative, width), 1.0, "{derivative}");
        }
    }

    /// **What a real GPU found.** A hairline landing exactly on a pixel
    /// boundary must still be drawn: the two pixels either side take half each,
    /// rather than both taking nothing.
    ///
    /// The coverage a line puts on screen is the same wherever it lands between
    /// two pixel centres, which is what stops a one-pixel grid dropping and
    /// regaining whole lines as the camera moves.
    #[test]
    fn a_hairline_on_a_pixel_boundary_is_still_drawn() {
        // One cell per hundred pixels, and a one-pixel line.
        let (derivative, width) = (0.01f32, 1.0);
        // The line exactly on a pixel *centre*: that pixel takes all of it and
        // its neighbours none.
        let centred: f32 = [-1.0f32, 0.0, 1.0]
            .into_iter()
            .map(|pixel| line_coverage(pixel * derivative, derivative, width))
            .sum();
        // The line exactly on a pixel *boundary*: the two either side halve it.
        let straddling: f32 = [-1.5f32, -0.5, 0.5, 1.5]
            .into_iter()
            .map(|pixel| line_coverage(pixel * derivative, derivative, width))
            .sum();
        assert!(
            line_coverage(0.5 * derivative, derivative, width) > 0.0,
            "a line on a pixel boundary vanished"
        );
        assert!(
            (centred - straddling).abs() < 1e-4,
            "the same line put {centred} on screen at a pixel centre and {straddling} at a \
             boundary; it would flicker as the camera moves"
        );
    }

    /// The lines repeat every cell, and `floor` rather than a truncation is
    /// what makes that true on the negative side of the origin.
    #[test]
    fn the_lines_repeat_on_both_sides_of_the_origin() {
        let (derivative, width) = (0.01f32, 1.0);
        for cell in [-3.0f32, -1.0, 0.0, 1.0, 7.0] {
            assert_eq!(
                line_coverage(cell, derivative, width),
                1.0,
                "cell boundary {cell} is a line"
            );
        }
        for cell in [-2.5f32, -0.5, 0.5, 6.5] {
            assert_eq!(
                line_coverage(cell, derivative, width),
                0.0,
                "cell centre {cell} is not a line"
            );
        }
    }

    /// A scale whose lines have merged into a sheet contributes nothing, which
    /// is what stops the horizon aliasing.
    #[test]
    fn a_scale_stops_once_its_lines_have_merged() {
        let width = 1.0;
        // A pixel spanning a whole cell: the lines are continuous, and the
        // scale is gone.
        assert_eq!(resolvable(1.0, width), 0.0);
        // Half a cell per pixel — a line covering half of every cell — is
        // where the fade starts, and it is half way down at three quarters.
        assert_eq!(resolvable(0.5, width), 1.0);
        assert_eq!(resolvable(0.75, width), 0.5);
        // A cell far wider than a pixel is untouched.
        assert_eq!(resolvable(0.001, width), 1.0);

        // It is the *product* that decides, so a wider line merges sooner: two
        // pixels of line in a two-pixel cell is the same sheet.
        assert_eq!(resolvable(0.5, 2.0), 0.0);
    }

    /// The density term is applied, not merely computed: a coverage of 1 at a
    /// derivative past the merge point still comes out as nothing.
    #[test]
    fn scale_coverage_applies_the_density_term() {
        let width = 1.0;
        let on_a_line = Vec2::ZERO;
        assert_eq!(
            scale_coverage(on_a_line, Vec2::splat(0.001), width),
            1.0,
            "a resolvable line is fully covered"
        );
        assert_eq!(
            scale_coverage(on_a_line, Vec2::splat(1.0), width),
            0.0,
            "a merged scale draws nothing even on a line"
        );
    }

    /// The axes combine with `max`, so a crossing is no brighter than a line.
    #[test]
    fn a_crossing_is_no_brighter_than_a_line() {
        let (derivative, width) = (Vec2::splat(0.01), 1.0);
        let on_one_line = scale_coverage(Vec2::new(0.0, 0.5), derivative, width);
        let on_a_crossing = scale_coverage(Vec2::ZERO, derivative, width);
        assert_eq!(on_a_crossing, on_one_line);
    }

    // -----------------------------------------------------------------------
    // Fade and composite
    // -----------------------------------------------------------------------

    /// The fade is a ramp to nothing at the fade distance, with no edge past
    /// it.
    #[test]
    fn the_fade_reaches_zero_at_the_fade_distance() {
        assert_eq!(distance_fade(0.0, 100.0), 1.0);
        assert_eq!(distance_fade(50.0, 100.0), 0.5);
        assert_eq!(distance_fade(100.0, 100.0), 0.0);
        assert_eq!(distance_fade(1000.0, 100.0), 0.0);
    }

    /// The fragment is premultiplied: every channel is inside its own alpha, so
    /// `src + dst * (1 - a)` composites it correctly.
    #[test]
    fn the_composite_is_premultiplied() {
        let style = GridStyle::default();
        for (fine, coarse, fade) in [
            (1.0f32, 1.0f32, 1.0f32),
            (1.0, 0.0, 1.0),
            (0.0, 1.0, 0.5),
            (0.4, 0.7, 0.9),
        ] {
            let [r, g, b, a] = composite(&style, fine, coarse, fade);
            assert!((0.0..=1.0).contains(&a), "alpha {a} left 0..=1");
            for channel in [r, g, b] {
                assert!(
                    channel <= a + 1e-6,
                    "channel {channel} exceeds its own alpha {a}, so it is not premultiplied"
                );
            }
        }
        // Nothing covered is nothing drawn — the fragment the shader discards.
        assert_eq!(composite(&style, 0.0, 0.0, 1.0), [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(composite(&style, 1.0, 1.0, 0.0), [0.0, 0.0, 0.0, 0.0]);
    }

    /// The coarse lines go over the fine ones: where both are covered, the
    /// result is the coarse colour's, not a sum brighter than either.
    #[test]
    fn the_coarse_lines_composite_over_the_fine_ones() {
        let style = GridStyle {
            fine_color: [1.0, 0.0, 0.0, 1.0],
            coarse_color: [0.0, 1.0, 0.0, 1.0],
            ..GridStyle::default()
        };
        // Both fully covered, and the coarse scale is opaque: the fine lines
        // are entirely hidden.
        assert_eq!(composite(&style, 1.0, 1.0, 1.0), [0.0, 1.0, 0.0, 1.0]);
        // Half-opaque coarse lines let exactly half the fine ones through.
        let style = GridStyle {
            coarse_color: [0.0, 1.0, 0.0, 0.5],
            ..style
        };
        assert_eq!(composite(&style, 1.0, 1.0, 1.0), [0.5, 0.5, 0.0, 1.0]);
    }

    // -----------------------------------------------------------------------
    // The block, and the shader it mirrors
    // -----------------------------------------------------------------------

    /// The block the shader declares, member for member and in this order.
    ///
    /// The offsets [`params_bytes`] writes are what make this the check that
    /// the *shader* agrees about which matrix comes first — swapping them
    /// produces a grid drawn in a camera nobody is looking through, which is
    /// still a picture.
    #[test]
    fn the_uniform_block_matches_the_struct_grid_slang_declares() {
        let declarations = [
            "float4x4 inv_view_proj;",
            "float4x4 view_proj;",
            "float4 params;",
            "float4 fine_color;",
            "float4 coarse_color;",
        ];
        let mut last = 0;
        for declaration in declarations {
            let at = SOURCE
                .find(declaration)
                .unwrap_or_else(|| panic!("grid.slang does not declare `{declaration}`"));
            assert!(
                at > last,
                "grid.slang declares `{declaration}` in a different order than params_bytes \
                 writes it"
            );
            last = at;
        }
    }

    /// The constants the two halves share, checked against the shader text.
    ///
    /// Nothing else can catch a drift here: the shader compiles either way, and
    /// a mismatch shows up only as a grid that behaves differently from the one
    /// these tests describe.
    #[test]
    fn the_constants_match_the_ones_grid_slang_declares() {
        for declaration in [
            format!("static const float PARALLEL_EPSILON = {PARALLEL_EPSILON:e};"),
            format!("static const float DERIVATIVE_FLOOR = {DERIVATIVE_FLOOR:e};"),
        ] {
            assert!(
                SOURCE.contains(&declaration),
                "grid.slang does not declare `{declaration}`"
            );
        }
    }

    /// The depth the shader unprojects its ray origin at is the seam's near
    /// plane.
    ///
    /// Reversed-Z sends the far plane to `w = 0`, so unprojecting there divides
    /// by nothing — the near plane is the end of the ray that is always safe,
    /// and it is only the near plane while `depth::NEAR` says so.
    #[test]
    fn the_ray_origin_is_unprojected_at_the_seams_near_plane() {
        let declaration = format!(
            "static const float NEAR_DEPTH = {:.1};",
            crcbl_hal::depth::NEAR
        );
        assert!(
            SOURCE.contains(&declaration),
            "grid.slang does not declare `{declaration}`"
        );
    }

    /// The block is two matrices and three rows, in that order, little-endian.
    #[test]
    fn the_block_is_two_matrices_and_three_rows() {
        let style = GridStyle {
            spacing: 2.0,
            coarse_multiple: 5.0,
            line_width: 3.0,
            fade_distance: 7.0,
            fine_color: [0.1, 0.2, 0.3, 0.4],
            coarse_color: [0.5, 0.6, 0.7, 0.8],
        };
        let bytes = params_bytes(Mat4::IDENTITY, Mat4::from_scale(Vec3::splat(2.0)), &style);

        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes(), "inv_view_proj[0][0]");
        assert_eq!(&bytes[64..68], &2.0f32.to_le_bytes(), "view_proj[0][0]");
        assert_eq!(&bytes[128..132], &2.0f32.to_le_bytes(), "spacing");
        assert_eq!(&bytes[132..136], &5.0f32.to_le_bytes(), "coarse_multiple");
        assert_eq!(&bytes[136..140], &3.0f32.to_le_bytes(), "line_width");
        assert_eq!(&bytes[140..144], &7.0f32.to_le_bytes(), "fade_distance");
        assert_eq!(&bytes[144..148], &0.1f32.to_le_bytes(), "fine_color.r");
        assert_eq!(&bytes[160..164], &0.5f32.to_le_bytes(), "coarse_color.r");
        assert_eq!(&bytes[172..176], &0.8f32.to_le_bytes(), "coarse_color.a");
    }

    // -----------------------------------------------------------------------
    // The pass
    // -----------------------------------------------------------------------

    /// **The pass writes the two matrices the way round its arguments say.**
    ///
    /// The block's own layout is [`the_block_is_two_matrices_and_three_rows`]'s
    /// and the shader's is
    /// [`the_uniform_block_matches_the_struct_grid_slang_declares`]'s; what
    /// neither can see is the one line inside [`Grid::add_pass`] that hands them
    /// to [`params_bytes`]. Swapped there, the grid is drawn through a camera
    /// nobody is looking through — which is still a picture, and still passes
    /// every other test in this file.
    ///
    /// So this runs the recorded pass body and reads the uniform buffer back off
    /// the null recorder, which is the only place in the tree that keeps what a
    /// host write put in one.
    #[test]
    fn the_pass_writes_the_inverse_first_and_the_forward_matrix_second() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let grid = Grid::new(device, 1, Format::Rgba16Float, Format::D32Float)
            .expect("the null backend builds every pipeline");

        // Two matrices that are each other's inverse and are plainly not equal,
        // so reading one where the other belongs cannot pass by coincidence.
        let view_proj = Mat4::from_scale_rotation_translation(
            Vec3::new(1.5, 0.75, 2.0),
            glam::Quat::from_rotation_y(0.7),
            Vec3::new(3.0, -2.0, 11.0),
        );
        let inv_view_proj = view_proj.inverse();

        let mut pool = TransientPool::new();
        let mut graph = RenderGraph::new(queue);
        let color = graph.create_image(
            "scene-color",
            TransientImageDesc::new(
                (64, 48),
                Format::Rgba16Float,
                ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            ),
        );
        let depth = graph.create_image(
            "scene-depth",
            TransientImageDesc::new(
                (64, 48),
                Format::D32Float,
                ImageUsage::DEPTH_STENCIL_ATTACHMENT | ImageUsage::SAMPLED,
            ),
        );
        grid.add_pass(&mut graph, 0, color, depth, view_proj, inv_view_proj);

        let compiled = graph.compile(&pool).expect("the graph compiles");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("grid block"),
            queue,
        });
        compiled
            .execute(device, &mut pool, encoder.as_mut(), None)
            .expect("the graph executed");

        let written = recorder
            .buffer_bytes(grid.uniforms[0])
            .expect("the pass body wrote the block");
        assert_eq!(
            &written[..64],
            &inv_view_proj
                .to_cols_array()
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<u8>>()[..],
            "the inverse view-projection is the block's first member",
        );
        assert_eq!(
            &written[64..128],
            &view_proj
                .to_cols_array()
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<u8>>()[..],
            "the forward view-projection is the block's second member",
        );

        drop(encoder);
        grid.destroy(device);
        pool.destroy(device);
    }

    /// The pass attaches the colour target and reads the depth without writing
    /// it — which is the whole of what "occluded by the scene, invisible to it"
    /// means at the graph level.
    #[test]
    fn the_pass_loads_its_target_and_only_reads_depth() {
        let (_recorder, device, queue) = open();
        let grid = Grid::new(device.as_ref(), 2, Format::Rgba16Float, Format::D32Float)
            .expect("the null backend builds every pipeline");

        let mut pool = TransientPool::new();
        let mut graph = RenderGraph::new(queue);
        let color = graph.create_image(
            "scene-color",
            TransientImageDesc::new(
                (64, 48),
                Format::Rgba16Float,
                ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            ),
        );
        let depth = graph.create_image(
            "scene-depth",
            TransientImageDesc::new(
                (64, 48),
                Format::D32Float,
                ImageUsage::DEPTH_STENCIL_ATTACHMENT | ImageUsage::SAMPLED,
            ),
        );
        grid.add_pass(&mut graph, 0, color, depth, Mat4::IDENTITY, Mat4::IDENTITY);

        let compiled = graph.compile(&pool).expect("the graph compiles");
        assert_eq!(compiled.passes().len(), 1);
        let barriers = compiled.passes()[0].barriers();
        let depth_barrier = barriers
            .images
            .iter()
            .find(|barrier| barrier.to == ResourceState::DepthStencilRead)
            .expect("the depth image is attached read-only");
        assert_eq!(depth_barrier.to, ResourceState::DepthStencilRead);
        assert!(
            barriers
                .images
                .iter()
                .all(|barrier| barrier.to != ResourceState::DepthStencilWrite),
            "the grid must never take a writable depth attachment"
        );

        drop(compiled);
        grid.destroy(device.as_ref());
        pool.destroy(device.as_ref());
    }

    /// Every frame slot gets its own block and its own group, so a frame in
    /// flight cannot have last frame's camera overwritten under it.
    #[test]
    fn the_block_is_a_ring() {
        let (_recorder, device, _queue) = open();
        let grid = Grid::new(device.as_ref(), 3, Format::Rgba16Float, Format::D32Float)
            .expect("the null backend builds every pipeline");
        assert_eq!(grid.uniforms.len(), 3);
        assert_eq!(grid.groups.len(), 3);
        for (slot, buffer) in grid.uniforms.iter().enumerate() {
            for other in &grid.uniforms[slot + 1..] {
                assert_ne!(buffer, other, "two frames share a block");
            }
        }
        for (slot, group) in grid.groups.iter().enumerate() {
            for other in &grid.groups[slot + 1..] {
                assert_ne!(group, other, "two frames share a group");
            }
        }
        grid.destroy(device.as_ref());
    }

    /// The style is what the block carries, so setting it is visible in the
    /// bytes rather than only in the field.
    #[test]
    fn the_style_reaches_the_block() {
        let (_recorder, device, _queue) = open();
        let mut grid = Grid::new(device.as_ref(), 1, Format::Rgba16Float, Format::D32Float)
            .expect("the null backend builds every pipeline");
        assert_eq!(grid.style().spacing, 1.0);
        grid.set_style(GridStyle {
            spacing: 0.25,
            ..GridStyle::default()
        });
        let bytes = params_bytes(Mat4::IDENTITY, Mat4::IDENTITY, grid.style());
        assert_eq!(&bytes[128..132], &0.25f32.to_le_bytes());
        grid.destroy(device.as_ref());
    }
}
