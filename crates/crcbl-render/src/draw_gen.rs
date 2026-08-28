//! GPU draw generation: the cull dispatch, the draw-argument dispatch, and the
//! buffers between them.
//!
//! ```text
//!  begin_frame ──▶ cull params (this frame's frustum)
//!
//!  add_passes ──┬─ compute "clear-counters" ──▶ cull stats, draw args,
//!               │                             counts+mesh args
//!               │                                        │ graph barrier
//!               ├─ compute "cull"      instances ──▶ survivors | · · ·
//!               │                                 ──▶ cull stats
//!               │                                        │ graph barrier
//!               └─ compute "draw-args" survivors ──▶ · · · | bucket runs
//!                                               ──▶ draw args
//!                                               ──▶ counts+mesh args
//!                                                        │ graph barrier
//!                        the caller's render pass ◀──────┘  IndirectArgument
//! ```
//!
//! `a | b` is one buffer with two regions; the passes below say which is which.
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
//! is passes end to end; and `crcbl-dx12` refuses one outright — a deliberate
//! non-fix argued at its `fill_buffer`, D3D12's fill being
//! `ClearUnorderedAccessViewUint` over a descriptor from a shader-visible heap.
//! The first reason is the one that binds here regardless of what any backend
//! decides: a frame of passes has nowhere to put a fill.
//!
//! So the zero is a dispatch of its own, `clear_counters.slang`, scheduled
//! ahead of the cull pass by [`DrawGen::add_passes`] like any other producer —
//! and the barrier between its write and the cull pass's first atomic is the
//! graph's, computed from what the two declare. Every backend that can run the
//! two passes this zeroes for can run this one, which a fill is not true of.
//!
//! **Every buffer here a shader writes is therefore
//! [`MemoryLocation::DeviceLocal`]**, which is not a tidiness point: D3D12 has
//! no unordered-access view of an upload-heap resource at all — the flag is
//! rejected at creation and the heap pins the resource to `GENERIC_READ` for
//! its lifetime — so the counters being host-visible and bound writable is what
//! took its device down. That rule is about the *binding*, not the frequency:
//! `docs/plan/25-lod.md`'s hysteresis state is written once a frame by
//! `draw_gen.slang` and is device-local for the same reason the counters are,
//! even though nothing zeroes it per frame. What does zero it is a start-up
//! copy, on [`crate::mesh_pool`]'s and [`crate::texture`]'s terms — see
//! [`DrawGen::group_state`] for why zeroing it twice would be worse than not
//! zeroing it at all. Read-only bindings are untouched by any of this, and the
//! tables beside it stay host-visible.
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
//!
//! # Buffers here are shared, and the accessors are views rather than allocations
//!
//! **The draw-argument pass binds eight storage buffers**, which is what a
//! WebGPU device guarantees per shader stage — see `shaders/draw_gen.slang`'s
//! header, which is where the merges and their costs are argued. It bound
//! fourteen until 2026-08 and could therefore not be created on a browser at the
//! default limits, nor on SwiftShader, which is every CI runner without a GPU.
//!
//! The consequence for a caller is that several of the accessors below **hand
//! back the same buffer**:
//!
//! * [`DrawGen::visible`] and [`DrawGen::runs`] are one buffer. The survivor
//!   list is at offset zero and bucket `b`'s run starts at
//!   [`DrawGen::bucket_base`]`(b) * 4` bytes.
//! * [`DrawGen::counts`] and [`DrawGen::mesh_args`] are one buffer. The counts
//!   are at offset zero and bucket `b`'s dispatch extents at
//!   [`DrawGen::mesh_args_offset`]`(b)`.
//! * Every host-written table — the bucket table, the per-bucket cluster counts
//!   and `docs/plan/25-lod.md`'s three selection tables — is one buffer, packed
//!   by [`crcbl_shaders::draw_gen::pack_tables`].
//!
//! So a reader that copies a region back must take its offset from the accessor
//! that names it. Reading at zero because a region used to be its own allocation
//! reads the region in front of it, and the words it finds there are plausible
//! `u32`s.

use crcbl_hal::{
    Barriers, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource,
    BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferUsage, CommandEncoderDesc,
    ComputePipelineDesc, ComputePipelineHandle, Device, HalError, MemoryLocation,
    PipelineLayoutDesc, PipelineLayoutHandle, QueueHandle, ResourceState, ShaderEntry,
    ShaderModuleDesc, ShaderStages, SubmitInfo, check_portable_storage_buffers,
};
use crcbl_shaders::{
    CLEAR_COUNTERS, CULL, DRAW_GEN, Stage, clear_counters, cull as cull_shader, draw_gen,
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
    /// extent of that bucket's mesh dispatch, and the only word of the
    /// mesh-dispatch arguments — see [`DrawGen::mesh_args`] — the GPU does not
    /// decide.
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
    ///
    /// **A buffer of its own**, unlike the counts and dispatch extents below,
    /// and for the one reason that survives the storage-buffer squeeze the module
    /// docs describe: the mesh path reads these as a shader read in the same pass
    /// that executes those as indirect arguments, and a resource is in one state
    /// at a time.
    pub args: BufferHandle,
    /// The same buffer as the graph knows it. Declare it in
    /// [`ResourceState::IndirectArgument`], or as a shader read on the mesh path.
    pub args_id: BufferId,
    /// The per-bucket draw counts **and** the per-bucket mesh-dispatch
    /// arguments, in that order, in one buffer.
    ///
    /// At [`DrawGen::count_offset`]`(b)` is bucket `b`'s `u32` draw count: `1`
    /// for a bucket with something in it and `0` for one without, which is what
    /// [`GeometryPath::IndirectCount`](crcbl_hal::GeometryPath::IndirectCount)'s
    /// call reads. At [`DrawGen::mesh_args_offset`]`(b)` are its
    /// [`draw_gen::MESH_ARGS_SIZE`] bytes of `(clusters, surviving instances,
    /// 1)`, which is what
    /// [`CommandEncoder::draw_mesh_tasks_indirect`](crcbl_hal::CommandEncoder::draw_mesh_tasks_indirect)
    /// reads on
    /// [`GeometryPath::MeshShader`](crcbl_hal::GeometryPath::MeshShader).
    ///
    /// One buffer because no pass reads both: the two indirect tails execute the
    /// counts and the mesh path executes the extents, so it is in
    /// [`ResourceState::IndirectArgument`] either way and never in two states at
    /// once.
    pub counts: BufferHandle,
    /// The same buffer as the graph knows it. Declare it in
    /// [`ResourceState::IndirectArgument`] — **once**, whichever region the
    /// pass reads.
    pub counts_id: BufferId,
    /// `cull.slang`'s survivor list **and** the per-bucket runs of surviving
    /// instance indices the vertex stage reads, in that order, in one buffer.
    /// Declare it as a shader read.
    ///
    /// A pass that draws reads only the runs, at
    /// [`DrawGen::bucket_base`]`(b)` words in — which is the number
    /// [`DrawConstants::base`](crcbl_shaders::mesh::DrawConstants::base) carries.
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
    /// Every host-written table `draw_gen.slang` reads, in one buffer: the
    /// bucket table, the per-bucket cluster counts and `docs/plan/25-lod.md`'s
    /// three selection tables.
    ///
    /// **Written once**, which is what makes the merge sound rather than merely
    /// convenient: all five are decided when a mesh becomes resident and none is
    /// rewritten per frame, so they change together or not at all.
    /// [`crcbl_shaders::draw_gen::pack_tables`] is what lays them out and
    /// [`table_offsets`](Self::table_offsets) is where each region starts.
    tables: BufferHandle,
    /// Where each region of [`tables`](Self::tables) begins, as the four words
    /// [`draw_gen::Params`] carries into the shader.
    table_offsets: draw_gen::TableOffsets,
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
    /// Zeroed at build and never again, which is where the monotonicity
    /// induction starts — by the start-up copy [`fill_at_start_up`] submits,
    /// because a shader writes this and a buffer a shader writes cannot be
    /// host-visible. A second zero anywhere would erase the history rather than
    /// establish it, so there is deliberately no per-frame clear of it.
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
    visible_count: Vec<BufferHandle>,
    /// `cull.slang`'s survivor list, then the per-bucket runs — see the module
    /// docs, and [`DrawGen::bucket_base`] for where a bucket's run starts.
    runs: Vec<BufferHandle>,
    args: Vec<BufferHandle>,
    /// The per-bucket draw counts, then the per-bucket mesh-dispatch extents.
    counts: Vec<BufferHandle>,
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
    /// `queue` carries the one start-up submit this makes: the copy that zeroes
    /// the hysteresis state, described on
    /// [`group_state`](Self::group_state) and blocked on before this returns.
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
    pub fn new(
        device: &dyn Device,
        queue: QueueHandle,
        desc: &DrawGenDesc<'_>,
    ) -> Result<Self, HalError> {
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
        match Self::build(device, queue, desc, &mut rollback) {
            Ok(built) => Ok(built),
            Err(error) => {
                rollback.run(device);
                Err(error)
            }
        }
    }

    fn build(
        device: &dyn Device,
        queue: QueueHandle,
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

        // **Five tables, one buffer**, because the draw-argument pass has eight
        // storage bindings to spend and a WebGPU device guarantees no more — see
        // the module docs. They are packed together rather than any other five
        // because they are the five written at build and never per frame: the
        // bucket table, its cluster counts, and `docs/plan/25-lod.md`'s three
        // selection tables all follow residency and nothing else.
        //
        // `pack_tables` is what pads an empty selection region out to one zeroed
        // record, which is the ordinary case for a renderer whose meshes have no
        // hierarchy — a mesh's `first_level` names element zero of the level
        // region whether it has a DAG or not.
        let packed = draw_gen::pack_tables(
            desc.bucket_meshes,
            desc.bucket_clusters,
            desc.mesh_levels,
            desc.level_groups,
            desc.level_meshes,
        )
        .ok_or_else(|| {
            HalError::InvalidDescriptor("more table words than a u32 addresses".to_string())
        })?;
        let tables = buffer(
            "tables",
            packed.bytes.len() as u64,
            BufferUsage::STORAGE,
            MemoryLocation::HostUpload,
        )?;
        device.write_buffer(tables, 0, &packed.bytes)?;
        let table_offsets = packed.offsets;

        // `docs/plan/25-lod.md`'s hysteresis state. **Zeroed here and by
        // nothing else ever again**: `draw_gen.slang` reads an element before it
        // writes it, so what the buffer holds on the very first frame is a real
        // input, and freshly allocated device memory holds whatever it holds. A
        // state that is not monotone up a DAG is a cut with a hole in it, so
        // this is the one write that keeps every later frame's induction
        // standing.
        //
        // Device-local like every other buffer a shader writes here, for the
        // reason the module docs give: D3D12 has no unordered-access view of an
        // upload-heap resource. So the zero arrives by a **start-up staging
        // copy** rather than a `write_buffer` — and not by the clearing
        // dispatch, which runs inside every frame. Running it twice would erase
        // exactly the history this buffer exists to carry, and once is what a
        // copy submitted before the first frame is.
        //
        // At least one word per instance even when nothing resident has a
        // hierarchy, because a zero-length buffer is not a descriptor any
        // backend binds and both shaders index it unconditionally.
        let group_stride = u32::try_from(desc.level_groups.len())
            .map_err(|_| HalError::InvalidDescriptor("more groups than a u32".to_string()))?
            .max(1);
        let group_state_bytes = u64::from(capacity) * u64::from(group_stride) * 4;
        let group_state = buffer(
            "lod group state",
            group_state_bytes,
            BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            MemoryLocation::DeviceLocal,
        )?;
        fill_at_start_up(
            device,
            queue,
            &format!("{stem} lod group state"),
            group_state,
            &vec![
                0u8;
                usize::try_from(group_state_bytes).map_err(|_| HalError::InvalidDescriptor(
                    format!(
                        "the lod group state is {group_state_bytes} bytes, which does not \
                             fit this host's address space"
                    )
                ))?
            ],
            ResourceState::ShaderReadWrite,
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
        let mut visible_count = Vec::with_capacity(frames);
        let mut runs = Vec::with_capacity(frames);
        let mut args = Vec::with_capacity(frames);
        let mut counts = Vec::with_capacity(frames);
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
                    bucket_clusters_at: table_offsets.bucket_clusters_at,
                    mesh_levels_at: table_offsets.mesh_levels_at,
                    level_groups_at: table_offsets.level_groups_at,
                    level_meshes_at: table_offsets.level_meshes_at,
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
            // `TRANSFER_SRC` on all four, and it is not there for tidiness:
            // everything these passes produce is written by a shader and read by
            // another, so a copy out is the *only* way anything can check that
            // what they produced is what a CPU would have recorded. `crcbl-vk`'s
            // `draw_gen` end-to-end does exactly that, and topic 03 §3.6's
            // culling-stats ring — the one readback the frame loop is allowed —
            // is the counter below.
            //
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
            // The survivor list and the per-bucket runs, in that order, in one
            // buffer — see the module docs. `cull.slang` writes the first
            // `capacity` words and `draw_gen.slang` scatters into the rest;
            // nothing reads a word of one region through the other, and the
            // shader is bound the whole buffer once because a read-only view of
            // half of it beside a writable view of the other half is a usage
            // conflict on WebGPU.
            runs.push(buffer(
                &format!("visible and bucket runs {frame}"),
                u64::from(capacity) * (1 + u64::from(bucket_count)) * 4,
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
            // The per-bucket draw counts, then the mesh path's dispatch extents
            // — one buffer, on the argument buffer's terms exactly: written by
            // the same pass, zeroed by the same one, and read by a driver rather
            // than by a shader, so `INDIRECT`, and `TRANSFER_SRC` so a test can
            // read back what the GPU decided.
            //
            // Merged because no pass reads both, so the buffer is never in two
            // resource states at once — see [`GeneratedDraws::counts`], and the
            // module docs on why a binding had to go.
            counts.push(buffer(
                &format!("draw counts and mesh dispatch args {frame}"),
                u64::from(bucket_count) * (4 + draw_gen::MESH_ARGS_SIZE as u64),
                BufferUsage::STORAGE
                    | BufferUsage::INDIRECT
                    | BufferUsage::TRANSFER_SRC
                    | BufferUsage::TRANSFER_DST,
                MemoryLocation::DeviceLocal,
            )?);
        }

        // --- the clearing pass ---
        let clear_entries = [
            uniform(0),
            storage(1, false),
            storage(2, false),
            // The draw counts and the mesh-dispatch extents, one buffer.
            storage(3, false),
        ];
        let clear_desc = BindGroupLayoutDesc {
            label: Some("clear counters"),
            entries: &clear_entries,
        };
        check_portable_storage_buffers(clear_desc.label, &[&clear_desc])?;
        let clear_layout = device.create_bind_group_layout(&clear_desc)?;
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
        let cull_entries = [
            uniform(0),
            // `StructuredBuffer` in the shader, so read-only here: the cull
            // pass decides what is visible and never edits an instance or a
            // mesh entry.
            storage(1, true),
            storage(2, true),
            storage(3, false),
            storage(4, false),
        ];
        let cull_desc = BindGroupLayoutDesc {
            label: Some("cull"),
            entries: &cull_entries,
        };
        check_portable_storage_buffers(cull_desc.label, &[&cull_desc])?;
        let cull_layout = device.create_bind_group_layout(&cull_desc)?;
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
        //
        // **Eight storage bindings, which is every one a WebGPU device
        // guarantees.** There is no headroom here: a ninth is a pass that cannot
        // be created in a browser at the default limits or on SwiftShader, which
        // is what a CI runner without a GPU has. `check_portable_storage_buffers`
        // below is what turns a ninth into a failure here rather than into a
        // pipeline-layout refusal on somebody else's device.
        let gen_entries = [
            uniform(0),
            // The instance array and the mesh table, read only.
            storage(1, true),
            storage(2, true),
            // The culling statistics, read only: this pass clamps against the
            // survivor count and adds to nothing.
            storage(3, true),
            // Every host-written table in one buffer, read only: the bucket
            // table, the per-bucket cluster counts and `docs/plan/25-lod.md`'s
            // three selection tables were all decided when a mesh became
            // resident.
            storage(4, true),
            // The survivor list and the per-bucket runs. Writable, and read
            // through the same descriptor — binding one buffer read-only *and*
            // writable in one group is a usage conflict on WebGPU.
            storage(5, false),
            // The indirect arguments.
            storage(6, false),
            // The draw counts and the mesh-dispatch extents.
            storage(7, false),
            // The hysteresis state, read *and* written: this pass is the
            // only writer, and it reads the previous frame's answer out of
            // the same element it then overwrites.
            storage(8, false),
        ];
        let gen_desc = BindGroupLayoutDesc {
            label: Some("draw args"),
            entries: &gen_entries,
        };
        check_portable_storage_buffers(gen_desc.label, &[&gen_desc])?;
        let gen_layout = device.create_bind_group_layout(&gen_desc)?;
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
                    // The survivor list is the front of the buffer whose tail
                    // holds the per-bucket runs; this pass writes only the front
                    // and `CullParams::capacity` is what bounds it there.
                    bound(3, runs[frame]),
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
                    bound(3, visible_count[frame]),
                    bound(4, tables),
                    bound(5, runs[frame]),
                    bound(6, args[frame]),
                    bound(7, counts[frame]),
                    bound(8, group_state),
                ],
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);
            gen_groups.push(group);
        }

        rollback.disarm();
        Ok(Self {
            tables,
            table_offsets,
            clear_params,
            group_state,
            group_stride,
            gen_params,
            cull_params,
            visible_count,
            runs,
            args,
            counts,
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

    /// Where bucket `bucket`'s run starts in [`DrawGen::runs`], in words, as the
    /// number `mesh.slang`'s `DrawConstants::base` carries.
    ///
    /// **Past the survivor list**, which shares that buffer and occupies its
    /// first [`visible_capacity`](Self::visible_capacity) words — see the module
    /// docs. A reader copying a run back multiplies this by four.
    #[must_use]
    pub const fn bucket_base(&self, bucket: u32) -> u32 {
        self.capacity + bucket * self.capacity
    }

    /// Byte offset of bucket `bucket`'s argument structure.
    #[must_use]
    pub const fn args_offset(&self, bucket: u32) -> u64 {
        bucket as u64 * draw_gen::DRAW_ARGS_SIZE as u64
    }

    /// Byte offset of bucket `bucket`'s draw count, which is at the front of the
    /// buffer [`DrawGen::counts`] hands back.
    #[must_use]
    pub const fn count_offset(&self, bucket: u32) -> u64 {
        bucket as u64 * 4
    }

    /// Byte offset of bucket `bucket`'s mesh-dispatch argument structure — what
    /// [`DrawIndirect::offset`](crcbl_hal::DrawIndirect::offset) carries in the
    /// call that reads it.
    ///
    /// **Past the one draw count per bucket** that shares the buffer, so this is
    /// never zero — see the module docs.
    #[must_use]
    pub const fn mesh_args_offset(&self, bucket: u32) -> u64 {
        self.bucket_count as u64 * 4 + bucket as u64 * draw_gen::MESH_ARGS_SIZE as u64
    }

    /// The buffer of per-bucket instance runs for `frame`, which the drawing
    /// pass's bind group names.
    ///
    /// The same buffer [`DrawGen::visible`] hands back: bucket `bucket`'s run
    /// starts [`bucket_base`](Self::bucket_base)`(bucket)` words in, past the
    /// survivor list.
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
    /// into the instance array, in no particular order, in the **first
    /// [`visible_capacity`](Self::visible_capacity) words** of the buffer.
    ///
    /// The same buffer [`DrawGen::runs`] hands back; the per-bucket runs follow
    /// the survivors in it. See the module docs.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn visible(&self, frame: usize) -> BufferHandle {
        self.runs[frame]
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

    /// `frame`'s per-bucket draw counts, in the **first
    /// [`bucket_count`](Self::bucket_count) words** of the buffer
    /// [`GeneratedDraws::counts`] names.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn counts(&self, frame: usize) -> BufferHandle {
        self.counts[frame]
    }

    /// Every host-written table the draw-argument pass reads, in one buffer —
    /// the bucket table, the per-bucket cluster counts and
    /// `docs/plan/25-lod.md`'s three selection tables.
    ///
    /// Shared by every frame, because all five are decided when a mesh becomes
    /// resident. Exposed for the reason the per-frame buffers are: the cluster
    /// counts are the only CPU-side half of an extent that is otherwise the
    /// GPU's, so a test with no GPU can still check the pass copied each
    /// bucket's own — [`table_offsets`](Self::table_offsets) is where each
    /// region starts.
    #[must_use]
    pub fn tables(&self) -> BufferHandle {
        self.tables
    }

    /// Where each region of [`tables`](Self::tables) begins, in words.
    #[must_use]
    pub const fn table_offsets(&self) -> draw_gen::TableOffsets {
        self.table_offsets
    }

    /// `frame`'s per-bucket mesh-dispatch arguments, at
    /// [`mesh_args_offset`](Self::mesh_args_offset)`(bucket)` in the buffer
    /// [`GeneratedDraws::counts`] names — the thing a test reads back to see that
    /// the extent the mesh path dispatched is the count culling produced rather
    /// than the instance pool's size.
    ///
    /// The same buffer [`DrawGen::counts`] hands back. **Not at offset zero**:
    /// the draw counts are in front of it.
    ///
    /// # Panics
    ///
    /// If `frame` is not a slot this was built with.
    #[must_use]
    pub fn mesh_args(&self, frame: usize) -> BufferHandle {
        self.counts[frame]
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
    ///
    /// **Which is why the one caller allowed to break that is a deliberate
    /// setting rather than an accident of what was in scope.**
    /// [`ForwardRenderer::set_frozen_selection_eye`] hands this a pinned eye
    /// while the frame block keeps the live camera, so the cut a reviewer is
    /// looking at is the one chosen for somewhere they are no longer standing —
    /// which is the only vantage point a cut can be judged from.
    ///
    /// [`ForwardRenderer::set_frozen_selection_eye`]: crate::ForwardRenderer::set_frozen_selection_eye
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
                bucket_clusters_at: self.table_offsets.bucket_clusters_at,
                mesh_levels_at: self.table_offsets.mesh_levels_at,
                level_groups_at: self.table_offsets.level_groups_at,
                level_meshes_at: self.table_offsets.level_meshes_at,
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

    /// How many passes [`add_passes`](Self::add_passes) adds to a frame.
    ///
    /// Exact rather than a ceiling: all three are recorded unconditionally, and
    /// the dispatch a frame has nothing for is a dispatch of no workgroups
    /// rather than a pass that drops out. What a caller sizing
    /// [`PassTimers`](crate::timing::PassTimers) adds up — see
    /// [`MAX_TIMED_PASSES`](crate::timing::MAX_TIMED_PASSES).
    pub const MAX_PASSES: u32 = 3;

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
        let visible_count = import(
            graph,
            "cull-count",
            self.visible_count[frame],
            ResourceState::ShaderRead,
        );
        // The survivor list and the per-bucket runs are one buffer, so they are
        // one id: importing it twice would give the graph two independent
        // histories of one resource and a barrier computed from half of it.
        let runs = import(
            graph,
            "visible-and-bucket-runs",
            self.runs[frame],
            ResourceState::ShaderRead,
        );
        let args = import(
            graph,
            "draw-args",
            self.args[frame],
            ResourceState::IndirectArgument,
        );
        // The draw counts and the mesh-dispatch extents, likewise.
        let counts = import(
            graph,
            "draw-counts-and-mesh-dispatch-args",
            self.counts[frame],
            ResourceState::IndirectArgument,
        );
        // **Not indexed by `frame`**, and that is the point: this one buffer is
        // what carries a decision from the previous frame into this one. It
        // arrives in `ShaderReadWrite` because that is what the last frame left
        // it in — and on the very first frame because that is what
        // `fill_at_start_up` left it in, which is why that copy ends with a
        // barrier rather than in `TransferDst`. `ResourceState::needs_barrier`
        // answers `true` for any
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
            // Both regions of it: the counts and the mesh-dispatch extents are
            // one buffer and therefore one declaration.
            .use_buffer(counts, ResourceState::ShaderReadWrite)
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
            .use_buffer(runs, ResourceState::ShaderReadWrite)
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
            .read_buffer(visible_count)
            // Read for the survivor list and written for the runs, which share
            // it — one declaration, and `ShaderReadWrite` is what it really is.
            .use_buffer(runs, ResourceState::ShaderReadWrite)
            .use_buffer(args, ResourceState::ShaderReadWrite)
            .use_buffer(counts, ResourceState::ShaderReadWrite)
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
        for buffer in [self.tables, self.clear_params, self.group_state]
            .into_iter()
            .chain(self.gen_params)
            .chain(self.cull_params)
            .chain(self.visible_count)
            .chain(self.runs)
            .chain(self.args)
            .chain(self.counts)
        {
            device.destroy_buffer(buffer);
        }
    }
}

/// A uniform-buffer layout entry for the compute stage.
///
/// Shared with [`crate::light_grid`], which builds a compute pass on exactly
/// these terms: one uniform block of parameters and a handful of storage
/// buffers, in the shader's declaration order.
pub(crate) const fn uniform(binding: u32) -> BindGroupLayoutEntry {
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
pub(crate) const fn storage(binding: u32, read_only: bool) -> BindGroupLayoutEntry {
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
pub(crate) const fn bound(binding: u32, buffer: BufferHandle) -> BindGroupEntry {
    BindGroupEntry {
        binding,
        array_index: 0,
        resource: BindingResource::whole_buffer(buffer),
    }
}

/// Creates a compute pipeline from `shader`'s single compute entry point,
/// destroying the module whether or not the pipeline was created.
pub(crate) fn compute_pipeline(
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
    compute_pipeline_entry(device, label, shader, entry_point, layout, workgroup_size)
}

/// [`compute_pipeline`] for a module with **more than one** compute entry
/// point, which is a thing Slang compiles and [`Shader::entry_point`] refuses
/// to guess between.
///
/// `crate::volumetric` is the caller that needs it: its scatter and integrate
/// kernels are two entry points of one module because they share the froxel
/// arithmetic and there is no `#include` to share it any other way.
///
/// The name is not checked against the module here — a wrong one fails at
/// pipeline creation, which is where every backend reports it.
///
/// [`Shader::entry_point`]: crcbl_shaders::Shader::entry_point
pub(crate) fn compute_pipeline_entry(
    device: &dyn Device,
    label: &str,
    shader: &crcbl_shaders::Shader,
    entry_point: &str,
    layout: PipelineLayoutHandle,
    workgroup_size: u32,
) -> Result<ComputePipelineHandle, HalError> {
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

/// Writes `contents` over `buffer` once, before any frame exists, and blocks
/// until the copy has run, leaving it in `final_state`.
///
/// A start-up staging copy on [`crate::mesh_pool`]'s and [`crate::texture`]'s
/// terms, and it joins them on the crate docs' list of named exceptions to the
/// no-manual-barriers rule. It is *not* a `write_buffer`, because the
/// destination is device-local; not [`crcbl_hal::CommandEncoder::clear_buffer`],
/// which `crcbl-dx12` refuses outright and the module docs argue about at
/// length; and not the clearing dispatch, which runs inside every frame rather
/// than once.
///
/// `final_state` is the state the buffer's first importer declares as its
/// initial one, so the graph's first barrier on it names a state the buffer is
/// really in: [`ResourceState::ShaderReadWrite`] for the LOD state below, and
/// [`ResourceState::ShaderRead`] for [`crate::exposure`]'s ring.
///
/// # Errors
///
/// [`HalError`] from any seam call, and from a `contents` too long to be a
/// buffer size on this host.
pub(crate) fn fill_at_start_up(
    device: &dyn Device,
    queue: QueueHandle,
    label: &str,
    buffer: BufferHandle,
    contents: &[u8],
    final_state: ResourceState,
) -> Result<(), HalError> {
    let size = u64::try_from(contents.len()).map_err(|_| {
        HalError::InvalidDescriptor(format!(
            "{label} is {} bytes, which does not fit a buffer size",
            contents.len()
        ))
    })?;
    let staging = device.create_buffer(&BufferDesc {
        label: Some(&format!("{label} staging")),
        size,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    })?;
    let filled = record_fill(device, queue, label, buffer, staging, contents, final_state);
    device.destroy_buffer(staging);
    filled
}

/// The half of [`fill_at_start_up`] the staging buffer is live across, so the
/// caller releases it on every path out.
fn record_fill(
    device: &dyn Device,
    queue: QueueHandle,
    label: &str,
    buffer: BufferHandle,
    staging: BufferHandle,
    contents: &[u8],
    final_state: ResourceState,
) -> Result<(), HalError> {
    device.write_buffer(staging, 0, contents)?;

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some(&format!("{label} zero")),
        queue,
    });
    // `Undefined` as the source, unlike [`crate::mesh_pool`]'s pools: this
    // buffer was created a few statements ago and has never held anything, so
    // there is no state to preserve and its contents are discarded rather than
    // transitioned.
    encoder.pipeline_barrier(&Barriers {
        buffers: &[BufferBarrier::new(
            buffer,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: staging,
        src_offset: 0,
        dst: buffer,
        dst_offset: 0,
        size: contents.len() as u64,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[BufferBarrier::new(
            buffer,
            ResourceState::TransferDst,
            final_state,
        )],
        ..Barriers::default()
    });
    let commands = encoder.finish()?;

    // Blocking is what makes this a start-up path: the staging buffer dies with
    // the caller's next statement, and a frame that read the state before the
    // copy landed would read whatever the allocation came with.
    let submitted = device
        .submit(queue, &SubmitInfo::new(&[commands]))
        .and_then(|()| device.wait_idle());
    device.destroy_command_buffer(commands);
    submitted
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
