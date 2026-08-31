//! The debug draw layer: an immediate-mode buffer of world-space line segments,
//! drawn pre-tonemap in HDR.
//!
//! ```text
//! any system ──line/aabb/sphere/frustum──▶ DebugDraw's segment buffer
//!                                                │ begin_frame: upload + clear
//!                                                ▼
//!                          scene colour ──▶ debug-draw ──▶ scene colour
//!                                                ▲
//!                                  scene depth (tested, never written)
//! ```
//!
//! `docs/plan/07-ui-debug.md` item 5 is what this is, and
//! `docs/plan/43-render-standards.md`'s foundations block is where it was
//! scheduled: the layer the four owed views —
//! `docs/plan/45-shadows.md`'s cascade overlay and atlas view,
//! `docs/plan/25-lod.md`'s cluster bounds and `docs/plan/44-lighting.md`'s light
//! reach — are drawn through.
//!
//! **World-anchored text is not here**, and it is not deferred silently: text
//! needs a glyph atlas and a rasteriser seam, `crcbl_ui::text::FontAtlas` and
//! [`crate::ui_pass`] already own one between them, and folding a second
//! consumer of it into this module would double the diff and put a screen-space
//! pass and a world-space one in one file. `docs/backlog.md` carries what the
//! slice needs. Everything this module offers is geometry.
//!
//! # Immediate mode, on [`crate::ui_pass`]'s shape
//!
//! A system appends during the frame; the buffer is uploaded and **cleared** by
//! the [`ForwardRenderer::begin_frame`](crate::ForwardRenderer::begin_frame)
//! that opens the next one, so a segment survives exactly the frame it was
//! appended in and nothing has to be un-appended. That is the same ring the UI
//! draw list rides — one host-visible buffer per frame in flight, grown by
//! doubling, and a count committed only after the upload succeeded.
//!
//! # Pre-tonemap, in HDR
//!
//! `docs/plan/18-render-features.md`'s interaction rule fixed this before the
//! layer existed: "Debug overlays (debug draw, gizmos) render pre-tonemap in HDR
//! (they're in the world) except UI-space panels." So a colour appended here is
//! linear radiance and is exposed and tonemapped like geometry — which is what
//! makes a debug line read as *being in the scene* rather than painted over it,
//! and it is the opposite of [`crate::grid`]'s decision, whose own header says
//! why reference chrome goes the other way.
//!
//! [`crate::forward`] is what puts the pass in the frame, and it lands
//! immediately before the tonemap: after the reflection composite, the bloom
//! chain and the auto-exposure histogram, so a debug line neither reflects in a
//! surface, blooms, nor moves the exposure the scene is metered at, and before
//! the tonemap, so it is exposed with everything else.
//!
//! # Nothing is drawn until something asks for it
//!
//! [`r_debug_draw`] is off by default and a frame that appended no segment
//! records **no pass at all** — no pipeline, no buffer, no barrier — which is
//! what makes every golden image in the workspace identical to the one it was
//! blessed as. The pipeline itself is built lazily, on the first frame that has
//! both, so a renderer nobody draws through never pays for it either.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BlendState, BufferDesc,
    BufferHandle, BufferUsage, ColorTargetState, ColorWrites, CompareOp, DepthBias,
    DepthStencilState, Device, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, LoadOp,
    MemoryLocation, MultisampleState, PipelineLayoutDesc, PipelineLayoutHandle, PrimitiveState,
    PrimitiveTopology, ShaderEntry, ShaderModuleDesc, ShaderStages, StoreOp,
    check_portable_storage_buffers,
};
use crcbl_shaders::{DEBUG_DRAW, Stage};
use glam::{Mat4, Vec3, Vec4Swizzles};

use crate::graph::{ImageId, RenderGraph};

crcbl_console::convar! {
    /// Draw the debug-draw layer's segments over the scene, pre-tonemap.
    pub static r_debug_draw: bool = false;
}

/// How many segments each of a sphere's three great circles is drawn with.
///
/// Twenty-four puts a vertex every fifteen degrees, which is where a circle
/// stops reading as a polygon at the sizes a debug sphere is looked at — a
/// light's reach or a cluster's bounding sphere, drawn against geometry rather
/// than on its own. It is also what fixes a sphere's cost:
/// [`DebugDraw::sphere`] appends three times this many segments.
pub const SPHERE_SEGMENTS: usize = 24;

/// The eight corners of a box, in the order [`BOX_EDGES`] indexes them.
///
/// Bit 0 is the x end, bit 1 the y end and bit 2 the z end, so corner `i` takes
/// the maximum on an axis exactly where that axis's bit is set. Written as a
/// rule rather than a table because both callers derive their corners from it —
/// an axis-aligned box from its two extremes, a frustum from the unit cube of
/// normalised device coordinates.
const fn corner_signs(corner: usize) -> [bool; 3] {
    [corner & 1 != 0, corner & 2 != 0, corner & 4 != 0]
}

/// A box's twelve edges, as pairs of [`corner_signs`] indices.
///
/// Four along each axis, which is what makes the list exhaustive: two corners
/// share an edge exactly when their indices differ in one bit.
const BOX_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

/// One endpoint of one segment, matching `debug_draw.slang`'s `DebugVertex`.
///
/// `position.w` is unused — the vertex stage supplies the homogeneous one — and
/// `color` is linear RGBA, written into the HDR scene target.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DebugVertex {
    /// The endpoint in world space, with an unused `w`.
    pub position: [f32; 4],
    /// Linear RGBA, alpha-blended over the scene.
    pub color: [f32; 4],
}

/// Bytes of `debug_draw.slang`'s constant block: one `float4x4`.
const CONSTANTS_SIZE: u64 = 64;

/// Starting size of each frame's segment buffer, in bytes.
const INITIAL_RING_BYTES: u64 = 4096;

/// The size a ring buffer grows to when `needed` bytes no longer fit.
///
/// Doubling rather than fitting exactly, on [`crate::ui_pass`]'s terms: a system
/// that appends a few more segments each frame reallocates a handful of times
/// rather than every frame.
fn grown(needed: u64) -> u64 {
    needed
        .max(INITIAL_RING_BYTES)
        .next_power_of_two()
        .next_multiple_of(256)
}

/// The eight world-space corners of the view volume `view_proj` projects, or
/// [`None`] where that volume has no finite corners.
///
/// **The [`None`] arm is the engine's own camera**, and it is why this is an
/// [`Option`] rather than a comment telling callers what not to pass. A
/// [`Projection::Perspective`](crate::Projection) here is an *infinite*
/// reversed-Z projection — its far plane is degenerate on purpose, which
/// [`crate::cull::Frustum::from_view_projection`] also carries — so the corners
/// at the far end of normalised device coordinates unproject to infinity, and a
/// caller that drew them would append segments with no finite endpoint and see
/// nothing rather than an error. The finite views are the ones the owed overlays
/// are about: a cascade's orthographic box and a spot light's finite
/// perspective.
#[must_use]
pub fn frustum_corners(view_proj: Mat4) -> Option<[Vec3; 8]> {
    let inverse = view_proj.inverse();
    let mut corners = [Vec3::ZERO; 8];
    for (corner, out) in corners.iter_mut().enumerate() {
        let [x, y, z] = corner_signs(corner);
        let ndc = glam::Vec4::new(
            if x { 1.0 } else { -1.0 },
            if y { 1.0 } else { -1.0 },
            // The engine's clip space is reversed-Z over `0.0..=1.0`, so the
            // `z` bit picks the near plane rather than a symmetric extreme.
            if z { 1.0 } else { 0.0 },
            1.0,
        );
        let clip = inverse * ndc;
        // One predicate, not two: a `w` of zero divides to an infinity and a
        // `w` of zero over a zero numerator to a NaN, and neither is finite —
        // so a separate `w == 0.0` arm would be a branch nothing can reach that
        // this one does not.
        *out = clip.xyz() / clip.w;
        if !out.is_finite() {
            return None;
        }
    }
    Some(corners)
}

/// The immediate-mode buffer every system appends to, and the pass that draws
/// it.
///
/// Built by [`ForwardRenderer`](crate::ForwardRenderer), which owns one and
/// hands it out through
/// [`debug_draw`](crate::ForwardRenderer::debug_draw); appending is all a system
/// ever does with it.
#[derive(Debug, Default)]
pub struct DebugDraw {
    /// This frame's segments, two entries per line, cleared by
    /// [`begin_frame`](Self::begin_frame).
    vertices: Vec<DebugVertex>,
    /// The pipeline and the rings, built on the first frame that has something
    /// to draw. [`None`] on a renderer nobody has drawn a segment through, which
    /// is what makes the layer cost nothing at all until it is used.
    gpu: Option<Gpu>,
}

/// Everything the pass owns once there has been a frame worth building it for.
#[derive(Debug)]
struct Gpu {
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the segment buffer, host-visible and rewritten each frame.
    buffers: Vec<BufferHandle>,
    /// `[frame]`: how many **bytes** the buffer above holds.
    capacity: Vec<u64>,
    /// `[frame]`: the constant block, one per frame in flight for the frame
    /// uniforms' reason exactly.
    uniforms: Vec<BufferHandle>,
    /// `[frame]`: the group naming the two buffers above.
    groups: Vec<BindGroupHandle>,
    /// `[frame]`: how many vertices the last [`DebugDraw::begin_frame`] on that
    /// slot uploaded. Zero is what makes [`DebugDraw::add_pass`] record nothing.
    counts: Vec<usize>,
}

impl DebugDraw {
    /// Whether the console has the layer switched on.
    ///
    /// The read half of [`r_debug_draw`], for a system deciding whether to build
    /// its overlay at all: appending while it is off is harmless — the buffer is
    /// cleared and never uploaded — but a system whose geometry costs something
    /// to compute wants to know before it computes it.
    #[must_use]
    pub fn enabled() -> bool {
        r_debug_draw.get_bool()
    }

    /// Appends one world-space segment.
    ///
    /// Every other primitive here is written in terms of this one.
    pub fn line(&mut self, from: Vec3, to: Vec3, color: [f32; 4]) {
        self.vertices.push(DebugVertex {
            position: [from.x, from.y, from.z, 1.0],
            color,
        });
        self.vertices.push(DebugVertex {
            position: [to.x, to.y, to.z, 1.0],
            color,
        });
    }

    /// Appends the twelve edges of the axis-aligned box spanning `min..=max`.
    pub fn aabb(&mut self, min: Vec3, max: Vec3, color: [f32; 4]) {
        let mut corners = [Vec3::ZERO; 8];
        for (corner, out) in corners.iter_mut().enumerate() {
            let [x, y, z] = corner_signs(corner);
            *out = Vec3::new(
                if x { max.x } else { min.x },
                if y { max.y } else { min.y },
                if z { max.z } else { min.z },
            );
        }
        self.box_edges(&corners, color);
    }

    /// Appends the twelve edges of the view volume `view_proj` projects.
    ///
    /// **A view volume with no finite corners appends nothing and says so**, on
    /// [`frustum_corners`]'s terms — which is the engine's own camera, whose
    /// far plane is at infinity. A caller that wants to handle that itself calls
    /// [`frustum_corners`] and [`box_edges`](Self::box_edges).
    pub fn frustum(&mut self, view_proj: Mat4, color: [f32; 4]) {
        let Some(corners) = frustum_corners(view_proj) else {
            crcbl_core::log::error!(
                "debug draw: a frustum whose corners are not finite was not drawn; an infinite \
                 reversed-Z projection has no far plane to draw"
            );
            return;
        };
        self.box_edges(&corners, color);
    }

    /// Appends the twelve edges joining eight corners in [`frustum_corners`]'
    /// order.
    ///
    /// What [`aabb`](Self::aabb) and [`frustum`](Self::frustum) share: a box and
    /// a view volume differ in where their corners come from and in nothing
    /// else.
    pub fn box_edges(&mut self, corners: &[Vec3; 8], color: [f32; 4]) {
        for (from, to) in BOX_EDGES {
            self.line(corners[from], corners[to], color);
        }
    }

    /// Appends three great circles of `radius` about `centre`, one in each
    /// coordinate plane, each of [`SPHERE_SEGMENTS`] segments.
    ///
    /// Three circles rather than a mesh of latitudes and longitudes: what a
    /// debug sphere has to show is where a radius reaches, and three rings show
    /// it from any angle without filling the frame with lines.
    pub fn sphere(&mut self, centre: Vec3, radius: f32, color: [f32; 4]) {
        for plane in 0..3 {
            let mut previous = None;
            for step in 0..=SPHERE_SEGMENTS {
                let angle = std::f32::consts::TAU * (step % SPHERE_SEGMENTS) as f32
                    / SPHERE_SEGMENTS as f32;
                let (sin, cos) = angle.sin_cos();
                let offset = match plane {
                    0 => Vec3::new(cos, sin, 0.0),
                    1 => Vec3::new(0.0, cos, sin),
                    _ => Vec3::new(sin, 0.0, cos),
                };
                let point = centre + offset * radius;
                if let Some(previous) = previous {
                    self.line(previous, point, color);
                }
                previous = Some(point);
            }
        }
    }

    /// How many segments have been appended since the renderer last opened a
    /// frame.
    ///
    /// The observable a check asking "did this system draw anything" wants, and
    /// what says a primitive helper appended the edges it claims to.
    #[must_use]
    pub fn segments(&self) -> usize {
        self.vertices.len() / 2
    }

    /// Uploads this frame's segments and clears the buffer.
    ///
    /// `frames` is how many frames the renderer keeps in flight, and it sizes
    /// the rings the first time there is anything to build them for.
    ///
    /// **Uploads nothing while [`r_debug_draw`] is off**, and nothing on a frame
    /// with no segment in it: the slot's count goes to zero, which is what makes
    /// [`add_pass`](Self::add_pass) record no pass. The CPU buffer is cleared
    /// either way, so a segment lives exactly the frame it was appended in.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the pipeline, a ring buffer or a write was refused.
    /// Nothing is left half-built: a failed first build releases what it
    /// created and the layer stays unbuilt.
    pub(crate) fn begin_frame(
        &mut self,
        device: &dyn Device,
        frames: usize,
        frame: usize,
        view_proj: Mat4,
    ) -> Result<(), HalError> {
        let drawing = Self::enabled() && !self.vertices.is_empty();
        if !drawing {
            self.vertices.clear();
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.counts[frame] = 0;
            }
            return Ok(());
        }
        let gpu = match self.gpu.as_mut() {
            Some(gpu) => gpu,
            None => self.gpu.insert(Gpu::new(device, frames)?),
        };
        gpu.upload(device, frame, &self.vertices, view_proj)?;
        self.vertices.clear();
        Ok(())
    }

    /// Whether the slot `frame` has anything for [`add_pass`](Self::add_pass) to
    /// record.
    pub(crate) fn records_pass(&self, frame: usize) -> bool {
        self.gpu.as_ref().is_some_and(|gpu| gpu.counts[frame] != 0)
    }

    /// Adds the pass that draws this frame's segments over `target`, tested
    /// against `depth`.
    ///
    /// Records **nothing** where the slot uploaded no segment, which is the
    /// whole of the layer's off switch — see the module docs.
    pub(crate) fn add_pass<'a>(
        &'a self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        target: ImageId,
        depth: ImageId,
    ) {
        let Some(gpu) = self.gpu.as_ref().filter(|gpu| gpu.counts[frame] != 0) else {
            return;
        };
        let pipeline = gpu.pipeline;
        let pipeline_layout = gpu.pipeline_layout;
        let group = gpu.groups[frame];
        let count = gpu.counts[frame] as u32;

        graph
            .add_render_pass("debug-draw")
            // Loaded and stored: the segments composite over the frame the
            // scene passes drew, in the HDR values they wrote.
            .color(target, LoadOp::Load, StoreOp::Store, Default::default())
            // Read-only, on [`crate::grid`]'s terms: attached so the hardware
            // depth test runs, declared this way so the graph moves it to a
            // depth-read state rather than one that would let this pass write
            // it.
            .depth_read(depth)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                encoder.draw(0..count, 0..1);
            });
    }

    /// The most passes this layer adds to one frame.
    ///
    /// The most rather than the count: a frame with nothing appended adds none.
    /// What a caller sizing [`PassTimers`](crate::timing::PassTimers) adds up.
    ///
    /// [`ForwardRenderer::add_passes`](crate::ForwardRenderer::add_passes) is
    /// what records it.
    pub const MAX_PASSES: u32 = 1;

    /// Releases everything, in dependency order. The device must be idle.
    ///
    /// A layer nobody drew through has nothing to release.
    pub(crate) fn destroy(self, device: &dyn Device) {
        if let Some(gpu) = self.gpu {
            gpu.destroy(device);
        }
    }
}

impl Gpu {
    /// Builds the pipeline and one ring per frame in flight.
    fn new(device: &dyn Device, frames: usize) -> Result<Self, HalError> {
        let mut rollback = Rollback::default();
        match Self::build(device, frames, &mut rollback) {
            Ok(gpu) => Ok(gpu),
            Err(error) => {
                rollback.run(device);
                Err(error)
            }
        }
    }

    /// The body of [`Gpu::new`], recording what it has created into `rollback`
    /// as it goes.
    fn build(
        device: &dyn Device,
        frames: usize,
        rollback: &mut Rollback,
    ) -> Result<Self, HalError> {
        // Both `VERTEX`, and declared in binding order — see
        // `crcbl_shaders::declaration_order`, and [`crate::ui_pass`], whose
        // storage buffer and constant block sit at the same two stages.
        let entries = [
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
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let desc = BindGroupLayoutDesc {
            label: Some("debug draw"),
            entries: &entries,
        };
        check_portable_storage_buffers(Some("debug draw"), &[&desc])?;
        let layout = device.create_bind_group_layout(&desc)?;
        rollback.bind_group_layouts.push(layout);

        let set_layouts = [layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("debug draw"),
            bind_group_layouts: &set_layouts,
            // No range at all, for [`crate::ui_pass`]'s reason: the block
            // arrives through the bind group so there is one code path on every
            // backend, including the one with no push constants.
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(pipeline_layout);

        let mut buffers = Vec::with_capacity(frames);
        let mut capacity = Vec::with_capacity(frames);
        let mut uniforms = Vec::with_capacity(frames);
        let mut groups = Vec::with_capacity(frames);
        for _ in 0..frames {
            let segments = device.create_buffer(&BufferDesc {
                label: Some("debug draw segments"),
                size: INITIAL_RING_BYTES,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(segments);
            let block = device.create_buffer(&BufferDesc {
                label: Some("debug draw camera"),
                size: CONSTANTS_SIZE,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(block);
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("debug draw frame"),
                layout,
                entries: &frame_entries(segments, block),
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            buffers.push(segments);
            capacity.push(INITIAL_RING_BYTES);
            uniforms.push(block);
            groups.push(group);
        }

        // Entry points resolved before the module exists, for
        // [`crate::ui_pass`]'s reason: a manifest that disagreed with the
        // artifact would otherwise fail inside the descriptor literal, with the
        // module already created and nothing holding it.
        let vertex = entry(Stage::Vertex)?;
        let fragment = entry(Stage::Fragment)?;
        let module = device.create_shader_module(&ShaderModuleDesc {
            label: Some(DEBUG_DRAW.source()),
            spirv: DEBUG_DRAW.spirv(),
            wgsl: DEBUG_DRAW.wgsl(),
            msl: DEBUG_DRAW.msl(),
            // A container per entry point, for the reason [`crate::forward`]'s
            // mesh module gives.
            dxil: &DEBUG_DRAW.dxil_containers(),
        })?;
        let targets = [ColorTargetState {
            format: crate::forward::SCENE_COLOR_FORMAT,
            // Straight alpha, as [`crate::ui_pass`] composites: the shader emits
            // the colour it was appended with and the blender weighs it.
            blend: Some(BlendState::alpha()),
            write_mask: ColorWrites::ALL,
        }];
        let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("debug draw"),
            layout: pipeline_layout,
            vertex: ShaderEntry {
                module,
                entry_point: vertex,
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: fragment,
            }),
            primitive: PrimitiveState {
                // Two consecutive vertices are one segment, which is what every
                // primitive in this module is decomposed into.
                topology: PrimitiveTopology::LineList,
                ..PrimitiveState::default()
            },
            depth_stencil: Some(DepthStencilState {
                format: crate::forward::SCENE_DEPTH_FORMAT,
                // **Tested, never written**, on [`crate::grid`]'s terms: an
                // overlay is occluded by the geometry in front of it and must
                // not occlude anything drawn after it.
                depth_write: false,
                // `Greater` under this engine's reversed-Z, so a segment exactly
                // on a surface the scene already drew loses to it.
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
            buffers,
            capacity,
            uniforms,
            groups,
            counts: vec![0; frames],
        })
    }

    /// Writes `vertices` and the camera into slot `frame`, growing the ring
    /// where this frame no longer fits.
    fn upload(
        &mut self,
        device: &dyn Device,
        frame: usize,
        vertices: &[DebugVertex],
        view_proj: Mat4,
    ) -> Result<(), HalError> {
        let needed = std::mem::size_of_val(vertices) as u64;
        if needed > self.capacity[frame] {
            let size = grown(needed);
            // Create before destroying, on [`crate::ui_pass`]'s terms: a
            // creation that fails must not leave a destroyed handle in the
            // struct for `destroy` to hand back to the device a second time.
            let fresh = device.create_buffer(&BufferDesc {
                label: Some("debug draw segments"),
                size,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
            device.destroy_buffer(std::mem::replace(&mut self.buffers[frame], fresh));
            self.capacity[frame] = size;
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("debug draw frame"),
                layout: self.layout,
                entries: &frame_entries(self.buffers[frame], self.uniforms[frame]),
                variable_count: None,
            })?;
            device.destroy_bind_group(std::mem::replace(&mut self.groups[frame], group));
        }
        device.write_buffer(self.buffers[frame], 0, bytemuck::cast_slice(vertices))?;
        device.write_buffer(
            self.uniforms[frame],
            0,
            bytemuck::cast_slice(&view_proj.to_cols_array()),
        )?;
        // Committed only after both writes succeeded, on [`crate::ui_pass`]'s
        // terms: a count ahead of its bytes is a vertex stage reading past what
        // was written.
        self.counts[frame] = vertices.len();
        Ok(())
    }

    /// Releases everything, in dependency order.
    fn destroy(self, device: &dyn Device) {
        for group in self.groups {
            device.destroy_bind_group(group);
        }
        for buffer in self.buffers {
            device.destroy_buffer(buffer);
        }
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.layout);
    }
}

/// The two entries every frame's group names.
fn frame_entries(segments: BufferHandle, block: BufferHandle) -> [BindGroupEntry; 2] {
    [
        BindGroupEntry {
            binding: 0,
            array_index: 0,
            resource: BindingResource::whole_buffer(segments),
        },
        BindGroupEntry {
            binding: 1,
            array_index: 0,
            resource: BindingResource::whole_buffer(block),
        },
    ]
}

/// `debug_draw.slang`'s entry point for `stage`.
fn entry(stage: Stage) -> Result<&'static str, HalError> {
    DEBUG_DRAW.entry_point(stage).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "debug_draw.slang exposes no unambiguous {stage:?} entry point; the committed SPIR-V \
             and its manifest disagree, which crates/crcbl-shaders/tools/compile-shaders.sh would \
             fix"
        ))
    })
}

/// What a partly-built [`Gpu`] has to give back.
///
/// [`Gpu::build`] creates a handful of objects with `?` between them and the
/// seam's `destroy_*` is explicit, so a failure half way through would otherwise
/// leak everything created before it. [`crate::grid`] carries the same shape for
/// the same reason.
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
    use std::collections::BTreeSet;

    /// Eight corners no two of which are near each other, so a position in the
    /// appended buffer identifies the corner it came from.
    fn marked_corners() -> [Vec3; 8] {
        let mut corners = [Vec3::ZERO; 8];
        for (corner, out) in corners.iter_mut().enumerate() {
            let [x, y, z] = corner_signs(corner);
            *out = Vec3::new(
                if x { 10.0 } else { -10.0 },
                if y { 20.0 } else { -20.0 },
                if z { 30.0 } else { -30.0 },
            );
        }
        corners
    }

    /// Every appended segment as the pair of `corners` indices it joins, lowest
    /// first.
    ///
    /// Panics rather than skipping when an endpoint is not one of `corners`: a
    /// segment this could not place is the failure the caller is asking about,
    /// and dropping it would make the set look right by being smaller.
    fn joined(draw: &DebugDraw, corners: &[Vec3; 8]) -> BTreeSet<(usize, usize)> {
        let index = |vertex: &DebugVertex| {
            let at = Vec3::new(vertex.position[0], vertex.position[1], vertex.position[2]);
            corners
                .iter()
                .position(|corner| (*corner - at).length() < 1.0e-4)
                .unwrap_or_else(|| panic!("{at:?} is not one of the corners"))
        };
        draw.vertices
            .chunks_exact(2)
            .map(|pair| {
                let (from, to) = (index(&pair[0]), index(&pair[1]));
                (from.min(to), from.max(to))
            })
            .collect()
    }

    /// The four corners of the face of a box that takes `end` on `axis`, and the
    /// four edges around it.
    fn face_edges(axis: usize, end: bool) -> BTreeSet<(usize, usize)> {
        let on_face: Vec<usize> = (0..8)
            .filter(|corner| corner_signs(*corner)[axis] == end)
            .collect();
        let mut edges = BTreeSet::new();
        for &from in &on_face {
            for &to in &on_face {
                // One bit apart is one edge; the two corners diagonally across
                // the face differ in two.
                if from < to && (from ^ to).count_ones() == 1 {
                    edges.insert((from, to));
                }
            }
        }
        edges
    }

    /// **A line is its two endpoints, in the colour it was appended with.**
    #[test]
    fn a_line_is_two_endpoints_in_the_colour_it_was_appended_with() {
        let mut draw = DebugDraw::default();
        draw.line(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(4.0, 5.0, 6.0),
            [0.25, 0.5, 0.75, 1.0],
        );
        assert_eq!(draw.segments(), 1);
        assert_eq!(draw.vertices[0].position, [1.0, 2.0, 3.0, 1.0]);
        assert_eq!(draw.vertices[1].position, [4.0, 5.0, 6.0, 1.0]);
        for vertex in &draw.vertices {
            assert_eq!(vertex.color, [0.25, 0.5, 0.75, 1.0]);
        }
    }

    /// **A box is twelve edges, and they are exactly the pairs of corners that
    /// differ on one axis** — so the six faces are all closed.
    ///
    /// Twelve *and* which twelve: a helper that appended the same edge twice, or
    /// joined the two corners of a diagonal, would satisfy a count.
    #[test]
    fn a_box_is_the_twelve_edges_that_close_its_six_faces() {
        let corners = marked_corners();
        let mut draw = DebugDraw::default();
        draw.box_edges(&corners, [1.0; 4]);

        assert_eq!(draw.segments(), BOX_EDGES.len());
        let drawn = joined(&draw, &corners);
        assert_eq!(
            drawn.len(),
            BOX_EDGES.len(),
            "an edge was appended twice: {drawn:?}"
        );
        for axis in 0..3 {
            for end in [false, true] {
                let face = face_edges(axis, end);
                assert_eq!(face.len(), 4, "a face of a box has four edges");
                assert!(
                    face.is_subset(&drawn),
                    "the face at axis {axis} end {end} is not closed: {face:?} against {drawn:?}"
                );
            }
        }
    }

    /// **An axis-aligned box's corners are its two extremes in every
    /// combination**, which is what makes [`DebugDraw::aabb`] the box a caller
    /// asked for rather than one of the right shape somewhere else.
    #[test]
    fn an_aabb_spans_the_two_extremes_it_was_given() {
        let (min, max) = (Vec3::new(-1.0, 2.0, -3.0), Vec3::new(4.0, 5.0, 6.0));
        let mut draw = DebugDraw::default();
        draw.aabb(min, max, [1.0; 4]);
        assert_eq!(draw.segments(), BOX_EDGES.len());
        for vertex in &draw.vertices {
            let at = Vec3::new(vertex.position[0], vertex.position[1], vertex.position[2]);
            for axis in 0..3 {
                assert!(
                    at[axis] == min[axis] || at[axis] == max[axis],
                    "{at:?} is off the box spanning {min:?}..={max:?}"
                );
            }
        }
        assert!(
            draw.vertices
                .iter()
                .any(|vertex| vertex.position[0] == min.x),
            "no endpoint reached the low x face"
        );
        assert!(
            draw.vertices
                .iter()
                .any(|vertex| vertex.position[0] == max.x),
            "no endpoint reached the high x face"
        );
    }

    /// **A frustum is the world-space box its view-projection maps onto
    /// normalised device coordinates**, corner for corner.
    ///
    /// The matrix here is a scale and a translate chosen so the answer is
    /// arithmetic rather than a second implementation of the unprojection: the
    /// world box `[-2,2] × [-3,3] × [1,5]` maps onto `x,y ∈ [-1,1]` and the
    /// engine's reversed-Z `z ∈ [0,1]`, with the near plane — `z = 1` — at
    /// `world z = 1`.
    #[test]
    fn a_frustum_is_the_world_box_its_view_projection_maps_onto_clip_space() {
        let view_proj = Mat4::from_translation(Vec3::new(0.0, 0.0, 1.25))
            * Mat4::from_scale(Vec3::new(0.5, 1.0 / 3.0, -0.25));
        let corners = frustum_corners(view_proj).expect("a finite view volume has corners");
        for (corner, at) in corners.iter().enumerate() {
            let [x, y, z] = corner_signs(corner);
            let expected = Vec3::new(
                if x { 2.0 } else { -2.0 },
                if y { 3.0 } else { -3.0 },
                // The `z` bit is the near plane under reversed-Z, which this
                // matrix puts at `world z = 1`; the far end is at `5`.
                if z { 1.0 } else { 5.0 },
            );
            assert!(
                (*at - expected).length() < 1.0e-4,
                "corner {corner} came back at {at:?} rather than {expected:?}"
            );
        }

        let mut draw = DebugDraw::default();
        draw.frustum(view_proj, [1.0; 4]);
        assert_eq!(draw.segments(), BOX_EDGES.len());
        let drawn = joined(&draw, &corners);
        for axis in 0..3 {
            for end in [false, true] {
                assert!(
                    face_edges(axis, end).is_subset(&drawn),
                    "the frustum's face at axis {axis} end {end} is not closed"
                );
            }
        }
    }

    /// **The engine's own camera has no drawable frustum, and it is refused
    /// rather than drawn.**
    ///
    /// [`crate::Projection`]'s perspective is an infinite reversed-Z one — its
    /// far plane is degenerate on purpose — so the far corners unproject to
    /// infinity. A caller that appended them would see nothing and have nothing
    /// to read.
    #[test]
    fn a_view_volume_with_no_finite_corners_is_refused() {
        let camera = crate::Camera::default();
        let view_proj = camera.view_projection(16.0 / 9.0);
        assert!(
            frustum_corners(view_proj).is_none(),
            "an infinite reversed-Z projection has no far corners to draw"
        );

        let mut draw = DebugDraw::default();
        draw.frustum(view_proj, [1.0; 4]);
        assert_eq!(
            draw.segments(),
            0,
            "a frustum with no finite corners must append nothing"
        );
    }

    /// **A sphere is three closed great circles at the radius it was given.**
    ///
    /// Every endpoint at the radius is the claim a segment count cannot make: a
    /// helper that stepped the angle wrongly, or scaled one ring, draws the
    /// right number of segments in the wrong place.
    #[test]
    fn a_sphere_is_three_closed_great_circles_at_its_radius() {
        let (centre, radius) = (Vec3::new(1.0, -2.0, 3.0), 4.0);
        let mut draw = DebugDraw::default();
        draw.sphere(centre, radius, [1.0; 4]);
        assert_eq!(draw.segments(), 3 * SPHERE_SEGMENTS);
        for vertex in &draw.vertices {
            let at = Vec3::new(vertex.position[0], vertex.position[1], vertex.position[2]);
            assert!(
                ((at - centre).length() - radius).abs() < 1.0e-3,
                "{at:?} is {} from the centre, not {radius}",
                (at - centre).length()
            );
        }
        // Each ring closes: its last endpoint is its first. Written as a
        // per-ring check rather than over the whole buffer, because three rings
        // that all ended where the *first* one started would satisfy anything
        // looser.
        for ring in 0..3 {
            let first = ring * SPHERE_SEGMENTS * 2;
            let last = first + SPHERE_SEGMENTS * 2 - 1;
            assert_eq!(
                draw.vertices[first].position, draw.vertices[last].position,
                "ring {ring} does not close"
            );
        }
    }
}
