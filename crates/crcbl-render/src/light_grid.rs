//! The light list and the froxel grid a compute pass assigns it to.
//!
//! ```text
//!  begin_frame ──▶ light rows (host) ──▶ lights[frame]
//!              ──▶ grid extent + camera ──▶ params[frame]
//!
//!  add_pass ──── compute "light-cluster"  lights ──▶ grid[frame]
//!                                                ──▶ cull stats (overflow)
//!                                                       │ graph barrier
//!                          the caller's render pass ◀────┘  ShaderRead
//! ```
//!
//! `docs/plan/18-render-features.md`'s clustered forward, on the same discipline
//! as [`crate::draw_gen`]: a compute pass produces a compacted list, the draw
//! reads it, the CPU uploads deltas and nothing else.
//!
//! # The grid is device-local, and that is a seam rule rather than a preference
//!
//! A shader writes it, so it cannot live on an upload heap: D3D12 has no
//! unordered-access view of an upload-heap resource at all — the flag is
//! rejected at creation and the heap pins the resource to `GENERIC_READ` for its
//! lifetime. [`crate::draw_gen`]'s module docs carry the full account and the
//! device it took down twice. The **light list** is host-uploaded and bound
//! read-only, which none of that applies to.
//!
//! # Overflow is a word of the culling statistics, not a buffer of its own
//!
//! `light_cluster.slang` adds every assignment its budget refused into
//! [`CLUSTER_OVERFLOW_WORD`](crcbl_shaders::light::CLUSTER_OVERFLOW_WORD), which is the third word of the buffer
//! `cull.slang` counts survivors into. Topic 03 §3.6 allows the frame loop one
//! readback and that buffer is it; a counter of its own would be a second one to
//! zero, to barrier and to copy back every frame. `clear_counters.slang` already
//! zeroes the whole buffer because it is *told* how many words it has.
//!
//! # How large the grid gets, and what happens past that
//!
//! [`FROXEL_CAPACITY`] froxels are reserved at start-up, which is enough for a
//! 4K frame at every depth slice. A frame whose tiles alone would not fit gets
//! **coarser tiles** — the tile size is a per-frame number in the frame block
//! rather than a constant, and [`Grid::for_frame`] doubles it until the grid
//! fits — so a very large viewport is clustered less finely rather than writing
//! off the end of a buffer. Slices are traded away first, tiles second, because
//! losing depth resolution costs less than losing screen resolution.

use crcbl_hal::{
    BindGroupDesc, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc,
    BufferHandle, BufferUsage, ComputePipelineHandle, Device, HalError, MemoryLocation,
    PipelineLayoutDesc, PipelineLayoutHandle, ResourceState,
};
use crcbl_shaders::light::{
    self, CLUSTER_DEPTH_SLICES, CLUSTER_PARAMS_SIZE, CLUSTER_STRIDE, CLUSTER_TILE_PIXELS,
    CLUSTER_WORKGROUP_SIZE, GpuLight, LIGHT_STRIDE,
};
use glam::{Mat4, Vec3};

use crate::draw_gen::{bound, compute_pipeline, storage, uniform};
use crate::graph::{BufferId, ImportedBuffer, RenderGraph};

/// Froxels the grid buffer holds, and the ceiling [`Grid::for_frame`] fits a
/// frame under.
///
/// Sixty-four thousand of them is 4.25 MB at [`CLUSTER_STRIDE`] words each. It
/// covers a 3840×2160 frame at every one of [`CLUSTER_DEPTH_SLICES`] slices
/// (60 × 34 × 24), and a 16384×16384 one — the largest image the engine will
/// create — at one slice exactly (256 × 256). Between those, slices are given up
/// before tiles are.
pub const FROXEL_CAPACITY: u32 = 64 * 1024;

/// The camera one frame's froxels are built around.
///
/// The four values arrive together because they are one camera's, and a
/// clustering pass handed a view projection from one frame and an eye from
/// another builds froxels no fragment is ever in — a failure that looks like
/// lights switching off at tile boundaries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameView {
    /// The viewport, in pixels.
    pub extent: (u32, u32),
    /// World → clip, the same matrix the frame draws with.
    pub view_projection: Mat4,
    /// World-space eye position.
    pub eye: Vec3,
    /// Whether the projection is a perspective one — see
    /// [`Grid::for_frame`], which is where the two are told apart.
    pub perspective: bool,
}

/// The extent of one frame's froxel grid.
///
/// Held together in one type because the four numbers are decided at once and
/// are read by two shaders: writing three of them into the frame block and
/// deriving the fourth somewhere else is how the clustering pass and the
/// fragment stage would come to index different froxels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    /// Froxels across the screen.
    pub x: u32,
    /// Froxels down the screen.
    pub y: u32,
    /// Depth slices: [`CLUSTER_DEPTH_SLICES`] for a perspective camera, `1` for
    /// an orthographic one, and fewer than either where the grid would not fit.
    pub slices: u32,
    /// How many pixels wide and tall one tile is.
    pub tile_pixels: u32,
}

impl Grid {
    /// The grid for a viewport, under a froxel budget.
    ///
    /// `perspective` decides whether there is a depth split at all: an
    /// orthographic camera's `clip.w` is `1` everywhere, so it has no view depth
    /// for a slice index to come from, and it runs with one slice per tile
    /// spanning its whole depth range. Coarser, correct, and stated — see
    /// `light_cluster.slang`'s orthographic branch, which builds that froxel a
    /// different way rather than pretending it is a perspective one.
    #[must_use]
    pub fn for_frame(extent: (u32, u32), perspective: bool, froxels: u32) -> Self {
        let tiles = |tile: u32| {
            (
                extent.0.max(1).div_ceil(tile),
                extent.1.max(1).div_ceil(tile),
            )
        };
        // Coarsen until a single slice's worth of tiles fits. Terminates: the
        // tile size doubles, so the count falls by four each round and reaches
        // one, which fits any budget of at least one froxel.
        let mut tile_pixels = CLUSTER_TILE_PIXELS.max(1);
        let (mut x, mut y) = tiles(tile_pixels);
        while x.saturating_mul(y) > froxels.max(1) {
            tile_pixels = tile_pixels.saturating_mul(2);
            (x, y) = tiles(tile_pixels);
        }
        let wanted = if perspective { CLUSTER_DEPTH_SLICES } else { 1 };
        let slices = (froxels.max(1) / (x * y)).min(wanted).max(1);
        Self {
            x,
            y,
            slices,
            tile_pixels,
        }
    }

    /// How many froxels this grid has, which is the clustering dispatch's extent
    /// and the number of records the pass writes.
    #[must_use]
    pub const fn froxels(&self) -> u32 {
        self.x * self.y * self.slices
    }

    /// The four numbers as
    /// [`FrameUniforms::cluster_grid`](crcbl_shaders::mesh::FrameUniforms::cluster_grid).
    #[must_use]
    pub const fn to_frame_block(&self) -> [u32; 4] {
        [self.x, self.y, self.slices, self.tile_pixels]
    }
}

/// How large a [`LightGrid`] is.
#[derive(Clone, Copy, Debug)]
pub struct LightGridDesc<'a> {
    /// Debug name; every buffer is named after it.
    pub label: Option<&'a str>,
    /// How many frames in flight, and so how long every ring here is.
    pub frames: usize,
    /// Rows the light list holds.
    pub lights: u32,
    /// Froxels the grid holds, which is what [`Grid::for_frame`] is asked to fit
    /// a frame under. [`FROXEL_CAPACITY`] unless a test wants a smaller one.
    pub froxels: u32,
    /// The culling-statistics ring the overflow counter is a word of — one
    /// buffer per frame in flight, the one
    /// [`DrawGen::visible_count`](crate::draw_gen::DrawGen::visible_count) hands
    /// out. Written by this pass and zeroed by the clearing dispatch that runs
    /// ahead of it.
    pub stats: &'a [BufferHandle],
}

/// The light list, the froxel grid, and the compute pass between them.
#[derive(Debug)]
pub struct LightGrid {
    lights: Vec<BufferHandle>,
    grid: Vec<BufferHandle>,
    params: Vec<BufferHandle>,
    groups: Vec<BindGroupHandle>,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: ComputePipelineHandle,
    capacity: u32,
    froxels: u32,
}

impl LightGrid {
    /// Builds every buffer, the bind groups and the clustering pipeline.
    ///
    /// # Errors
    ///
    /// [`HalError`] if a resource could not be created, or if the descriptor
    /// asks for a ring shorter than the statistics ring it was handed.
    pub fn new(device: &dyn Device, desc: &LightGridDesc<'_>) -> Result<Self, HalError> {
        if desc.frames == 0 || desc.lights == 0 || desc.froxels == 0 {
            return Err(HalError::InvalidDescriptor(
                "a light grid needs at least one frame in flight, one light row and one froxel"
                    .to_string(),
            ));
        }
        if desc.stats.len() < desc.frames {
            return Err(HalError::InvalidDescriptor(format!(
                "a light grid over {} frames needs {} statistics buffers, got {}",
                desc.frames,
                desc.frames,
                desc.stats.len()
            )));
        }
        let label = desc.label.unwrap_or("lights");

        let mut rollback = Rollback::default();
        let result = Self::build(device, desc, label, &mut rollback);
        if result.is_err() {
            rollback.run(device);
        }
        result
    }

    fn build(
        device: &dyn Device,
        desc: &LightGridDesc<'_>,
        label: &str,
        rollback: &mut Rollback,
    ) -> Result<Self, HalError> {
        let layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some(label),
            // `light_cluster.slang`'s declaration order, which is the only order
            // Metal and D3D12 agree about — see `crcbl_shaders::declaration_order`.
            entries: &[
                uniform(0),
                storage(1, true),
                storage(2, false),
                storage(3, false),
            ],
        })?;
        rollback.layouts.push(layout);
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some(label),
            bind_group_layouts: &[layout],
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(pipeline_layout);
        let pipeline = compute_pipeline(
            device,
            label,
            &crcbl_shaders::LIGHT_CLUSTER,
            pipeline_layout,
            CLUSTER_WORKGROUP_SIZE,
        )?;
        rollback.pipelines.push(pipeline);

        let mut lights = Vec::with_capacity(desc.frames);
        let mut grid = Vec::with_capacity(desc.frames);
        let mut params = Vec::with_capacity(desc.frames);
        let mut groups = Vec::with_capacity(desc.frames);
        for frame in 0..desc.frames {
            let mut buffer = |name: &str, size: u64, usage, memory| {
                let handle = device.create_buffer(&BufferDesc {
                    label: Some(&format!("{label} {name} {frame}")),
                    size,
                    usage,
                    memory,
                })?;
                rollback.buffers.push(handle);
                Ok::<_, HalError>(handle)
            };
            // Host-uploaded and bound **read-only** by both the clustering pass
            // and the fragment stage, so the device-local rule the grid below is
            // under does not reach it: what that rule is really about is a
            // writable binding, and D3D12 refuses an unordered-access view of an
            // upload heap rather than a shader resource view of one.
            let list = buffer(
                "list",
                u64::from(desc.lights) * LIGHT_STRIDE as u64,
                BufferUsage::STORAGE,
                MemoryLocation::HostUpload,
            )?;
            // **Device-local, and `TRANSFER_SRC` so a test can read back what the
            // GPU decided.** A shader writes this one.
            let froxels = buffer(
                "grid",
                u64::from(desc.froxels) * u64::from(CLUSTER_STRIDE) * 4,
                BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                MemoryLocation::DeviceLocal,
            )?;
            let block = buffer(
                "params",
                CLUSTER_PARAMS_SIZE as u64,
                BufferUsage::UNIFORM,
                MemoryLocation::HostUpload,
            )?;
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some(&format!("{label} {frame}")),
                layout,
                entries: &[
                    bound(0, block),
                    bound(1, list),
                    bound(2, froxels),
                    bound(3, desc.stats[frame]),
                ],
                // No binding here is an array, so nothing has a runtime length
                // to declare — the same `None` every other pass in this crate
                // passes.
                variable_count: None,
            })?;
            rollback.groups.push(group);
            lights.push(list);
            grid.push(froxels);
            params.push(block);
            groups.push(group);
        }

        Ok(Self {
            lights,
            grid,
            params,
            groups,
            layout,
            pipeline_layout,
            pipeline,
            capacity: desc.lights,
            froxels: desc.froxels,
        })
    }

    /// Rows the light list holds.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Froxels the grid holds.
    #[must_use]
    pub const fn froxel_capacity(&self) -> u32 {
        self.froxels
    }

    /// `frame`'s light list, for a caller binding it into a drawing pipeline.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn lights(&self, frame: usize) -> BufferHandle {
        self.lights[frame]
    }

    /// `frame`'s froxel grid, likewise — and the buffer a test copies out to see
    /// which lights the pass assigned where.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn grid(&self, frame: usize) -> BufferHandle {
        self.grid[frame]
    }

    /// Writes `frame`'s light rows and the parameters the clustering pass reads.
    ///
    /// `rows` are written in order and **the sun is expected to be among them**;
    /// nothing here treats any row specially, which is the point. Rows past
    /// [`capacity`](Self::capacity) are refused rather than truncated: a light
    /// silently missing from every froxel is the seam this whole module exists
    /// to prevent, and it would not show up in the overflow counter — that
    /// counts what the *froxel* budget refused, which is a different number.
    ///
    /// Call once per frame, before [`LightGrid::add_pass`], against the frame
    /// slot the instance pool rotated to.
    ///
    /// # Errors
    ///
    /// [`HalError`] if a write failed, or if `rows` is longer than the list.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub fn begin_frame(
        &self,
        device: &dyn Device,
        frame: usize,
        rows: &[GpuLight],
        grid: Grid,
        view: FrameView,
    ) -> Result<(), HalError> {
        let count = u32::try_from(rows.len()).unwrap_or(u32::MAX);
        if count > self.capacity {
            return Err(HalError::InvalidDescriptor(format!(
                "{count} lights in a list of {}; raise the renderer's light capacity rather than \
                 dropping one, which no counter would report",
                self.capacity
            )));
        }
        let mut bytes = vec![0u8; rows.len() * LIGHT_STRIDE];
        for (slot, row) in rows.iter().enumerate() {
            bytes[slot * LIGHT_STRIDE..(slot + 1) * LIGHT_STRIDE].copy_from_slice(&row.to_bytes());
        }
        if !bytes.is_empty() {
            device.write_buffer(self.lights[frame], 0, &bytes)?;
        }

        // Row 3 of the view-projection, so the pass can take a point's view
        // depth with one dot product. Handed over rather than derived from the
        // inverse, so the depth the froxels are cut at and the depth
        // `mesh.slang` recomputes for its slice come from the same matrix.
        let columns = view.view_projection.to_cols_array();
        let depth_row = [columns[3], columns[7], columns[11], columns[15]];
        device.write_buffer(
            self.params[frame],
            0,
            &light::ClusterParams {
                inverse_view_proj: view.view_projection.inverse().to_cols_array(),
                eye: view.eye.extend(1.0).to_array(),
                depth_row,
                grid_x: grid.x,
                grid_y: grid.y,
                slices: grid.slices,
                light_count: count,
                viewport_x: view.extent.0.max(1),
                viewport_y: view.extent.1.max(1),
                perspective: u32::from(view.perspective),
                tile_pixels: grid.tile_pixels,
            }
            .to_bytes(),
        )
    }

    /// How many passes [`add_pass`](Self::add_pass) adds to a frame.
    ///
    /// Exact, for [`DrawGen::MAX_PASSES`](crate::draw_gen::DrawGen::MAX_PASSES)'s
    /// reason: the dispatch is one invocation per froxel and a frame always has
    /// froxels, so the pass is never the one that drops out.
    pub const MAX_PASSES: u32 = 1;

    /// Adds the clustering pass to `graph` and returns the grid's id, for a
    /// caller that has to declare its own read of it.
    ///
    /// `stats` is the id [`GeneratedDraws::visible_count_id`] handed back, not a
    /// fresh import: the clearing dispatch zeroes
    /// [`CLUSTER_OVERFLOW_WORD`](crcbl_shaders::light::CLUSTER_OVERFLOW_WORD) and this pass adds to it, and the barrier
    /// between the two is the graph's — computed from the accesses both declare
    /// on **one** id. Importing the same buffer twice would be two resources the
    /// graph cannot order against each other.
    ///
    /// Must therefore be called *after*
    /// [`DrawGen::add_passes`](crate::draw_gen::DrawGen::add_passes).
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    ///
    /// [`GeneratedDraws::visible_count_id`]: crate::draw_gen::GeneratedDraws::visible_count_id
    pub fn add_pass(
        &self,
        graph: &mut RenderGraph<'_>,
        frame: usize,
        stats: BufferId,
        grid: Grid,
    ) -> BufferId {
        // The grid arrives in the state the previous frame that used this slot
        // left it in, which is the state declared as final: the drawing pass
        // reads it. Vacuous on the first frame, and the real prior use on every
        // later one.
        let froxels = graph.import_buffer(
            "light-grid",
            ImportedBuffer {
                buffer: self.grid[frame],
                initial: ResourceState::ShaderRead,
                final_state: ResourceState::ShaderRead,
            },
        );
        let pipeline = self.pipeline;
        let layout = self.pipeline_layout;
        let group = self.groups[frame];
        // One invocation per froxel, and never zero: `Grid::for_frame` floors
        // every extent at one tile and one slice, and Metal rejects an empty
        // dispatch outright rather than treating it as a no-op.
        let groups = grid.froxels().div_ceil(CLUSTER_WORKGROUP_SIZE);
        graph
            .add_compute_pass("light-cluster")
            // `ShaderReadWrite` on both rather than a write-only state: a
            // storage-buffer descriptor permits reads whatever the shader does
            // with it, and the overflow counter is genuinely read-modify-written.
            .use_buffer(froxels, ResourceState::ShaderReadWrite)
            .use_buffer(stats, ResourceState::ShaderReadWrite)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(pipeline);
                encoder.bind_group(0, group, &[], layout);
                encoder.dispatch(groups, 1, 1);
            });
        froxels
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        for group in self.groups {
            device.destroy_bind_group(group);
        }
        device.destroy_bind_group_layout(self.layout);
        for buffer in self.lights.into_iter().chain(self.grid).chain(self.params) {
            device.destroy_buffer(buffer);
        }
    }
}

/// What a failed [`LightGrid::new`] has to give back, in the order it must.
#[derive(Default)]
struct Rollback {
    pipelines: Vec<ComputePipelineHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    groups: Vec<BindGroupHandle>,
    layouts: Vec<BindGroupLayoutHandle>,
    buffers: Vec<BufferHandle>,
}

impl Rollback {
    fn run(self, device: &dyn Device) {
        for pipeline in self.pipelines {
            device.destroy_compute_pipeline(pipeline);
        }
        for layout in self.pipeline_layouts {
            device.destroy_pipeline_layout(layout);
        }
        for group in self.groups {
            device.destroy_bind_group(group);
        }
        for layout in self.layouts {
            device.destroy_bind_group_layout(layout);
        }
        for buffer in self.buffers {
            device.destroy_buffer(buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ordinary case: a 4K frame keeps the default tile size and every
    /// slice, which is what [`FROXEL_CAPACITY`] was chosen to buy.
    #[test]
    fn a_4k_frame_keeps_the_default_tiles_and_every_slice() {
        let grid = Grid::for_frame((3840, 2160), true, FROXEL_CAPACITY);
        assert_eq!(grid.tile_pixels, CLUSTER_TILE_PIXELS);
        assert_eq!(grid.slices, CLUSTER_DEPTH_SLICES);
        assert_eq!((grid.x, grid.y), (60, 34));
        assert!(
            grid.froxels() <= FROXEL_CAPACITY,
            "{} froxels in a buffer of {FROXEL_CAPACITY}",
            grid.froxels()
        );
    }

    /// An orthographic camera has one slice, whatever the budget allows, because
    /// it has no view depth to slice by.
    #[test]
    fn an_orthographic_frame_gets_one_slice() {
        let grid = Grid::for_frame((256, 192), false, FROXEL_CAPACITY);
        assert_eq!(grid.slices, 1);
        assert_eq!(grid.tile_pixels, CLUSTER_TILE_PIXELS);
        assert_eq!((grid.x, grid.y), (4, 3));
    }

    /// Slices are given up before tiles are, and only when they have to be.
    #[test]
    fn a_budget_too_small_for_every_slice_loses_slices_first() {
        // Four by three tiles at 256x192; a budget of 24 leaves two slices.
        let grid = Grid::for_frame((256, 192), true, 24);
        assert_eq!(grid.tile_pixels, CLUSTER_TILE_PIXELS, "tiles are kept");
        assert_eq!((grid.x, grid.y), (4, 3));
        assert_eq!(grid.slices, 2);
        assert!(grid.froxels() <= 24);
    }

    /// Past that, tiles coarsen — and the result still fits, which is the whole
    /// obligation. The largest image the engine will create is the case this
    /// exists for.
    #[test]
    fn a_frame_too_large_for_the_budget_coarsens_its_tiles_and_still_fits() {
        for extent in [(16384u32, 16384u32), (7680, 4320), (100_000, 100_000)] {
            let grid = Grid::for_frame(extent, true, FROXEL_CAPACITY);
            assert!(
                grid.froxels() <= FROXEL_CAPACITY,
                "{extent:?} produced {} froxels, over the {FROXEL_CAPACITY} the buffer holds",
                grid.froxels()
            );
            assert!(grid.slices >= 1 && grid.x >= 1 && grid.y >= 1);
            assert!(
                u64::from(grid.x) * u64::from(grid.tile_pixels) >= u64::from(extent.0),
                "the tiles must still cover the frame, or the fragments past them \
                 read a froxel built for somewhere else"
            );
            assert!(u64::from(grid.y) * u64::from(grid.tile_pixels) >= u64::from(extent.1));
        }
    }

    /// A zero extent — a minimised window — is one froxel rather than a dispatch
    /// of none, which Metal rejects outright.
    #[test]
    fn a_zero_extent_is_still_one_froxel() {
        let grid = Grid::for_frame((0, 0), true, FROXEL_CAPACITY);
        assert!(grid.froxels() >= 1);
        assert_eq!((grid.x, grid.y), (1, 1));
    }

    #[test]
    fn the_frame_block_carries_the_four_numbers_in_order() {
        let grid = Grid {
            x: 3,
            y: 5,
            slices: 7,
            tile_pixels: 11,
        };
        assert_eq!(grid.to_frame_block(), [3, 5, 7, 11]);
        assert_eq!(grid.froxels(), 105);
    }
}
