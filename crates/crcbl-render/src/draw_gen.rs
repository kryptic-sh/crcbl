//! GPU draw generation: the cull dispatch, the draw-argument dispatch, and the
//! buffers between them.
//!
//! ```text
//!  begin_frame ──▶ cull params (this frame's frustum)
//!
//!  add_passes ──┬─ compute "clear-counters" ──▶ visible_count, draw_args,
//!               │                                  draw_counts, mesh_args
//!               │                                        │ graph barrier
//!               ├─ compute "cull"      instances ──▶ visible + visible_count
//!               │                                        │ graph barrier
//!               └─ compute "draw-args" visible ──▶ visible_instances
//!                                              ──▶ draw_args + draw_counts
//!                                              ──▶ mesh_args
//!                                                        │ graph barrier
//!                        the caller's render pass ◀──────┘  IndirectArgument
//! ```
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.3, both halves: "compute pass:
//! frustum cull against instance AABBs → compacted visible instance list →
//! `draw_indexed_indirect` records + count buffer". The first dispatch is
//! `cull.slang` and the second is `draw_gen.slang`, with `clear_counters.slang`
//! ahead of both; **there is not one hand-written barrier here** — the three
//! passes declare what they touch and [`crate::graph`] computes the
//! transitions, including the one into [`ResourceState::IndirectArgument`] that
//! the seam calls the single most important barrier in a GPU-driven frame.
//!
//! # Buckets, and why the CPU still records a draw at all
//!
//! A draw's instances have to be addressable by the vertex stage, and
//! `mesh.slang`'s header measures the only number the four shader targets agree
//! about: `SV_InstanceID`, and only while the draw's first instance is zero. So
//! the arguments cannot be one per visible instance distinguished by a first
//! instance — that reaches the shader as the instance index on two targets and
//! as zero on the other two.
//!
//! What they are instead is §3.3's 2026-07-27 correction: **a fixed bucket
//! table**, one argument structure per bucket, with an instance count the GPU
//! writes and a contiguous run of surviving instance indices the vertex stage
//! walks. The caller records one indirect call per bucket — a number decided
//! when the table is built and independent of what the scene holds, which is the
//! property the whole stage is for.
//!
//! Today a bucket is one resident mesh, because the index range in an argument
//! structure is per draw and instances of two meshes cannot share one. The
//! `(material template, permutation, pass)` bucket the correction describes is
//! the same table with a longer key.
//!
//! # The counters a dispatch zeroes, and why it is a dispatch
//!
//! Both shaders only ever *add*: the cull pass's survivor counter, each bucket's
//! instance count and the y extent of its mesh dispatch are atomics, and an
//! argument buffer left holding last frame's totals would draw last frame's
//! scene twice over. Something has to zero them.
//!
//! The seam has a fill for exactly this — `CommandEncoder::fill_buffer`, "the
//! idiomatic way to zero an indirect count buffer" — and it is unusable here
//! twice over. A fill is legal only *outside* a pass, and a render-graph frame
//! is passes end to end; and `crcbl-dx12` refuses one outright, because D3D12's
//! fill is `ClearUnorderedAccessViewUint` over a descriptor from a
//! shader-visible heap, which that backend does not create.
//!
//! So the zero is a dispatch of its own, `clear_counters.slang`, scheduled
//! ahead of the cull pass by [`DrawGen::add_passes`] like any other producer —
//! and the barrier between its write and the cull pass's first atomic is the
//! graph's, computed from what the two declare. Every backend that can run the
//! two passes this zeroes for can run this one, which a fill is not true of.
//!
//! **Every per-frame buffer here is therefore
//! [`MemoryLocation::DeviceLocal`]**, which is not a tidiness point: D3D12 has
//! no unordered-access view of an upload-heap resource at all — the flag is
//! rejected at creation and the heap pins the resource to `GENERIC_READ` for
//! its lifetime — so the counters being host-visible and bound writable is what
//! took its device down.
//!
//! The ones the clearing pass owns carry [`BufferUsage::TRANSFER_DST`] as well
//! as `TRANSFER_SRC`, and for the same reason: a buffer only ever written by a
//! shader is one nothing can poison, and a test that cannot poison a counter
//! cannot tell a zero this pass wrote from the zero the allocation came with.
//! `crcbl-vk`'s `draw_gen` end-to-end fills them with a sentinel before the
//! frame.
//!
//! # The overflow is visible, and it is not silent
//!
//! `cull.slang`'s counter is the *true* survivor count and can exceed the
//! visible list's capacity; `draw_gen.slang` clamps before it indexes anything
//! and generates draws for the prefix that fit. [`DrawGen::visible_capacity`] is
//! what a caller sizes against, and the counter is where a scene that outgrew it
//! says so.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, ComputePipelineDesc, ComputePipelineHandle, Device, HalError, MemoryLocation,
    PipelineLayoutDesc, PipelineLayoutHandle, ResourceState, ShaderEntry, ShaderModuleDesc,
    ShaderStages,
};
use crcbl_shaders::{
    CLEAR_COUNTERS, CULL, DRAW_GEN, Stage, clear_counters, cull as cull_shader, draw_gen,
    level_select,
};

use crate::cull::Frustum;
use crate::graph::{BufferId, ImportedBuffer, RenderGraph};

/// How large a [`DrawGen`] is, and what it draws.
#[derive(Clone, Copy, Debug)]
pub struct DrawGenDesc<'a> {
    /// Debug name; every buffer is named after it.
    pub label: Option<&'a str>,
    /// The instance array, one buffer per frame in flight — the ring
    /// [`crate::instance_pool::InstancePool::buffers`] hands out. Its length is
    /// how many frames this keeps state for.
    pub instances: &'a [BufferHandle],
    /// The mesh table both dispatches resolve ranges and bounds out of.
    pub mesh_table: BufferHandle,
    /// Which mesh each bucket draws, as an index into the mesh table, in bucket
    /// order. One entry per indirect call the caller will record.
    pub bucket_meshes: &'a [u32],
    /// How many clusters each bucket's mesh has, in the same order — the x
    /// extent of that bucket's mesh dispatch, and the only word of
    /// [`GeneratedDraws::mesh_args`] the GPU does not decide.
    ///
    /// **Zero on every geometry path with no mesh stage**, where nothing reads
    /// those arguments. It is still the same length as
    /// [`bucket_meshes`](Self::bucket_meshes): the pass writes one structure
    /// per bucket whatever the path, exactly as it writes one draw argument
    /// per bucket.
    pub bucket_clusters: &'a [u32],
    /// Where each mesh's cluster DAG is, **indexed by mesh id** — so this is
    /// parallel to the mesh table and long enough for every id an instance can
    /// name, not one entry per bucket.
    ///
    /// [`MeshLevels::FLAT`](crcbl_shaders::level_select::MeshLevels::FLAT) with
    /// its `first_level` filled in is what an entry
    /// for a mesh with no hierarchy holds, and it is also what a caller whose
    /// geometry path selects per cluster hands over for every mesh — see
    /// `crcbl_shaders::level_select`, which is where that arrangement is written
    /// down.
    pub mesh_levels: &'a [crcbl_shaders::level_select::MeshLevels],
    /// Every mesh's DAG groups end to end, in the order `mesh_levels` points
    /// into. Empty where nothing has a hierarchy.
    pub level_groups: &'a [crcbl_shaders::level_select::LevelGroup],
    /// Which mesh draws which level: element `first_level + d` of a mesh's run
    /// is the mesh id of its level `d`.
    pub level_meshes: &'a [u32],
    /// Instances the pool this culls can hold.
    ///
    /// Sizes the visible list *and* every bucket's run, which is what makes a
    /// bucket unable to overflow: at most this many instances survive, and they
    /// are spread across the buckets rather than duplicated into each.
    ///
    /// It also sizes `docs/plan/25-lod.md`'s hysteresis state, which is one word
    /// per (instance slot, group) — see [`DrawGen::group_state`].
    pub instance_capacity: u32,
}

/// What one frame's generated draws live in.
///
/// Returned by [`DrawGen::add_passes`] and consumed by the caller's render
/// pass, which needs both halves: the [`BufferId`]s to declare its reads with,
/// so the graph transitions them, and the handles to name in the indirect call
/// itself.
#[derive(Clone, Copy, Debug)]
pub struct GeneratedDraws {
    /// The indirect arguments, [`draw_gen::DRAW_ARGS_SIZE`] bytes per bucket.
    pub args: BufferHandle,
    /// The same buffer as the graph knows it. Declare it in
    /// [`ResourceState::IndirectArgument`].
    pub args_id: BufferId,
    /// One `u32` draw count per bucket: `1` for a bucket with something in it
    /// and `0` for one without. What
    /// [`GeometryPath::IndirectCount`](crcbl_hal::GeometryPath::IndirectCount)'s
    /// call reads.
    pub counts: BufferHandle,
    /// The same buffer as the graph knows it.
    pub counts_id: BufferId,
    /// The per-bucket mesh-dispatch arguments,
    /// [`draw_gen::MESH_ARGS_SIZE`] bytes per bucket: `(clusters, surviving
    /// instances, 1)`. What
    /// [`CommandEncoder::draw_mesh_tasks_indirect`](crcbl_hal::CommandEncoder::draw_mesh_tasks_indirect)
    /// reads on [`GeometryPath::MeshShader`](crcbl_hal::GeometryPath::MeshShader).
    pub mesh_args: BufferHandle,
    /// The same buffer as the graph knows it. Declare it in
    /// [`ResourceState::IndirectArgument`].
    ///
    /// A buffer of its own rather than more words on
    /// [`args`](Self::args), because the mesh path reads *those* as a shader
    /// read in the same pass and a resource is in one state at a time.
    pub mesh_args_id: BufferId,
    /// The per-bucket runs of surviving instance indices, which the vertex
    /// stage reads. Declare it as a shader read.
    pub runs_id: BufferId,
    /// `docs/plan/25-lod.md`'s hysteresis state, as the graph knows it.
    ///
    /// Here because a caller whose geometry path has an amplification stage
    /// **reads** it: `mesh_cluster.slang` looks up the two groups a cluster
    /// names, so the pass that runs it declares [`ResourceState::ShaderRead`] on
    /// this id and the graph orders that read after the draw-argument pass's
    /// write. A caller on a path with no amplification stage declares nothing,
    /// and the state is then written and read by that pass alone.
    pub group_state_id: BufferId,
    /// The frame's culling statistics, as the graph knows them — the buffer
    /// [`DrawGen::visible_count`] hands out.
    ///
    /// Here because §3.5's amplification stage **writes** it: it counts
    /// surviving clusters into
    /// [`CLUSTER_SURVIVOR_WORD`](crcbl_shaders::cull::CLUSTER_SURVIVOR_WORD),
    /// so the pass that runs it declares
    /// [`ResourceState::ShaderReadWrite`] on this id and the graph orders that
    /// write after the draw-argument pass's read. A caller on a geometry path
    /// with no amplification stage declares nothing and the counter keeps the
    /// zero the clearing pass wrote.
    pub visible_count_id: BufferId,
}

/// The cull and draw-argument dispatches, and everything they need.
#[derive(Debug)]
pub struct DrawGen {
    /// Which mesh each bucket draws, as `draw_gen.slang` reads it. Written once:
    /// the bucket table is fixed when it is built.
    bucket_meshes: BufferHandle,
    /// How many clusters each bucket's mesh has, written once for the same
    /// reason: a mesh's clusters are decided when it becomes resident.
    bucket_clusters: BufferHandle,
    /// `docs/plan/25-lod.md`'s three selection tables, written once: a mesh's
    /// hierarchy is decided when it becomes resident, exactly as its clusters
    /// are.
    mesh_levels: BufferHandle,
    level_groups: BufferHandle,
    level_meshes: BufferHandle,
    /// The two buffer lengths the clearing dispatch zeroes. Shared by every
    /// frame's group, because both are fixed when the bucket table is built.
    clear_params: BufferHandle,
    /// `docs/plan/25-lod.md`'s hysteresis state: one word per (instance slot,
    /// group), holding whether that instance had that group expanded.
    ///
    /// **One buffer, deliberately not a ring**, and `draw_gen.slang`'s own
    /// declaration is where that is argued: a frame reads what the last frame
    /// wrote, an instance the frustum rejected writes nothing at all, and a
    /// per-frame slot would hand such an instance a state that is neither its
    /// own nor monotone — which is a crack rather than a stale level. Frames are
    /// ordered against each other by the graph instead, out of the
    /// `ShaderReadWrite` both the writing and the reading pass declare on it.
    ///
    /// Zeroed at build, which is where the monotonicity induction starts.
    group_state: BufferHandle,
    /// How many groups one instance's run of [`group_state`](Self::group_state)
    /// holds — every resident mesh's group count summed, and at least one so the
    /// buffer is never zero-length.
    group_stride: u32,

    // One per frame in flight, indexed by the caller's frame slot.
    /// The block naming the bucket count, the two capacities and this frame's
    /// selection camera.
    ///
    /// **Ringed since the uniform cut arrived**, where it used to be one shared
    /// buffer: the camera and the pixel budget change every frame, and a frame
    /// still in flight is a frame still reading them.
    gen_params: Vec<BufferHandle>,
    cull_params: Vec<BufferHandle>,
    visible: Vec<BufferHandle>,
    visible_count: Vec<BufferHandle>,
    runs: Vec<BufferHandle>,
    args: Vec<BufferHandle>,
    counts: Vec<BufferHandle>,
    mesh_args: Vec<BufferHandle>,
    clear_groups: Vec<BindGroupHandle>,
    cull_groups: Vec<BindGroupHandle>,
    gen_groups: Vec<BindGroupHandle>,

    clear_layout: BindGroupLayoutHandle,
    clear_pipeline_layout: PipelineLayoutHandle,
    clear_pipeline: ComputePipelineHandle,
    cull_layout: BindGroupLayoutHandle,
    cull_pipeline_layout: PipelineLayoutHandle,
    cull_pipeline: ComputePipelineHandle,
    gen_layout: BindGroupLayoutHandle,
    gen_pipeline_layout: PipelineLayoutHandle,
    gen_pipeline: ComputePipelineHandle,

    bucket_count: u32,
    capacity: u32,
}

impl DrawGen {
    /// Builds both pipelines, both bind groups per frame, and every buffer
    /// between them.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. A failure part-way through releases
    /// everything already created, so a caller that gives up leaves nothing
    /// behind.
    ///
    /// # Panics
    ///
    /// If `instances` is empty or `bucket_meshes` is: a ring with no buffers has
    /// nothing to bind, and a table with no buckets generates no draws at all.
    pub fn new(device: &dyn Device, desc: &DrawGenDesc<'_>) -> Result<Self, HalError> {
        assert!(
            !desc.instances.is_empty(),
            "draw generation needs at least one instance buffer to cull"
        );
        assert!(
            !desc.bucket_meshes.is_empty(),
            "draw generation with no buckets would generate no draws"
        );
        assert_eq!(
            desc.bucket_clusters.len(),
            desc.bucket_meshes.len(),
            "one cluster count per bucket: a shorter table would leave a bucket's mesh dispatch \
             reading a cluster count that belongs to another bucket, or to nothing"
        );
        let mut rollback = Rollback::default();
        match Self::build(device, desc, &mut rollback) {
            Ok(built) => Ok(built),
            Err(error) => {
                rollback.run(device);
                Err(error)
            }
        }
    }

    fn build(
        device: &dyn Device,
        desc: &DrawGenDesc<'_>,
        rollback: &mut Rollback,
    ) -> Result<Self, HalError> {
        let stem = desc.label.unwrap_or("draw generation");
        let bucket_count = u32::try_from(desc.bucket_meshes.len())
            .map_err(|_| HalError::InvalidDescriptor("more buckets than a u32".to_string()))?;
        let capacity = desc.instance_capacity;

        let mut buffer = |label: &str, size: u64, usage, memory| -> Result<_, HalError> {
            let handle = device.create_buffer(&BufferDesc {
                label: Some(&format!("{stem} {label}")),
                size,
                usage,
                memory,
            })?;
            rollback.buffers.push(handle);
            Ok(handle)
        };

        let bucket_meshes = buffer(
            "bucket table",
            u64::from(bucket_count) * 4,
            BufferUsage::STORAGE,
            MemoryLocation::HostUpload,
        )?;
        let mut table = Vec::with_capacity(desc.bucket_meshes.len() * 4);
        for mesh in desc.bucket_meshes {
            table.extend_from_slice(&mesh.to_le_bytes());
        }
        device.write_buffer(bucket_meshes, 0, &table)?;

        let bucket_clusters = buffer(
            "bucket clusters",
            u64::from(bucket_count) * 4,
            BufferUsage::STORAGE,
            MemoryLocation::HostUpload,
        )?;
        let mut clusters = Vec::with_capacity(desc.bucket_clusters.len() * 4);
        for count in desc.bucket_clusters {
            clusters.extend_from_slice(&count.to_le_bytes());
        }
        device.write_buffer(bucket_clusters, 0, &clusters)?;

        // `docs/plan/25-lod.md`'s selection tables. **Never zero-length**: a
        // buffer of no bytes is not a descriptor any backend will bind, and a
        // renderer whose meshes have no hierarchy at all is the ordinary case
        // rather than an error — so an empty table uploads one zeroed record
        // that no `MeshLevels` ever names.
        let mut table = |label: &str, bytes: Vec<u8>, stride: usize| -> Result<_, HalError> {
            let bytes = if bytes.is_empty() {
                vec![0u8; stride]
            } else {
                bytes
            };
            let handle = buffer(
                label,
                bytes.len() as u64,
                BufferUsage::STORAGE,
                MemoryLocation::HostUpload,
            )?;
            device.write_buffer(handle, 0, &bytes)?;
            Ok(handle)
        };
        let mesh_levels = table(
            "mesh levels",
            level_select::mesh_levels_bytes(desc.mesh_levels),
            level_select::MESH_LEVELS_STRIDE,
        )?;
        let level_groups = table(
            "level groups",
            level_select::level_group_bytes(desc.level_groups),
            level_select::LEVEL_GROUP_STRIDE,
        )?;
        let level_meshes = table(
            "level meshes",
            desc.level_meshes
                .iter()
                .flat_map(|mesh| mesh.to_le_bytes())
                .collect(),
            4,
        )?;

        // `docs/plan/25-lod.md`'s hysteresis state. **Zeroed here and by
        // nothing else ever again**: `draw_gen.slang` reads an element before it
        // writes it, so what the buffer holds on the very first frame is a real
        // input, and freshly allocated device memory holds whatever it holds. A
        // state that is not monotone up a DAG is a cut with a hole in it, so
        // this is the one write that keeps every later frame's induction
        // standing. `HostUpload` for that reason and no other — nothing writes
        // it from the host again.
        //
        // At least one word per instance even when nothing resident has a
        // hierarchy, because a zero-length buffer is not a descriptor any
        // backend binds and both shaders index it unconditionally.
        let group_stride = u32::try_from(desc.level_groups.len())
            .map_err(|_| HalError::InvalidDescriptor("more groups than a u32".to_string()))?
            .max(1);
        let group_state = buffer(
            "lod group state",
            u64::from(capacity) * u64::from(group_stride) * 4,
            BufferUsage::STORAGE,
            MemoryLocation::HostUpload,
        )?;
        device.write_buffer(
            group_state,
            0,
            &vec![0u8; capacity as usize * group_stride as usize * 4],
        )?;

        let clear_params = buffer(
            "clear params",
            clear_counters::PARAMS_SIZE as u64,
            BufferUsage::UNIFORM,
            MemoryLocation::HostUpload,
        )?;
        // The argument buffer's length in words, from the crate that owns the
        // argument layout — so the clearing shader never re-declares it.
        let args_words = bucket_count * draw_gen::DRAW_ARGS_WORDS as u32;
        device.write_buffer(
            clear_params,
            0,
            &clear_counters::Params {
                args_words,
                counts_words: bucket_count,
                stats_words: cull_shader::STATS_WORDS,
                mesh_args_words: bucket_count * draw_gen::MESH_ARGS_WORDS as u32,
            }
            .to_bytes(),
        )?;

        let frames = desc.instances.len();
        let mut gen_params = Vec::with_capacity(frames);
        let mut cull_params = Vec::with_capacity(frames);
        let mut visible = Vec::with_capacity(frames);
        let mut visible_count = Vec::with_capacity(frames);
        let mut runs = Vec::with_capacity(frames);
        let mut args = Vec::with_capacity(frames);
        let mut counts = Vec::with_capacity(frames);
        let mut mesh_args = Vec::with_capacity(frames);
        for frame in 0..frames {
            // The static half is written here and the camera half in
            // `begin_frame`, so a frame that never called it still names the
            // right bucket count rather than reading a zeroed block.
            let params = buffer(
                &format!("params {frame}"),
                draw_gen::PARAMS_SIZE as u64,
                BufferUsage::UNIFORM,
                MemoryLocation::HostUpload,
            )?;
            device.write_buffer(
                params,
                0,
                &draw_gen::Params {
                    bucket_count,
                    bucket_capacity: capacity,
                    visible_capacity: capacity,
                    group_stride,
                    ..draw_gen::Params::default()
                }
                .to_bytes(),
            )?;
            gen_params.push(params);
            cull_params.push(buffer(
                &format!("cull params {frame}"),
                cull_shader::PARAMS_SIZE as u64,
                BufferUsage::UNIFORM,
                MemoryLocation::HostUpload,
            )?);
            // `TRANSFER_SRC` on all five, and it is not there for tidiness:
            // everything these passes produce is written by a shader and read by
            // another, so a copy out is the *only* way anything can check that
            // what they produced is what a CPU would have recorded. `crcbl-vk`'s
            // `draw_gen` end-to-end does exactly that, and topic 03 §3.6's
            // culling-stats ring — the one readback the frame loop is allowed —
            // is the counter below.
            visible.push(buffer(
                &format!("visible {frame}"),
                u64::from(capacity) * 4,
                BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                MemoryLocation::DeviceLocal,
            )?);
            // `TRANSFER_DST` on the three the clearing dispatch owns, so a test
            // can poison them — see the module docs. They are device-local like
            // everything else here: nothing writes them from the host any more.
            //
            // **Two words, not one**, and they belong to different passes: the
            // cull dispatch counts surviving instances into the first and
            // `mesh_cluster.slang`'s amplification stage counts surviving
            // clusters into the second. One buffer keeps §3.6's ring at one
            // readback — see [`crcbl_shaders::cull::STATS_WORDS`].
            visible_count.push(buffer(
                &format!("cull stats {frame}"),
                u64::from(cull_shader::STATS_WORDS) * 4,
                BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
                MemoryLocation::DeviceLocal,
            )?);
            runs.push(buffer(
                &format!("bucket runs {frame}"),
                u64::from(bucket_count) * u64::from(capacity) * 4,
                BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                MemoryLocation::DeviceLocal,
            )?);
            args.push(buffer(
                &format!("draw args {frame}"),
                u64::from(bucket_count) * draw_gen::DRAW_ARGS_SIZE as u64,
                BufferUsage::STORAGE
                    | BufferUsage::INDIRECT
                    | BufferUsage::TRANSFER_SRC
                    | BufferUsage::TRANSFER_DST,
                MemoryLocation::DeviceLocal,
            )?);
            counts.push(buffer(
                &format!("draw counts {frame}"),
                u64::from(bucket_count) * 4,
                BufferUsage::STORAGE
                    | BufferUsage::INDIRECT
                    | BufferUsage::TRANSFER_SRC
                    | BufferUsage::TRANSFER_DST,
                MemoryLocation::DeviceLocal,
            )?);
            // The mesh path's dispatch extents, on the argument buffer's terms
            // exactly: written by the same pass, zeroed by the same one, and
            // read by a driver rather than by a shader — so `INDIRECT`, and
            // `TRANSFER_SRC` so a test can read back what the GPU decided.
            mesh_args.push(buffer(
                &format!("mesh dispatch args {frame}"),
                u64::from(bucket_count) * draw_gen::MESH_ARGS_SIZE as u64,
                BufferUsage::STORAGE
                    | BufferUsage::INDIRECT
                    | BufferUsage::TRANSFER_SRC
                    | BufferUsage::TRANSFER_DST,
                MemoryLocation::DeviceLocal,
            )?);
        }

        // --- the clearing pass ---
        let clear_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("clear counters"),
            entries: &[
                uniform(0),
                storage(1, false),
                storage(2, false),
                storage(3, false),
                storage(4, false),
            ],
        })?;
        rollback.bind_group_layouts.push(clear_layout);
        let clear_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("clear counters"),
            bind_group_layouts: &[clear_layout],
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(clear_pipeline_layout);
        let clear_pipeline = compute_pipeline(
            device,
            "clear counters",
            &CLEAR_COUNTERS,
            clear_pipeline_layout,
            clear_counters::WORKGROUP_SIZE,
        )?;
        rollback.pipelines.push(clear_pipeline);

        // --- the cull pass ---
        //
        // Binding order is `cull.slang`'s declaration order, which is a rule
        // rather than a convention: Slang's Metal target hands each resource the
        // next index in its own table, so a layout that renumbered them would
        // bind the frustum where the instance array goes. See
        // `crcbl_shaders`' declaration-order lint.
        let cull_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("cull"),
            entries: &[
                uniform(0),
                // `StructuredBuffer` in the shader, so read-only here: the cull
                // pass decides what is visible and never edits an instance or a
                // mesh entry.
                storage(1, true),
                storage(2, true),
                storage(3, false),
                storage(4, false),
            ],
        })?;
        rollback.bind_group_layouts.push(cull_layout);
        let cull_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("cull"),
            bind_group_layouts: &[cull_layout],
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(cull_pipeline_layout);
        let cull_pipeline = compute_pipeline(
            device,
            "cull",
            &CULL,
            cull_pipeline_layout,
            cull_shader::WORKGROUP_SIZE,
        )?;
        rollback.pipelines.push(cull_pipeline);

        // --- the draw-argument pass ---
        let gen_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("draw args"),
            entries: &[
                uniform(0),
                storage(1, true),
                storage(2, true),
                storage(3, true),
                storage(4, true),
                storage(5, true),
                storage(6, false),
                storage(7, false),
                storage(8, false),
                // The per-bucket cluster counts, read only: the mesh dispatch's
                // x extent is the host's and this pass copies it through.
                storage(9, true),
                storage(10, false),
                // `docs/plan/25-lod.md`'s selection tables, read only: what a
                // mesh's hierarchy is was decided when it became resident.
                storage(11, true),
                storage(12, true),
                storage(13, true),
                // The hysteresis state, read *and* written: this pass is the
                // only writer, and it reads the previous frame's answer out of
                // the same element it then overwrites.
                storage(14, false),
            ],
        })?;
        rollback.bind_group_layouts.push(gen_layout);
        let gen_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("draw args"),
            bind_group_layouts: &[gen_layout],
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(gen_pipeline_layout);
        let gen_pipeline = compute_pipeline(
            device,
            "draw args",
            &DRAW_GEN,
            gen_pipeline_layout,
            draw_gen::WORKGROUP_SIZE,
        )?;
        rollback.pipelines.push(gen_pipeline);

        let mut clear_groups = Vec::with_capacity(frames);
        let mut cull_groups = Vec::with_capacity(frames);
        let mut gen_groups = Vec::with_capacity(frames);
        for frame in 0..frames {
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("clear counters"),
                layout: clear_layout,
                entries: &[
                    bound(0, clear_params),
                    bound(1, visible_count[frame]),
                    bound(2, args[frame]),
                    bound(3, counts[frame]),
                    bound(4, mesh_args[frame]),
                ],
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            clear_groups.push(group);

            // **This frame's slot of every ring, not a shared buffer.** Binding
            // one buffer here for every group would undo the ring and put a
            // frame's writes where the previous frame is still reading.
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("cull"),
                layout: cull_layout,
                entries: &[
                    bound(0, cull_params[frame]),
                    bound(1, desc.instances[frame]),
                    bound(2, desc.mesh_table),
                    bound(3, visible[frame]),
                    bound(4, visible_count[frame]),
                ],
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            cull_groups.push(group);

            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("draw args"),
                layout: gen_layout,
                entries: &[
                    bound(0, gen_params[frame]),
                    bound(1, desc.instances[frame]),
                    bound(2, desc.mesh_table),
                    bound(3, visible[frame]),
                    bound(4, visible_count[frame]),
                    bound(5, bucket_meshes),
                    bound(6, runs[frame]),
                    bound(7, args[frame]),
                    bound(8, counts[frame]),
                    bound(9, bucket_clusters),
                    bound(10, mesh_args[frame]),
                    bound(11, mesh_levels),
                    bound(12, level_groups),
                    bound(13, level_meshes),
                    bound(14, group_state),
                ],
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            gen_groups.push(group);
        }

        rollback.disarm();
        Ok(Self {
            bucket_meshes,
            bucket_clusters,
            mesh_levels,
            level_groups,
            level_meshes,
            clear_params,
            group_state,
            group_stride,
            gen_params,
            cull_params,
            visible,
            visible_count,
            runs,
            args,
            counts,
            mesh_args,
            clear_groups,
            cull_groups,
            gen_groups,
            clear_layout,
            clear_pipeline_layout,
            clear_pipeline,
            cull_layout,
            cull_pipeline_layout,
            cull_pipeline,
            gen_layout,
            gen_pipeline_layout,
            gen_pipeline,
            bucket_count,
            capacity,
        })
    }

    /// Buckets in the table, which is how many indirect calls a caller records.
    #[must_use]
    pub const fn bucket_count(&self) -> u32 {
        self.bucket_count
    }

    /// Instances one bucket's run holds, which is also the visible list's
    /// capacity — see [`DrawGenDesc::instance_capacity`].
    #[must_use]
    pub const fn visible_capacity(&self) -> u32 {
        self.capacity
    }

    /// Where bucket `bucket`'s run starts in [`DrawGen::runs`], as the number
    /// `mesh.slang`'s `DrawConstants::base` carries.
    #[must_use]
    pub const fn bucket_base(&self, bucket: u32) -> u32 {
        bucket * self.capacity
    }

    /// Byte offset of bucket `bucket`'s argument structure.
    #[must_use]
    pub const fn args_offset(&self, bucket: u32) -> u64 {
        bucket as u64 * draw_gen::DRAW_ARGS_SIZE as u64
    }

    /// Byte offset of bucket `bucket`'s draw count.
    #[must_use]
    pub const fn count_offset(&self, bucket: u32) -> u64 {
        bucket as u64 * 4
    }

    /// Byte offset of bucket `bucket`'s mesh-dispatch argument structure — what
    /// [`DrawIndirect::offset`](crcbl_hal::DrawIndirect::offset) carries in the
    /// call that reads it.
    #[must_use]
    pub const fn mesh_args_offset(&self, bucket: u32) -> u64 {
        bucket as u64 * draw_gen::MESH_ARGS_SIZE as u64
    }

    /// The buffer of per-bucket instance runs for `frame`, which the drawing
    /// pass's bind group names.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn runs(&self, frame: usize) -> BufferHandle {
        self.runs[frame]
    }

    /// `frame`'s cull parameters — the frustum [`DrawGen::begin_frame`] wrote.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn cull_params(&self, frame: usize) -> BufferHandle {
        self.cull_params[frame]
    }

    /// `frame`'s compacted visible list — `cull.slang`'s survivors, as indices
    /// into the instance array, in no particular order.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn visible(&self, frame: usize) -> BufferHandle {
        self.visible[frame]
    }

    /// `frame`'s culling statistics: surviving instances in
    /// [`INSTANCE_SURVIVOR_WORD`](crcbl_shaders::cull::INSTANCE_SURVIVOR_WORD)
    /// and surviving clusters in
    /// [`CLUSTER_SURVIVOR_WORD`](crcbl_shaders::cull::CLUSTER_SURVIVOR_WORD).
    ///
    /// **Topic 03 §3.6's culling-stats readback is this buffer, on a delayed
    /// ring — the one readback the frame loop is allowed.** Both counters live
    /// here rather than in a buffer each so that stays one copy per frame.
    ///
    /// The instance count is the **true** one, which can exceed
    /// [`DrawGen::visible_capacity`] — so it is also where a scene that outgrew
    /// the list says so. The cluster count is what §3.5's amplification stage
    /// kept, and is zero on every path that has no amplification stage: the two
    /// indirect tails, and a device with `Features::MESH_SHADER` and no
    /// `Features::TASK_SHADER`.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn visible_count(&self, frame: usize) -> BufferHandle {
        self.visible_count[frame]
    }

    /// `frame`'s indirect arguments, [`draw_gen::DRAW_ARGS_SIZE`] bytes per
    /// bucket. The same buffer [`GeneratedDraws::args`] names.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn args(&self, frame: usize) -> BufferHandle {
        self.args[frame]
    }

    /// `frame`'s per-bucket draw counts. The same buffer
    /// [`GeneratedDraws::counts`] names.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn counts(&self, frame: usize) -> BufferHandle {
        self.counts[frame]
    }

    /// The per-bucket cluster counts the host wrote at build — the x extent of
    /// each bucket's mesh dispatch, before the pass copies it into
    /// [`GeneratedDraws::mesh_args`].
    ///
    /// Shared by every frame, because a mesh's clusters are decided when it
    /// becomes resident. Exposed for the reason the per-frame buffers are: what
    /// this holds is the only CPU-side half of an extent that is otherwise the
    /// GPU's, so a test with no GPU can still check it is each bucket's own.
    #[must_use]
    pub fn bucket_clusters(&self) -> BufferHandle {
        self.bucket_clusters
    }

    /// `frame`'s per-bucket mesh-dispatch arguments. The same buffer
    /// [`GeneratedDraws::mesh_args`] names, and the thing a test reads back to
    /// see that the extent the mesh path dispatched is the count culling
    /// produced rather than the instance pool's size.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn mesh_args(&self, frame: usize) -> BufferHandle {
        self.mesh_args[frame]
    }

    /// `docs/plan/25-lod.md`'s hysteresis state, for a caller binding it into a
    /// mesh pipeline that reads it.
    ///
    /// One buffer for every frame in flight, unlike everything else here that a
    /// caller binds — see [`DrawGen::group_state`](Self::group_state)'s field
    /// docs, which is where that is argued.
    #[must_use]
    pub fn group_state(&self) -> BufferHandle {
        self.group_state
    }

    /// The stride between two instances in that buffer, which is what
    /// [`ClusterDrawConstants::group_stride`] has to carry for the amplification
    /// stage to index it the same way this pass does.
    ///
    /// [`ClusterDrawConstants::group_stride`]: crcbl_shaders::meshlet::ClusterDrawConstants::group_stride
    #[must_use]
    pub const fn group_stride(&self) -> u32 {
        self.group_stride
    }

    /// Writes `frame`'s cull parameters — this frame's frustum, how much of the
    /// instance array to test, and the camera `docs/plan/25-lod.md`'s uniform
    /// cut selects a level from.
    ///
    /// The three counters both dispatches add to are **not** zeroed here: that
    /// is the clearing pass [`DrawGen::add_passes`] schedules, and the module
    /// docs say why it has to be a dispatch inside the frame rather than a host
    /// write before it.
    ///
    /// Call once per frame, before [`DrawGen::add_passes`], against the frame
    /// slot the instance pool rotated to — the buffer written here is the one
    /// that slot's bind groups name.
    ///
    /// `instance_count` is how many elements of the instance array the cull
    /// dispatch tests, which is
    /// [`InstancePool::slot_count`](crate::instance_pool::InstancePool::slot_count)
    /// and **not** its live count: an array with a hole in it still has live
    /// instances above the hole.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the write failed.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    /// `camera_position` and `lod_params` are the two the drawing pass writes
    /// into [`FrameUniforms`](crcbl_shaders::mesh::FrameUniforms) — passed in
    /// rather than re-derived here for the reason the frustum is: a pass that
    /// selects detail against one camera while another draws is a difference
    /// nothing in a frame can see.
    pub fn begin_frame(
        &self,
        device: &dyn Device,
        frame: usize,
        frustum: &Frustum,
        instance_count: u32,
        camera_position: [f32; 3],
        lod_params: [f32; 3],
    ) -> Result<(), HalError> {
        device.write_buffer(
            self.gen_params[frame],
            0,
            &draw_gen::Params {
                bucket_count: self.bucket_count,
                bucket_capacity: self.capacity,
                visible_capacity: self.capacity,
                group_stride: self.group_stride,
                camera_position,
                lod_params,
            }
            .to_bytes(),
        )?;
        device.write_buffer(
            self.cull_params[frame],
            0,
            &cull_shader::Params {
                planes: frustum.planes.map(|plane| plane.to_array()),
                instance_count,
                capacity: self.capacity,
            }
            .to_bytes(),
        )
    }

    /// Adds the cull and draw-argument passes to `graph` and returns what the
    /// caller's render pass draws from.
    ///
    /// The barriers between the three — including the transition into
    /// [`ResourceState::IndirectArgument`] — are the graph's, computed from the
    /// accesses declared here and the ones the caller declares on the
    /// [`GeneratedDraws`] ids.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    pub fn add_passes(
        &self,
        graph: &mut RenderGraph<'_>,
        frame: usize,
        instance_count: u32,
    ) -> GeneratedDraws {
        // Each buffer arrives in the state the *previous* frame that used this
        // slot left it in, which is the state declared as final below. Vacuous
        // on the first frame, when nothing has been written and there is nothing
        // to order against, and the real prior use on every later one.
        let import = |graph: &mut RenderGraph<'_>, label: &str, buffer, state| {
            graph.import_buffer(
                label,
                ImportedBuffer {
                    buffer,
                    initial: state,
                    final_state: state,
                },
            )
        };
        let visible = import(
            graph,
            "cull-visible",
            self.visible[frame],
            ResourceState::ShaderRead,
        );
        let visible_count = import(
            graph,
            "cull-count",
            self.visible_count[frame],
            ResourceState::ShaderRead,
        );
        let runs = import(
            graph,
            "bucket-runs",
            self.runs[frame],
            ResourceState::ShaderRead,
        );
        let args = import(
            graph,
            "draw-args",
            self.args[frame],
            ResourceState::IndirectArgument,
        );
        let counts = import(
            graph,
            "draw-counts",
            self.counts[frame],
            ResourceState::IndirectArgument,
        );
        let mesh_args = import(
            graph,
            "mesh-dispatch-args",
            self.mesh_args[frame],
            ResourceState::IndirectArgument,
        );
        // **Not indexed by `frame`**, and that is the point: this one buffer is
        // what carries a decision from the previous frame into this one. It
        // arrives in `ShaderReadWrite` because that is what the last frame left
        // it in, and `ResourceState::needs_barrier` answers `true` for any
        // transition touching a write — so the first barrier of this frame
        // carries a source scope covering that frame's writes and the mesh
        // stage's reads of them, whether or not that frame is still in flight.
        let group_state = import(
            graph,
            "lod-group-state",
            self.group_state,
            ResourceState::ShaderReadWrite,
        );

        // The zero every atomic below counts up from, and the first thing in the
        // frame that touches any of the three. Its barrier into the cull pass is
        // the graph's, out of the `ShaderReadWrite` declared on both sides.
        let clear_pipeline = self.clear_pipeline;
        let clear_layout = self.clear_pipeline_layout;
        let clear_group = self.clear_groups[frame];
        // The longest of the three buffers, which is the arguments: one
        // structure per bucket, and `new` refuses a table with no buckets, so
        // this is never the empty dispatch Metal rejects.
        let clear_groups = (self.bucket_count * draw_gen::DRAW_ARGS_WORDS as u32)
            .div_ceil(clear_counters::WORKGROUP_SIZE);
        graph
            .add_compute_pass("clear-counters")
            .use_buffer(visible_count, ResourceState::ShaderReadWrite)
            .use_buffer(args, ResourceState::ShaderReadWrite)
            .use_buffer(counts, ResourceState::ShaderReadWrite)
            .use_buffer(mesh_args, ResourceState::ShaderReadWrite)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(clear_pipeline);
                encoder.bind_group(0, clear_group, &[], clear_layout);
                encoder.dispatch(clear_groups, 1, 1);
            });

        let cull_pipeline = self.cull_pipeline;
        let cull_layout = self.cull_pipeline_layout;
        let cull_group = self.cull_groups[frame];
        let cull_groups = instance_count.div_ceil(cull_shader::WORKGROUP_SIZE);
        graph
            .add_compute_pass("cull")
            // `ShaderReadWrite` rather than a write-only state for both: a
            // storage-buffer descriptor permits reads whatever the shader does
            // with it, and the counter is genuinely read-modify-written.
            .use_buffer(visible, ResourceState::ShaderReadWrite)
            .use_buffer(visible_count, ResourceState::ShaderReadWrite)
            .execute(move |ctx| {
                // An empty instance array is a dispatch of no workgroups, which
                // Metal rejects outright rather than treating as a no-op. There
                // is nothing to cull, so there is nothing to record.
                if cull_groups == 0 {
                    return;
                }
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(cull_pipeline);
                encoder.bind_group(0, cull_group, &[], cull_layout);
                encoder.dispatch(cull_groups, 1, 1);
            });

        let gen_pipeline = self.gen_pipeline;
        let gen_layout = self.gen_pipeline_layout;
        let gen_group = self.gen_groups[frame];
        // One invocation owns bucket `i` *and* scatters visible instance `i`, so
        // the dispatch covers the larger of the two — and is never empty, because
        // the static half of every bucket's arguments has to be written even for
        // a frame that culled everything.
        let gen_groups = instance_count
            .max(self.bucket_count)
            .div_ceil(draw_gen::WORKGROUP_SIZE);
        graph
            .add_compute_pass("draw-args")
            .read_buffer(visible)
            .read_buffer(visible_count)
            .use_buffer(runs, ResourceState::ShaderReadWrite)
            .use_buffer(args, ResourceState::ShaderReadWrite)
            .use_buffer(counts, ResourceState::ShaderReadWrite)
            .use_buffer(mesh_args, ResourceState::ShaderReadWrite)
            .use_buffer(group_state, ResourceState::ShaderReadWrite)
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_compute_pipeline(gen_pipeline);
                encoder.bind_group(0, gen_group, &[], gen_layout);
                encoder.dispatch(gen_groups, 1, 1);
            });

        GeneratedDraws {
            args: self.args[frame],
            args_id: args,
            counts: self.counts[frame],
            counts_id: counts,
            mesh_args: self.mesh_args[frame],
            mesh_args_id: mesh_args,
            runs_id: runs,
            group_state_id: group_state,
            visible_count_id: visible_count,
        }
    }

    /// Releases everything, in dependency order. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        device.destroy_compute_pipeline(self.gen_pipeline);
        device.destroy_pipeline_layout(self.gen_pipeline_layout);
        device.destroy_compute_pipeline(self.cull_pipeline);
        device.destroy_pipeline_layout(self.cull_pipeline_layout);
        device.destroy_compute_pipeline(self.clear_pipeline);
        device.destroy_pipeline_layout(self.clear_pipeline_layout);
        for group in self
            .gen_groups
            .into_iter()
            .chain(self.cull_groups)
            .chain(self.clear_groups)
        {
            device.destroy_bind_group(group);
        }
        device.destroy_bind_group_layout(self.gen_layout);
        device.destroy_bind_group_layout(self.cull_layout);
        device.destroy_bind_group_layout(self.clear_layout);
        for buffer in [
            self.bucket_meshes,
            self.bucket_clusters,
            self.mesh_levels,
            self.level_groups,
            self.level_meshes,
            self.clear_params,
            self.group_state,
        ]
        .into_iter()
        .chain(self.gen_params)
        .chain(self.cull_params)
        .chain(self.visible)
        .chain(self.visible_count)
        .chain(self.runs)
        .chain(self.args)
        .chain(self.counts)
        .chain(self.mesh_args)
        {
            device.destroy_buffer(buffer);
        }
    }
}

/// A uniform-buffer layout entry for the compute stage.
const fn uniform(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        kind: BindingKind::UniformBuffer { dynamic: false },
        count: 1,
        flags: BindingFlags::empty(),
    }
}

/// A storage-buffer layout entry for the compute stage. `read_only` is the
/// shader's own `StructuredBuffer` versus `RWStructuredBuffer`, so it is the
/// truth rather than a hint.
const fn storage(binding: u32, read_only: bool) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        kind: BindingKind::StorageBuffer {
            read_only,
            dynamic: false,
        },
        count: 1,
        flags: BindingFlags::empty(),
    }
}

/// A whole-buffer bind group entry.
const fn bound(binding: u32, buffer: BufferHandle) -> BindGroupEntry {
    BindGroupEntry {
        binding,
        array_index: 0,
        resource: BindingResource::whole_buffer(buffer),
    }
}

/// Creates a compute pipeline from `shader`'s single compute entry point,
/// destroying the module whether or not the pipeline was created.
fn compute_pipeline(
    device: &dyn Device,
    label: &str,
    shader: &crcbl_shaders::Shader,
    layout: PipelineLayoutHandle,
    workgroup_size: u32,
) -> Result<ComputePipelineHandle, HalError> {
    // Resolved before the module exists, for `crate::forward`'s reason: a
    // manifest that disagreed with the artifact would otherwise fail inside the
    // descriptor literal, with the module already created and nothing holding
    // it.
    let entry_point = shader.entry_point(Stage::Compute).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "{}.slang exposes no unambiguous compute entry point; the committed SPIR-V and its \
             manifest disagree, which crates/crcbl-shaders/tools/compile-shaders.sh would fix",
            shader.name()
        ))
    })?;
    let module = device.create_shader_module(&ShaderModuleDesc {
        label: Some(shader.name()),
        spirv: shader.spirv(),
        wgsl: shader.wgsl(),
        msl: shader.msl(),
        // One DXIL container per entry point, all of them, exactly as the
        // two-stage graphics modules pass theirs — the backend picks the one
        // `entry_point` names below.
        dxil: &shader.dxil_containers(),
    })?;
    let pipeline = device.create_compute_pipeline(&ComputePipelineDesc {
        label: Some(label),
        layout,
        compute: ShaderEntry {
            module,
            entry_point,
        },
        // The shader's own number rather than a literal: a dispatch sized
        // against a different one tests part of the array and reads as a cull.
        workgroup_size: [workgroup_size, 1, 1],
    });
    device.destroy_shader_module(module);
    pipeline
}

/// What a partly-built [`DrawGen`] has to give back.
///
/// `build` creates two dozen objects with `?` between them and the seam's
/// `destroy_*` is explicit, so a failure half way through would otherwise leak
/// everything created before it.
#[derive(Debug, Default)]
struct Rollback {
    buffers: Vec<BufferHandle>,
    bind_groups: Vec<BindGroupHandle>,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    pipelines: Vec<ComputePipelineHandle>,
}

impl Rollback {
    /// Forgets everything, once the objects have an owner that will destroy
    /// them.
    fn disarm(&mut self) {
        *self = Self::default();
    }

    /// Releases everything, in the same dependency order as
    /// [`DrawGen::destroy`].
    fn run(self, device: &dyn Device) {
        for handle in self.pipelines {
            device.destroy_compute_pipeline(handle);
        }
        for handle in self.pipeline_layouts {
            device.destroy_pipeline_layout(handle);
        }
        for handle in self.bind_groups {
            device.destroy_bind_group(handle);
        }
        for handle in self.bind_group_layouts {
            device.destroy_bind_group_layout(handle);
        }
        for handle in self.buffers {
            device.destroy_buffer(handle);
        }
    }
}
