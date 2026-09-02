//! The GPU half of the probe visibility capture: a depth cube per probe, drawn
//! from the scene's own static triangles, folded into the octahedral layers
//! `mesh.slang` reads.
//!
//! ```text
//!  world triangles ─┐
//!                   ├─▶ probe_capture.slang ─▶ distance atlas (R32Float tiles)
//!  six face matrices┘                                     │
//!                                                         ▼
//!  texel directions ──────────────▶ probe_octahedral.slang ──▶ moments buffer
//!                                                         │
//!                          copy_buffer_to_image ──────────▶ Rg32Float D2Array
//! ```
//!
//! `docs/plan/50-irradiance-probes.md`'s raster sample producer, the half that
//! says "depth cube per probe … captured on load". [`crate::probe_visibility`]
//! owns the geometry a capture is *about* and
//! [`crcbl_shaders::probe_visibility`] owns the layout it produces; this module
//! is the pass between them.
//!
//! # Why not the shadow atlas's machinery
//!
//! [`crate::shadow`] already renders a point light as six 90° faces into a tile
//! atlas, in the same `+X, -X, +Y, -Y, +Z, -Z` order, and its **arithmetic** is
//! reused here — `face_matrix` is [`crate::shadow::point_matrix`]'s shape and
//! `face_of` is `mesh.slang`'s `point_face`. Its **pass** is not, and cannot
//! be: that atlas is a fixed budget of `SHADOW_TILES` slots handed out to a
//! frame's light list by a per-frame allocator, filled by the render graph from
//! the draw-generation path under a shadow cadence. A capture of sixty probes
//! wants three hundred and sixty tiles at once, off the graph, before the first
//! frame exists and with no light in the room. Sharing the tile budget would
//! mean either evicting the frame's shadows or capping the probe count at a
//! sixtieth of what a clipmap wants, so this draws its own atlas — and it draws
//! *distance* rather than depth, which a shadow map has no use for.
//!
//! # It is transient, and it is a one-off
//!
//! Every object here — the pipelines, the atlas, the buffers — is created, used
//! and destroyed inside [`capture`](crate::probe_capture::capture). It runs once
//! when a scene's static geometry is placed, so holding a pipeline for the rest
//! of the process's life would be paying storage for something with one caller
//! and no second use yet. The clipmap's recapture-on-scroll is what would want them
//! hoisted, and it does not exist.

use crcbl_hal::{
    Barriers, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource,
    BufferBarrier, BufferDesc, BufferHandle, BufferImageCopy, BufferUsage, ClearValue,
    ColorAttachment, ColorTargetState, ColorWrites, CommandEncoder, CommandEncoderDesc, CompareOp,
    ComputePassDesc, ComputePipelineHandle, CullMode, DepthBias, DepthStencilAttachment,
    DepthStencilState, Device, Extent3d, Format, GraphicsPipelineDesc, GraphicsPipelineHandle,
    HalError, ImageAspect, ImageBarrier, ImageDesc, ImageHandle, ImageSubresourceLayers,
    ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType,
    LoadOp, MemoryLocation, MultisampleState, Offset3d, PipelineLayoutDesc, PipelineLayoutHandle,
    PrimitiveState, QueueHandle, Rect2d, RenderPassDesc, ResourceState, SampleType, ShaderEntry,
    ShaderModuleDesc, ShaderStages, StoreOp, SubmitInfo, Viewport, check_portable_storage_buffers,
};
use crcbl_shaders::probe::ProbeVolume;
use crcbl_shaders::probe_visibility::{EXTENT, TEXEL_BYTES, texel_direction};
use crcbl_shaders::{PROBE_CAPTURE, PROBE_OCTAHEDRAL, Stage};
use glam::{Mat4, Vec3};

use crate::probe_visibility::{Occluder, Occluders, world_triangles};
use crate::texture::UploadedTexture;

/// Faces of a cube, and therefore tiles a probe takes in the atlas.
///
/// [`crate::shadow::POINT_FACES`]' number, and the same six directions in the
/// same order. Written out here rather than imported because this atlas is not
/// that one and a change to the shadow budget must not silently reshape a
/// visibility capture.
const FACES: u32 = 6;

/// Texels along one side of a cube-face tile.
///
/// **Four times the octahedral map's own resolution, and that ratio is the
/// point.** A layer's interior is
/// [`SIDE`](crcbl_shaders::probe_visibility::SIDE) texels across the whole
/// sphere, so one of its texels spans about `sqrt(4π / SIDE²)` steradians —
/// roughly thirteen degrees. The resolve takes one *point* sample of a face per
/// direction, so the distance it records is the distance in a direction up to
/// half a face texel away from the one the layer means: at ninety degrees
/// across `FACE` texels that is under a degree, a twentieth of the octahedral
/// texel the value stands for, and well inside
/// [`SURFACE_BIAS`](crcbl_shaders::probe_visibility::SURFACE_BIAS) for any
/// surface a metre-scale room puts near a probe. Doubling it again would halve
/// an error already an order under the quantity it perturbs, for four times the
/// pixels.
const FACE: u32 = 64;

/// Tiles across the distance atlas, and therefore its width in tiles.
const ATLAS_COLUMNS: u32 = 32;

/// The most tile rows one atlas holds. With [`ATLAS_COLUMNS`] and [`FACE`] this
/// is what caps the atlas at 2048 × 2048 — inside every device's
/// `max_image_2d`, and 16 MiB of `R32Float` beside as much `D32Float`, which is
/// a transient a load path can afford.
const ATLAS_ROWS: u32 = 32;

/// Probes captured in one atlas full of tiles.
///
/// A capture of more than this is split into chunks that share one encoder and
/// one submission and take turns in the same atlas — which is what keeps the
/// memory constant as the probe count grows, and is the shape the clipmap's
/// thousands of probes need.
const PROBES_PER_CHUNK: u32 = ATLAS_COLUMNS * ATLAS_ROWS / FACES;

/// The near plane of a face's frustum, in world units.
///
/// **Small on purpose**: geometry nearer than this is clipped away and the
/// texel it would have filled reads as open space, which is a probe that leaks
/// through the wall it is touching. A millimetre in a metre-scale world is
/// under any surface bias, and reversed-Z carries the resulting far/near ratio
/// without complaint because the depth here only decides *which* surface a
/// pixel keeps, never how far away it is — that number is the colour target's.
const NEAR: f32 = 1.0e-3;

/// The least a face's far plane may be, in world units, for a scene whose
/// geometry is all but on top of its probes.
const MIN_FAR: f32 = 1.0;

/// Threads per workgroup in `probe_octahedral.slang`.
const WORKGROUP: u32 = 64;

/// Bytes of `probe_capture.slang`'s `CaptureFace`: a `float4x4` and a `float4`.
const FACE_BLOCK_BYTES: u64 = 64 + 16;

/// Bytes of `probe_octahedral.slang`'s `OctahedralParams`: eight `uint`s.
const PARAMS_BYTES: u64 = 8 * 4;

/// Bytes one `float4x4` occupies in the face-matrix buffer.
const MATRIX_BYTES: u64 = 64;

/// The direction point-light face `face` looks along —
/// [`crate::shadow::face_axis`]' convention, which [`face_of`] indexes and
/// [`face_matrix`] looks down.
///
/// # Panics
///
/// If `face` is not one of [`FACES`] faces.
fn face_axis(face: u32) -> Vec3 {
    match face {
        0 => Vec3::X,
        1 => Vec3::NEG_X,
        2 => Vec3::Y,
        3 => Vec3::NEG_Y,
        4 => Vec3::Z,
        5 => Vec3::NEG_Z,
        _ => panic!("a cube has {FACES} faces, not a face {face}"),
    }
}

/// Which face of the cube around a probe a direction falls in.
///
/// The six 90° frusta partition space by the major axis, so this is exact
/// rather than approximate; `probe_octahedral.slang`'s header says why. **The
/// shader does not transcribe this**: the answer travels with the direction in
/// the table [`record`] uploads, on the same terms the octahedral mapping does
/// — one place writes the rule, and the device reads data.
#[must_use]
fn face_of(direction: [f32; 3]) -> u32 {
    let axis = direction.map(f32::abs);
    if axis[0] >= axis[1] && axis[0] >= axis[2] {
        return u32::from(direction[0] < 0.0);
    }
    if axis[1] >= axis[2] {
        return 2 + u32::from(direction[1] < 0.0);
    }
    4 + u32::from(direction[2] < 0.0)
}

/// A probe's world → face-clip matrix: a **90° perspective** down that face's
/// axis, reversed-Z.
///
/// [`crate::shadow::point_matrix`]'s arithmetic, including its choice of up
/// vector, and for its reasons: at 90° with a square aspect the frustum's edge
/// planes are the diagonals where the major axis changes, so six of these tile
/// the sphere exactly. `far` is shared by every face of every probe, because it
/// only has to reach past the scene.
///
/// # Panics
///
/// If `face` is not one of [`FACES`] faces.
fn face_matrix(origin: Vec3, face: u32, far: f32) -> Mat4 {
    let axis = face_axis(face);
    // Not parallel to the axis. Which of the two it is only has to be
    // consistent, because the matrix that draws the tile and the matrix that
    // reads it are this one.
    let up = if axis.y == 0.0 { Vec3::Y } else { Vec3::Z };
    let view = glam::camera::rh::view::look_at_mat4(origin, origin + axis, up);
    glam::camera::rh::proj::directx::perspective(std::f32::consts::FRAC_PI_2, 1.0, far, NEAR) * view
}

/// Where tile `tile` of a chunk sits in the atlas.
fn tile_rect(tile: u32) -> Rect2d {
    let column = tile % ATLAS_COLUMNS;
    let row = tile / ATLAS_COLUMNS;
    Rect2d {
        x: (column * FACE) as i32,
        y: (row * FACE) as i32,
        width: FACE,
        height: FACE,
    }
}

/// The far plane every face uses: far enough that no triangle in the scene is
/// clipped away from any probe.
///
/// The scene's own bounding box against the volume's, so it is a fact about
/// this capture rather than a constant that a large room would silently
/// outgrow. [`MIN_FAR`] is the floor for a degenerate scene, where the box is a
/// point.
fn scene_far(triangles: &[[[f32; 3]; 3]], probes: &[Vec3]) -> f32 {
    let mut low = [f32::MAX; 3];
    let mut high = [f32::MIN; 3];
    for triangle in triangles {
        for corner in triangle {
            for axis in 0..3 {
                low[axis] = low[axis].min(corner[axis]);
                high[axis] = high[axis].max(corner[axis]);
            }
        }
    }
    let mut far = MIN_FAR;
    for probe in probes {
        // The farthest corner of the box from this probe, which is the farthest
        // point of it: for each axis, whichever end is further away.
        let mut reach = 0.0f32;
        for axis in 0..3 {
            let span = (probe[axis] - low[axis])
                .abs()
                .max((probe[axis] - high[axis]).abs());
            reach += span * span;
        }
        far = far.max(reach.sqrt());
    }
    // A hair past the box, so a triangle exactly on the far plane is kept.
    far * 1.01
}

/// Every probe of `volume`, in the table's own order — layer `i` of the image
/// is probe row `i`.
///
/// **The clipmap's levels one after another, finest first**, each of them
/// `x`-fastest within itself. That is
/// [`ProbeVolume::level_row`](crcbl_shaders::probe::ProbeVolume::level_row)'s
/// order and `mesh.slang`'s `probe_row` reads it, which is what makes a row and
/// a layer one probe across the whole volume rather than only within a level.
fn probe_positions(volume: &ProbeVolume) -> Vec<Vec3> {
    let mut positions = Vec::with_capacity(volume.total() as usize);
    for level in 0..volume.level_count() {
        for z in 0..volume.counts[2].max(1) {
            for y in 0..volume.counts[1].max(1) {
                for x in 0..volume.counts[0].max(1) {
                    positions.push(Vec3::from_array(volume.position(level, [x, y, z])));
                }
            }
        }
    }
    positions
}

/// The row pitch of one layer of the moments buffer, in **bytes**: at least a
/// row of texels, on the device's copy alignment, and a whole number of texels.
///
/// [`crate::texture`]'s `padded_row_pitch` for the same reason it exists there
/// — the copy expresses its pitch in texels, so a byte pitch that is not
/// divisible by the texel size cannot be expressed at all.
fn padded_row_pitch(row_bytes: u64, texel: u64, alignment: u64) -> u64 {
    let mut a = texel;
    let mut b = alignment;
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let step = texel / a * alignment;
    row_bytes.next_multiple_of(step.max(1))
}

/// Everything [`capture`] created, so one path releases it whether the
/// capture succeeded or failed part-way.
#[derive(Default)]
struct Transients {
    buffers: Vec<BufferHandle>,
    images: Vec<ImageHandle>,
    views: Vec<ImageViewHandle>,
    bind_groups: Vec<BindGroupHandle>,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    graphics: Vec<GraphicsPipelineHandle>,
    compute: Vec<ComputePipelineHandle>,
}

impl Transients {
    /// Releases everything, dependants before what they name. The device must
    /// be idle.
    fn run(self, device: &dyn Device) {
        for handle in self.bind_groups {
            device.destroy_bind_group(handle);
        }
        for handle in self.graphics {
            device.destroy_graphics_pipeline(handle);
        }
        for handle in self.compute {
            device.destroy_compute_pipeline(handle);
        }
        for handle in self.pipeline_layouts {
            device.destroy_pipeline_layout(handle);
        }
        for handle in self.bind_group_layouts {
            device.destroy_bind_group_layout(handle);
        }
        for handle in self.views {
            device.destroy_image_view(handle);
        }
        for handle in self.images {
            device.destroy_image(handle);
        }
        for handle in self.buffers {
            device.destroy_buffer(handle);
        }
    }
}

/// The shapes every step of one capture agrees on: how the probes are split
/// into chunks, how big the atlas that serves a chunk is, and where a probe's
/// moments sit in the buffer the copy reads.
///
/// One struct rather than a dozen locals threaded through five functions,
/// because every one of them is derived from the probe count and the device's
/// copy alignment and none of them may disagree between the pass that writes
/// and the copy that reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    /// Probes in the whole capture, which is layers in the image.
    probes: u32,
    /// Probes in one chunk — [`PROBES_PER_CHUNK`], or fewer if that is all
    /// there are.
    chunk_probes: u32,
    /// Chunks the capture is split into.
    chunks: u32,
    /// The distance atlas's extent, in texels.
    atlas_width: u32,
    atlas_height: u32,
    /// Bytes between two rows of one layer of the moments buffer.
    row_pitch: u64,
    /// Bytes between two layers of it.
    layer_pitch: u64,
}

impl Layout {
    /// The layout `probes` probes take on `device`.
    fn new(device: &dyn Device, probes: u32) -> Self {
        let chunk_probes = probes.clamp(1, PROBES_PER_CHUNK);
        let chunk_tiles = chunk_probes * FACES;
        let copy_alignment = device
            .caps()
            .limits
            .optimal_buffer_copy_offset_alignment
            .max(1);
        let row_bytes = u64::from(EXTENT) * TEXEL_BYTES as u64;
        let row_pitch = padded_row_pitch(row_bytes, TEXEL_BYTES as u64, copy_alignment);
        Self {
            probes,
            chunk_probes,
            chunks: probes.div_ceil(chunk_probes),
            atlas_width: ATLAS_COLUMNS * FACE,
            atlas_height: chunk_tiles.div_ceil(ATLAS_COLUMNS).max(1) * FACE,
            row_pitch,
            layer_pitch: row_pitch * u64::from(EXTENT),
        }
    }

    /// The `buffer_row_length` a copy of one layer names, in texels.
    fn row_texels(&self) -> u32 {
        u32::try_from(self.row_pitch / TEXEL_BYTES as u64).unwrap_or(u32::MAX)
    }

    /// The chunk-local probe count and the capture-global index its probe 0 is.
    fn chunk(&self, chunk: u32) -> (u32, u32) {
        let base = chunk * self.chunk_probes;
        (base, (self.probes - base).min(self.chunk_probes))
    }
}

/// The host-visible buffers a capture reads, written once before the encoder
/// opens.
#[derive(Clone, Copy, Debug)]
struct Inputs {
    /// The world-space triangle soup the cube pass pulls its vertices from.
    geometry: BufferHandle,
    /// Vertices in that soup — three per triangle.
    vertices: u32,
    /// One `float4` per texel of a layer: the direction, and its cube face in
    /// `w`.
    directions: BufferHandle,
    /// Every probe's six world → face-clip matrices.
    matrices: BufferHandle,
    /// Every tile's `CaptureFace` block, one dynamic offset apart.
    blocks: BufferHandle,
    /// Bytes between two of those blocks.
    face_stride: u32,
    /// Every chunk's `OctahedralParams` block, one dynamic offset apart.
    params: BufferHandle,
    /// Bytes between two of those.
    params_stride: u32,
}

/// The distance atlas and the depth buffer that resolves it.
#[derive(Clone, Copy, Debug)]
struct Atlas {
    colour: ImageHandle,
    colour_view: ImageViewHandle,
    depth: ImageHandle,
    depth_view: ImageViewHandle,
}

/// One pipeline and the single group it binds.
#[derive(Clone, Copy, Debug)]
struct Bound<Pipeline> {
    pipeline: Pipeline,
    layout: PipelineLayoutHandle,
    group: BindGroupHandle,
}

/// Captures a visibility map for every probe of `volume` on the device, and
/// returns the `Rg32Float` array image `mesh.slang` binds.
///
/// [`None`] when there is nothing to capture — a volume with no probes, or a
/// scene with nothing placed. There is no geometry for a map to be about, and
/// leaving the caller's placeholder bound is the value that occludes nothing.
///
/// **The device must be idle and stays idle**: this records its own barriers,
/// submits, and blocks on [`Device::wait_idle`], on [`crate::texture`]'s terms.
/// It is a load path, not a frame path.
///
/// # Errors
///
/// [`HalError`] from any seam call. Everything created is released on every
/// path out, including the failing ones.
pub(crate) fn capture(
    device: &dyn Device,
    queue: QueueHandle,
    volume: &ProbeVolume,
    geometry: &Occluders,
    occluders: &[Occluder],
) -> Result<Option<UploadedTexture>, HalError> {
    let probes = volume.total();
    if probes == 0 || geometry.is_empty() {
        return Ok(None);
    }
    let triangles = world_triangles(geometry, occluders);
    if triangles.is_empty() {
        return Ok(None);
    }

    let mut transients = Transients::default();
    let result = record(device, queue, volume, &triangles, probes, &mut transients);
    transients.run(device);
    result.map(Some)
}

/// [`capture`]'s body, recording what it creates into `transients` as it
/// goes.
///
/// The image it returns is deliberately **not** in `transients`: it is the one
/// object that outlives the call.
fn record(
    device: &dyn Device,
    queue: QueueHandle,
    volume: &ProbeVolume,
    triangles: &[[[f32; 3]; 3]],
    probes: u32,
    transients: &mut Transients,
) -> Result<UploadedTexture, HalError> {
    let layout = Layout::new(device, probes);
    let positions = probe_positions(volume);
    let far = scene_far(triangles, &positions);
    let inputs = upload_inputs(device, transients, &layout, &positions, triangles, far)?;
    let atlas = build_atlas(device, transients, &layout)?;

    let moments = make_buffer(
        device,
        transients,
        "probe visibility moments",
        layout.layer_pitch * u64::from(layout.probes),
        // Written by a shader, so **device local** — see
        // `crcbl_hal::MemoryLocation`, which refuses the alternative by name.
        BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
        MemoryLocation::DeviceLocal,
    )?;

    let raster = build_raster(device, transients, &inputs)?;
    let resolve = build_resolve(device, transients, &inputs, atlas.colour_view, moments)?;
    let captured = build_target(device, &layout)?;

    let plan = Recording {
        layout,
        inputs,
        atlas,
        raster,
        resolve,
        moments,
        image: captured.image,
    };
    match encode(device, queue, &plan) {
        Ok(()) => Ok(captured),
        Err(error) => {
            captured.destroy(device);
            Err(error)
        }
    }
}

/// Writes the triangle soup, the direction table, the face matrices, the
/// per-tile blocks and the per-chunk parameters.
///
/// **Every buffer spans the whole capture rather than one chunk**, and that is
/// what lets the chunks share an encoder: a host-visible buffer rewritten
/// between two recorded passes would hold only its last contents by the time
/// either of them ran.
fn upload_inputs(
    device: &dyn Device,
    transients: &mut Transients,
    layout: &Layout,
    positions: &[Vec3],
    triangles: &[[[f32; 3]; 3]],
    far: f32,
) -> Result<Inputs, HalError> {
    let vertices = u32::try_from(triangles.len() * 3).map_err(|_| {
        HalError::InvalidDescriptor(format!(
            "a probe visibility capture of {} triangles is past what one draw can name",
            triangles.len()
        ))
    })?;
    let mut soup = Vec::with_capacity(vertices as usize * 3 * size_of::<f32>());
    for triangle in triangles {
        for corner in triangle {
            for value in corner {
                soup.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    let geometry = make_buffer(
        device,
        transients,
        "probe capture geometry",
        soup.len() as u64,
        BufferUsage::STORAGE,
        MemoryLocation::HostUpload,
    )?;
    device.write_buffer(geometry, 0, &soup)?;

    // `texel_direction` for every texel of a layer, once, with the cube face it
    // falls in beside it: the table is the same for every probe because the
    // layout is, and evaluating it on the host is what keeps
    // `crcbl_shaders::probe_visibility` the only place the octahedral mapping
    // and the border rule are written, and this module the only place the face
    // order is.
    let mut table = Vec::with_capacity((EXTENT * EXTENT) as usize * 4 * size_of::<f32>());
    for y in 0..EXTENT {
        for x in 0..EXTENT {
            let direction = texel_direction(x, y);
            let face = face_of(direction) as f32;
            for value in direction.iter().chain(std::iter::once(&face)) {
                table.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    let directions = make_buffer(
        device,
        transients,
        "probe capture directions",
        table.len() as u64,
        BufferUsage::STORAGE,
        MemoryLocation::HostUpload,
    )?;
    device.write_buffer(directions, 0, &table)?;

    let alignment = device
        .caps()
        .limits
        .min_uniform_buffer_offset_alignment
        .max(1);
    let face_stride = u32::try_from(FACE_BLOCK_BYTES.next_multiple_of(alignment))
        .map_err(|_| dynamic_offset_refusal(alignment))?;
    let params_stride = u32::try_from(PARAMS_BYTES.next_multiple_of(alignment))
        .map_err(|_| dynamic_offset_refusal(alignment))?;

    let tiles = positions.len() * FACES as usize;
    let mut matrix_bytes = Vec::with_capacity(tiles * MATRIX_BYTES as usize);
    let mut block_bytes = vec![0u8; tiles * face_stride as usize];
    for (probe, origin) in positions.iter().enumerate() {
        for face in 0..FACES {
            let columns = face_matrix(*origin, face, far).to_cols_array();
            for value in columns {
                matrix_bytes.extend_from_slice(&value.to_le_bytes());
            }
            let at = (probe * FACES as usize + face as usize) * face_stride as usize;
            write_floats(&mut block_bytes, at, &columns);
            write_floats(
                &mut block_bytes,
                at + MATRIX_BYTES as usize,
                &origin.to_array(),
            );
        }
    }
    let matrices = make_buffer(
        device,
        transients,
        "probe capture matrices",
        matrix_bytes.len() as u64,
        BufferUsage::STORAGE,
        MemoryLocation::HostUpload,
    )?;
    device.write_buffer(matrices, 0, &matrix_bytes)?;
    let blocks = make_buffer(
        device,
        transients,
        "probe capture faces",
        block_bytes.len() as u64,
        BufferUsage::UNIFORM,
        MemoryLocation::HostUpload,
    )?;
    device.write_buffer(blocks, 0, &block_bytes)?;

    let mut param_bytes = vec![0u8; layout.chunks as usize * params_stride as usize];
    for chunk in 0..layout.chunks {
        let (base, count) = layout.chunk(chunk);
        let at = chunk as usize * params_stride as usize;
        // `probe_octahedral.slang`'s `OctahedralParams`, in declaration order.
        write_words(
            &mut param_bytes,
            at,
            &[
                count,
                base,
                EXTENT,
                FACE,
                ATLAS_COLUMNS,
                u32::try_from(layout.row_pitch / size_of::<f32>() as u64).unwrap_or(u32::MAX),
                u32::try_from(layout.layer_pitch / size_of::<f32>() as u64).unwrap_or(u32::MAX),
                0,
            ],
        );
    }
    let params = make_buffer(
        device,
        transients,
        "probe capture params",
        param_bytes.len() as u64,
        BufferUsage::UNIFORM,
        MemoryLocation::HostUpload,
    )?;
    device.write_buffer(params, 0, &param_bytes)?;

    Ok(Inputs {
        geometry,
        vertices,
        directions,
        matrices,
        blocks,
        face_stride,
        params,
        params_stride,
    })
}

/// The `R32Float` distance atlas one chunk's tiles are drawn into, and the
/// `D32Float` buffer that decides which surface keeps each pixel.
fn build_atlas(
    device: &dyn Device,
    transients: &mut Transients,
    layout: &Layout,
) -> Result<Atlas, HalError> {
    let extent = Extent3d::d2(layout.atlas_width, layout.atlas_height);
    let colour = device.create_image(&ImageDesc {
        label: Some("probe capture distance"),
        image_type: ImageType::D2,
        format: Format::R32Float,
        extent,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
    })?;
    transients.images.push(colour);
    let colour_view = device.create_image_view(&ImageViewDesc {
        label: Some("probe capture distance"),
        image: colour,
        view_type: ImageViewType::D2,
        format: Format::R32Float,
        range: color_range(1),
    })?;
    transients.views.push(colour_view);

    let depth = device.create_image(&ImageDesc {
        label: Some("probe capture depth"),
        image_type: ImageType::D2,
        format: Format::D32Float,
        extent,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
    })?;
    transients.images.push(depth);
    let depth_view = device.create_image_view(&ImageViewDesc {
        label: Some("probe capture depth"),
        image: depth,
        view_type: ImageViewType::D2,
        format: Format::D32Float,
        range: depth_range(),
    })?;
    transients.views.push(depth_view);

    Ok(Atlas {
        colour,
        colour_view,
        depth,
        depth_view,
    })
}

/// `probe_capture.slang`'s pipeline: the tile's block at 0 and the triangle
/// soup at 1, in that source's declaration order.
fn build_raster(
    device: &dyn Device,
    transients: &mut Transients,
    inputs: &Inputs,
) -> Result<Bound<GraphicsPipelineHandle>, HalError> {
    let entries = [
        BindGroupLayoutEntry {
            binding: 0,
            // The fragment stage reads the probe's origin out of the same
            // block, so both stages see it.
            visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
            // Dynamic, which is how a tile says which face it is on a device
            // with no push constants.
            kind: BindingKind::UniformBuffer { dynamic: true },
            count: 1,
            flags: BindingFlags::empty(),
        },
        BindGroupLayoutEntry {
            binding: 1,
            visibility: ShaderStages::VERTEX,
            kind: BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: BindingFlags::empty(),
        },
    ];
    let desc = BindGroupLayoutDesc {
        label: Some("probe capture"),
        entries: &entries,
    };
    check_portable_storage_buffers(Some("probe capture"), &[&desc])?;
    let group_layout = device.create_bind_group_layout(&desc)?;
    transients.bind_group_layouts.push(group_layout);
    let layout = device.create_pipeline_layout(&PipelineLayoutDesc {
        label: Some("probe capture"),
        bind_group_layouts: &[group_layout],
        push_constants: None,
    })?;
    transients.pipeline_layouts.push(layout);
    let group = device.create_bind_group(&BindGroupDesc {
        label: Some("probe capture"),
        layout: group_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                array_index: 0,
                // **One block, not the whole buffer.** The binding is dynamic,
                // so the offset the bind adds is on top of this one, and both
                // Vulkan and WebGPU require the sum to stay inside the buffer.
                resource: BindingResource::Buffer {
                    buffer: inputs.blocks,
                    offset: 0,
                    size: FACE_BLOCK_BYTES,
                },
            },
            BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: BindingResource::whole_buffer(inputs.geometry),
            },
        ],
        variable_count: None,
    })?;
    transients.bind_groups.push(group);

    // Entry points resolved before the module exists, for `crate::grid`'s
    // reason: a manifest that disagreed with the artifact would otherwise fail
    // inside the descriptor literal, with the module already created and
    // nothing holding it.
    let vertex = entry_point(&PROBE_CAPTURE, Stage::Vertex)?;
    let fragment = entry_point(&PROBE_CAPTURE, Stage::Fragment)?;
    let module = device.create_shader_module(&ShaderModuleDesc {
        label: Some(PROBE_CAPTURE.source()),
        spirv: PROBE_CAPTURE.spirv(),
        wgsl: PROBE_CAPTURE.wgsl(),
        msl: PROBE_CAPTURE.msl(),
        dxil: &PROBE_CAPTURE.dxil_containers(),
    })?;
    let targets = [ColorTargetState {
        format: Format::R32Float,
        // No blend: the nearest surface replaces what a further one wrote, and
        // `R32Float` is not a blendable format on a WebGPU core device anyway.
        blend: None,
        write_mask: ColorWrites::ALL,
    }];
    let pipeline = device.create_graphics_pipeline(&GraphicsPipelineDesc {
        label: Some("probe capture"),
        layout,
        vertex: ShaderEntry {
            module,
            entry_point: vertex,
        },
        fragment: Some(ShaderEntry {
            module,
            entry_point: fragment,
        }),
        primitive: PrimitiveState {
            // **Two-sided**, matching what this capture replaced: a probe
            // standing inside geometry sees that geometry's back faces, and the
            // distance to them is exactly what says it is inside.
            cull_mode: CullMode::None,
            ..PrimitiveState::default()
        },
        depth_stencil: Some(DepthStencilState {
            format: Format::D32Float,
            depth_write: true,
            // `Greater` under reversed-Z: the nearest surface wins the pixel,
            // and along one pixel's own ray nearest in `z` is nearest in
            // distance, which is what lets the colour target carry the second
            // while the test is on the first.
            depth_compare: CompareOp::Greater,
            stencil: None,
            bias: DepthBias::default(),
        }),
        multisample: MultisampleState::default(),
        color_targets: &targets,
    });
    device.destroy_shader_module(module);
    let pipeline = pipeline?;
    transients.graphics.push(pipeline);
    Ok(Bound {
        pipeline,
        layout,
        group,
    })
}

/// `probe_octahedral.slang`'s pipeline, in that source's declaration order:
/// the chunk's parameters, the direction table, the face matrices, the atlas
/// and the moments.
fn build_resolve(
    device: &dyn Device,
    transients: &mut Transients,
    inputs: &Inputs,
    atlas: ImageViewHandle,
    moments: BufferHandle,
) -> Result<Bound<ComputePipelineHandle>, HalError> {
    let readable = |binding: u32| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        kind: BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
        },
        count: 1,
        flags: BindingFlags::empty(),
    };
    let entries = [
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::COMPUTE,
            kind: BindingKind::UniformBuffer { dynamic: true },
            count: 1,
            flags: BindingFlags::empty(),
        },
        readable(1),
        readable(2),
        BindGroupLayoutEntry {
            binding: 3,
            visibility: ShaderStages::COMPUTE,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                // `R32Float` is unfilterable without a device feature, and
                // WebGPU checks the layout against the view's format at bind
                // time however the shader goes on to read it — this one only
                // ever `Load`s.
                sample_type: SampleType::UnfilterableFloat,
            },
            count: 1,
            flags: BindingFlags::empty(),
        },
        BindGroupLayoutEntry {
            binding: 4,
            visibility: ShaderStages::COMPUTE,
            kind: BindingKind::StorageBuffer {
                read_only: false,
                dynamic: false,
            },
            count: 1,
            flags: BindingFlags::empty(),
        },
    ];
    let desc = BindGroupLayoutDesc {
        label: Some("probe octahedral"),
        entries: &entries,
    };
    check_portable_storage_buffers(Some("probe octahedral"), &[&desc])?;
    let group_layout = device.create_bind_group_layout(&desc)?;
    transients.bind_group_layouts.push(group_layout);
    let layout = device.create_pipeline_layout(&PipelineLayoutDesc {
        label: Some("probe octahedral"),
        bind_group_layouts: &[group_layout],
        push_constants: None,
    })?;
    transients.pipeline_layouts.push(layout);
    let group = device.create_bind_group(&BindGroupDesc {
        label: Some("probe octahedral"),
        layout: group_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::Buffer {
                    buffer: inputs.params,
                    offset: 0,
                    size: PARAMS_BYTES,
                },
            },
            BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: BindingResource::whole_buffer(inputs.directions),
            },
            BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: BindingResource::whole_buffer(inputs.matrices),
            },
            BindGroupEntry {
                binding: 3,
                array_index: 0,
                resource: BindingResource::ImageView(atlas),
            },
            BindGroupEntry {
                binding: 4,
                array_index: 0,
                resource: BindingResource::whole_buffer(moments),
            },
        ],
        variable_count: None,
    })?;
    transients.bind_groups.push(group);
    let pipeline = crate::draw_gen::compute_pipeline(
        device,
        "probe octahedral",
        &PROBE_OCTAHEDRAL,
        layout,
        WORKGROUP,
    )?;
    transients.compute.push(pipeline);
    Ok(Bound {
        pipeline,
        layout,
        group,
    })
}

/// The `Rg32Float` array image the frame binds — the one object a capture
/// leaves behind.
fn build_target(device: &dyn Device, layout: &Layout) -> Result<UploadedTexture, HalError> {
    let image = device.create_image(&ImageDesc {
        label: Some("probe visibility"),
        image_type: ImageType::D2,
        format: Format::Rg32Float,
        extent: Extent3d {
            width: EXTENT,
            height: EXTENT,
            // A `D2` image's `depth_or_layers` is its array length.
            depth_or_layers: layout.probes,
        },
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
    })?;
    match device.create_image_view(&ImageViewDesc {
        label: Some("probe visibility"),
        image,
        view_type: ImageViewType::D2Array,
        format: Format::Rg32Float,
        range: color_range(layout.probes),
    }) {
        Ok(view) => Ok(UploadedTexture { image, view }),
        Err(error) => {
            device.destroy_image(image);
            Err(error)
        }
    }
}

/// Everything [`encode`] needs, which is more than an argument list should
/// be.
struct Recording {
    layout: Layout,
    inputs: Inputs,
    atlas: Atlas,
    raster: Bound<GraphicsPipelineHandle>,
    resolve: Bound<ComputePipelineHandle>,
    moments: BufferHandle,
    /// The image the copy fills. Not the whole [`UploadedTexture`], because a
    /// copy names an image and this function has no business with the view.
    image: ImageHandle,
}

/// Records every chunk's cube pass and resolve, copies the moments into the
/// image, submits, and waits.
fn encode(device: &dyn Device, queue: QueueHandle, plan: &Recording) -> Result<(), HalError> {
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("probe visibility capture"),
        queue,
    });

    // The atlas is reused chunk by chunk, so it re-enters `ColorAttachment`
    // from wherever the previous chunk's resolve left it. The first pass
    // discards its contents instead, which is what `Undefined` means.
    let mut atlas_state = ResourceState::Undefined;
    let mut depth_state = ResourceState::Undefined;
    for chunk in 0..plan.layout.chunks {
        let (base, count) = plan.layout.chunk(chunk);
        encoder.pipeline_barrier(&Barriers {
            images: &[
                ImageBarrier::new(
                    plan.atlas.colour,
                    color_range(1),
                    atlas_state,
                    ResourceState::ColorAttachment,
                ),
                ImageBarrier::new(
                    plan.atlas.depth,
                    depth_range(),
                    depth_state,
                    ResourceState::DepthStencilWrite,
                ),
            ],
            ..Default::default()
        });
        atlas_state = ResourceState::ColorAttachment;
        depth_state = ResourceState::DepthStencilWrite;

        draw_cubes(&mut encoder, plan, base, count);

        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                plan.atlas.colour,
                color_range(1),
                atlas_state,
                ResourceState::ShaderRead,
            )],
            ..Default::default()
        });
        atlas_state = ResourceState::ShaderRead;

        encoder.begin_compute_pass(&ComputePassDesc {
            label: Some("probe octahedral"),
            timestamp_writes: None,
        });
        encoder.bind_compute_pipeline(plan.resolve.pipeline);
        encoder.bind_group(
            0,
            plan.resolve.group,
            &[chunk * plan.inputs.params_stride],
            plan.resolve.layout,
        );
        let threads = count * EXTENT * EXTENT;
        encoder.dispatch(threads.div_ceil(WORKGROUP), 1, 1);
        encoder.end_compute_pass();
    }

    copy_moments(&mut encoder, plan);

    let commands = encoder.finish()?;
    let submitted = device
        .submit(queue, &SubmitInfo::new(&[commands]))
        .and_then(|()| device.wait_idle());
    device.destroy_command_buffer(commands);
    submitted
}

/// One render pass over the atlas, with a viewport and a draw per tile.
///
/// **A draw per tile rather than one instanced draw over all of them**: a tile
/// is a viewport, and remapping six frusta into one clip space instead would
/// leave each face's triangles clipped against the whole atlas rather than
/// against its own tile, so a wall running off the edge of one face would be
/// drawn across its neighbour.
fn draw_cubes(encoder: &mut Box<dyn CommandEncoder>, plan: &Recording, base: u32, count: u32) {
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("probe capture cube"),
        color_attachments: &[ColorAttachment {
            view: plan.atlas.colour_view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            // **The open-space texel.** A pixel no triangle covers keeps
            // `crcbl_shaders::probe_visibility::FAR`, which no surface is
            // further away than, so the probe occludes nothing in that
            // direction — which is what "the capture saw open space" means.
            clear: ClearValue::color([crcbl_shaders::probe_visibility::FAR, 0.0, 0.0, 0.0]),
        }],
        depth_stencil_attachment: Some(DepthStencilAttachment {
            view: plan.atlas.depth_view,
            read_only: false,
            depth_load: LoadOp::Clear,
            // Nothing reads it after the pass: the distance is in the colour
            // target and this only decided which fragment wrote it.
            depth_store: StoreOp::Discard,
            stencil_load: LoadOp::DontCare,
            stencil_store: StoreOp::Discard,
            clear: ClearValue::default(),
        }),
        render_area: Rect2d {
            x: 0,
            y: 0,
            width: plan.layout.atlas_width,
            height: plan.layout.atlas_height,
        },
        timestamp_writes: None,
    });
    encoder.bind_graphics_pipeline(plan.raster.pipeline);
    for tile in 0..count * FACES {
        let rect = tile_rect(tile);
        encoder.set_viewport(&Viewport {
            x: rect.x as f32,
            y: rect.y as f32,
            width: rect.width as f32,
            height: rect.height as f32,
            ..Viewport::from_size(rect.width, rect.height)
        });
        encoder.set_scissor(&rect);
        let offset = (base * FACES + tile) * plan.inputs.face_stride;
        encoder.bind_group(0, plan.raster.group, &[offset], plan.raster.layout);
        encoder.draw(0..plan.inputs.vertices, 0..1);
    }
    encoder.end_render_pass();
}

/// Moves the finished moments out of the buffer the resolve wrote and into the
/// layers of the image the frame binds — one copy per layer, because a copy
/// region's extent is a 2D extent on every backend here.
fn copy_moments(encoder: &mut Box<dyn CommandEncoder>, plan: &Recording) {
    let every_layer = color_range(plan.layout.probes);
    encoder.pipeline_barrier(&Barriers {
        buffers: &[BufferBarrier::new(
            plan.moments,
            ResourceState::ShaderWrite,
            ResourceState::TransferSrc,
        )],
        images: &[ImageBarrier::new(
            plan.image,
            every_layer,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Default::default()
    });
    for layer in 0..plan.layout.probes {
        encoder.copy_buffer_to_image(&BufferImageCopy {
            buffer: plan.moments,
            buffer_offset: u64::from(layer) * plan.layout.layer_pitch,
            buffer_row_length: plan.layout.row_texels(),
            buffer_image_height: EXTENT,
            image: plan.image,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: layer,
                layer_count: 1,
            },
            image_offset: Offset3d { x: 0, y: 0, z: 0 },
            image_extent: Extent3d::d2(EXTENT, EXTENT),
        });
    }
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            plan.image,
            every_layer,
            ResourceState::TransferDst,
            ResourceState::ShaderRead,
        )],
        ..Default::default()
    });
}

/// A colour subresource range over every layer of the one level.
const fn color_range(layers: u32) -> ImageSubresourceRange {
    ImageSubresourceRange {
        aspect: ImageAspect::COLOR,
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: layers,
    }
}

/// The depth aspect of the one level of the one layer.
const fn depth_range() -> ImageSubresourceRange {
    ImageSubresourceRange {
        aspect: ImageAspect::DEPTH,
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    }
}

/// Writes `values` into `bytes` at `at`, little-endian, one word each.
fn write_floats(bytes: &mut [u8], at: usize, values: &[f32]) {
    for (lane, value) in values.iter().enumerate() {
        let slot = at + lane * size_of::<f32>();
        bytes[slot..slot + size_of::<f32>()].copy_from_slice(&value.to_le_bytes());
    }
}

/// [`write_floats`] for a block of `uint`s.
fn write_words(bytes: &mut [u8], at: usize, values: &[u32]) {
    for (lane, value) in values.iter().enumerate() {
        let slot = at + lane * size_of::<u32>();
        bytes[slot..slot + size_of::<u32>()].copy_from_slice(&value.to_le_bytes());
    }
}

/// Creates a buffer and records it for release.
fn make_buffer(
    device: &dyn Device,
    transients: &mut Transients,
    label: &str,
    size: u64,
    usage: BufferUsage,
    memory: MemoryLocation,
) -> Result<BufferHandle, HalError> {
    let handle = device.create_buffer(&BufferDesc {
        label: Some(label),
        size,
        usage,
        memory,
    })?;
    transients.buffers.push(handle);
    Ok(handle)
}

/// The refusal a device whose dynamic-offset alignment does not fit a `u32`
/// earns — `crate::sprite_pass`' wording, for its reason.
fn dynamic_offset_refusal(alignment: u64) -> HalError {
    HalError::InvalidDescriptor(format!(
        "min_uniform_buffer_offset_alignment is {alignment}, which no dynamic offset can express"
    ))
}

/// The entry point `stage` of `shader`, or a refusal naming the drift.
fn entry_point(shader: &crcbl_shaders::Shader, stage: Stage) -> Result<&str, HalError> {
    shader.entry_point(stage).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "{}.slang exposes no unambiguous {stage:?} entry point; the committed artifact and \
             its manifest disagree, which crates/crcbl-shaders/tools/compile-shaders.sh would fix",
            shader.name()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The pair the resolve rests on**: for every texel of a layer, the face
    /// its direction's major axis picks is a face whose 90° frustum really does
    /// contain that direction, and the tile coordinate it projects to is inside
    /// the tile.
    ///
    /// This is the host mirror of `probe_octahedral.slang`'s whole lookup —
    /// `face_of`, then `mul(faces[…], float4(direction, 0))`, then the NDC to
    /// tile-uv map — written here as the same expressions in the same order. A
    /// face order that disagreed with [`face_matrix`], or a `y` flip the wrong
    /// way round, reads a tile of somewhere else and draws a frame that still
    /// has occlusion in it.
    #[test]
    fn the_six_faces_cover_every_texel_direction() {
        let origin = Vec3::new(1.5, -2.0, 0.75);
        let far = 40.0;
        for y in 0..EXTENT {
            for x in 0..EXTENT {
                let direction = texel_direction(x, y);
                let face = face_of(direction);
                let matrix = face_matrix(origin, face, far);
                let clip = matrix * glam::Vec4::new(direction[0], direction[1], direction[2], 0.0);
                assert!(
                    clip.w > 0.0,
                    "texel ({x}, {y}) projects behind face {face}'s eye"
                );
                let ndc = clip.truncate() / clip.w;
                assert!(
                    ndc.x.abs() <= 1.0 + 1.0e-4 && ndc.y.abs() <= 1.0 + 1.0e-4,
                    "texel ({x}, {y}) lands at {ndc:?} of face {face}, outside its tile"
                );
                // And the point one unit along the direction from the probe
                // projects to the same place, which is what lets the shader
                // pass `w = 0` and never name the probe's position.
                let point = Vec3::from_array(direction) + origin;
                let full = matrix * point.extend(1.0);
                let full_ndc = full.truncate() / full.w;
                assert!(
                    (full_ndc.x - ndc.x).abs() <= 1.0e-4 && (full_ndc.y - ndc.y).abs() <= 1.0e-4,
                    "texel ({x}, {y}) projects to {ndc:?} as a direction and {full_ndc:?} as a \
                     point"
                );
            }
        }
    }

    /// The face a direction straight down an axis picks is that axis's own
    /// face, in the order `crate::shadow` uses — [`face_of`] and
    /// [`face_axis`] agreeing is what makes the uploaded table's `w` name the
    /// tile [`face_matrix`] drew.
    #[test]
    fn each_axis_picks_its_own_face() {
        for face in 0..FACES {
            let axis = face_axis(face);
            assert_eq!(face_of(axis.to_array()), face, "face {face}'s own axis");
        }
    }

    /// Every tile of a chunk lands inside the atlas, and no two tiles overlap.
    #[test]
    fn the_tiles_of_one_chunk_tile_the_atlas() {
        let mut seen = std::collections::HashSet::new();
        for tile in 0..PROBES_PER_CHUNK * FACES {
            let rect = tile_rect(tile);
            assert!(rect.x >= 0 && rect.y >= 0);
            assert!(
                rect.x as u32 + rect.width <= ATLAS_COLUMNS * FACE,
                "tile {tile} runs past the atlas's width"
            );
            assert!(
                rect.y as u32 + rect.height <= ATLAS_ROWS * FACE,
                "tile {tile} runs past the atlas's height"
            );
            assert!(seen.insert((rect.x, rect.y)), "tile {tile} shares a corner");
        }
    }

    /// The row pitch is at least a row, on the alignment, and a whole number of
    /// texels — the three things the copy needs of it, on every alignment a
    /// device reports.
    #[test]
    fn the_row_pitch_holds_a_row_on_every_alignment() {
        let row = u64::from(EXTENT) * TEXEL_BYTES as u64;
        for alignment in [1u64, 4, 8, 16, 64, 256, 512] {
            let pitch = padded_row_pitch(row, TEXEL_BYTES as u64, alignment);
            assert!(pitch >= row, "an alignment of {alignment} lost a texel");
            assert!(pitch.is_multiple_of(alignment), "off {alignment}");
            assert!(
                pitch.is_multiple_of(TEXEL_BYTES as u64),
                "a pitch of {pitch} is not a whole number of texels"
            );
        }
    }

    /// The far plane reaches past every corner of the scene from every probe,
    /// which is what stops a wall being clipped out of a capture.
    #[test]
    fn the_far_plane_reaches_the_whole_scene() {
        let triangles = [[[-5.0, 0.0, -5.0], [5.0, 0.0, -5.0], [0.0, 3.0, 6.0]]];
        let probes = [Vec3::new(-4.0, 1.0, -4.0), Vec3::new(4.0, 2.0, 5.0)];
        let far = scene_far(&triangles, &probes);
        for probe in probes {
            for triangle in &triangles {
                for corner in triangle {
                    let reach = (Vec3::from_array(*corner) - probe).length();
                    assert!(far > reach, "a corner {reach} away is past a far of {far}");
                }
            }
        }
    }

    /// An empty scene still yields a usable far plane rather than a zero one,
    /// which would make every matrix degenerate.
    #[test]
    fn an_empty_scene_still_has_a_far_plane() {
        assert!(scene_far(&[], &[Vec3::ZERO]) >= MIN_FAR);
    }

    /// **One capture covers every level of the clipmap**, and layer `i` stands
    /// where probe row `i` does.
    ///
    /// The layer order is the whole of what makes a row and a visibility map one
    /// probe, and nothing else in the tree can catch it drifting: `mesh.slang`'s
    /// `probe_row` reads a layer index it computed itself, so a capture that
    /// walked its levels in another order draws a room lit through the wrong
    /// walls and no assertion anywhere says which. The positions are checked
    /// against [`ProbeVolume::position`], which is where the clipmap's geometry
    /// is decided.
    #[test]
    fn a_capture_walks_every_level_in_the_order_the_table_is_in() {
        let volume = ProbeVolume {
            origin: [-2.0, 1.0, -3.0],
            inv_spacing: [0.5, 1.0, 0.25],
            counts: [3, 2, 2],
            levels: 3,
        };
        let positions = probe_positions(&volume);
        assert_eq!(
            positions.len(),
            volume.total() as usize,
            "a capture must have one layer per row of the whole table"
        );
        for level in 0..volume.level_count() {
            for z in 0..volume.counts[2] {
                for y in 0..volume.counts[1] {
                    for x in 0..volume.counts[0] {
                        let row = volume.level_row(level)
                            + (z * volume.counts[1] + y) * volume.counts[0]
                            + x;
                        assert_eq!(
                            positions[row as usize],
                            Vec3::from_array(volume.position(level, [x, y, z])),
                            "row {row} is level {level}'s cell ({x}, {y}, {z})"
                        );
                    }
                }
            }
        }
        // Anti-vacuity: a coarser level really does stand somewhere else, so
        // the equalities above are not comparing one grid against itself three
        // times over.
        assert_ne!(positions[0], positions[volume.per_level() as usize]);
    }
}
