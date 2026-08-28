//! `docs/plan/51-volumetrics.md` rung 1: the froxel volume the medium is
//! integrated in, and the pass that composites it over the frame.
//!
//! ```text
//!  begin_frame ──▶ camera + medium + cascades ──▶ params[frame]
//!
//!  add_passes ── compute "volumetric-scatter"   ──▶ froxels[frame]     (per slice)
//!                                       shadow-atlas ──▶ visibility[frame]
//!             ── compute "volumetric-integrate" ──▶ froxels[frame]     (per tile)
//!             ── render  "volumetric-composite" scene-color ──▶ fogged
//! ```
//!
//! A module of its own on [`crate::ssr`]'s terms exactly: two pipelines, a
//! buffer ring and the pass group, none of it reachable from the geometry
//! passes except through [`Volumetric::add_passes`].
//!
//! # Why a buffer and not a 3D texture
//!
//! Every published froxel implementation writes a volume texture, and this one
//! writes [`crate::light_grid`]'s subdivision into a storage buffer instead.
//! The reason is a seam fact rather than a preference: [`crate::transient`] is
//! 2D-only — its descriptor carries no depth and its pool hard-codes
//! `ImageType::D2` — so a volume here would be the engine's **first** 3D image
//! on four backends at once. That is the shape of gap that had a read-only
//! depth attachment compile everywhere and refuse on D3D12. The froxel grid is
//! already a storage buffer the clustering pass writes and the fragment stage
//! reads, so the scattering rides a subdivision that four backends already run.
//!
//! # The buffer holds two different things, one after the other
//!
//! `volumetric.slang` writes each froxel's **own** scattered radiance and
//! transmittance, and then overwrites every froxel with the **exclusive prefix**
//! of the column in front of it. The composite therefore reads "everything
//! between the eye and this slice's start", and integrates the last partial
//! slice itself along the pixel's own ray — which is what keeps a frame off the
//! slice boundaries. An exponential split makes the far slices hundreds of
//! units deep, and snapping a pixel to one of those is the banding this design
//! exists to avoid.
//!
//! # The medium is charged exactly once
//!
//! `mesh.slang` composites the same exponential height fog analytically. Both
//! running is the medium charged twice, so [`ForwardRenderer::add_passes`] zeroes
//! the frame block's density on a frame that adds these passes — the froxel path
//! owns the medium when it exists. That is also the rung's observable: the
//! environment term's single-scattering albedo is one, so a column with no sun
//! in it is algebraically the closed form the fragment stage was computing and
//! the two frames have to agree. The sun is added on top of that and is zero
//! until a caller sets [`Fog::sun_scattering`], which keeps the equality as this
//! feature's off-switch rather than as a stage it has left behind.
//!
//! # The sun is occluded per froxel, in a buffer of its own
//!
//! The scatter pass looks each froxel's midpoint up in the cascades
//! [`crate::shadow`] filled, and scales the *sun* term of its scattering source
//! by what comes back — the environment term is what reaches the medium from
//! every direction, and an occluder in front of the sun does not remove it. The
//! answer is written to a second buffer the scan does not touch, because
//! `volumetric_composite.slang` integrates the last partial slice for itself and
//! needs the same source this pass used;
//! [`VISIBILITY_STRIDE`] carries
//! the argument for why that is a buffer rather than a second cascade walk.
//!
//! [`ForwardRenderer::add_passes`]: crate::forward::ForwardRenderer::add_passes

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ClearValue, ColorTargetState, ComputePipelineHandle, Device, Format,
    GraphicsPipelineHandle, HalError, ImageViewHandle, ImageViewType, LoadOp, MemoryLocation,
    PipelineLayoutDesc, PipelineLayoutHandle, ResourceState, SampleType, SamplerHandle,
    ShaderStages, StoreOp, check_portable_storage_buffers,
};
use crcbl_shaders::volumetric::{
    FROXEL_STRIDE, PARAMS_SIZE, VISIBILITY_STRIDE, VolumetricParams, WORKGROUP_SIZE,
};
use crcbl_shaders::{VOLUMETRIC, VOLUMETRIC_COMPOSITE};

use crate::camera::{DirectionalLight, Fog};
use crate::draw_gen::{bound, compute_pipeline_entry, storage, uniform};
use crate::graph::{ImageId, ImportedBuffer, RenderGraph};
use crate::light_grid::{FrameView, Grid};
use crate::shadow::{self, Cascades};
use crate::ssao::cached_group;

/// Vertices in the over-sized full-screen triangle `volumetric_composite.slang`
/// generates from `SV_VertexID`. No geometry is bound anywhere.
const FULLSCREEN_VERTICES: u32 = 3;

/// The graph transients [`Volumetric::add_passes`] reads and writes.
///
/// One struct rather than three positional arguments, on
/// [`SsrImages`](crate::ssr::SsrImages)' terms: three values of one type are
/// three values that can be swapped silently at the call site.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VolumetricImages {
    /// The depth prepass's stored depth: how far along its ray each pixel's
    /// column is cut off.
    pub(crate) depth: ImageId,
    /// The frame as it stands — the forward pass's scene colour with the sky
    /// drawn into it.
    pub(crate) color: ImageId,
    /// Where the composite writes, and the image the caller must go on to use
    /// in [`VolumetricImages::color`]'s place.
    pub(crate) composited: ImageId,
    /// The sun's cascades, as [`crate::shadow`] left them.
    ///
    /// Read by the **scatter** pass, which is what puts a shaft in the column:
    /// declaring it is what moves the atlas from the depth attachment the
    /// shadow pass wrote into a layout a compute stage can sample.
    pub(crate) shadow_atlas: ImageId,
}

/// The buffers one frame's froxel column lives in.
///
/// **A window for a test, and the froxel path's only public surface.** The
/// column is checked against `crcbl_shaders::volumetric` on the host, per
/// froxel, which is the one thing that can tell a wrong scatter from a wrong
/// scan: the frame-level checks compare a picture against the closed form, and
/// at a single-scattering albedo of one those two mistakes compose to the same
/// answer. Handing out the handles is what lets that comparison exist without
/// this module's internals leaving it.
///
/// [`DrawGen::visible_count`](crate::draw_gen::DrawGen::visible_count) is the
/// same shape and the precedent for it.
#[derive(Clone, Copy, Debug)]
pub struct FroxelBuffers {
    /// The parameter block both compute passes and the composite read —
    /// [`crcbl_shaders::volumetric::PARAMS_SIZE`] bytes, in the layout
    /// [`VolumetricParams::from_bytes`] decodes.
    pub params: BufferHandle,
    /// The column: [`FROXEL_STRIDE`] bytes per froxel, holding the **exclusive
    /// prefix** in front of each slice once the frame's passes have run.
    pub froxels: BufferHandle,
    /// What fraction of the sun each froxel sees — [`VISIBILITY_STRIDE`] bytes
    /// each, and the one thing in the column the scan does not overwrite, which
    /// is what lets a host model rebuild each slice's own scattering source
    /// without walking the shadow atlas itself.
    pub visibility: BufferHandle,
}

/// What lights the medium on this frame.
///
/// One struct rather than three more arguments, on [`VolumetricImages`]' terms:
/// the three are read together and mean nothing apart. The medium's own
/// properties are [`Fog`]; the sun and its cascades are what a shaft is cast
/// by, and they have to be the frame's own — `Cascades::new` builds the
/// matrices from the camera and the light, so a block carrying one frame's
/// light with another's cascades would shadow a shaft against boxes that are no
/// longer where it is.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Medium<'a> {
    /// The medium: its density, its falloff, its colour and how it scatters the
    /// sun.
    pub(crate) fog: Fog,
    /// The sun, whose radiance the medium scatters and whose direction the
    /// phase function is evaluated against.
    pub(crate) sun: &'a DirectionalLight,
    /// The cascades that sun was shadowed through, which is what occludes the
    /// shaft.
    pub(crate) cascades: &'a Cascades,
}

/// Everything the froxel volume owns.
#[derive(Debug)]
pub(crate) struct Volumetric {
    /// `[frame]`: the parameter block both shaders read. A ring for the frame
    /// uniforms' reason — the previous frame may still be reading its own.
    params: Vec<BufferHandle>,
    /// `[frame]`: the froxel volume. **Device-local**, because a shader writes
    /// it: D3D12 has no unordered-access view of an upload-heap resource at
    /// all — [`crate::draw_gen`]'s module docs carry the full account.
    /// `TRANSFER_SRC` so a test can read back what the GPU decided.
    froxels: Vec<BufferHandle>,
    /// `[frame]`: one `f32` per froxel, how much of the sun that froxel sees.
    ///
    /// Written by the scatter pass and read by the composite, and untouched by
    /// the scan between them — which is the whole reason it is a second buffer
    /// rather than a component of the first, argued at
    /// [`VISIBILITY_STRIDE`].
    visibility: Vec<BufferHandle>,
    compute_layout: BindGroupLayoutHandle,
    compute_pipeline_layout: PipelineLayoutHandle,
    scatter: ComputePipelineHandle,
    integrate: ComputePipelineHandle,
    /// `[frame]`: the compute group. Names only buffers this owns, so it is
    /// built once rather than cached against a graph-realised view.
    compute_groups: Vec<BindGroupHandle>,
    composite_layout: BindGroupLayoutHandle,
    composite_pipeline_layout: PipelineLayoutHandle,
    composite_pipeline: GraphicsPipelineHandle,
    /// `[frame]`: the composite group, cached against both its views together.
    ///
    /// **One per frame in flight** on [`crate::ssr`]'s terms: this group names
    /// [`Volumetric::params`] and [`Volumetric::froxels`] as well as two graph
    /// transients, and both of those are rings — a single cache keyed on the
    /// views alone would hand the even frames' buffers to the odd frames.
    composite_groups: Vec<Option<(Vec<ImageViewHandle>, BindGroupHandle)>>,
}

impl Volumetric {
    /// Passes [`Volumetric::add_passes`] adds to a frame.
    ///
    /// Exact rather than a ceiling: a frame always has froxels — [`Grid`] floors
    /// every extent at one tile and one slice — so none of the three ever drops
    /// out.
    pub(crate) const PASSES: u32 = 3;

    /// Builds both pipelines, the buffer rings and the compute groups.
    ///
    /// `froxels` is the volume's capacity, which is
    /// [`FROXEL_CAPACITY`](crate::light_grid::FROXEL_CAPACITY) outside a test
    /// that wants a smaller one, and it is what [`Grid::for_frame`] is asked to
    /// fit a frame under.
    ///
    /// `build_fullscreen` is handed in rather than duplicated, on
    /// [`Ssr::new`](crate::ssr::Ssr::new)'s terms: it is [`crate::forward`]'s,
    /// because the shape it carries is documented there.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. **Nothing is released on the failing
    /// path**, for the reason every other builder in this crate gives: the
    /// caller holds a rollback, and this is stored in it whole.
    pub(crate) fn new(
        device: &dyn Device,
        frames: usize,
        froxels: u32,
        shadow_atlas: ImageViewHandle,
        shadow_sampler: SamplerHandle,
        build_fullscreen: impl Fn(
            &dyn Device,
            &str,
            &crcbl_shaders::Shader,
            PipelineLayoutHandle,
            &[ColorTargetState],
        ) -> Result<GraphicsPipelineHandle, HalError>,
    ) -> Result<Self, HalError> {
        if frames == 0 || froxels == 0 {
            return Err(HalError::InvalidDescriptor(
                "a froxel volume needs at least one frame in flight and one froxel".to_string(),
            ));
        }
        // `volumetric.slang`'s declaration order, which is the only order Metal
        // and D3D12 agree about — see `crcbl_shaders::declaration_order`.
        let compute_entries = [
            uniform(0),
            storage(1, false),
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::COMPUTE,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // `Depth`, for the reason the composite's own depth binding
                    // gives: WebGPU binds a `D32Float` view only through a depth
                    // sample type, and the WGSL declares `texture_depth_2d`.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::COMPUTE,
                // **A comparison sampler in a compute stage**, which is what is
                // new at this rung. The shader asks with `SampleCmpLevelZero`,
                // which needs no derivatives and is legal outside a fragment
                // shader on all four targets — unlike the implicit-LOD form the
                // forward pass uses.
                kind: BindingKind::Sampler { comparison: true },
                count: 1,
                flags: BindingFlags::empty(),
            },
            storage(4, false),
        ];
        let compute_desc = BindGroupLayoutDesc {
            label: Some("volumetric"),
            entries: &compute_entries,
        };
        check_portable_storage_buffers(Some("volumetric"), &[&compute_desc])?;
        let compute_layout = device.create_bind_group_layout(&compute_desc)?;
        let compute_set_layouts = [compute_layout];
        let compute_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("volumetric"),
            bind_group_layouts: &compute_set_layouts,
            push_constants: None,
        })?;
        // Two entry points of one module, which is why this is
        // `compute_pipeline_entry` rather than `compute_pipeline`: the pair
        // shares the froxel arithmetic and shaders here have no `#include`.
        let scatter = compute_pipeline_entry(
            device,
            "volumetric scatter",
            &VOLUMETRIC,
            "scatterMain",
            compute_pipeline_layout,
            WORKGROUP_SIZE,
        )?;
        let integrate = compute_pipeline_entry(
            device,
            "volumetric integrate",
            &VOLUMETRIC,
            "integrateMain",
            compute_pipeline_layout,
            WORKGROUP_SIZE,
        )?;

        let composite_entries = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // `Depth`, and the paragraph on `crate::ssr`'s own depth
                    // binding is this one's too: it is the same image, read
                    // through the same `texture_depth_2d`.
                    sample_type: SampleType::Depth,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    // The `Rgba16Float` scene target: an ordinary colour image,
                    // and the WGSL artifact declares `texture_2d<f32>` for it.
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::StorageBuffer {
                    // The prefix scan, read and not written — `StructuredBuffer`
                    // in the shader, which is the truth rather than a hint.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::FRAGMENT,
                kind: BindingKind::StorageBuffer {
                    // Each froxel's share of the sun, as the scatter pass
                    // measured it. Read and never written here.
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: BindingFlags::empty(),
            },
        ];
        let composite_desc = BindGroupLayoutDesc {
            label: Some("volumetric composite"),
            entries: &composite_entries,
        };
        check_portable_storage_buffers(Some("volumetric composite"), &[&composite_desc])?;
        let composite_layout = device.create_bind_group_layout(&composite_desc)?;
        let composite_set_layouts = [composite_layout];
        let composite_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("volumetric composite"),
            bind_group_layouts: &composite_set_layouts,
            push_constants: None,
        })?;
        // The same format the forward pass wrote: this image stands in for the
        // scene colour from here on, so a narrower one would tonemap a truncated
        // frame — and fog adds a radiance rather than only removing one.
        let targets = [ColorTargetState::opaque(Format::Rgba16Float)];
        let composite_pipeline = build_fullscreen(
            device,
            "volumetric composite",
            &VOLUMETRIC_COMPOSITE,
            composite_pipeline_layout,
            &targets,
        )?;

        let mut params = Vec::with_capacity(frames);
        let mut volumes = Vec::with_capacity(frames);
        let mut visibility = Vec::with_capacity(frames);
        let mut compute_groups = Vec::with_capacity(frames);
        for frame in 0..frames {
            let block = device.create_buffer(&BufferDesc {
                label: Some(&format!("volumetric params {frame}")),
                size: PARAMS_SIZE as u64,
                // `TRANSFER_SRC` for the reason the froxel volume carries it: a
                // test reads the block back rather than deriving a second copy
                // of it, so what it models the column from is what the shaders
                // were handed. Host-visible memory is not a substitute — an
                // upload heap is write-combined, and reading it back on the
                // host is a different question from what the device saw.
                usage: BufferUsage::UNIFORM | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })?;
            let volume = device.create_buffer(&BufferDesc {
                label: Some(&format!("volumetric froxels {frame}")),
                size: u64::from(froxels) * FROXEL_STRIDE as u64,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })?;
            let seen = device.create_buffer(&BufferDesc {
                label: Some(&format!("volumetric visibility {frame}")),
                size: u64::from(froxels) * VISIBILITY_STRIDE as u64,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })?;
            // **Built once rather than cached against the graph**, which the
            // shadow atlas is what makes possible: the renderer owns that image
            // for its whole life and hands the graph the same view every frame,
            // so this group names nothing a frame can change.
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some(&format!("volumetric {frame}")),
                layout: compute_layout,
                entries: &[
                    bound(0, block),
                    bound(1, volume),
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::ImageView(shadow_atlas),
                    },
                    BindGroupEntry {
                        binding: 3,
                        array_index: 0,
                        resource: BindingResource::Sampler(shadow_sampler),
                    },
                    bound(4, seen),
                ],
                // No binding here is an array, so nothing has a runtime length
                // to declare — the same `None` every other pass in this crate
                // passes.
                variable_count: None,
            })?;
            params.push(block);
            volumes.push(volume);
            visibility.push(seen);
            compute_groups.push(group);
        }

        Ok(Self {
            params,
            froxels: volumes,
            visibility,
            compute_layout,
            compute_pipeline_layout,
            scatter,
            integrate,
            compute_groups,
            composite_layout,
            composite_pipeline_layout,
            composite_pipeline,
            composite_groups: vec![None; frames],
        })
    }

    /// Writes `frame`'s parameter block: the camera, the grid and the medium.
    ///
    /// `grid` must be the same [`Grid`] handed to [`Volumetric::add_passes`] and
    /// the same one the frame's clustering pass used — the composite converts a
    /// pixel to a froxel index with these numbers, and a block from one grid
    /// with a dispatch from another reads a column built for somewhere else.
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
        grid: Grid,
        view: FrameView,
        medium: Medium<'_>,
    ) -> Result<(), HalError> {
        let Medium { fog, sun, cascades } = medium;
        // Row 3 of the view-projection, so a shader can take a point's view
        // depth with one dot product — [`crate::light_grid`]'s block carries the
        // same row for the same reason, and both come from this one matrix so
        // the two passes cut the frustum at the same depths.
        let columns = view.view_projection.to_cols_array();
        let depth_row = [columns[3], columns[7], columns[11], columns[15]];
        let mut shadow_view_proj = [[0.0f32; 16]; shadow::CASCADES];
        for (matrix, cascade) in shadow_view_proj.iter_mut().zip(&cascades.view_proj) {
            *matrix = cascade.to_cols_array();
        }
        device.write_buffer(
            self.params[frame],
            0,
            &VolumetricParams {
                inverse_view_proj: view.view_projection.inverse().to_cols_array(),
                eye: view.eye.extend(1.0).to_array(),
                depth_row,
                fog_params: [fog.density, fog.falloff, fog.reference_height, 0.0],
                fog_color: fog.color.extend(0.0).to_array(),
                // Towards the sun, which is `mesh.slang`'s `to_light` and the
                // opposite of the direction its light travels — the phase
                // function's own convention, spelled out on
                // `volumetric_phase`. Normalised here rather than trusted: a
                // caller may hand the renderer any vector, and a longer one
                // would scale a cosine past the clamp and flatten the lobe.
                sun_direction: sun
                    .direction
                    .normalize_or_zero()
                    .extend(fog.anisotropy)
                    .to_array(),
                // Zero unless a caller asked for it, which is what keeps the
                // column algebraically the closed form until they do.
                sun_radiance: (sun.color * fog.sun_scattering).extend(0.0).to_array(),
                // **The cascades the frame block already carries**, handed to
                // the scatter pass so a froxel and the surface behind it are
                // shadowed by one set of matrices. `Cascades::new` builds them
                // from the camera and the light alone, so there is no version
                // of this where the two could be a frame apart.
                shadow_view_proj,
                cascade_far: cascades.far,
                shadow_params: Cascades::params(),
                grid_x: grid.x,
                grid_y: grid.y,
                slices: grid.slices,
                tile_pixels: grid.tile_pixels,
                viewport_x: view.extent.0.max(1),
                viewport_y: view.extent.1.max(1),
                froxel_count: grid.froxels(),
            }
            .to_bytes(),
        )
    }

    /// `frame`'s froxel column and the numbers it was built from.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn buffers(&self, frame: usize) -> FroxelBuffers {
        FroxelBuffers {
            params: self.params[frame],
            froxels: self.froxels[frame],
            visibility: self.visibility[frame],
        }
    }

    /// Adds the scatter, integrate and composite passes, in that order.
    ///
    /// [`VolumetricImages::composited`] is what the caller must go on to use:
    /// the medium is *in* it, and the scene colour it was composited over is not
    /// the finished picture any more.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub(crate) fn add_passes<'a>(
        &'a mut self,
        graph: &mut RenderGraph<'a>,
        frame: usize,
        grid: Grid,
        images: VolumetricImages,
    ) {
        let VolumetricImages {
            depth,
            color,
            composited,
            shadow_atlas,
        } = images;
        // The volume arrives in the state the previous frame that used this slot
        // left it in, which is the state declared as final: the composite reads
        // it. Vacuous on the first frame, and the real prior use on every later
        // one.
        let volume = graph.import_buffer(
            "volumetric-froxels",
            ImportedBuffer {
                buffer: self.froxels[frame],
                initial: ResourceState::ShaderRead,
                final_state: ResourceState::ShaderRead,
            },
        );
        // The same import the column gets, and for its reason: the composite is
        // the last pass to read it, which is the state the next frame in this
        // slot will find it in.
        let seen = graph.import_buffer(
            "volumetric-visibility",
            ImportedBuffer {
                buffer: self.visibility[frame],
                initial: ResourceState::ShaderRead,
                final_state: ResourceState::ShaderRead,
            },
        );
        let compute_layout = self.compute_pipeline_layout;
        let compute_group = self.compute_groups[frame];
        let scatter = self.scatter;
        let integrate = self.integrate;

        // One invocation per froxel, and never zero: `Grid::for_frame` floors
        // every extent at one tile and one slice, and Metal rejects an empty
        // dispatch outright rather than treating it as a no-op.
        let froxel_groups = grid.froxels().div_ceil(WORKGROUP_SIZE);
        graph
            .add_compute_pass("volumetric-scatter")
            // `ShaderReadWrite` rather than a write-only state, on the light
            // grid's terms: a storage-buffer descriptor permits reads whatever
            // the shader does with it.
            .use_buffer(volume, ResourceState::ShaderReadWrite)
            .use_buffer(seen, ResourceState::ShaderReadWrite)
            // The cascades this pass looks a froxel up in. A read rather than a
            // write, and it is what orders this dispatch after the shadow pass
            // that filled them.
            .read_image(shadow_atlas)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(scatter);
                encoder.bind_group(0, compute_group, &[], compute_layout);
                encoder.dispatch(froxel_groups, 1, 1);
            });

        // One invocation per **tile**, not per froxel: this pass walks a column
        // front to back and turns each froxel into the exclusive prefix of the
        // ones in front of it, which is a serial scan over the slice axis.
        let tile_groups = grid
            .x
            .max(1)
            .saturating_mul(grid.y.max(1))
            .div_ceil(WORKGROUP_SIZE);
        graph
            .add_compute_pass("volumetric-integrate")
            // The scatter's own output, read and overwritten in place — the
            // graph's barrier between the two comes from both declaring this
            // one id.
            .use_buffer(volume, ResourceState::ShaderReadWrite)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(integrate);
                encoder.bind_group(0, compute_group, &[], compute_layout);
                encoder.dispatch(tile_groups, 1, 1);
            });

        let pipeline = self.composite_pipeline;
        let pipeline_layout = self.composite_pipeline_layout;
        let layout = self.composite_layout;
        let params = self.params[frame];
        let froxels = self.froxels[frame];
        let visibility = self.visibility[frame];
        let cached = &mut self.composite_groups[frame];
        graph
            .add_render_pass("volumetric-composite")
            // `DontCare`, not `Clear`: the full-screen triangle writes every
            // pixel of the target, so loading or clearing it is pure bandwidth.
            .color(
                composited,
                LoadOp::DontCare,
                StoreOp::Store,
                ClearValue::default(),
            )
            // The forward pass left the colour in `ColorAttachment` and the
            // prepass left the depth in `DepthStencilWrite`. Declaring the reads
            // is what moves each into a shader-readable layout, and without them
            // every backend reads whatever the last writer left behind.
            .read_image(color)
            .read_image(depth)
            .read_buffer(volume)
            .read_buffer(seen)
            .execute(move |ctx| {
                let color_view = ctx.image_view(color);
                let depth_view = ctx.image_view(depth);
                let device = ctx.device();
                let entries = vec![
                    BindGroupEntry {
                        binding: 0,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(params),
                    },
                    // The two below are overwritten by `cached_group` with the
                    // realised views; written here so the list is a complete
                    // description of the layout rather than one with holes in it.
                    BindGroupEntry {
                        binding: 1,
                        array_index: 0,
                        resource: BindingResource::ImageView(depth_view),
                    },
                    BindGroupEntry {
                        binding: 2,
                        array_index: 0,
                        resource: BindingResource::ImageView(color_view),
                    },
                    BindGroupEntry {
                        binding: 3,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(froxels),
                    },
                    BindGroupEntry {
                        binding: 4,
                        array_index: 0,
                        resource: BindingResource::whole_buffer(visibility),
                    },
                ];
                let Some(group) = cached_group(
                    cached,
                    device,
                    &[(1, depth_view), (2, color_view)],
                    "volumetric composite",
                    layout,
                    entries,
                ) else {
                    return;
                };
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                encoder.bind_group(0, group, &[], pipeline_layout);
                encoder.draw(0..FULLSCREEN_VERTICES, 0..1);
            });
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for cached in self.composite_groups.into_iter().flatten() {
            device.destroy_bind_group(cached.1);
        }
        device.destroy_graphics_pipeline(self.composite_pipeline);
        device.destroy_pipeline_layout(self.composite_pipeline_layout);
        device.destroy_bind_group_layout(self.composite_layout);
        device.destroy_compute_pipeline(self.integrate);
        device.destroy_compute_pipeline(self.scatter);
        device.destroy_pipeline_layout(self.compute_pipeline_layout);
        for group in self.compute_groups {
            device.destroy_bind_group(group);
        }
        device.destroy_bind_group_layout(self.compute_layout);
        for buffer in self
            .params
            .into_iter()
            .chain(self.froxels)
            .chain(self.visibility)
        {
            device.destroy_buffer(buffer);
        }
    }
}
