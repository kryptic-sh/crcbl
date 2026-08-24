//! The skinning prepass: joint palettes and bind-pose vertices in, skinned
//! vertices written back into a transient region of the same vertex pool.
//!
//! ```text
//!  MeshPool ──reserve_vertices ×2──▶ SkinnedRegion { prev, current }
//!
//!  begin_frame ──▶ palettes[frame]      (one buffer, every range end to end)
//!              ──▶ bindings[frame]      (one buffer, every range end to end)
//!              ──▶ params[frame][slot]  (one uniform block per range)
//!
//!  add_pass ──── compute "skinning"  vertex pool ──▶ vertex pool
//!                                         │ graph barrier
//!                    the caller's mesh pass ◀┘  ShaderRead
//! ```
//!
//! `docs/plan/17-animation.md`'s "GPU skinning" section, and the host side of
//! `crates/crcbl-shaders/shaders/skinning.slang`. The kernel is that file's;
//! everything here is what it cannot do for itself — allocate the region it
//! writes into, upload the palette it blends onto, and refuse the inputs it has
//! no way to reject.
//!
//! # What the pass writes is what the vertex stage pulls
//!
//! The one sentence the whole design hangs off, and the shader's header carries
//! it in full: a skinned draw is an ordinary draw whose base vertex names the
//! region this pass filled. That is why [`SkinnedRegion`] is a range of
//! [`crate::mesh_pool`]'s pool rather than a buffer of its own.
//!
//! Where the base comes from is **one branch, in the raster stages**: an
//! instance carrying
//! [`GpuInstance::BASE_VERTEX_OVERRIDE`](crcbl_shaders::mesh::GpuInstance::BASE_VERTEX_OVERRIDE)
//! takes it from its own record instead of from the mesh entry the draw
//! resolved. Nothing else downstream grows a skinning branch — not
//! [`crate::cull`], not [`crate::draw_gen`], not [`crate::shadow`] — because a
//! skinned instance goes on naming its **source** mesh, so the bucket it is
//! scattered into, the levels it selects through and the box it is culled
//! against are all the ones the undeformed mesh already had.
//!
//! # The region is double-buffered, and nothing reads the previous half yet
//!
//! `docs/plan/17-animation.md`'s 2026-07-27 correction: "TAA motion vectors for
//! skinned meshes need previous-frame skinned **positions**, not just a previous
//! transform… the skinned-output pool region is double-buffered (prev/current
//! ping-pong) from day one — a pool-layout decision that is nearly free now and
//! a skinning-pipeline rewrite later."
//!
//! So [`SkinnedRegion`] reserves **two** runs and [`Skinning::begin_frame`]
//! alternates which one a frame writes. **There is no reader of the other one.**
//! There is no TAA pass in the engine, [`SkinnedRegion::previous_base`] has no
//! caller outside this module's tests, and the memory the prev half occupies is
//! bought today and spent later. That is the correction being followed
//! deliberately, not an unfinished half of this module: the alternative is
//! shipping a pass whose output has one home and then re-pointing every bind
//! group, every barrier and every draw that names it.
//!
//! # One dispatch per range, one bind group per range
//!
//! [`crcbl_shaders::skinning::Params`] describes exactly one contiguous run of
//! vertices, so a frame with three animated characters is three dispatches. Each
//! needs its own uniform block, so [`Skinning`] holds one params buffer and one
//! bind group per (frame, range) slot and [`SkinningDesc::ranges`] is how many
//! of those exist. The palette buffer, the skin-binding buffer and the vertex
//! pool are shared by all of them, and a range names its own start in each.
//!
//! The GPU-driven form — one dispatch over a table of ranges — is what §3.3's
//! other passes look like and is deliberately not here. It needs a range table
//! the shader can index, which is a second layout to pin against `slangc`, and
//! the kernel as committed reads one block.
//!
//! # Where a bad joint index is refused
//!
//! In [`Skinning::begin_frame`], by name, before anything is uploaded. The
//! shader clamps every index against the palette length, so a malformed asset
//! draws wrongly rather than reading past a storage buffer — but a clamp is a
//! containment, not a diagnosis, and `docs/backlog.md` records that
//! `crcbl-scene` cannot do better because a glTF primitive does not know its
//! skin at import time. This call does: it is handed the palette and the
//! bindings together, and it is the last place either is a Rust value. See
//! [`SkinningError::JointOutOfRange`].
//!
//! # The oracle
//!
//! [`skin_vertex`] is this module's [`crate::cull::visible_instances`]: ordinary
//! Rust that computes what `computeMain` computes, so a test that reads the
//! pool back has something to compare against. Nothing in a frame calls it, and
//! that is the point of an oracle.
//!
//! **Nothing in this workspace executes the kernel yet.** `crcbl-render` depends
//! on `crcbl-hal` and no backend, so its own tests run against the null backend,
//! which records a dispatch and never runs one. The readback that proves the
//! kernel belongs beside the cull one in `crates/crcbl-vk/tests/vk_e2e/`, which
//! is where `crcbl_render::cull`'s oracle is already used that way.

use crcbl_hal::{
    BindGroupDesc, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc,
    BufferHandle, BufferUsage, ComputePipelineHandle, Device, HalError, MemoryLocation,
    PipelineLayoutDesc, PipelineLayoutHandle, ResourceState, check_portable_storage_buffers,
};
use crcbl_shaders::mesh::MeshVertex;
use crcbl_shaders::skinning::{
    JOINT_STRIDE, JOINTS_PER_VERTEX, PARAMS_SIZE, Params, SKIN_BINDING_STRIDE, SkinBinding,
    WORKGROUP_SIZE,
};
use glam::{Mat3, Mat4, Vec3, Vec4};

use crate::draw_gen::{bound, compute_pipeline, storage, uniform};
use crate::graph::{BufferId, ImportedBuffer, RenderGraph};
use crate::mesh_pool::{MeshHandle, MeshPool, MeshPoolError};

/// A skinned mesh's transient region of the vertex pool: two runs of the same
/// length, one written per frame and one holding the frame before it.
///
/// Reserved with [`SkinnedRegion::reserve`] and given back with
/// [`SkinnedRegion::release`]. Neither [`Clone`] nor [`Copy`], because a copy
/// is a second thing that would try to release the same two runs.
///
/// See the [module docs](self) for why there are two runs when only one has a
/// reader.
#[derive(Debug)]
pub struct SkinnedRegion {
    /// The two runs' first vertices, indexed by a frame's parity.
    bases: [u32; 2],
    vertex_count: u32,
}

impl SkinnedRegion {
    /// Reserves both halves from `pool`.
    ///
    /// Two separate reservations rather than one run of `2 * vertex_count`: a
    /// fragmented pool can hold two halves where it has no single block for the
    /// pair, and nothing here needs the two to be adjacent.
    ///
    /// # Errors
    ///
    /// Whatever [`MeshPool::reserve_vertices`] refuses. A failure on the second
    /// half gives the first one back before it returns, so a refused
    /// reservation leaks no pool space.
    pub fn reserve(pool: &mut MeshPool, vertex_count: u32) -> Result<Self, MeshPoolError> {
        let first = pool.reserve_vertices(vertex_count)?;
        let second = match pool.reserve_vertices(vertex_count) {
            Ok(second) => second,
            Err(error) => {
                pool.release_vertices(first, vertex_count);
                return Err(error);
            }
        };
        Ok(Self {
            bases: [first, second],
            vertex_count,
        })
    }

    /// Gives both halves back to `pool`.
    ///
    /// The device must not be reading either of them: this is pool bookkeeping
    /// and records no barrier, exactly like [`MeshPool::free`].
    pub fn release(self, pool: &mut MeshPool) {
        for base in self.bases {
            pool.release_vertices(base, self.vertex_count);
        }
    }

    /// Vertices in each half, which is also the skinned mesh's vertex count.
    #[must_use]
    pub const fn vertex_count(&self) -> u32 {
        self.vertex_count
    }

    /// The half a frame of this parity writes — the run a draw of this mesh
    /// takes its base vertex from.
    ///
    /// `parity` is [`Skinning::parity`], the alternating bit of the frame most
    /// recently begun.
    #[must_use]
    pub const fn base(&self, parity: u32) -> u32 {
        self.bases[(parity & 1) as usize]
    }

    /// The other half: what the frame before this one skinned.
    ///
    /// **Nothing reads this yet** — see the [module docs](self). It is the half
    /// TAA's motion vectors will need, and it exists now so that the pool layout
    /// does not have to change when they arrive.
    #[must_use]
    pub const fn previous_base(&self, parity: u32) -> u32 {
        self.bases[((parity ^ 1) & 1) as usize]
    }
}

/// A resident mesh a draw can be pointed at the skinned output of: a
/// [`SkinnedRegion`], the mesh it was deformed from, and that mesh's own first
/// vertex.
///
/// ```text
///  source mesh ─┬─ region half 0   drawn on even frames
///               └─ region half 1   drawn on odd frames
/// ```
///
/// # The parity rides on the instance, not on the mesh table
///
/// [`SkinnedRegion::base`] alternates every frame, and an instance drawn out of
/// a region carries that base itself — see
/// [`GpuInstance::base_vertex`](crcbl_shaders::mesh::GpuInstance::base_vertex),
/// which the raster stages read in place of the mesh entry's when
/// [`GpuInstance::BASE_VERTEX_OVERRIDE`](crcbl_shaders::mesh::GpuInstance::BASE_VERTEX_OVERRIDE)
/// is set. So a frame's ping-pong is a write to the instance array, which
/// [`crate::instance_pool`] rings, and **no mesh-table entry is rewritten
/// between frames** — the property [`crate::mesh_pool`]'s module docs rest that
/// table's lack of a ring on.
///
/// [`mesh_id`](Self::mesh_id) is therefore the **source** mesh's id, one entry
/// and not two: everything that resolves through it — the bucket
/// `draw_gen.slang` scatters the instance into, the level tables it indexes and
/// the bounding box `cull.slang` reads — is the source mesh's, and only the base
/// vertex differs.
///
/// # It does not own the mesh
///
/// `source`'s vertices, indices and table entry are still the pool's. Freeing
/// that mesh while one of these is live leaves an instance naming an entry that
/// reads as the empty range, which draws nothing.
#[derive(Debug)]
pub struct SkinnedMesh {
    region: SkinnedRegion,
    /// The source mesh's table id: what
    /// [`GpuInstance::mesh`](crcbl_shaders::mesh::GpuInstance::mesh) holds for
    /// every instance drawn out of this region, under either parity.
    mesh: u32,
    /// The bind pose's first vertex — what [`SkinRange::input_base`] wants, kept
    /// here so the one call that needs it cannot be handed the skinned base by
    /// mistake.
    input_base: u32,
}

impl SkinnedMesh {
    /// Reserves a region as long as `source`'s vertex run.
    ///
    /// Nothing writes the region: it holds whatever its allocation came with
    /// until a [`Skinning`] dispatch fills it, exactly as
    /// [`MeshPool::reserve_vertices`] says. A draw out of it before that has
    /// happened is a draw of undefined vertices, which is what
    /// [`crate::forward::ForwardRenderer::add_skinned_passes`] exists to make
    /// impossible to record.
    ///
    /// # Errors
    ///
    /// [`MeshPoolError::NotResident`] if `source` is not a resident mesh of this
    /// pool, and whatever [`SkinnedRegion::reserve`] refuses — a pool with no
    /// room for both halves. **A failure gives back everything the earlier steps
    /// took**, so a refused reservation leaks no pool space.
    pub fn reserve(pool: &mut MeshPool, source: MeshHandle) -> Result<Self, MeshPoolError> {
        let range = pool
            .mesh(source)
            .ok_or(MeshPoolError::NotResident { handle: source })?;
        let vertex_count = pool
            .vertex_count(source)
            .ok_or(MeshPoolError::NotResident { handle: source })?;
        let region = SkinnedRegion::reserve(pool, vertex_count)?;
        Ok(Self {
            region,
            mesh: source.index(),
            input_base: range.base_vertex,
        })
    }

    /// Gives the region back.
    ///
    /// The device must not be reading either half: this records no barrier, on
    /// [`SkinnedRegion::release`]'s terms. It takes no table entry back, because
    /// it never took one — the source mesh's entry is the pool's and outlives
    /// this.
    pub fn release(self, pool: &mut MeshPool) {
        self.region.release(pool);
    }

    /// The region the dispatch writes and the draws read.
    #[must_use]
    pub const fn region(&self) -> &SkinnedRegion {
        &self.region
    }

    /// Vertices in each half, which is the bind pose's vertex count.
    #[must_use]
    pub const fn vertex_count(&self) -> u32 {
        self.region.vertex_count()
    }

    /// The bind pose's first vertex: [`SkinRange::input_base`].
    #[must_use]
    pub const fn input_base(&self) -> u32 {
        self.input_base
    }

    /// The mesh-table id an instance drawn out of this region carries — what
    /// [`GpuInstance::mesh`](crcbl_shaders::mesh::GpuInstance::mesh) holds.
    ///
    /// The **source** mesh's, and the same under either parity: what changes
    /// per frame is
    /// [`GpuInstance::base_vertex`](crcbl_shaders::mesh::GpuInstance::base_vertex),
    /// which [`SkinnedRegion::base`] answers for.
    #[must_use]
    pub const fn mesh_id(&self) -> u32 {
        self.mesh
    }

    /// This mesh's [`SkinRange`] for one frame, given the palette and the
    /// bindings.
    ///
    /// The only way to build one where `input_base` and `region` can disagree
    /// about which mesh they belong to, which is the mistake that would skin one
    /// primitive's vertices into another's region.
    #[must_use]
    pub const fn skin_range<'a>(
        &'a self,
        palette: &'a [Mat4],
        bindings: &'a [SkinBinding],
    ) -> SkinRange<'a> {
        SkinRange {
            input_base: self.input_base,
            region: &self.region,
            palette,
            bindings,
        }
    }
}

/// What a [`Skinning`] refuses to do.
#[derive(Debug, thiserror::Error)]
pub enum SkinningError {
    /// A skin binding names a joint the range's palette has not got.
    ///
    /// **The check `crcbl-scene` could not make.** A glTF primitive's
    /// `JOINTS_0` is validated at import against the attribute lengths and
    /// against nothing else, because a primitive does not know which skin will
    /// be applied to it; this call is handed the palette and the bindings
    /// together and can say. The shader clamps such an index rather than
    /// reading past the palette, so the containment is real either way — but a
    /// clamped index is a mesh that deforms wrongly with nothing to look at,
    /// and this names the vertex.
    #[error(
        "range {range}'s vertex {vertex} is bound to joint {joint} in slot {slot}, and its \
         palette holds {palette} — the shader would clamp it and the mesh would deform wrongly"
    )]
    JointOutOfRange {
        /// Which of the frame's ranges, in the order they were handed over.
        range: usize,
        /// The vertex within that range.
        vertex: usize,
        /// Which of the four joint slots of that vertex.
        slot: usize,
        /// The index it named.
        joint: u32,
        /// Matrices the range's palette actually holds.
        palette: usize,
    },
    /// A range's palette is empty.
    ///
    /// Refused here rather than left to
    /// [`Params::to_bytes`](crcbl_shaders::skinning::Params::to_bytes), which
    /// panics: the shader clamps against `joint_count - 1`, so an empty palette
    /// is every index in the buffer.
    #[error("range {range} has an empty joint palette, which the shader's index clamp would wrap")]
    EmptyPalette {
        /// Which of the frame's ranges.
        range: usize,
    },
    /// A range's skin bindings are not one per vertex of its region.
    ///
    /// They are parallel arrays — binding `i` describes bind-pose vertex `i` —
    /// so a short list would leave the tail of the mesh reading another range's
    /// bindings, and a long one is a caller that has the wrong region.
    #[error(
        "range {range} covers {vertices} vertices and was given {bindings} skin bindings; they \
         are parallel runs and must be the same length"
    )]
    BindingCountMismatch {
        /// Which of the frame's ranges.
        range: usize,
        /// Vertices in the range's region.
        vertices: u32,
        /// Bindings offered.
        bindings: usize,
    },
    /// More ranges, palette matrices or skin bindings than this pass was built
    /// for.
    ///
    /// Refused rather than truncated, for [`crate::light_grid`]'s reason: a
    /// character silently missing from the frame is what the capacity exists to
    /// prevent, and nothing downstream would report it.
    #[error(
        "this skinning pass holds {capacity} {what} and the frame asked for {asked}; raise \
         the renderer's skinning capacity rather than dropping a character, which no counter \
         would report"
    )]
    OverCapacity {
        /// `"ranges"`, `"palette matrices"` or `"skin bindings"`.
        what: &'static str,
        /// What the pass was built with.
        capacity: u32,
        /// What the frame needed.
        asked: u64,
    },
    /// Anything the seam refused.
    #[error(transparent)]
    Hal(#[from] HalError),
}

/// How large a [`Skinning`] is, and what it writes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkinningDesc<'a> {
    /// Debug name; every buffer, group and pipeline is named after it.
    pub label: Option<&'a str>,
    /// How many frames in flight, and so how long every ring here is.
    pub frames: usize,
    /// Skinned ranges one frame can dispatch — one animated primitive each.
    ///
    /// A params buffer and a bind group exist per (frame, range), so this is a
    /// capacity rather than a hint.
    pub ranges: u32,
    /// Palette matrices the joint buffer holds, summed across every range of
    /// one frame.
    pub joints: u32,
    /// Skin bindings the binding buffer holds, likewise — which is the total
    /// **skinned** vertex count, not the vertex pool's capacity. See
    /// [`Params::binding_base`](crcbl_shaders::skinning::Params::binding_base).
    pub bindings: u32,
    /// The vertex pool this pass reads the bind pose from and writes the
    /// skinned vertices into: [`MeshPool::vertex_buffer`].
    ///
    /// One binding, read at one base and written at another — WebGPU forbids
    /// the same range appearing as a writable storage buffer and as anything
    /// else in one usage scope, so the two-view spelling is a
    /// `createBindGroup` failure in the browser rather than a matter of taste.
    pub vertices: BufferHandle,
}

/// One range the caller asks to be skinned this frame: one animated
/// primitive's bind pose, its region, its palette and its per-vertex bindings.
#[derive(Clone, Copy, Debug)]
pub struct SkinRange<'a> {
    /// First vertex of the **bind-pose** run in the pool —
    /// [`MeshRange::base_vertex`](crate::mesh_pool::MeshRange::base_vertex).
    pub input_base: u32,
    /// Where the skinned vertices go. Its
    /// [`vertex_count`](SkinnedRegion::vertex_count) is the dispatch's extent.
    pub region: &'a SkinnedRegion,
    /// This instance's joint palette — what
    /// [`crcbl_anim::Palette::matrices`] hands out: each joint's global
    /// transform times its inverse bind matrix, with the skinned node's own
    /// transform deliberately absent.
    ///
    /// [`crcbl_anim::Palette::matrices`]: https://docs.rs/crcbl-anim
    pub palette: &'a [Mat4],
    /// One entry per vertex of the bind-pose run, in the same order.
    pub bindings: &'a [SkinBinding],
}

/// The palette buffer, the skin-binding buffer, the per-range uniform blocks
/// and the compute pass over them.
#[derive(Debug)]
pub struct Skinning {
    /// `[frame]` — every range's palette, end to end.
    palettes: Vec<BufferHandle>,
    /// `[frame]` — every range's skin bindings, end to end.
    bindings: Vec<BufferHandle>,
    /// `[frame][range]` — one uniform block each.
    params: Vec<Vec<BufferHandle>>,
    /// `[frame][range]`, in the same order.
    groups: Vec<Vec<BindGroupHandle>>,
    /// `[frame]` — the group count of each range [`Skinning::begin_frame`] was
    /// last handed for that slot, in range order.
    active: Vec<Vec<u32>>,
    layout: BindGroupLayoutHandle,
    pipeline_layout: PipelineLayoutHandle,
    pipeline: ComputePipelineHandle,
    vertices: BufferHandle,
    range_capacity: u32,
    joint_capacity: u32,
    binding_capacity: u32,
    /// Frames [`Skinning::begin_frame`] has accepted. Its low bit is the
    /// ping-pong.
    frames_begun: u64,
}

impl Skinning {
    /// Builds every buffer, every bind group and the skinning pipeline.
    ///
    /// # Errors
    ///
    /// [`SkinningError::Hal`] if a resource could not be created, and
    /// [`HalError::InvalidDescriptor`] through it for a descriptor asking for
    /// no frames, no ranges, no joints or no bindings — a buffer of zero bytes
    /// is not a buffer on any backend the engine targets.
    pub fn new(device: &dyn Device, desc: &SkinningDesc<'_>) -> Result<Self, SkinningError> {
        if desc.frames == 0 || desc.ranges == 0 || desc.joints == 0 || desc.bindings == 0 {
            return Err(HalError::InvalidDescriptor(
                "a skinning pass needs at least one frame in flight, one range, one palette \
                 matrix and one skin binding"
                    .to_string(),
            )
            .into());
        }
        let label = desc.label.unwrap_or("skinning");

        let mut rollback = Rollback::default();
        let result = Self::build(device, desc, label, &mut rollback);
        if result.is_err() {
            rollback.run(device);
        }
        result
    }

    fn build(
        device: &dyn Device,
        desc: &SkinningDesc<'_>,
        label: &str,
        rollback: &mut Rollback,
    ) -> Result<Self, SkinningError> {
        // `skinning.slang`'s declaration order, which
        // `crcbl_shaders::skinning`'s bind-group table records and
        // `crcbl_shaders::declaration_order` holds the shader to. The vertex
        // pool is the one writable entry.
        let entries = [
            uniform(0),
            storage(1, true),
            storage(2, true),
            storage(3, false),
        ];
        let layout_desc = BindGroupLayoutDesc {
            label: Some(label),
            entries: &entries,
        };
        check_portable_storage_buffers(Some(label), &[&layout_desc])?;
        let layout = device.create_bind_group_layout(&layout_desc)?;
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
            &crcbl_shaders::SKINNING,
            pipeline_layout,
            WORKGROUP_SIZE,
        )?;
        rollback.pipelines.push(pipeline);

        let mut palettes = Vec::with_capacity(desc.frames);
        let mut bindings = Vec::with_capacity(desc.frames);
        let mut params = Vec::with_capacity(desc.frames);
        let mut groups = Vec::with_capacity(desc.frames);
        for frame in 0..desc.frames {
            let mut buffer = |name: &str, size: u64, usage| {
                let handle = device.create_buffer(&BufferDesc {
                    label: Some(&format!("{label} {name} {frame}")),
                    size,
                    usage,
                    // Host-uploaded and bound **read-only**, which is what keeps
                    // them off `crate::draw_gen`'s device-local rule: D3D12
                    // refuses an unordered-access view of an upload heap, not a
                    // shader resource view of one. The one buffer a shader
                    // writes here is the vertex pool, and `crate::mesh_pool`
                    // already makes that device-local.
                    memory: MemoryLocation::HostUpload,
                })?;
                rollback.buffers.push(handle);
                Ok::<_, HalError>(handle)
            };
            let palette = buffer(
                "palette",
                u64::from(desc.joints) * JOINT_STRIDE as u64,
                BufferUsage::STORAGE,
            )?;
            let skin = buffer(
                "bindings",
                u64::from(desc.bindings) * SKIN_BINDING_STRIDE as u64,
                BufferUsage::STORAGE,
            )?;

            let mut frame_params = Vec::with_capacity(desc.ranges as usize);
            let mut frame_groups = Vec::with_capacity(desc.ranges as usize);
            for range in 0..desc.ranges {
                let block = buffer(
                    &format!("params {range}"),
                    PARAMS_SIZE as u64,
                    BufferUsage::UNIFORM,
                )?;
                let group = device.create_bind_group(&BindGroupDesc {
                    label: Some(&format!("{label} {frame} {range}")),
                    layout,
                    entries: &[
                        bound(0, block),
                        bound(1, palette),
                        bound(2, skin),
                        bound(3, desc.vertices),
                    ],
                    // No binding here is an array, so nothing has a runtime
                    // length to declare.
                    variable_count: None,
                })?;
                rollback.groups.push(group);
                frame_params.push(block);
                frame_groups.push(group);
            }

            palettes.push(palette);
            bindings.push(skin);
            params.push(frame_params);
            groups.push(frame_groups);
        }

        Ok(Self {
            palettes,
            bindings,
            params,
            groups,
            active: vec![Vec::new(); desc.frames],
            layout,
            pipeline_layout,
            pipeline,
            vertices: desc.vertices,
            range_capacity: desc.ranges,
            joint_capacity: desc.joints,
            binding_capacity: desc.bindings,
            frames_begun: 0,
        })
    }

    /// Ranges one frame can dispatch.
    #[must_use]
    pub const fn range_capacity(&self) -> u32 {
        self.range_capacity
    }

    /// Palette matrices one frame's joint buffer holds, across every range.
    #[must_use]
    pub const fn joint_capacity(&self) -> u32 {
        self.joint_capacity
    }

    /// Skin bindings one frame's binding buffer holds, across every range.
    #[must_use]
    pub const fn binding_capacity(&self) -> u32 {
        self.binding_capacity
    }

    /// Which half of every [`SkinnedRegion`] the frame most recently begun
    /// writes.
    ///
    /// Zero before the first [`begin_frame`](Self::begin_frame), which names a
    /// half no dispatch has filled — there is nothing to draw yet either way.
    #[must_use]
    pub const fn parity(&self) -> u32 {
        (self.frames_begun % 2) as u32
    }

    /// Validates `ranges`, alternates the ping-pong, and uploads the palette,
    /// the skin bindings and one uniform block per range.
    ///
    /// Call once per frame, before [`Skinning::add_pass`], against the frame
    /// slot the instance pool rotated to. A frame with no skinned meshes calls
    /// it with an empty slice, which clears the slot's plan — otherwise the
    /// pass would re-dispatch the ranges of whichever frame last used it.
    ///
    /// # Errors
    ///
    /// [`SkinningError::JointOutOfRange`], [`SkinningError::EmptyPalette`],
    /// [`SkinningError::BindingCountMismatch`] and
    /// [`SkinningError::OverCapacity`] for inputs the dispatch could not be
    /// built from, all of them checked **before** anything is written, so a
    /// refused frame leaves the slot's buffers alone; and
    /// [`SkinningError::Hal`] if a write failed, after which the slot
    /// dispatches nothing until a later frame succeeds on it.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub fn begin_frame(
        &mut self,
        device: &dyn Device,
        frame: usize,
        ranges: &[SkinRange<'_>],
    ) -> Result<(), SkinningError> {
        self.check(ranges)?;

        // Cleared before anything is written rather than replaced after: a
        // write that fails part-way would otherwise leave this slot dispatching
        // the *previous* frame's plan against uniform blocks half of which have
        // already been overwritten, which is a mesh skinned onto another
        // instance's palette. Nothing dispatched is the honest outcome.
        self.active[frame].clear();

        // The ping-pong, and the only place it moves: one step per frame that
        // was accepted, so `parity` alternates and the half a frame did not
        // write is the half the frame before it did.
        self.frames_begun = self.frames_begun.wrapping_add(1);
        let parity = self.parity();

        let mut palette_bytes = Vec::new();
        let mut binding_bytes = Vec::new();
        let mut plan = Vec::with_capacity(ranges.len());
        for (slot, range) in ranges.iter().enumerate() {
            let vertex_count = range.region.vertex_count();
            let block = Params {
                vertex_count,
                input_base: range.input_base,
                output_base: range.region.base(parity),
                binding_base: (binding_bytes.len() / SKIN_BINDING_STRIDE) as u32,
                joint_base: (palette_bytes.len() / JOINT_STRIDE) as u32,
                joint_count: range.palette.len() as u32,
            };
            // `to_bytes` panics on an overlap and on an empty palette. Neither
            // can happen here: `check` refused the empty palette by name, and
            // the two runs are separate `MeshPool` allocations, which the free
            // list never hands out twice. It is the backstop, not the check.
            device.write_buffer(self.params[frame][slot], 0, &block.to_bytes())?;

            for matrix in range.palette {
                for value in matrix.to_cols_array() {
                    palette_bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            for binding in range.bindings {
                binding_bytes.extend_from_slice(&binding.to_bytes());
            }
            plan.push(vertex_count.div_ceil(WORKGROUP_SIZE));
        }

        if !palette_bytes.is_empty() {
            device.write_buffer(self.palettes[frame], 0, &palette_bytes)?;
        }
        if !binding_bytes.is_empty() {
            device.write_buffer(self.bindings[frame], 0, &binding_bytes)?;
        }
        self.active[frame] = plan;
        Ok(())
    }

    /// Everything [`begin_frame`](Self::begin_frame) refuses, checked before it
    /// writes anything.
    fn check(&self, ranges: &[SkinRange<'_>]) -> Result<(), SkinningError> {
        let over = |what, capacity, asked| SkinningError::OverCapacity {
            what,
            capacity,
            asked,
        };
        if ranges.len() as u64 > u64::from(self.range_capacity) {
            return Err(over("ranges", self.range_capacity, ranges.len() as u64));
        }

        let mut joints = 0u64;
        let mut bindings = 0u64;
        for (index, range) in ranges.iter().enumerate() {
            if range.palette.is_empty() {
                return Err(SkinningError::EmptyPalette { range: index });
            }
            let vertices = range.region.vertex_count();
            if range.bindings.len() as u64 != u64::from(vertices) {
                return Err(SkinningError::BindingCountMismatch {
                    range: index,
                    vertices,
                    bindings: range.bindings.len(),
                });
            }
            // **The check the shader cannot make and `crcbl-scene` could not
            // make either** — see `SkinningError::JointOutOfRange`. Every slot
            // of every vertex, including the ones carrying zero weight: the
            // shader reads all four matrices before it scales any of them, so a
            // weightless slot naming a joint past the palette is the same
            // clamped read as a weighted one.
            for (vertex, binding) in range.bindings.iter().enumerate() {
                for (slot, joint) in binding.joints.iter().enumerate() {
                    if *joint as usize >= range.palette.len() {
                        return Err(SkinningError::JointOutOfRange {
                            range: index,
                            vertex,
                            slot,
                            joint: *joint,
                            palette: range.palette.len(),
                        });
                    }
                }
            }
            joints += range.palette.len() as u64;
            bindings += range.bindings.len() as u64;
        }
        if joints > u64::from(self.joint_capacity) {
            return Err(over("palette matrices", self.joint_capacity, joints));
        }
        if bindings > u64::from(self.binding_capacity) {
            return Err(over("skin bindings", self.binding_capacity, bindings));
        }
        Ok(())
    }

    /// How many passes [`add_pass`](Self::add_pass) adds to a frame.
    ///
    /// **At most one**, not exactly one: a frame whose
    /// [`begin_frame`](Self::begin_frame) was handed no ranges adds nothing,
    /// because a pass with an empty body would still cost the vertex pool two
    /// barriers it has no use for.
    pub const MAX_PASSES: u32 = 1;

    /// Adds the skinning pass to `graph` and returns the vertex pool's id, or
    /// [`None`] when this frame has nothing to skin.
    ///
    /// The id is what a caller declaring its own read of the pool must use: the
    /// graph orders this pass against a mesh pass only when both name the
    /// **same** buffer node, and importing the pool twice would be two
    /// resources it cannot order against each other. So a mesh pass drawing
    /// skinned geometry passes this id to
    /// [`PassBuilder::read_buffer`](crate::graph::PassBuilder::read_buffer),
    /// and must be added after this.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub fn add_pass(&self, graph: &mut RenderGraph<'_>, frame: usize) -> Option<BufferId> {
        let dispatches: Vec<(BindGroupHandle, u32)> = self.active[frame]
            .iter()
            .enumerate()
            .map(|(slot, &groups)| (self.groups[frame][slot], groups))
            .collect();
        if dispatches.is_empty() {
            return None;
        }

        // The pool arrives in the state `MeshPool::upload` left it in and the
        // state every mesh pass reads it in, which are the same one.
        let pool = graph.import_buffer(
            "vertex-pool",
            ImportedBuffer {
                buffer: self.vertices,
                initial: ResourceState::ShaderRead,
                final_state: ResourceState::ShaderRead,
            },
        );
        let pipeline = self.pipeline;
        let layout = self.pipeline_layout;
        graph
            .add_compute_pass("skinning")
            // One declaration for one binding: the bind pose is read and the
            // skinned run is written through the same descriptor, so there is
            // no read-only half to declare separately.
            .use_buffer(pool, ResourceState::ShaderReadWrite)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(pipeline);
                for (group, groups) in dispatches {
                    encoder.bind_group(0, group, &[], layout);
                    encoder.dispatch(groups, 1, 1);
                }
            });
        Some(pool)
    }

    /// Releases everything, in dependency order. The device must be idle.
    ///
    /// The vertex pool is **not** released here: it is
    /// [`crate::mesh_pool`]'s, and this pass only ever borrowed the handle.
    pub fn destroy(self, device: &dyn Device) {
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        for group in self.groups.into_iter().flatten() {
            device.destroy_bind_group(group);
        }
        device.destroy_bind_group_layout(self.layout);
        for buffer in self
            .palettes
            .into_iter()
            .chain(self.bindings)
            .chain(self.params.into_iter().flatten())
        {
            device.destroy_buffer(buffer);
        }
    }
}

/// What a failed [`Skinning::new`] has to give back, in the order it must.
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

// --- the oracle ------------------------------------------------------------

/// The matrix `computeMain` blends before it transforms anything: the four
/// palette matrices scaled by the four weights and summed.
///
/// **Blended first, transformed once** — the shader's own order, and for a
/// normal it is not the same answer as transforming four times and blending the
/// results. The kernel's header carries that argument.
///
/// `joints` are clamped against `palette.len() - 1` exactly as the shader
/// clamps them, so this reproduces what a malformed asset actually draws rather
/// than what it meant to.
///
/// # Panics
///
/// If `palette` is empty, which is the clamp's `joint_count - 1` wrapping and
/// what [`Params::to_bytes`](crcbl_shaders::skinning::Params::to_bytes) refuses
/// on the buffer side.
#[must_use]
pub fn blend(palette: &[Mat4], binding: &SkinBinding) -> Mat4 {
    assert!(
        !palette.is_empty(),
        "a skinned vertex with an empty joint palette has nothing to blend onto, and the \
         shader's index clamp would wrap"
    );
    let last = palette.len() - 1;
    let mut blended = Mat4::ZERO;
    for slot in 0..JOINTS_PER_VERTEX {
        let joint = (binding.joints[slot] as usize).min(last);
        blended += palette[joint] * binding.weights[slot];
    }
    blended
}

/// The matrix that carries a normal through the linear part `basis` carries a
/// tangent through: the **cofactor** matrix, matching `normal_basis` in
/// `crates/crcbl-shaders/shaders/skinning.slang` term for term.
///
/// Row `i` is the cross product of the other two rows in cyclic order, which is
/// `det(basis)` times the inverse transpose. A normal is perpendicular to a
/// surface and only an angle-preserving transform carries a perpendicular the
/// way it carries a tangent, so the bare 3×3 is the wrong matrix the moment a
/// joint carries a non-uniform scale.
///
/// The cross-product form rather than `inverse().transpose() * determinant()`:
/// it is what the shader computes, and it is defined for a singular basis —
/// where the inverse is not, and where a blend of joints can genuinely land.
#[must_use]
pub fn normal_basis(basis: Mat3) -> Mat3 {
    let (first, second, third) = (basis.row(0), basis.row(1), basis.row(2));
    // The three cross products are the cofactor's *rows*, and `from_cols` takes
    // columns — so this builds the transpose and puts it back.
    Mat3::from_cols(second.cross(third), third.cross(first), first.cross(second)).transpose()
}

/// One vertex as `computeMain` writes it: the oracle a readback compares
/// against.
///
/// Ordinary Rust, and nothing in a frame calls it — [`crate::cull`]'s
/// [`visible_instances`](crate::cull::visible_instances) is the same shape for
/// the same reason. A dispatch that runs and writes a buffer nobody reads is
/// indistinguishable from one that does nothing.
///
/// Everything the kernel does, in the order it does it:
///
/// * the four palette matrices are blended by the four weights, **used exactly
///   as stored and never renormalised** — a set summing to 0.9 pulls its vertex
///   a tenth of the way to the palette's origin, which is the loudest failure
///   available and what the shader deliberately keeps;
/// * the position goes through the blended matrix;
/// * the normal goes through [`normal_basis`] of its linear part, and is
///   normalised **only if it is non-zero** — the shader's guard, whose fallback
///   is the bind-pose normal, because a `NaN` written into the pool is read
///   again by the shadow passes;
/// * `color` and `uv` are copied through, because the destination is a
///   different range of the pool and a field left alone is a field holding
///   whatever the allocator last put there.
///
/// # Panics
///
/// If `palette` is empty — see [`blend`].
#[must_use]
pub fn skin_vertex(palette: &[Mat4], binding: &SkinBinding, vertex: &MeshVertex) -> MeshVertex {
    let blended = blend(palette, binding);
    let position = blended
        * Vec4::new(
            vertex.position[0],
            vertex.position[1],
            vertex.position[2],
            1.0,
        );
    let bind_pose = Vec3::new(vertex.normal[0], vertex.normal[1], vertex.normal[2]);
    let carried = normal_basis(Mat3::from_mat4(blended)) * bind_pose;
    let square_length = carried.dot(carried);
    let normal = if square_length > 0.0 {
        carried / square_length.sqrt()
    } else {
        bind_pose
    };
    MeshVertex {
        position: [position.x, position.y, position.z, 1.0],
        normal: [normal.x, normal.y, normal.z, 0.0],
        color: vertex.color,
        uv: vertex.uv,
    }
}

#[cfg(test)]
mod tests {
    use crcbl_hal::null::{Command, NullInstance, Recorder};
    use crcbl_hal::{CommandEncoderDesc, DeviceDesc, Instance, QueueHandle, QueueKind};
    use crcbl_shaders::mesh::VERTEX_STRIDE;

    use super::*;
    use crate::mesh_pool::MeshPoolDesc;
    use crate::transient::TransientPool;

    fn open() -> (Recorder, Box<dyn Device>, QueueHandle) {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (recorder, device, queue)
    }

    /// A pool wide enough for every reservation the tests below make, unless
    /// one is asking for the exhausted case on purpose.
    fn pool(device: &dyn Device, vertices: u32) -> MeshPool {
        MeshPool::new(
            device,
            &MeshPoolDesc {
                label: Some("skinning test"),
                vertex_capacity: vertices,
                index_capacity: 64,
                mesh_capacity: 4,
            },
        )
        .expect("the null backend accepts every descriptor")
    }

    /// A skinning pass over `pool`'s vertex buffer, sized for the frame the
    /// caller is about to build.
    fn skinning(
        device: &dyn Device,
        pool: &MeshPool,
        ranges: u32,
        joints: u32,
        bindings: u32,
    ) -> Skinning {
        Skinning::new(
            device,
            &SkinningDesc {
                label: Some("skinning test"),
                frames: 2,
                ranges,
                joints,
                bindings,
                vertices: pool.vertex_buffer(),
            },
        )
        .expect("the null backend accepts every descriptor")
    }

    /// `n` skin bindings, every vertex bound wholly to joint 0.
    fn rigid(n: usize) -> Vec<SkinBinding> {
        vec![
            SkinBinding {
                joints: [0; JOINTS_PER_VERTEX],
                weights: [1.0, 0.0, 0.0, 0.0],
            };
            n
        ]
    }

    /// Compiles and executes a one-pass graph over `frame`, so the recorder
    /// holds the commands the pass body really recorded.
    ///
    /// The encoder is **finished**, because the null backend buffers a command
    /// stream and hands it to the recorder there — a test that skipped it would
    /// read an empty command list and pass whatever the body did.
    fn run(
        recorder: &Recorder,
        device: &dyn Device,
        queue: QueueHandle,
        pass: &Skinning,
        frame: usize,
    ) {
        let mut transients = TransientPool::new();
        let mut graph = RenderGraph::new(queue);
        pass.add_pass(&mut graph, frame);
        let compiled = graph.compile(&transients).expect("a legal frame");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("skinning"),
            queue,
        });
        compiled
            .execute(device, &mut transients, encoder.as_mut(), None)
            .expect("the graph executed");
        let commands = encoder.finish().expect("a legal command stream");
        device.destroy_command_buffer(commands);
        // A dispatch outside a compute pass, a group bound to a dead object, a
        // binding wider than the device allows: the null backend records each
        // of those rather than refusing, so a test that never asked would pass
        // over a command stream Vulkan rejects.
        recorder.assert_valid();
    }

    /// Every `dispatch`'s group count, in the order they were recorded.
    fn dispatches(recorder: &Recorder) -> Vec<u32> {
        recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::Dispatch { x, y, z } => {
                    assert_eq!((y, z), (1, 1), "the kernel is one-dimensional");
                    Some(x)
                }
                _ => None,
            })
            .collect()
    }

    /// The six words of a range's uniform block, read back out of the bytes
    /// that actually reached the buffer rather than mirrored from the inputs.
    fn params_of(recorder: &Recorder, pass: &Skinning, frame: usize, slot: usize) -> Params {
        let bytes = recorder
            .buffer_bytes(pass.params[frame][slot])
            .expect("the block is live");
        assert_eq!(bytes.len(), PARAMS_SIZE);
        let word = |at: usize| {
            u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes of a word"))
        };
        Params {
            vertex_count: word(0),
            input_base: word(4),
            output_base: word(8),
            binding_base: word(12),
            joint_base: word(16),
            joint_count: word(20),
        }
    }

    // --- the drawable region -------------------------------------------------

    /// **Releasing a skinned mesh gives back the region**, and the space is
    /// reusable straight away.
    ///
    /// The second reservation is what says the vertices really came back rather
    /// than only that the free counter moved: a pool sized for one region and
    /// its source refuses the second reserve outright if the first leaked.
    #[test]
    fn releasing_a_skinned_mesh_returns_its_region() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut pool = pool(device, 64);
        const VERTICES: u32 = 8;
        let source = pool
            .upload(
                device,
                queue,
                "source",
                &vec![0u8; VERTICES as usize * VERTEX_STRIDE],
                &[0, 1, 2],
            )
            .expect("the pool has room");
        pool.flush(device).expect("the null backend completes it");
        let (free_before, _) = pool.vertex_space();

        let skinned = SkinnedMesh::reserve(&mut pool, source).expect("room for two halves");
        assert_eq!(
            skinned.vertex_count(),
            VERTICES,
            "each half is as long as the bind pose it deforms"
        );
        assert_eq!(
            skinned.input_base(),
            pool.mesh(source).expect("resident").base_vertex,
            "the range reads the bind pose where the pool put it"
        );
        assert_eq!(
            skinned.mesh_id(),
            source.index(),
            "a skinned instance names the mesh it was deformed from, so the bucket, the \
             level tables and the bounding box all resolve as the source's"
        );
        assert_eq!(
            pool.vertex_space().0,
            free_before - 2 * VERTICES,
            "two halves are taken, not one"
        );

        skinned.release(&mut pool);
        assert_eq!(
            pool.vertex_space().0,
            free_before,
            "the region goes back whole, and the free list coalesces it"
        );

        let again = SkinnedMesh::reserve(&mut pool, source).expect("the vertices came back");
        again.release(&mut pool);
        pool.destroy(device);
        recorder.assert_valid();
    }

    /// **A reservation takes no mesh-table entry**, so a pool whose table has no
    /// spare slot at all can still hold a skinned mesh.
    ///
    /// The table below has exactly one entry and the source mesh occupies it.
    /// Before 2026-08 this call took two more and a table of one refused it;
    /// what says the mechanism is gone rather than merely unused is that the
    /// reservation now succeeds here.
    #[test]
    fn a_reservation_needs_no_spare_table_entry() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut pool = MeshPool::new(
            device,
            &MeshPoolDesc {
                label: Some("no spare entry"),
                vertex_capacity: 64,
                index_capacity: 64,
                mesh_capacity: 1,
            },
        )
        .expect("the null backend accepts every descriptor");
        let source = pool
            .upload(
                device,
                queue,
                "source",
                &vec![0u8; 8 * VERTEX_STRIDE],
                &[0, 1, 2],
            )
            .expect("the pool has room");
        pool.flush(device).expect("the null backend completes it");

        let skinned = SkinnedMesh::reserve(&mut pool, source)
            .expect("a region is vertices, and the table is untouched");
        assert_eq!(
            skinned.mesh_id(),
            source.index(),
            "and the entry it draws through is the one the source already had"
        );
        skinned.release(&mut pool);

        pool.destroy(device);
        recorder.assert_valid();
    }

    /// A reservation the vertex pool has no room for leaks nothing.
    ///
    /// The half-and-then-refused path in [`SkinnedRegion::reserve`], reached
    /// through the call that wraps it: a pool with room for one half and not two
    /// must come back with its free list exactly as it was.
    #[test]
    fn a_refused_region_gives_back_the_half_it_took() {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        const VERTICES: u32 = 8;
        let mut pool = MeshPool::new(
            device,
            &MeshPoolDesc {
                label: Some("room for one half"),
                vertex_capacity: 2 * VERTICES + VERTICES / 2,
                index_capacity: 64,
                mesh_capacity: 4,
            },
        )
        .expect("the null backend accepts every descriptor");
        let source = pool
            .upload(
                device,
                queue,
                "source",
                &vec![0u8; VERTICES as usize * VERTEX_STRIDE],
                &[0, 1, 2],
            )
            .expect("the pool has room");
        pool.flush(device).expect("the null backend completes it");
        let space = pool.vertex_space();

        let refused = SkinnedMesh::reserve(&mut pool, source);
        assert!(
            refused.is_err(),
            "the pool has room for one half of the region and not for two: {refused:?}"
        );
        assert_eq!(
            pool.vertex_space(),
            space,
            "a refused reservation leaks no pool space"
        );

        pool.destroy(device);
        recorder.assert_valid();
    }

    // --- the oracle ---------------------------------------------------------

    /// A vertex, distinct in every field so a copy landing in the wrong lane
    /// could not compare equal by accident.
    fn vertex(position: Vec3, normal: Vec3) -> MeshVertex {
        MeshVertex {
            position: [position.x, position.y, position.z, 1.0],
            normal: [normal.x, normal.y, normal.z, 0.0],
            color: [0.25, 0.5, 0.75, 1.0],
            uv: [0.125, 0.375, 0.0, 0.0],
        }
    }

    /// **A vertex whose weight is entirely on one joint is transformed
    /// exactly** — the kernel's own claim, and the one case where linear blend
    /// skinning owes an exact answer rather than an approximation.
    ///
    /// Exact equality rather than a tolerance, and it is not a lucky one: the
    /// blend is `M * 1.0` plus three `M * 0.0`, and adding a zero matrix to a
    /// finite one changes no bit.
    #[test]
    fn a_vertex_bound_wholly_to_one_joint_is_transformed_exactly() {
        let joint = Mat4::from_rotation_translation(
            glam::Quat::from_rotation_z(core::f32::consts::FRAC_PI_3),
            Vec3::new(2.0, -3.0, 0.5),
        );
        let palette = [Mat4::IDENTITY, joint];
        let binding = SkinBinding {
            joints: [1, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        };
        let bind_pose = vertex(Vec3::new(1.0, 2.0, 3.0), Vec3::Y);

        let skinned = skin_vertex(&palette, &binding, &bind_pose);

        let expected = joint * Vec4::new(1.0, 2.0, 3.0, 1.0);
        assert_eq!(
            skinned.position,
            [expected.x, expected.y, expected.z, 1.0],
            "a rigidly bound vertex is the joint's transform of it and nothing else"
        );
        // A rotation is its own cofactor to within rounding, so the normal is
        // the rotated one; the tolerance is the normalise, not the blend.
        let want = Mat3::from_mat4(joint) * Vec3::Y;
        for (got, want) in skinned.normal[..3].iter().zip(want.to_array()) {
            assert!(
                (got - want).abs() < 1e-6,
                "a rigidly bound normal is the joint's rotation of it: {:?} against {want}",
                skinned.normal
            );
        }
        assert_eq!(skinned.normal[3], 0.0, "the normal's w is written as zero");
        assert_eq!(skinned.color, bind_pose.color, "colour is copied through");
        assert_eq!(skinned.uv, bind_pose.uv, "uv is copied through");
    }

    /// A vertex blended across two joints, against the value worked out by
    /// hand.
    ///
    /// Two pure translations and weights of a quarter and three quarters —
    /// both exact in binary, and both linear parts identity — so the blended
    /// matrix is the identity with `0.25 * t0 + 0.75 * t1` in its translation
    /// and the answer has no rounding in it at all.
    #[test]
    fn a_vertex_blended_across_two_joints_is_the_weighted_sum_of_both() {
        let palette = [
            Mat4::from_translation(Vec3::new(4.0, 0.0, 0.0)),
            Mat4::from_translation(Vec3::new(0.0, 8.0, 0.0)),
        ];
        let binding = SkinBinding {
            joints: [0, 1, 0, 0],
            weights: [0.25, 0.75, 0.0, 0.0],
        };
        let bind_pose = vertex(Vec3::new(1.0, 1.0, 1.0), Vec3::Z);

        let skinned = skin_vertex(&palette, &binding, &bind_pose);

        assert_eq!(
            skinned.position,
            [1.0 + 0.25 * 4.0, 1.0 + 0.75 * 8.0, 1.0, 1.0],
            "a quarter of the first joint's translation and three quarters of the second's"
        );
        assert_eq!(
            skinned.normal,
            [0.0, 0.0, 1.0, 0.0],
            "a blend of translations has an identity linear part, so the normal is unmoved"
        );
    }

    /// **A normal under a joint carrying a non-uniform scale goes through the
    /// cofactor matrix, and the bare 3×3 is a different vector.**
    ///
    /// The case the cofactor exists for: a scale of `(4, 1, 1)` stretches a
    /// 45° normal *toward* the stretched axis if it is carried like a tangent,
    /// and the surface's perpendicular tilts the other way. Both halves are
    /// asserted, because "it used the cofactor" is only evidence if the bare
    /// basis would have given something else.
    #[test]
    fn a_normal_under_a_non_uniform_scale_goes_through_the_cofactor_not_the_basis() {
        let scale = Mat4::from_scale(Vec3::new(4.0, 1.0, 1.0));
        let palette = [scale];
        let binding = SkinBinding {
            joints: [0, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        };
        let diagonal = Vec3::new(1.0, 1.0, 0.0).normalize();
        let skinned = skin_vertex(&palette, &binding, &vertex(Vec3::X, diagonal));

        // Cofactor of diag(4,1,1) is diag(1, 4, 4), so the normal tilts away
        // from x; the bare basis is diag(4,1,1) and tilts toward it.
        let through_cofactor = (Vec3::new(1.0, 4.0, 0.0)).normalize();
        let through_basis = (Vec3::new(4.0, 1.0, 0.0)).normalize();
        let got = Vec3::new(skinned.normal[0], skinned.normal[1], skinned.normal[2]);
        assert!(
            (got - through_cofactor).length() < 1e-6,
            "{got:?} is not the cofactor's answer {through_cofactor:?}"
        );
        assert!(
            (got - through_basis).length() > 0.5,
            "the bare basis would have given {through_basis:?}, which is not far from {got:?} — \
             this test would pass with the wrong matrix"
        );
        assert!(
            (got.length() - 1.0).abs() < 1e-6,
            "the shader normalises, so the written normal is unit length"
        );
    }

    /// The cross-product form and the textbook one agree wherever the textbook
    /// one is defined.
    ///
    /// The shader is written in cross products because they are defined for a
    /// singular basis, where `inverse()` is not — so this is what says the two
    /// are the same matrix and not merely two plausible ones.
    #[test]
    fn the_cofactor_matches_the_inverse_transpose_scaled_by_the_determinant() {
        for basis in [
            Mat3::from_mat4(Mat4::from_scale(Vec3::new(2.0, 3.0, 0.5))),
            Mat3::from_rotation_y(0.7) * Mat3::from_diagonal(Vec3::new(1.0, 4.0, 0.25)),
            // A mirrored basis: negative determinant, which is the case
            // `graphitemaster/normals_revisited` is about.
            Mat3::from_diagonal(Vec3::new(-1.0, 2.0, 3.0)),
        ] {
            let textbook = basis.inverse().transpose() * basis.determinant();
            let cofactor = normal_basis(basis);
            for (got, want) in cofactor
                .to_cols_array()
                .iter()
                .zip(textbook.to_cols_array())
            {
                assert!(
                    (got - want).abs() < 1e-4,
                    "{cofactor:?} is not {textbook:?} for basis {basis:?}"
                );
            }
        }
    }

    /// A blend that collapses to nothing keeps the bind-pose normal rather
    /// than writing a `NaN` into the pool.
    ///
    /// The shader's guarded normalise, and it is guarded because the geometry
    /// it would poison is read again by the shadow passes: a `NaN` position
    /// makes triangles vanish rather than making anything report an error.
    #[test]
    fn a_blend_that_collapses_to_zero_keeps_the_bind_pose_normal() {
        let palette = [Mat4::IDENTITY];
        let binding = SkinBinding {
            joints: [0; JOINTS_PER_VERTEX],
            weights: [0.0; JOINTS_PER_VERTEX],
        };
        let bind_pose = vertex(Vec3::new(1.0, 2.0, 3.0), Vec3::Y);

        let skinned = skin_vertex(&palette, &binding, &bind_pose);

        assert_eq!(
            skinned.position,
            [0.0, 0.0, 0.0, 1.0],
            "no weight at all puts the vertex at the palette's origin"
        );
        assert_eq!(
            skinned.normal, bind_pose.normal,
            "and the normal is the bind pose's, finite and already unit length"
        );
        assert!(
            skinned.normal.iter().all(|value| value.is_finite()),
            "nothing written into the vertex pool may be NaN"
        );
    }

    /// Weights that do not sum to one deflate their vertex toward the
    /// palette's origin, which is the failure the kernel deliberately keeps.
    ///
    /// Renormalising here would be this engine overruling
    /// `crcbl_scene::GltfPrimitive::weights`, which reports them as the file
    /// stored them so that a file whose weights do not sum is one whose author
    /// hears about it.
    #[test]
    fn weights_that_do_not_sum_to_one_deflate_the_vertex_rather_than_being_renormalised() {
        let palette = [Mat4::IDENTITY];
        let binding = SkinBinding {
            joints: [0; JOINTS_PER_VERTEX],
            weights: [0.9, 0.0, 0.0, 0.0],
        };
        let skinned = skin_vertex(
            &palette,
            &binding,
            &vertex(Vec3::new(10.0, 0.0, 0.0), Vec3::Y),
        );
        assert_eq!(
            skinned.position[0], 9.0,
            "a tenth of the way toward the origin, not renormalised back out to 10"
        );
    }

    /// A joint index past the palette is clamped the way the shader clamps it
    /// — so the oracle predicts what a malformed asset really draws, not what
    /// it meant to.
    #[test]
    fn a_joint_index_past_the_palette_is_clamped_the_way_the_shader_clamps_it() {
        let palette = [
            Mat4::from_translation(Vec3::X),
            Mat4::from_translation(Vec3::Y),
        ];
        let past = SkinBinding {
            joints: [99, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        };
        let last = SkinBinding {
            joints: [1, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        };
        let bind_pose = vertex(Vec3::ZERO, Vec3::Y);
        assert_eq!(
            skin_vertex(&palette, &past, &bind_pose).position,
            skin_vertex(&palette, &last, &bind_pose).position,
            "an out-of-range index resolves to the palette's last matrix"
        );
    }

    // --- the transient region ----------------------------------------------

    /// **The two halves alternate with the parity, and the one a frame does
    /// not write is the one the frame before it did.**
    ///
    /// This is `docs/plan/17-animation.md`'s 2026-07-27 correction as an
    /// assertion. A region that answered the same base for both parities would
    /// be a single-buffered one wearing the API of a double-buffered one, and
    /// nothing else in the tree would notice until TAA arrived.
    #[test]
    fn a_regions_two_halves_alternate_with_the_parity() {
        let (_recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 64);
        let region = SkinnedRegion::reserve(&mut pool, 8).expect("room for both halves");

        assert_ne!(
            region.base(0),
            region.base(1),
            "the two parities must name different runs, or the previous frame's skinned \
             positions are this frame's"
        );
        assert_eq!(region.base(0), region.previous_base(1));
        assert_eq!(region.base(1), region.previous_base(0));
        // The parity is a bit, so every even frame index is one half.
        assert_eq!(region.base(2), region.base(0));
        assert_eq!(region.base(3), region.base(1));
        assert_eq!(region.vertex_count(), 8);
    }

    /// Neither half overlaps the other, and neither overlaps the bind pose —
    /// which is the precondition
    /// [`Params::to_bytes`](crcbl_shaders::skinning::Params::to_bytes) panics
    /// on and which the allocator is what actually makes true.
    ///
    /// The bind pose here is a **real upload**, not a second reservation, so
    /// the claim is about the pool the renderer uses rather than about the free
    /// list in isolation.
    #[test]
    fn a_regions_halves_never_overlap_each_other_or_the_bind_pose() {
        let (_recorder, device, queue) = open();
        let mut pool = pool(device.as_ref(), 64);
        let count = 6u32;
        let handle = pool
            .upload(
                device.as_ref(),
                queue,
                "bind pose",
                &vec![0u8; count as usize * VERTEX_STRIDE],
                &[0, 1, 2],
            )
            .expect("room in both pools");
        pool.flush(device.as_ref()).expect("the null backend lands");
        let bind_pose = pool.mesh(handle).expect("resident").base_vertex;

        let region = SkinnedRegion::reserve(&mut pool, count).expect("room for both halves");
        let runs = [
            bind_pose..bind_pose + count,
            region.base(0)..region.base(0) + count,
            region.base(1)..region.base(1) + count,
        ];
        for (index, run) in runs.iter().enumerate() {
            for other in &runs[index + 1..] {
                assert!(
                    run.end <= other.start || other.end <= run.start,
                    "{run:?} overlaps {other:?}; one invocation would read a vertex another had \
                     already overwritten"
                );
            }
        }
        // And the block the pass would build from them is one `to_bytes`
        // accepts, on either parity.
        for parity in [0, 1] {
            let _ = Params {
                vertex_count: count,
                input_base: bind_pose,
                output_base: region.base(parity),
                binding_base: 0,
                joint_base: 0,
                joint_count: 1,
            }
            .to_bytes();
        }
    }

    /// Releasing gives both halves back, so a pool that held one region can
    /// hold the next.
    #[test]
    fn a_released_region_gives_both_halves_back() {
        let (_recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 16);
        let before = pool.vertex_space();
        let region = SkinnedRegion::reserve(&mut pool, 8).expect("exactly fits");
        assert_eq!(pool.vertex_space().0, 0, "both halves are taken");
        region.release(&mut pool);
        assert_eq!(
            pool.vertex_space(),
            before,
            "everything is free again, and in one block"
        );
        SkinnedRegion::reserve(&mut pool, 8).expect("the space really came back");
    }

    /// A pool with room for one half and not two gives the first one back
    /// rather than leaking it.
    ///
    /// The half-allocated failure is the one a rollback is written for, and it
    /// is invisible without this: the reservation succeeds, the error is
    /// returned, and the pool is quietly smaller for the rest of the process.
    #[test]
    fn a_pool_with_room_for_one_half_gives_it_back_rather_than_leaking_it() {
        let (_recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 8);
        let before = pool.vertex_space();
        let error = SkinnedRegion::reserve(&mut pool, 6).expect_err("no room for twelve vertices");
        assert!(
            matches!(error, MeshPoolError::PoolExhausted { .. }),
            "{error} does not name the exhausted pool"
        );
        assert_eq!(
            pool.vertex_space(),
            before,
            "the first half must have gone back"
        );
    }

    /// A reservation of nothing is refused by its own name.
    #[test]
    fn a_region_of_no_vertices_is_refused() {
        let (_recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 8);
        let error = SkinnedRegion::reserve(&mut pool, 0).expect_err("no address to hand back");
        assert!(
            matches!(error, MeshPoolError::EmptyReservation),
            "{error} does not name the empty reservation"
        );
    }

    // --- the pass ------------------------------------------------------------

    /// **The dispatch covers every vertex and no more.**
    ///
    /// One invocation is one vertex and the division rounds up, so a count that
    /// is not a multiple of the workgroup size still gets a whole last group —
    /// whose tail the shader discards itself. A dispatch sized with a truncating
    /// divide skins a *prefix* of the mesh, which reads as a rigging mistake
    /// rather than as a dispatch that was too short.
    #[test]
    fn the_dispatch_covers_every_vertex_and_no_more() {
        for count in [1u32, 63, 64, 65, 127, 128, 200] {
            let (recorder, device, queue) = open();
            let mut pool = pool(device.as_ref(), count * 2 + 1);
            let region = SkinnedRegion::reserve(&mut pool, count).expect("room");
            let mut pass = skinning(device.as_ref(), &pool, 1, 2, count);
            let bindings = rigid(count as usize);
            pass.begin_frame(
                device.as_ref(),
                0,
                &[SkinRange {
                    input_base: 0,
                    region: &region,
                    palette: &[Mat4::IDENTITY, Mat4::IDENTITY],
                    bindings: &bindings,
                }],
            )
            .expect("a legal frame");
            run(&recorder, device.as_ref(), queue, &pass, 0);

            let groups = dispatches(&recorder);
            assert_eq!(groups.len(), 1, "one range is one dispatch");
            assert_eq!(
                groups[0],
                count.div_ceil(WORKGROUP_SIZE),
                "{count} vertices at {WORKGROUP_SIZE} per group"
            );
            assert!(
                u64::from(groups[0]) * u64::from(WORKGROUP_SIZE) >= u64::from(count),
                "{count} vertices need at least that many invocations, and {} groups give {}",
                groups[0],
                u64::from(groups[0]) * u64::from(WORKGROUP_SIZE)
            );
            pass.destroy(device.as_ref());
            region.release(&mut pool);
            pool.destroy(device.as_ref());
        }
    }

    /// Three ranges are three dispatches, in the order they were handed over,
    /// each against its own bind group.
    #[test]
    fn every_range_gets_its_own_dispatch_and_its_own_bind_group() {
        let (recorder, device, queue) = open();
        let mut pool = pool(device.as_ref(), 64);
        let counts = [1u32, 2, 3];
        let regions: Vec<SkinnedRegion> = counts
            .iter()
            .map(|count| SkinnedRegion::reserve(&mut pool, *count).expect("room"))
            .collect();
        let mut pass = skinning(device.as_ref(), &pool, 3, 6, 8);
        let bindings: Vec<Vec<SkinBinding>> =
            counts.iter().map(|count| rigid(*count as usize)).collect();
        let ranges: Vec<SkinRange<'_>> = (0..3)
            .map(|index| SkinRange {
                input_base: 32 + index as u32,
                region: &regions[index],
                palette: &[Mat4::IDENTITY, Mat4::IDENTITY],
                bindings: &bindings[index],
            })
            .collect();
        pass.begin_frame(device.as_ref(), 1, &ranges)
            .expect("a legal frame");
        run(&recorder, device.as_ref(), queue, &pass, 1);

        assert_eq!(dispatches(&recorder), vec![1, 1, 1], "three ranges");
        let bound: Vec<_> = recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::BindGroup { group, .. } => Some(group),
                _ => None,
            })
            .collect();
        assert_eq!(
            bound, pass.groups[1],
            "each dispatch is preceded by that range's own group, in range order"
        );
    }

    /// **The uniform block names this frame's half**, and the next frame's
    /// names the other one.
    ///
    /// Read back out of the buffer the pass wrote rather than recomputed, so
    /// this is evidence about the upload and not about the plan repeating
    /// itself.
    #[test]
    fn the_params_block_names_this_frames_half_and_the_next_frames_names_the_other() {
        let (recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 64);
        let region = SkinnedRegion::reserve(&mut pool, 5).expect("room");
        let mut pass = skinning(device.as_ref(), &pool, 1, 3, 5);
        let bindings = rigid(5);
        let palette = [Mat4::IDENTITY; 3];

        let mut seen = Vec::new();
        for frame in [0usize, 1] {
            pass.begin_frame(
                device.as_ref(),
                frame,
                &[SkinRange {
                    input_base: 40,
                    region: &region,
                    palette: &palette,
                    bindings: &bindings,
                }],
            )
            .expect("a legal frame");
            let block = params_of(&recorder, &pass, frame, 0);
            assert_eq!(
                block,
                Params {
                    vertex_count: 5,
                    input_base: 40,
                    output_base: region.base(pass.parity()),
                    binding_base: 0,
                    joint_base: 0,
                    joint_count: 3,
                }
            );
            seen.push(block.output_base);
        }
        assert_ne!(
            seen[0], seen[1],
            "two consecutive frames must write different halves of the region"
        );
    }

    /// A second range's bases are the first range's lengths, so the two
    /// dispatches read disjoint runs of the shared palette and binding buffers.
    ///
    /// One base serving both would make the second character read the first
    /// one's skin.
    #[test]
    fn each_ranges_palette_and_binding_bases_follow_the_range_before_it() {
        let (recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 64);
        let first = SkinnedRegion::reserve(&mut pool, 3).expect("room");
        let second = SkinnedRegion::reserve(&mut pool, 4).expect("room");
        let mut pass = skinning(device.as_ref(), &pool, 2, 5, 7);
        let (three, four) = (rigid(3), rigid(4));
        pass.begin_frame(
            device.as_ref(),
            0,
            &[
                SkinRange {
                    input_base: 50,
                    region: &first,
                    palette: &[Mat4::IDENTITY; 2],
                    bindings: &three,
                },
                SkinRange {
                    input_base: 54,
                    region: &second,
                    palette: &[Mat4::IDENTITY; 3],
                    bindings: &four,
                },
            ],
        )
        .expect("a legal frame");

        let first_block = params_of(&recorder, &pass, 0, 0);
        let second_block = params_of(&recorder, &pass, 0, 1);
        assert_eq!((first_block.joint_base, first_block.binding_base), (0, 0));
        assert_eq!(
            (second_block.joint_base, second_block.binding_base),
            (2, 3),
            "the second range starts where the first one's palette and bindings ended"
        );
        assert_eq!((first_block.joint_count, second_block.joint_count), (2, 3));
    }

    /// The palette and the skin bindings reach their buffers as the bytes the
    /// shader indexes: `to_cols_array` order at
    /// [`JOINT_STRIDE`](crcbl_shaders::skinning::JOINT_STRIDE), and
    /// [`SkinBinding::to_bytes`] at its own stride.
    #[test]
    fn the_uploaded_palette_and_bindings_are_the_bytes_the_shader_reads() {
        let (recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 32);
        let region = SkinnedRegion::reserve(&mut pool, 2).expect("room");
        let mut pass = skinning(device.as_ref(), &pool, 1, 2, 2);
        let palette = [
            Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)),
            Mat4::from_scale(Vec3::new(2.0, 4.0, 8.0)),
        ];
        let bindings = [
            SkinBinding {
                joints: [1, 0, 1, 0],
                weights: [0.5, 0.25, 0.125, 0.125],
            },
            SkinBinding {
                joints: [0, 1, 0, 1],
                weights: [1.0, 0.0, 0.0, 0.0],
            },
        ];
        pass.begin_frame(
            device.as_ref(),
            0,
            &[SkinRange {
                input_base: 8,
                region: &region,
                palette: &palette,
                bindings: &bindings,
            }],
        )
        .expect("a legal frame");

        let written = recorder
            .buffer_bytes(pass.palettes[0])
            .expect("the palette is live");
        for (index, matrix) in palette.iter().enumerate() {
            let want: Vec<u8> = matrix
                .to_cols_array()
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            assert_eq!(
                &written[index * JOINT_STRIDE..(index + 1) * JOINT_STRIDE],
                &want[..],
                "palette matrix {index} is not the columns the shader reads with no transpose"
            );
        }
        let written = recorder
            .buffer_bytes(pass.bindings[0])
            .expect("the binding buffer is live");
        for (index, binding) in bindings.iter().enumerate() {
            assert_eq!(
                &written[index * SKIN_BINDING_STRIDE..(index + 1) * SKIN_BINDING_STRIDE],
                &binding.to_bytes()[..],
                "skin binding {index} is not what the shader crate encodes"
            );
        }
    }

    /// **A binding naming a joint the palette has not got is refused, loudly
    /// and by name.**
    ///
    /// The gap `docs/backlog.md` records: `crcbl-scene` cannot make this check
    /// because a glTF primitive does not know its skin at import, and the
    /// shader can only clamp. This call knows both, so it is where the refusal
    /// belongs — and the message has to name the vertex, or a rigger is left
    /// with "somewhere in this mesh".
    #[test]
    fn a_binding_naming_a_joint_the_palette_has_not_got_is_refused() {
        let (recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 32);
        let region = SkinnedRegion::reserve(&mut pool, 3).expect("room");
        let mut pass = skinning(device.as_ref(), &pool, 1, 2, 3);
        let mut bindings = rigid(3);
        // The third joint slot of the middle vertex, carrying no weight at all
        // — the shader reads all four matrices before it scales any of them, so
        // a weightless slot is the same clamped read as a weighted one.
        bindings[1].joints[2] = 7;

        let error = pass
            .begin_frame(
                device.as_ref(),
                0,
                &[SkinRange {
                    input_base: 0,
                    region: &region,
                    palette: &[Mat4::IDENTITY, Mat4::IDENTITY],
                    bindings: &bindings,
                }],
            )
            .expect_err("joint 7 of a two-joint palette");

        match error {
            SkinningError::JointOutOfRange {
                range,
                vertex,
                slot,
                joint,
                palette,
            } => assert_eq!((range, vertex, slot, joint, palette), (0, 1, 2, 7, 2)),
            other => panic!("{other} does not name the joint that is out of range"),
        }
        assert!(
            recorder
                .buffer_bytes(pass.params[0][0])
                .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0)),
            "a refused frame must not have written a uniform block"
        );
        assert!(
            pass.active[0].is_empty(),
            "and it must not have left a dispatch plan behind"
        );
    }

    /// The palette length is a range's own, not the buffer's: a joint index
    /// legal for a longer palette elsewhere in the frame is still refused here.
    #[test]
    fn a_joint_index_is_checked_against_its_own_ranges_palette() {
        let (_recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 32);
        let long = SkinnedRegion::reserve(&mut pool, 1).expect("room");
        let short = SkinnedRegion::reserve(&mut pool, 1).expect("room");
        let mut pass = skinning(device.as_ref(), &pool, 2, 6, 2);
        let mut named_three = rigid(1);
        named_three[0].joints[0] = 3;

        // Legal against a four-joint palette…
        pass.begin_frame(
            device.as_ref(),
            0,
            &[SkinRange {
                input_base: 0,
                region: &long,
                palette: &[Mat4::IDENTITY; 4],
                bindings: &named_three,
            }],
        )
        .expect("joint 3 of a four-joint palette");

        // …and refused against the two-joint one beside it.
        let error = pass
            .begin_frame(
                device.as_ref(),
                0,
                &[
                    SkinRange {
                        input_base: 0,
                        region: &long,
                        palette: &[Mat4::IDENTITY; 4],
                        bindings: &rigid(1),
                    },
                    SkinRange {
                        input_base: 1,
                        region: &short,
                        palette: &[Mat4::IDENTITY; 2],
                        bindings: &named_three,
                    },
                ],
            )
            .expect_err("joint 3 of a two-joint palette");
        assert!(
            matches!(error, SkinningError::JointOutOfRange { range: 1, .. }),
            "{error} blames the wrong range"
        );
    }

    /// An empty palette is refused here rather than left to the shader crate's
    /// panic, which is what would otherwise take the frame down.
    #[test]
    fn an_empty_palette_is_refused_rather_than_reaching_the_blocks_panic() {
        let (_recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 32);
        let region = SkinnedRegion::reserve(&mut pool, 1).expect("room");
        let mut pass = skinning(device.as_ref(), &pool, 1, 1, 1);
        let error = pass
            .begin_frame(
                device.as_ref(),
                0,
                &[SkinRange {
                    input_base: 0,
                    region: &region,
                    palette: &[],
                    bindings: &rigid(1),
                }],
            )
            .expect_err("no joints to blend onto");
        assert!(
            matches!(error, SkinningError::EmptyPalette { range: 0 }),
            "{error}"
        );
    }

    /// The skin bindings are parallel to the bind-pose run, so a list that is
    /// not one per vertex is refused rather than silently covering part of the
    /// mesh.
    #[test]
    fn a_binding_run_that_is_not_one_per_vertex_is_refused() {
        let (_recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 32);
        let region = SkinnedRegion::reserve(&mut pool, 4).expect("room");
        let mut pass = skinning(device.as_ref(), &pool, 1, 1, 8);
        for offered in [3usize, 5] {
            let error = pass
                .begin_frame(
                    device.as_ref(),
                    0,
                    &[SkinRange {
                        input_base: 0,
                        region: &region,
                        palette: &[Mat4::IDENTITY],
                        bindings: &rigid(offered),
                    }],
                )
                .expect_err("four vertices need four bindings");
            assert!(
                matches!(
                    error,
                    SkinningError::BindingCountMismatch {
                        vertices: 4,
                        bindings,
                        ..
                    } if bindings == offered
                ),
                "{error}"
            );
        }
    }

    /// Every capacity is refused by name rather than truncated: a character
    /// silently missing from the frame is what these bounds exist to prevent.
    #[test]
    fn a_frame_over_any_capacity_is_refused_and_names_which() {
        let (_recorder, device, _queue) = open();
        let mut pool = pool(device.as_ref(), 64);
        let region = SkinnedRegion::reserve(&mut pool, 2).expect("room");
        let two = rigid(2);
        let range = |palette: &'static [Mat4]| SkinRange {
            input_base: 0,
            region: &region,
            palette,
            bindings: &two,
        };
        const ONE: &[Mat4] = &[Mat4::IDENTITY];
        const THREE: &[Mat4] = &[Mat4::IDENTITY, Mat4::IDENTITY, Mat4::IDENTITY];

        let mut pass = skinning(device.as_ref(), &pool, 1, 8, 8);
        let error = pass
            .begin_frame(device.as_ref(), 0, &[range(ONE), range(ONE)])
            .expect_err("two ranges in a pass built for one");
        assert!(
            matches!(
                error,
                SkinningError::OverCapacity {
                    what: "ranges",
                    capacity: 1,
                    asked: 2
                }
            ),
            "{error}"
        );

        let mut pass = skinning(device.as_ref(), &pool, 2, 2, 8);
        let error = pass
            .begin_frame(device.as_ref(), 0, &[range(THREE)])
            .expect_err("three matrices in a buffer of two");
        assert!(
            matches!(
                error,
                SkinningError::OverCapacity {
                    what: "palette matrices",
                    capacity: 2,
                    asked: 3
                }
            ),
            "{error}"
        );

        let mut pass = skinning(device.as_ref(), &pool, 2, 8, 3);
        let error = pass
            .begin_frame(device.as_ref(), 0, &[range(ONE), range(ONE)])
            .expect_err("four bindings in a buffer of three");
        assert!(
            matches!(
                error,
                SkinningError::OverCapacity {
                    what: "skin bindings",
                    capacity: 3,
                    asked: 4
                }
            ),
            "{error}"
        );
    }

    /// A frame with nothing to skin adds no pass, so the vertex pool pays no
    /// barrier for a dispatch that would not have happened.
    ///
    /// And a slot whose previous frame *did* have ranges is cleared by the
    /// empty call rather than re-dispatching them, which is the failure the
    /// plan being per-slot state would otherwise produce.
    #[test]
    fn a_frame_with_nothing_to_skin_adds_no_pass_and_clears_the_slot() {
        let (recorder, device, queue) = open();
        let mut pool = pool(device.as_ref(), 32);
        let region = SkinnedRegion::reserve(&mut pool, 4).expect("room");
        let mut pass = skinning(device.as_ref(), &pool, 1, 1, 4);
        pass.begin_frame(
            device.as_ref(),
            0,
            &[SkinRange {
                input_base: 0,
                region: &region,
                palette: &[Mat4::IDENTITY],
                bindings: &rigid(4),
            }],
        )
        .expect("a legal frame");

        let mut graph = RenderGraph::new(queue);
        assert!(
            pass.add_pass(&mut graph, 0).is_some(),
            "a frame with a range has a pass"
        );

        pass.begin_frame(device.as_ref(), 0, &[])
            .expect("an empty frame is legal");
        recorder.clear();
        let mut graph = RenderGraph::new(queue);
        assert!(
            pass.add_pass(&mut graph, 0).is_none(),
            "a frame with no ranges adds nothing"
        );
        let compiled = graph.compile(&TransientPool::new()).expect("a legal frame");
        assert!(
            compiled.passes().is_empty(),
            "and the graph has no skinning pass in it"
        );
        run(&recorder, device.as_ref(), queue, &pass, 0);
        assert!(
            dispatches(&recorder).is_empty(),
            "nothing is dispatched for a frame with nothing to skin"
        );
    }

    /// The bind-group layout is the order `skinning.slang` declares its
    /// resources, and the vertex pool is the one writable entry.
    ///
    /// `crcbl_shaders::skinning` records the same table and holds the shader to
    /// it; this is the other half — a layout built against a different
    /// assignment binds the palette where the skin bindings belong, and every
    /// backend accepts it happily because both are read-only storage buffers.
    #[test]
    fn the_bind_group_layout_is_the_order_the_shader_declares() {
        let (recorder, device, _queue) = open();
        let pool = pool(device.as_ref(), 8);
        let pass = skinning(device.as_ref(), &pool, 1, 1, 1);
        let (_, entries) = recorder
            .bind_group_layouts_created()
            .into_iter()
            .find(|(label, _)| label.as_deref() == Some("skinning test"))
            .expect("the pass created its layout");
        assert_eq!(entries.len(), 4);
        assert!(matches!(
            entries[0].kind,
            crcbl_hal::BindingKind::UniformBuffer { .. }
        ));
        for (binding, read_only) in [(1usize, true), (2, true), (3, false)] {
            match entries[binding].kind {
                crcbl_hal::BindingKind::StorageBuffer { read_only: got, .. } => assert_eq!(
                    got, read_only,
                    "binding {binding} is the wrong kind of storage buffer"
                ),
                other => panic!("binding {binding} is {other:?}, not a storage buffer"),
            }
        }
        pass.destroy(device.as_ref());
    }

    /// Every object the pass created is released, and the vertex pool it
    /// borrowed is not.
    #[test]
    fn destroy_releases_everything_the_pass_made_and_nothing_it_borrowed() {
        let (recorder, device, _queue) = open();
        let pool = pool(device.as_ref(), 8);
        let borrowed = recorder.total_live_objects();
        let pass = skinning(device.as_ref(), &pool, 2, 4, 4);
        assert!(recorder.total_live_objects() > borrowed);
        pass.destroy(device.as_ref());
        assert_eq!(
            recorder.total_live_objects(),
            borrowed,
            "the pass released everything it made, and left the pool's buffers alone"
        );
        pool.destroy(device.as_ref());
        assert_eq!(
            recorder.total_live_objects(),
            0,
            "and the pool's own buffers were still there to release"
        );
    }

    /// A descriptor asking for no frames, ranges, joints or bindings is
    /// refused: a buffer of zero bytes is not a buffer on any backend the
    /// engine targets.
    #[test]
    fn a_descriptor_with_a_zero_in_it_is_refused() {
        let (_recorder, device, _queue) = open();
        let pool = pool(device.as_ref(), 8);
        let base = SkinningDesc {
            label: Some("zero"),
            frames: 1,
            ranges: 1,
            joints: 1,
            bindings: 1,
            vertices: pool.vertex_buffer(),
        };
        for desc in [
            SkinningDesc { frames: 0, ..base },
            SkinningDesc { ranges: 0, ..base },
            SkinningDesc { joints: 0, ..base },
            SkinningDesc {
                bindings: 0,
                ..base
            },
        ] {
            let error = Skinning::new(device.as_ref(), &desc).expect_err("a zero is refused");
            assert!(matches!(error, SkinningError::Hal(_)), "{error}");
        }
    }
}
