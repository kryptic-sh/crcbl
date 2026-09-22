//! `docs/plan/55-water.md` rung 1: bodies of water drawn over the opaque frame.
//!
//! ```text
//!          ┌─────────── the HDR frame ───────────┐   ┌── scene-depth ──┐
//!          ▼                                     │   ▼                 │
//! ssr-blur ──▶ water-copy ──▶ water-color ──┐    │   water-depth ──┐   │
//!                                           ▼    ▼                 ▼   ▼
//!                                         water (draws each body into both) ──▶ bloom …
//! ```
//!
//! Two halves, owned in two places, for `crate::forward::view`'s line between
//! the scene and a camera:
//!
//! * [`WaterBodies`] is the **scene's**: the bodies a caller set, meshed once by
//!   [`crcbl_water::surface_mesh`], and a ring of host-visible buffers holding
//!   the grid, the indices and one medium row per body. Every view draws the
//!   same water.
//! * [`Water`] is **one camera's**: the two pipelines, its uniform ring and the
//!   bind groups naming its own transients.
//!
//! # Two passes, and why the first is a draw
//!
//! `water-copy` is `crate::hiz`'s construction: a full-screen triangle that
//! `Load`s the HDR frame into a colour target and the opaque depth into a
//! `D32Float` depth target through `SV_Depth`, under an always-passing compare.
//! The graph refuses to attach and sample one image in one pass, so the surface
//! needs a second image of each to read while it draws into the originals. An
//! image-to-image copy would fill them too, and is not used: no depth copy of
//! that kind is exercised on any backend in this tree, and a copy pass has no
//! timer, where `docs/plan/55-water.md` prices each pass.
//!
//! `water` then attaches the HDR frame (loaded and stored) and the scene depth
//! (loaded, stored, **written**) and draws each body's indexed grid under
//! [`CompareOp::Greater`] — reversed-Z, [`crcbl_hal::depth`]'s convention — so a
//! fragment exists only where the surface is nearer than the geometry already
//! there, and afterwards the depth holds the surface for the passes that test
//! against it. `water.slang` writes the whole composite itself, so there is no
//! blend state. Bloom, exposure, the debug draw layer, the tonemap and the
//! overlays read the same two images they always did.
//!
//! # A body is content, not an effect
//!
//! On [`crate::sky_pass`]'s terms: a frame with no body records **no pass**,
//! takes no transient and binds nothing, so it is the frame this renderer drew
//! before this module existed, bit for bit. There is no
//! [`RenderEffects`](crate::RenderEffects) bit to turn water off; removing the
//! bodies is what does.
//!
//! # What is shared with the reflection pass
//!
//! The surface reflects the sky and the probes exactly as `ssr.slang` falls back
//! to them, so it reads the same inputs: [`crate::ssr::Ssr`]'s uniform block
//! and its sky prefilter table, [`crate::sky_pass::SkyPass`]'s sky-view LUT, and
//! the forward pass's probe rows and visibility maps. **Nothing is uploaded a
//! second time.** `Ssr` is built for every view whether or not
//! [`RenderEffects::REFLECTIONS`](crate::RenderEffects::REFLECTIONS) is on, and
//! writes its block every frame either way, so this module reads its handles
//! rather than keeping copies. It does not read the `DFG` pair `Ssr` also
//! uploads: water's Fresnel is Schlick's at a fixed `F0`, which is that table's
//! roughness-zero row by construction.
//!
//! [`CompareOp::Greater`]: crcbl_hal::CompareOp::Greater

use std::ops::Range;

use crcbl_hal::{
    BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, CompareOp, DepthBias, DepthStencilState, Device,
    Format, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageViewHandle, ImageViewType,
    IndexFormat, LoadOp, MemoryLocation, MultisampleState, PipelineLayoutDesc,
    PipelineLayoutHandle, PrimitiveState, ResourceState, SampleType, SamplerHandle, ShaderEntry,
    ShaderModuleDesc, ShaderStages, StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::water::{
    MEDIUM_STRIDE, PARAMS_SIZE, VERTEX_STRIDE, WaterMedium, WaterParams, WaterVertex,
};
use crcbl_shaders::{Stage, WATER, WATER_COPY};
use crcbl_water::{BodyError, WaterBody, surface_mesh};

use crate::graph::{BufferId, ImageId, ImportedBuffer, RenderGraph};
use crate::ssao::cached_group;

/// The grid spacing every body's surface is meshed at, in metres.
///
/// One metre. At rung 1 the surface is flat and the grid's only job is to exist
/// for the rung whose waves displace it, so the spacing is what keeps a pool's
/// vertex count small; the rung that displaces vertices is the one that prices
/// a finer grid against its wavelengths.
pub(crate) const SURFACE_SPACING: f32 = 1.0;

/// Vertices in the over-sized full-screen triangle `water_copy.slang` generates.
const FULLSCREEN_VERTICES: u32 = 3;

/// The bodies a renderer draws, meshed, and the buffers each frame slot draws
/// them from.
///
/// # Uploaded per slot, when the slot comes round
///
/// [`WaterBodies::set`] meshes and stores; it does not touch the device. Each
/// frame slot's buffers are brought up to date by
/// [`WaterBodies::begin_frame`] for that slot, which runs when the frame that
/// last used the slot has finished — the ring's guarantee, and the reason every
/// per-frame block in this crate is a ring. Writing every slot at the moment a
/// caller replaced the bodies would rewrite, or destroy, buffers a submitted
/// frame is still reading.
#[derive(Debug)]
pub(crate) struct WaterBodies {
    /// The bodies as the caller set them.
    bodies: Vec<WaterBody>,
    /// Every body's grid, packed as `water.slang`'s `WaterVertex` rows.
    vertices: Vec<u8>,
    /// Every body's triangles, as absolute indices into [`Self::vertices`].
    indices: Vec<u32>,
    /// One `WaterMedium` row per body, in body order.
    media: Vec<u8>,
    /// Each body's range of [`Self::indices`], in body order.
    draws: Vec<Range<u32>>,
    /// Bumped by every [`WaterBodies::set`], so a slot knows it is stale.
    revision: u64,
    /// `[frame]`: what each slot's buffers hold.
    slots: Vec<Slot>,
}

/// One frame slot's buffers, what they hold, and the revision it was written
/// at.
#[derive(Debug, Default)]
struct Slot {
    /// The revision the slot holds; zero before its first frame, which no
    /// [`WaterBodies::set`] leaves the revision at.
    revision: u64,
    buffers: Option<SlotBuffers>,
    /// Each body's range of the slot's index buffer, **as uploaded** — empty
    /// for a slot whose revision has no water. A frame draws these rather than
    /// the latest set's, so bodies replaced between a frame's start and its
    /// passes cannot draw ranges the slot's buffers do not hold.
    draws: Vec<Range<u32>>,
}

/// One frame slot's three buffers and the bytes each can hold.
#[derive(Clone, Copy, Debug)]
struct SlotBuffers {
    vertices: BufferHandle,
    indices: BufferHandle,
    media: BufferHandle,
    capacity: [u64; 3],
}

/// What one frame draws: the slot's buffers and each body's index range.
#[derive(Clone, Debug)]
pub(crate) struct WaterFrame {
    pub(crate) vertices: BufferHandle,
    pub(crate) indices: BufferHandle,
    pub(crate) media: BufferHandle,
    pub(crate) draws: Vec<Range<u32>>,
}

impl WaterBodies {
    /// No bodies, and `frames` slots with no buffers.
    pub(crate) fn new(frames: usize) -> Self {
        Self {
            bodies: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            media: Vec::new(),
            draws: Vec::new(),
            revision: 0,
            slots: (0..frames).map(|_| Slot::default()).collect(),
        }
    }

    /// Meshes `bodies` and replaces what this draws with them.
    ///
    /// **All or nothing**: every body is meshed before anything is replaced, so
    /// a body that fails leaves the previous set drawing.
    ///
    /// # Errors
    ///
    /// The first body's [`BodyError`] that [`surface_mesh`] refuses, or
    /// [`BodyError::TooManyVertices`] when the bodies together outgrow a `u32`
    /// index.
    pub(crate) fn set(&mut self, bodies: &[WaterBody]) -> Result<(), BodyError> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut media = Vec::with_capacity(bodies.len() * MEDIUM_STRIDE);
        let mut draws = Vec::with_capacity(bodies.len());
        for (body_index, body) in bodies.iter().enumerate() {
            let mesh = surface_mesh(body, SURFACE_SPACING)?;
            let base = index_of(vertices.len() / VERTEX_STRIDE)?;
            let body_row = index_of(body_index)?;
            for position in mesh.positions {
                vertices.extend_from_slice(
                    &WaterVertex {
                        position,
                        body: body_row,
                    }
                    .to_bytes(),
                );
            }
            let first = index_of(indices.len())?;
            for index in mesh.indices {
                indices.push(base.checked_add(index).ok_or(BodyError::TooManyVertices)?);
            }
            draws.push(first..index_of(indices.len())?);
            media.extend_from_slice(
                &WaterMedium {
                    absorption: body.medium.absorption,
                    scattering: body.medium.scattering,
                }
                .to_bytes(),
            );
        }
        self.bodies = bodies.to_vec();
        self.vertices = vertices;
        self.indices = indices;
        self.media = media;
        // A body whose mesh came out empty draws nothing, and a frame of only
        // those is a frame with no water.
        draws.retain(|range| !range.is_empty());
        self.draws = draws;
        self.revision += 1;
        Ok(())
    }

    /// The bodies as they were last set.
    pub(crate) fn bodies(&self) -> &[WaterBody] {
        &self.bodies
    }

    /// Brings `frame`'s buffers up to the bodies last set, if they are behind.
    ///
    /// A buffer that still fits is written in place; one that does not is
    /// replaced, creating the new one before destroying the old so a failure
    /// leaves no destroyed handle behind.
    ///
    /// # Errors
    ///
    /// [`HalError`] from a buffer creation or a mapped write.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn begin_frame(
        &mut self,
        device: &dyn Device,
        frame: usize,
    ) -> Result<(), HalError> {
        let slot = &mut self.slots[frame];
        if slot.revision == self.revision {
            return Ok(());
        }
        if self.draws.is_empty() {
            // No water at this revision: the slot draws nothing, and keeps its
            // buffers for the next set that has some.
            slot.draws.clear();
            slot.revision = self.revision;
            return Ok(());
        }
        let index_bytes: Vec<u8> = self
            .indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        let wanted = [
            self.vertices.len() as u64,
            index_bytes.len() as u64,
            self.media.len() as u64,
        ];
        let buffers = match slot.buffers {
            Some(buffers)
                if buffers
                    .capacity
                    .iter()
                    .zip(wanted)
                    .all(|(have, want)| *have >= want) =>
            {
                buffers
            }
            existing => {
                let created = SlotBuffers::create(device, wanted)?;
                if let Some(old) = existing {
                    old.destroy(device);
                }
                slot.buffers = Some(created);
                created
            }
        };
        device.write_buffer(buffers.vertices, 0, &self.vertices)?;
        device.write_buffer(buffers.indices, 0, &index_bytes)?;
        device.write_buffer(buffers.media, 0, &self.media)?;
        // Committed only after every write succeeded, on `crate::ui_pass`'s
        // terms: ranges ahead of their bytes are a draw reading past what was
        // written.
        slot.draws.clone_from(&self.draws);
        slot.revision = self.revision;
        Ok(())
    }

    /// What `frame` draws, or `None` for a frame with no water.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn frame(&self, frame: usize) -> Option<WaterFrame> {
        let slot = &self.slots[frame];
        if slot.draws.is_empty() {
            return None;
        }
        let buffers = slot.buffers?;
        Some(WaterFrame {
            vertices: buffers.vertices,
            indices: buffers.indices,
            media: buffers.media,
            draws: slot.draws.clone(),
        })
    }

    /// Releases every slot's buffers. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for buffers in self.slots.into_iter().filter_map(|slot| slot.buffers) {
            buffers.destroy(device);
        }
    }
}

impl SlotBuffers {
    /// Three host-visible buffers of the `capacity` bytes each asks for.
    ///
    /// **Nothing is left behind on failure**: a buffer created before one that
    /// fails is destroyed here, so the caller's slot keeps what it had.
    fn create(device: &dyn Device, capacity: [u64; 3]) -> Result<Self, HalError> {
        let descs = [
            ("water vertices", BufferUsage::STORAGE),
            ("water indices", BufferUsage::INDEX),
            ("water media", BufferUsage::STORAGE),
        ];
        let mut created = Vec::with_capacity(descs.len());
        for ((label, usage), size) in descs.into_iter().zip(capacity) {
            match device.create_buffer(&BufferDesc {
                label: Some(label),
                size,
                usage,
                memory: MemoryLocation::HostUpload,
            }) {
                Ok(buffer) => created.push(buffer),
                Err(error) => {
                    for buffer in created {
                        device.destroy_buffer(buffer);
                    }
                    return Err(error);
                }
            }
        }
        Ok(Self {
            vertices: created[0],
            indices: created[1],
            media: created[2],
            capacity,
        })
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_buffer(self.vertices);
        device.destroy_buffer(self.indices);
        device.destroy_buffer(self.media);
    }
}

/// `value` as a `u32` index, or [`BodyError::TooManyVertices`] for a set of
/// bodies whose meshes together outgrow one.
fn index_of(value: usize) -> Result<u32, BodyError> {
    u32::try_from(value).map_err(|_| BodyError::TooManyVertices)
}

/// The graph transients [`Water::add_passes`] reads and writes.
///
/// One struct rather than five positional arguments, on
/// [`SsrImages`](crate::ssr::SsrImages)' terms.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WaterImages {
    /// The HDR frame the passes before water left: copied, then drawn into.
    pub(crate) color: ImageId,
    /// The scene depth: copied, then tested and written.
    pub(crate) depth: ImageId,
    /// Where `water-copy` writes the frame's colour.
    pub(crate) color_copy: ImageId,
    /// Where `water-copy` writes the frame's depth.
    pub(crate) depth_copy: ImageId,
    /// The shadow atlas the frame's shadow pass drew.
    pub(crate) shadow_atlas: ImageId,
}

/// Everything else the surface reads, as handles other passes own.
#[derive(Clone, Debug)]
pub(crate) struct WaterInputs {
    /// This view's `FrameUniforms` slot — the forward pass's block.
    pub(crate) frame_block: BufferHandle,
    /// This view's `SsrParams` slot — [`crate::ssr::Ssr::uniforms`].
    pub(crate) reflection_block: BufferHandle,
    /// This view's froxel column — [`crate::volumetric::Volumetric::buffers`].
    pub(crate) froxels: crate::volumetric::FroxelBuffers,
    /// The column's two buffers as the graph knows them, when this frame's
    /// froxel passes imported them. `None` on a frame that ran no froxel volume,
    /// and then this pass imports them itself so the read is still declared.
    pub(crate) froxel_ids: Option<(BufferId, BufferId)>,
    /// The probe rows and their graph id.
    pub(crate) probes: BufferHandle,
    pub(crate) probe_id: BufferId,
    /// The per-probe visibility maps, or their placeholder.
    pub(crate) probe_visibility: ImageViewHandle,
    /// [`crate::ssr::Ssr::sky_prefilter_view`].
    pub(crate) sky_prefilter: ImageViewHandle,
    /// [`crate::sky_pass::SkyPass::lut`] for this slot.
    pub(crate) sky_view: BufferHandle,
    /// What the scene draws this frame.
    pub(crate) mesh: WaterFrame,
}

/// The bindings of `water.slang`, in its declaration order.
///
/// Named so the bind group below and the layout cannot disagree about a number.
mod binding {
    pub(super) const PARAMS: u32 = 0;
    pub(super) const FRAME: u32 = 1;
    pub(super) const REFLECTION: u32 = 2;
    pub(super) const FROXEL_BLOCK: u32 = 3;
    pub(super) const VERTICES: u32 = 4;
    pub(super) const MEDIA: u32 = 5;
    pub(super) const COLOR: u32 = 6;
    pub(super) const DEPTH: u32 = 7;
    pub(super) const SHADOW_ATLAS: u32 = 8;
    pub(super) const SHADOW_SAMPLER: u32 = 9;
    pub(super) const PROBES: u32 = 10;
    pub(super) const SKY_PREFILTER: u32 = 11;
    pub(super) const PROBE_VISIBILITY: u32 = 12;
    pub(super) const SKY_VIEW: u32 = 13;
    pub(super) const FROXELS: u32 = 14;
    pub(super) const LIGHTING: u32 = 15;
}

/// The surface group cached against the views it names and the mesh buffers,
/// which move when a slot's buffers are replaced.
type SurfaceCache = (
    Option<[BufferHandle; 2]>,
    Option<(Vec<ImageViewHandle>, BindGroupHandle)>,
);

/// Everything one camera's water passes own.
#[derive(Debug)]
pub(crate) struct Water {
    copy_layout: BindGroupLayoutHandle,
    copy_pipeline_layout: PipelineLayoutHandle,
    copy_pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the copy group, cached against its two source views.
    copy_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
    surface_layout: BindGroupLayoutHandle,
    surface_pipeline_layout: PipelineLayoutHandle,
    surface_pipeline: GraphicsPipelineHandle,
    /// `[frame]`: `water.slang`'s own block.
    uniforms: Vec<BufferHandle>,
    /// `[frame]`: the surface group — see [`SurfaceCache`].
    surface_groups: Vec<SurfaceCache>,
    /// The atlas's comparison sampler, which the renderer owns for its life.
    shadow_sampler: SamplerHandle,
}

impl Water {
    /// Passes [`Water::add_passes`] adds to a frame that has water.
    pub(crate) const PASSES: u32 = 2;

    /// Full-screen draws among those passes: `water-copy`'s triangle.
    pub(crate) const FULLSCREEN_PASSES: u64 = 1;

    /// Builds both pipelines and the uniform ring.
    ///
    /// `build_fullscreen_with` is [`crate::forward`]'s, handed in on
    /// [`crate::sky_pass::SkyPass::new`]'s terms; the copy pipeline is its shape
    /// with a colour target **and** a depth target written under an
    /// always-passing compare.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**, for the reason every other builder in this crate gives: the
    /// caller holds a rollback, and this is stored in it whole.
    pub(crate) fn new(
        device: &dyn Device,
        frames: usize,
        shadow_sampler: SamplerHandle,
        build_fullscreen_with: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            &[ColorTargetState],
            Option<DepthStencilState>,
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        let copy_entries = [
            sampled(0, ShaderStages::FRAGMENT, SampleType::Float),
            sampled(1, ShaderStages::FRAGMENT, SampleType::Depth),
        ];
        let copy_desc = BindGroupLayoutDesc {
            label: Some("water copy"),
            entries: &copy_entries,
        };
        check_portable_storage_buffers(Some("water copy"), &[&copy_desc])?;
        let copy_layout = device.create_bind_group_layout(&copy_desc)?;
        let copy_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("water copy"),
            bind_group_layouts: &[copy_layout],
            push_constants: None,
        })?;
        let copy_pipeline = build_fullscreen_with(
            device,
            "water copy",
            &WATER_COPY,
            copy_pipeline_layout,
            &[ColorTargetState::opaque(Format::Rgba16Float)],
            Some(DepthStencilState {
                format: Format::D32Float,
                depth_write: true,
                depth_compare: CompareOp::Always,
                stencil: None,
                bias: DepthBias::default(),
            }),
        )?;

        // **Every binding is visible to both stages**, though the vertex stage
        // reads only the frame block and the vertices and the fragment stage
        // never reads the vertices. Slang's Metal backend materialises every
        // global into every entry point — `msl/water.metal`'s `fragmentMain`
        // takes `water_vertices [[buffer(4)]]` and its `vertexMain` takes every
        // fragment resource — and Metal's draw validation refused the fragment
        // function with "missing buffer binding at index 4 for water_vertices"
        // on CI's macOS runner. `crate::forward`'s DFG binding and
        // `crate::sky_pass`'s sky-view binding take both stages for the same
        // reason; this layout takes it for every binding rather than learning
        // them one CI run at a time, and the storage buffers it puts in the
        // vertex stage stay within the browser's limit, which
        // `check_portable_storage_buffers` below holds it to.
        let both = ShaderStages::VERTEX.union(ShaderStages::FRAGMENT);
        let surface_entries = [
            uniform(binding::PARAMS, both),
            uniform(binding::FRAME, both),
            uniform(binding::REFLECTION, both),
            uniform(binding::FROXEL_BLOCK, both),
            storage(binding::VERTICES, both, VERTEX_STRIDE as u32),
            storage(binding::MEDIA, both, MEDIUM_STRIDE as u32),
            sampled(binding::COLOR, both, SampleType::Float),
            sampled(binding::DEPTH, both, SampleType::Depth),
            sampled(binding::SHADOW_ATLAS, both, SampleType::Depth),
            BindGroupLayoutEntry {
                binding: binding::SHADOW_SAMPLER,
                visibility: both,
                kind: BindingKind::Sampler { comparison: true },
                count: 1,
                flags: BindingFlags::empty(),
            },
            storage(
                binding::PROBES,
                both,
                crcbl_shaders::probe::PROBE_STRIDE as u32,
            ),
            sampled(binding::SKY_PREFILTER, both, SampleType::Float),
            // `D2Array` and `UnfilterableFloat`, for `crate::ssr`'s note on the
            // same image: WebGPU checks both against the `Rg32Float` view.
            BindGroupLayoutEntry {
                binding: binding::PROBE_VISIBILITY,
                visibility: both,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2Array,
                    sample_type: SampleType::UnfilterableFloat,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            storage(
                binding::SKY_VIEW,
                both,
                crcbl_shaders::atmosphere::SKY_VIEW_ROW_BYTES as u32,
            ),
            storage(
                binding::FROXELS,
                both,
                crcbl_shaders::volumetric::FROXEL_STRIDE as u32,
            ),
            storage(
                binding::LIGHTING,
                both,
                crcbl_shaders::volumetric::LIGHTING_STRIDE as u32,
            ),
        ];
        let surface_desc = BindGroupLayoutDesc {
            label: Some("water"),
            entries: &surface_entries,
        };
        check_portable_storage_buffers(Some("water"), &[&surface_desc])?;
        let surface_layout = device.create_bind_group_layout(&surface_desc)?;
        let surface_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("water"),
            bind_group_layouts: &[surface_layout],
            push_constants: None,
        })?;
        let surface_pipeline = build_surface(device, surface_pipeline_layout)?;

        let mut uniforms = Vec::with_capacity(frames);
        for _ in 0..frames {
            uniforms.push(device.create_buffer(&BufferDesc {
                label: Some("water params"),
                size: PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?);
        }

        Ok(Self {
            copy_layout,
            copy_pipeline_layout,
            copy_pipeline,
            copy_groups: vec![None; frames],
            surface_layout,
            surface_pipeline_layout,
            surface_pipeline,
            uniforms,
            surface_groups: (0..frames).map(|_| (None, None)).collect(),
            shadow_sampler,
        })
    }

    /// Writes `frame`'s block.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the mapped write.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn begin_frame(
        &self,
        device: &dyn Device,
        frame: usize,
        params: WaterParams,
    ) -> Result<(), HalError> {
        device.write_buffer(self.uniforms[frame], 0, &params.to_bytes())
    }

    /// Adds `water-copy` and `water`, in that order.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        images: WaterImages,
        inputs: WaterInputs,
    ) {
        let WaterImages {
            color,
            depth,
            color_copy,
            depth_copy,
            shadow_atlas,
        } = images;

        let copy_pipeline = self.copy_pipeline;
        let copy_pipeline_layout = self.copy_pipeline_layout;
        let copy_layout = self.copy_layout;
        let copy_cached = &mut self.copy_groups[frame];
        graph
            .add_render_pass("water-copy")
            // `DontCare` on both: the triangle writes every texel of each.
            .color(
                color_copy,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            .depth(
                depth_copy,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            .read_image(color)
            .read_image(depth)
            .execute(move |ctx| {
                let color_view = ctx.image_view(color);
                let depth_view = ctx.image_view(depth);
                let device = ctx.device();
                let entries = [
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::ImageView(color_view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::ImageView(depth_view),
                    },
                ];
                let Some(group) = cached_group(
                    copy_cached,
                    device,
                    &[(0, color_view), (1, depth_view)],
                    "water copy",
                    copy_layout,
                    &entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(copy_pipeline);
                encoder.bind_group(0, group, &[], copy_pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });

        let WaterInputs {
            frame_block,
            reflection_block,
            froxels,
            froxel_ids,
            probes,
            probe_id,
            probe_visibility,
            sky_prefilter,
            sky_view,
            mesh,
        } = inputs;
        // Declared whether or not the froxel passes ran: the layout binds both
        // buffers on every frame, and a bound buffer the graph was never told
        // about is one it cannot order against a later writer. On a frame that
        // ran them this is the id their own passes used; on one that did not,
        // the import is this pass's, in the state those passes leave them in.
        let (froxel_id, lighting_id) = froxel_ids.unwrap_or_else(|| {
            let mut imported = |label: &str, buffer| {
                graph.import_buffer(
                    label,
                    ImportedBuffer {
                        buffer,
                        initial: ResourceState::ShaderRead,
                        final_state: ResourceState::ShaderRead,
                    },
                )
            };
            (
                imported("volumetric-froxels", froxels.froxels),
                imported("volumetric-lighting", froxels.lighting),
            )
        });

        let surface_pipeline = self.surface_pipeline;
        let surface_pipeline_layout = self.surface_pipeline_layout;
        let surface_layout = self.surface_layout;
        let params = self.uniforms[frame];
        let shadow_sampler = self.shadow_sampler;
        let (cached_buffers, surface_cached) = &mut self.surface_groups[frame];
        graph
            .add_render_pass("water")
            // **Loaded**, like the sky pass's attachment: the surface covers
            // some pixels and every other one has to stay what the frame drew.
            .color(color, LoadOp::Load, StoreOp::Store, ClearValue::default())
            // Loaded and stored **with writes on**: the surface is tested
            // against the opaque frame and then stands in the depth buffer for
            // every later pass that tests against it.
            .depth(depth, LoadOp::Load, StoreOp::Store, ClearValue::default())
            .read_image(color_copy)
            .read_image(depth_copy)
            .read_image(shadow_atlas)
            .read_buffer(probe_id)
            .read_buffer(froxel_id)
            .read_buffer(lighting_id)
            .execute(move |ctx| {
                let color_view = ctx.image_view(color_copy);
                let depth_view = ctx.image_view(depth_copy);
                let atlas_view = ctx.image_view(shadow_atlas);
                let device = ctx.device();
                // The slot's mesh buffers are replaced when the bodies outgrow
                // them, and a group naming the old ones is stale whatever its
                // views say.
                let named = [mesh.vertices, mesh.media];
                if *cached_buffers != Some(named) {
                    if let Some((_, stale)) = surface_cached.take() {
                        device.destroy_bind_group(stale);
                    }
                    *cached_buffers = Some(named);
                }
                let buffer = |binding, buffer| BindGroupEntry {
                    binding,
                    array_index: 0,
                    resource: BindingResource::whole_buffer(buffer),
                };
                let view = |binding, view| BindGroupEntry {
                    binding,
                    array_index: 0,
                    resource: BindingResource::ImageView(view),
                };
                let entries = [
                    buffer(binding::PARAMS, params),
                    buffer(binding::FRAME, frame_block),
                    buffer(binding::REFLECTION, reflection_block),
                    buffer(binding::FROXEL_BLOCK, froxels.params),
                    buffer(binding::VERTICES, mesh.vertices),
                    buffer(binding::MEDIA, mesh.media),
                    view(binding::COLOR, color_view),
                    view(binding::DEPTH, depth_view),
                    view(binding::SHADOW_ATLAS, atlas_view),
                    BindGroupEntry {
                        binding: binding::SHADOW_SAMPLER,
                        array_index: 0,
                        resource: BindingResource::Sampler(shadow_sampler),
                    },
                    buffer(binding::PROBES, probes),
                    view(binding::SKY_PREFILTER, sky_prefilter),
                    view(binding::PROBE_VISIBILITY, probe_visibility),
                    buffer(binding::SKY_VIEW, sky_view),
                    buffer(binding::FROXELS, froxels.froxels),
                    buffer(binding::LIGHTING, froxels.lighting),
                ];
                let Some(group) = cached_group(
                    surface_cached,
                    device,
                    &[
                        (binding::COLOR, color_view),
                        (binding::DEPTH, depth_view),
                        (binding::SHADOW_ATLAS, atlas_view),
                        (binding::PROBE_VISIBILITY, probe_visibility),
                    ],
                    "water",
                    surface_layout,
                    &entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(surface_pipeline);
                encoder.bind_group(0, group, &[], surface_pipeline_layout);
                encoder.bind_index_buffer(mesh.indices, 0, IndexFormat::Uint32);
                // One draw per body: its own run of the shared index buffer,
                // whose values are absolute, so the base vertex is zero.
                for draw in &mesh.draws {
                    encoder.draw_indexed(draw.clone(), 0, 0..1);
                }
            });
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for (_, group) in self.copy_groups.into_iter().flatten().chain(
            self.surface_groups
                .into_iter()
                .filter_map(|(_, cached)| cached),
        ) {
            device.destroy_bind_group(group);
        }
        device.destroy_graphics_pipeline(self.surface_pipeline);
        device.destroy_pipeline_layout(self.surface_pipeline_layout);
        device.destroy_bind_group_layout(self.surface_layout);
        device.destroy_graphics_pipeline(self.copy_pipeline);
        device.destroy_pipeline_layout(self.copy_pipeline_layout);
        device.destroy_bind_group_layout(self.copy_layout);
        for buffer in self.uniforms {
            device.destroy_buffer(buffer);
        }
    }
}

/// The surface's pipeline: the grid pulled from storage, depth-tested and
/// written under reversed-Z, one opaque `Rgba16Float` target.
fn build_surface(
    device: &dyn Device,
    layout: PipelineLayoutHandle,
) -> Result<GraphicsPipelineHandle, HalError> {
    let entry = |stage: Stage| {
        WATER.entry_point(stage).ok_or_else(|| {
            HalError::ShaderCompilation(format!(
                "water.slang exposes no unambiguous {stage:?} entry point; the committed \
                 artifacts and their manifest disagree, which \
                 crates/crcbl-shaders/tools/compile-shaders.sh would fix"
            ))
        })
    };
    // Resolved before the module exists, on `crate::debug_draw`'s terms.
    let vertex = entry(Stage::Vertex)?;
    let fragment = entry(Stage::Fragment)?;
    let module = device.create_shader_module(&ShaderModuleDesc {
        label: Some(WATER.source()),
        spirv: WATER.spirv(),
        wgsl: WATER.wgsl(),
        msl: WATER.msl(),
        dxil: &WATER.dxil_containers(),
    })?;
    let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
        label: Some("water"),
        layout,
        vertex: ShaderEntry {
            module,
            entry_point: vertex,
        },
        fragment: Some(ShaderEntry {
            module,
            entry_point: fragment,
        }),
        // Every triangle faces up, and nothing is culled: a camera below the
        // surface is rung 7's, and until then a surface seen from beneath is
        // still drawn rather than vanishing.
        primitive: PrimitiveState::default(),
        depth_stencil: Some(DepthStencilState {
            format: Format::D32Float,
            depth_write: true,
            depth_compare: CompareOp::Greater,
            stencil: None,
            bias: DepthBias::default(),
        }),
        multisample: MultisampleState::default(),
        color_targets: &[ColorTargetState::opaque(Format::Rgba16Float)],
    });
    device.destroy_shader_module(module);
    pipeline
}

/// A uniform-buffer layout entry.
const fn uniform(binding: u32, visibility: ShaderStages) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        kind: BindingKind::UniformBuffer { dynamic: false },
        count: 1,
        flags: BindingFlags::empty(),
    }
}

/// A read-only storage-buffer layout entry of `stride` bytes a row.
const fn storage(binding: u32, visibility: ShaderStages, stride: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        kind: BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
            stride,
        },
        count: 1,
        flags: BindingFlags::empty(),
    }
}

/// A 2D sampled-image layout entry.
const fn sampled(
    binding: u32,
    visibility: ShaderStages,
    sample_type: SampleType,
) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        kind: BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type,
        },
        count: 1,
        flags: BindingFlags::empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`Water::PASSES`] is what [`crate::forward`] adds to its pass bound, so
    /// it has to be the number of `add_render_pass` calls in
    /// [`Water::add_passes`] — `crate::sky_pass`'s test of the same name.
    #[test]
    fn the_declared_pass_count_is_the_one_the_body_adds() {
        let source = include_str!("water.rs");
        let body = source
            .split_once("pub(crate) fn add_passes<'a>(")
            .expect("this file declares `add_passes`")
            .1
            // `"\n    }"` and not `"\n    }\n"`: a Windows checkout reads this
            // file with CRLF endings, and the brace is followed by `\r` there.
            .split_once("\n    }")
            .expect("the function has a body")
            .0;
        let added = body.matches(".add_render_pass(").count() as u64;
        assert_eq!(added, u64::from(Water::PASSES));
        // And one of them is the full-screen copy the counters count as a
        // triangle.
        assert_eq!(
            body.matches("encoder.draw(0..FULLSCREEN_VERTICES").count() as u64,
            Water::FULLSCREEN_PASSES
        );
    }

    /// **Every binding of the layout is the shader's, in order**: the constants
    /// in [`binding`] against `water.slang`'s declarations.
    #[test]
    fn the_binding_constants_are_the_shaders_declaration_order() {
        let source = include_str!("../../crcbl-shaders/shaders/water.slang");
        for (number, name) in [
            (binding::PARAMS, "ConstantBuffer<WaterParams> water;"),
            (binding::FRAME, "ConstantBuffer<FrameUniforms> frame;"),
            (binding::REFLECTION, "ConstantBuffer<SsrParams> camera;"),
            (
                binding::FROXEL_BLOCK,
                "ConstantBuffer<VolumetricParams> params;",
            ),
            (
                binding::VERTICES,
                "StructuredBuffer<WaterVertex> water_vertices;",
            ),
            (binding::MEDIA, "StructuredBuffer<WaterMedium> media;"),
            (binding::COLOR, "Texture2D<float4> scene_color;"),
            (binding::DEPTH, "DepthTexture2D scene_depth;"),
            (binding::SHADOW_ATLAS, "DepthTexture2D shadow_atlas;"),
            (
                binding::SHADOW_SAMPLER,
                "SamplerComparisonState shadow_sampler;",
            ),
            (binding::PROBES, "StructuredBuffer<GpuProbe> probes;"),
            (binding::SKY_PREFILTER, "Texture2D<float4> sky_prefilter;"),
            (
                binding::PROBE_VISIBILITY,
                "Texture2DArray<float4> probe_visibility;",
            ),
            (binding::SKY_VIEW, "StructuredBuffer<float4> sky_view;"),
            (binding::FROXELS, "StructuredBuffer<float4> volumetrics;"),
            (binding::LIGHTING, "StructuredBuffer<float4> lighting;"),
        ] {
            // The declaration and then its D3D12 register, whose numbers are
            // `crcbl_shaders`' `declaration_order` lint's to check.
            let spelled = format!(
                "[[vk::binding({number}, 0)]]\n{} D3D12_REGISTER(",
                name.trim_end_matches(';')
            );
            assert!(
                source.contains(&spelled),
                "water.slang does not declare `{name}` at binding {number}"
            );
        }
    }

    fn pool() -> WaterBody {
        WaterBody {
            outline: vec![[0.0, 0.0], [3.0, 0.0], [3.0, 2.0], [0.0, 2.0]],
            level: 0.5,
            medium: crcbl_water::Medium {
                absorption: [0.4, 0.1, 0.05],
                scattering: [0.01, 0.02, 0.03],
            },
        }
    }

    /// **Two bodies share one buffer and index it absolutely**: the second
    /// body's indices start past the first body's vertices, and each vertex
    /// names its own body's medium row.
    #[test]
    fn two_bodies_pack_into_one_grid_with_absolute_indices() {
        let mut second = pool();
        second.outline = vec![[10.0, 0.0], [12.0, 0.0], [12.0, 1.0]];
        second.medium.absorption = [1.0, 2.0, 3.0];
        let mut bodies = WaterBodies::new(1);
        bodies.set(&[pool(), second]).expect("both mesh");

        let first_vertices = surface_mesh(&pool(), SURFACE_SPACING)
            .expect("meshes")
            .positions
            .len();
        assert_eq!(bodies.draws.len(), 2);
        let second_draw = bodies.draws[1].clone();
        let lowest = bodies.indices[second_draw.start as usize..second_draw.end as usize]
            .iter()
            .min()
            .copied()
            .expect("the second body has triangles");
        assert_eq!(lowest as usize, first_vertices);

        let body_of = |vertex: usize| {
            let at = vertex * VERTEX_STRIDE + 16;
            u32::from_le_bytes(bodies.vertices[at..at + 4].try_into().expect("four bytes"))
        };
        assert_eq!(body_of(0), 0);
        assert_eq!(body_of(first_vertices), 1);
        let absorption_at = MEDIUM_STRIDE;
        assert_eq!(
            f32::from_le_bytes(
                bodies.media[absorption_at..absorption_at + 4]
                    .try_into()
                    .expect("four bytes")
            ),
            1.0
        );
    }

    /// **A body that fails to mesh leaves the previous set drawing**, and the
    /// revision does not move.
    #[test]
    fn a_refused_body_changes_nothing() {
        let mut bodies = WaterBodies::new(1);
        bodies.set(&[pool()]).expect("meshes");
        let revision = bodies.revision;
        let draws = bodies.draws.clone();
        let mut bad = pool();
        bad.outline.truncate(2);
        assert!(bodies.set(&[pool(), bad]).is_err());
        assert_eq!(bodies.revision, revision);
        assert_eq!(bodies.draws, draws);
        assert_eq!(bodies.bodies(), &[pool()]);
    }

    /// **A slot draws what it uploaded, and no bodies is no frame**, whatever
    /// the slot last held.
    ///
    /// Bodies replaced between a frame's start and its passes must not hand the
    /// passes ranges the slot's buffers were never written with, and a set with
    /// no bodies must stop the slot drawing at its next frame rather than leave
    /// the last water standing.
    #[test]
    fn a_slot_draws_what_it_uploaded_until_its_next_frame() {
        use crcbl_hal::null::NullInstance;
        use crcbl_hal::{DeviceDesc, Instance};

        let instance = NullInstance::gpu_driven();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend opens a device");
        let device = device.as_ref();

        let mut bodies = WaterBodies::new(1);
        assert!(bodies.frame(0).is_none(), "nothing set, nothing drawn");

        bodies.set(&[pool()]).expect("meshes");
        assert!(bodies.frame(0).is_none(), "set but not yet uploaded");
        bodies.begin_frame(device, 0).expect("uploads");
        let one = bodies.frame(0).expect("the slot holds the pool").draws;
        assert_eq!(one.len(), 1);

        let mut second = pool();
        second.outline = vec![[10.0, 0.0], [12.0, 0.0], [12.0, 1.0]];
        bodies.set(&[pool(), second]).expect("both mesh");
        assert_eq!(
            bodies
                .frame(0)
                .expect("the slot still holds the pool")
                .draws,
            one,
            "a set between a frame's start and its passes changed what the slot draws"
        );
        bodies.begin_frame(device, 0).expect("uploads");
        assert_eq!(bodies.frame(0).expect("the slot holds both").draws.len(), 2);

        bodies.set(&[]).expect("an empty set is a set");
        bodies
            .begin_frame(device, 0)
            .expect("an empty frame writes nothing");
        assert!(
            bodies.frame(0).is_none(),
            "the removed water is still drawn"
        );
        bodies.destroy(device);
    }
}
