use crate::harness::Headless;
use crate::mesh::{MESH_EXTENT, mesh_camera};
use crcbl_hal::{
    Barriers, BufferCopy, BufferDesc, BufferHandle, BufferUsage, CommandEncoderDesc, Features,
    GeometryPath, MemoryLocation, PresentInfo, ResourceState, SubmitInfo,
};
use crcbl_render::cull::{Aabb, Frustum, visible_instances};
use crcbl_render::{ForwardRenderer, Projection, RenderGraph, TransientPool};
use crcbl_shaders::draw_gen::{DRAW_ARGS_SIZE, DrawIndexedArgs};
use crcbl_shaders::mesh::{
    CUBE_INDEX_COUNT, CUBE_VERTEX_COUNT, GpuInstance, GpuMesh, PYRAMID_INDEX_COUNT, VERTEX_STRIDE,
};
use glam::{Mat4, Vec3};

// --- GPU draw generation, against the draws a CPU would have recorded --------
//
// `docs/plan/03-gpu-driven-rendering.md` §3.3's second half: the compacted
// visible list becomes `draw_indexed_indirect` records and a count buffer.
// `crcbl-vk`'s `cull` suite checks the first half against
// `crcbl_render::cull::visible_instances`; this checks the second against the
// draw calls `crcbl_render::forward` used to record itself, which is the only
// comparison that can tell a *translation* from a picture that happens to look
// right.
//
// Every number below is copied out of the buffers the frame actually drew from,
// after the frame drew from them. A golden image says the picture is unchanged;
// it cannot say the arguments are right, because a wrong `first_index` that
// happens to name the same range and a right one produce the same pixels.
//
// # The reference is built here rather than read out of the device
//
// The mesh table and the instance array are `HostUpload` storage buffers with no
// transfer usage, so neither can be copied back — and reading the expectation
// out of the same table the shader read would check that two consumers agree
// rather than that either is right. So the expectation is assembled from
// `crcbl_shaders::mesh`'s own constants and geometry: the pool suballocates in
// upload order, so the cube is at base vertex 0 and the pyramid starts where it
// ends. An allocator that stopped doing that fails here rather than silently
// drawing the other mesh.

/// Where the pyramid sits when the frame is meant to contain it. Beside the
/// cube and inside the frustum, like `crcbl screenshot --scene cube`.
const PYRAMID_AT: Vec3 = Vec3::new(-1.05, 0.0, 0.0);

/// Where it sits when the frame is meant to cull it: far off to the right of a
/// camera looking at the origin, and nowhere near any plane, so the answer does
/// not depend on the conservative bound being tight.
const PYRAMID_AWAY: Vec3 = Vec3::new(60.0, 0.0, 0.0);

/// What a counter is filled with before a frame that must overwrite it.
///
/// All four bytes equal, which is not decoration: the seam's fill takes a `u32`
/// and Metal's `fillBuffer:range:value:` repeats a *byte*, so a word whose bytes
/// differ has no encoding there. `crcbl-mtl`'s own probe uses the same value.
const SENTINEL: u32 = 0xABAB_ABAB;

/// Entries of one bucket's run to copy back. The scene has one instance per
/// bucket and this is comfortably more, so a run that scattered too *many*
/// entries shows up as a non-zero word past the count rather than as a copy that
/// stopped early.
const RUN_SAMPLE: u64 = 8;

/// The mesh table as the pool builds it, in upload order: the cube first, the
/// pyramid immediately after it.
///
/// The bounds are the box around the vertices the pool is handed, which is what
/// `MeshPool::upload` records — recomputed here from the same geometry rather
/// than copied from the pool, so the two are independent.
fn mesh_table() -> Vec<GpuMesh> {
    let entry = |base_vertex: usize, base_index: usize, index_count: usize, bytes: &[u8]| {
        let bounds = local_bounds(bytes);
        GpuMesh {
            base_vertex: base_vertex as u32,
            base_index: base_index as u32,
            index_count: index_count as u32,
            bounds_min: bounds.min.to_array(),
            bounds_max: bounds.max.to_array(),
        }
    };
    vec![
        entry(
            0,
            0,
            CUBE_INDEX_COUNT,
            &crcbl_shaders::mesh::cube_vertex_bytes(),
        ),
        entry(
            CUBE_VERTEX_COUNT,
            CUBE_INDEX_COUNT,
            PYRAMID_INDEX_COUNT,
            &crcbl_shaders::mesh::pyramid_vertex_bytes(),
        ),
    ]
}

/// The box holding every vertex position in `vertices` — the first three floats
/// of each vertex.
fn local_bounds(vertices: &[u8]) -> Aabb {
    let positions = vertices.chunks_exact(VERTEX_STRIDE).map(|vertex| {
        let float_at = |offset: usize| {
            f32::from_le_bytes(vertex[offset..offset + 4].try_into().expect("four bytes"))
        };
        Vec3::new(float_at(0), float_at(4), float_at(8))
    });
    Aabb::from_points(positions).expect("neither mesh is empty")
}

/// The instance array as the renderer builds it: the cube at element 0 from
/// `new`, the pyramid at element 1 from the first `set_pyramid`.
fn scene(model: Mat4, pyramid: Vec3) -> Vec<GpuInstance> {
    let at = |transform: Mat4, mesh| GpuInstance {
        transform: transform.to_cols_array(),
        mesh,
        flags: GpuInstance::LIVE,
        ..GpuInstance::default()
    };
    vec![at(model, 0), at(Mat4::from_translation(pyramid), 1)]
}

/// What one frame's draw generation produced, copied back out of the buffers the
/// frame drew from.
struct Generated {
    /// One argument structure per bucket.
    args: Vec<DrawIndexedArgs>,
    /// One draw count per bucket — what `IndirectCount`'s call reads.
    counts: Vec<u32>,
    /// The first [`RUN_SAMPLE`] entries of each bucket's run of instance
    /// indices.
    runs: Vec<Vec<u32>>,
    /// `cull.slang`'s true survivor count.
    visible_count: u32,
}

impl Generated {
    /// Bucket `bucket`'s run, as far as its argument structure says it was
    /// filled, sorted.
    ///
    /// Sorted because the order is an atomic's: `draw_gen.slang` hands slots out
    /// as invocations arrive and nothing about the picture depends on which
    /// instance landed where.
    fn run(&self, bucket: usize) -> Vec<u32> {
        let count = self.args[bucket].instance_count as usize;
        assert!(
            count <= self.runs[bucket].len(),
            "bucket {bucket} claims {count} instances, which is more than this test copied \
             back — widen RUN_SAMPLE"
        );
        let mut run = self.runs[bucket][..count].to_vec();
        run.sort_unstable();
        run
    }
}

/// Renders one frame through the real `ForwardRenderer` and the real graph, then
/// copies every buffer the draw generation wrote back to the host.
fn generate(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    camera: &crcbl_render::Camera,
    model: Mat4,
) -> Generated {
    generate_with(headless, renderer, pool, camera, model, None)
}

/// The same frame, with the three counters the frame's clearing pass owns filled
/// with `poison` first.
///
/// Recorded into the frame's **own** encoder, ahead of the graph, so the fill
/// and the dispatch that has to undo it are one submission. The fill is a
/// transfer and is therefore recorded outside any pass, which is the only place
/// the seam allows one — and is exactly why the zeroing itself cannot be a fill.
///
/// The barriers around the fill, like those around the copies below, are **this
/// test's**, not the frame's: the graph left each buffer in the state its next
/// frame declares, and both take them out of it and put them back. Nothing about
/// the frame itself hand-writes a barrier — `crcbl_render::draw_gen` declares
/// its accesses and the graph computes the transitions.
fn generate_with(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    camera: &crcbl_render::Camera,
    model: Mat4,
    poison: Option<u32>,
) -> Generated {
    let device = headless.device.as_ref();
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");

    renderer
        .begin_frame(
            device,
            camera,
            &crcbl_render::DirectionalLight::default(),
            model,
            MESH_EXTENT,
        )
        .expect("the uniform buffers are writable");

    let draws = renderer.draws();
    let frame = renderer.frame();
    let buckets = draws.bucket_count() as usize;
    let args_bytes = buckets as u64 * DRAW_ARGS_SIZE as u64;
    let counts_bytes = buckets as u64 * 4;
    let run_bytes = RUN_SAMPLE * 4;
    let total = args_bytes + counts_bytes + 4 + run_bytes * buckets as u64;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("draw gen readback"),
            size: total,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let (args, counts, runs, visible_count) = (
        draws.args(frame),
        draws.counts(frame),
        draws.runs(frame),
        draws.visible_count(frame),
    );
    let run_offsets: Vec<u64> = (0..draws.bucket_count())
        .map(|bucket| u64::from(draws.bucket_base(bucket)) * 4)
        .collect();

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("draw gen frame"),
        queue: headless.queue,
    });
    if let Some(value) = poison {
        // The three the frame's clearing pass owns, each with the state this
        // slot's imports declare it arrives in and the bytes it holds.
        let counters = [
            (visible_count, ResourceState::ShaderRead, 4u64),
            (args, ResourceState::IndirectArgument, args_bytes),
            (counts, ResourceState::IndirectArgument, counts_bytes),
        ];
        let around = |into_transfer: bool| -> Vec<crcbl_hal::BufferBarrier> {
            counters
                .iter()
                .map(|(buffer, state, _)| {
                    let (from, to) = if into_transfer {
                        (*state, ResourceState::TransferDst)
                    } else {
                        (ResourceState::TransferDst, *state)
                    };
                    crcbl_hal::BufferBarrier {
                        buffer: *buffer,
                        from,
                        to,
                        queue_transfer: None,
                    }
                })
                .collect()
        };
        encoder.pipeline_barrier(&Barriers {
            buffers: &around(true),
            ..Barriers::default()
        });
        for (buffer, _, size) in counters {
            encoder.fill_buffer(buffer, 0, size, value);
        }
        encoder.pipeline_barrier(&Barriers {
            buffers: &around(false),
            ..Barriers::default()
        });
    }
    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl_render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                final_state: ResourceState::Present,
            },
        );
        let _ = renderer.add_passes(&mut graph, target, MESH_EXTENT);
        graph.compile(&*pool).expect("a legal frame")
    };
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");

    // Out of the states the graph left them in, and back into them afterwards —
    // the next frame on this slot declares those as its starting point.
    let resting = [
        (args, ResourceState::IndirectArgument),
        (counts, ResourceState::IndirectArgument),
        (runs, ResourceState::ShaderRead),
        (visible_count, ResourceState::ShaderRead),
    ];
    let transitions = |to_transfer: bool| -> Vec<crcbl_hal::BufferBarrier> {
        resting
            .iter()
            .map(|(buffer, state)| {
                let (from, to) = if to_transfer {
                    (*state, ResourceState::TransferSrc)
                } else {
                    (ResourceState::TransferSrc, *state)
                };
                crcbl_hal::BufferBarrier {
                    buffer: *buffer,
                    from,
                    to,
                    queue_transfer: None,
                }
            })
            .collect()
    };
    encoder.pipeline_barrier(&Barriers {
        buffers: &transitions(true),
        ..Barriers::default()
    });
    let mut copy = |src: BufferHandle, src_offset: u64, dst_offset: u64, size: u64| {
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src,
            src_offset,
            dst: staging,
            dst_offset,
            size,
        });
    };
    copy(args, 0, 0, args_bytes);
    copy(counts, 0, args_bytes, counts_bytes);
    copy(visible_count, 0, args_bytes + counts_bytes, 4);
    for (bucket, offset) in run_offsets.iter().enumerate() {
        copy(
            runs,
            *offset,
            args_bytes + counts_bytes + 4 + run_bytes * bucket as u64,
            run_bytes,
        );
    }
    encoder.pipeline_barrier(&Barriers {
        buffers: &transitions(false),
        ..Barriers::default()
    });

    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");

    let mut bytes = vec![0u8; total as usize];
    headless.readback(staging, total, &mut bytes);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let word_at = |offset: u64| {
        let at = offset as usize;
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    };
    Generated {
        args: (0..buckets)
            .map(|bucket| {
                let at = bucket * DRAW_ARGS_SIZE;
                DrawIndexedArgs::from_bytes(
                    bytes[at..at + DRAW_ARGS_SIZE]
                        .try_into()
                        .expect("one whole structure"),
                )
            })
            .collect(),
        counts: (0..buckets as u64)
            .map(|bucket| word_at(args_bytes + bucket * 4))
            .collect(),
        visible_count: word_at(args_bytes + counts_bytes),
        runs: (0..buckets as u64)
            .map(|bucket| {
                let base = args_bytes + counts_bytes + 4 + run_bytes * bucket;
                (0..RUN_SAMPLE)
                    .map(|slot| word_at(base + slot * 4))
                    .collect()
            })
            .collect(),
    }
}

/// Opens the pieces every test here needs.
fn setup() -> (Headless, ForwardRenderer, TransientPool) {
    let headless = Headless::open_for_mesh();
    let renderer = ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
        .expect("the forward renderer builds");
    (headless, renderer, TransientPool::new())
}

/// Releases everything in dependency order.
fn teardown(headless: Headless, renderer: ForwardRenderer, mut pool: TransientPool) {
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.instance.validation_report().assert_clean();
    headless.finish();
}

/// **The generated arguments are the draws the CPU would have recorded.**
///
/// One structure per bucket, its index range the mesh's own, both bases zero,
/// and an instance count equal to the number of survivors of that mesh — which
/// is exactly the `draw_indexed(base_index..base_index + index_count, 0, 0..1)`
/// per visible object that this pass recorded before the arguments existed.
///
/// The run beside each structure has to name the same instances the CPU
/// reference kept, or the count is right and the objects drawn are not.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_generated_arguments_are_the_draws_the_cpu_would_have_recorded() {
    let (headless, mut renderer, mut pool) = setup();
    renderer.set_pyramid(Some(Mat4::from_translation(PYRAMID_AT)));

    let camera = mesh_camera(Projection::default());
    let model = ForwardRenderer::spin(0.35);
    let generated = generate(&headless, &mut renderer, &mut pool, &camera, model);

    let meshes = mesh_table();
    let instances = scene(model, PYRAMID_AT);
    let aspect = MESH_EXTENT.0 as f32 / MESH_EXTENT.1 as f32;
    let frustum = Frustum::from_view_projection(camera.view_projection(aspect));
    let visible = visible_instances(&frustum, &instances, &meshes);
    assert_eq!(
        visible,
        vec![0, 1],
        "the scene this checks against must have both objects on screen, or the \
         comparison below is about an empty frame"
    );
    assert_eq!(
        generated.visible_count, 2,
        "and the GPU must have kept the same two"
    );

    for (bucket, entry) in meshes.iter().enumerate() {
        let kept: Vec<u32> = visible
            .iter()
            .copied()
            .filter(|index| instances[*index as usize].mesh as usize == bucket)
            .collect();
        assert_eq!(
            generated.args[bucket],
            DrawIndexedArgs {
                index_count: entry.index_count,
                instance_count: kept.len() as u32,
                first_index: entry.base_index,
                // Zero for both, and this is the assertion that would have
                // caught the old bug: a base vertex travelling through the draw
                // is subtracted straight back out of `SV_VertexID` on SPIR-V and
                // is not on WGSL or MSL.
                vertex_offset: 0,
                first_instance: 0,
            },
            "bucket {bucket}'s arguments must be the draw the CPU would have recorded"
        );
        assert_eq!(
            generated.run(bucket),
            kept,
            "and its run must name the instances the reference kept"
        );
        assert_eq!(
            generated.counts[bucket], 1,
            "a bucket with something in it draws once"
        );
    }

    teardown(headless, renderer, pool);
}

/// **The draw count comes from the GPU, and a culled bucket's is zero.**
///
/// A frame where the count happens to equal the object total proves nothing
/// about culling, so this puts the pyramid sixty metres to the right of a camera
/// looking at the origin: the survivor count drops, the pyramid's bucket
/// generates an argument structure claiming no instances, and its draw count —
/// the word `GeometryPath::IndirectCount`'s call reads instead of a number the
/// CPU knows — is zero.
///
/// The cube's bucket is checked in the same frame, so a run that had zeroed
/// *everything* fails here rather than passing as "it culled".
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_culled_bucket_generates_no_draw_and_says_so_in_the_count() {
    let (headless, mut renderer, mut pool) = setup();
    renderer.set_pyramid(Some(Mat4::from_translation(PYRAMID_AWAY)));

    let camera = mesh_camera(Projection::default());
    let model = ForwardRenderer::spin(0.35);
    let generated = generate(&headless, &mut renderer, &mut pool, &camera, model);

    let meshes = mesh_table();
    let instances = scene(model, PYRAMID_AWAY);
    let aspect = MESH_EXTENT.0 as f32 / MESH_EXTENT.1 as f32;
    let frustum = Frustum::from_view_projection(camera.view_projection(aspect));
    assert_eq!(
        visible_instances(&frustum, &instances, &meshes),
        vec![0],
        "the reference must cull the pyramid here, or this test is about nothing"
    );

    assert_eq!(
        generated.visible_count, 1,
        "one of the two instances survived, and the count the GPU wrote says so"
    );
    assert_eq!(
        generated.args[0].instance_count, 1,
        "the cube still draws: {:?}",
        generated.args
    );
    assert_eq!(generated.counts[0], 1, "and its bucket still draws once");
    assert_eq!(
        generated.args[1].instance_count, 0,
        "the pyramid's bucket must claim no instances: {:?}",
        generated.args
    );
    assert_eq!(
        generated.counts[1], 0,
        "and its draw count must be zero, which is the number the CPU never learns"
    );
    // The static half of the culled bucket's arguments is still written: an
    // argument structure left at zero would be indistinguishable from one whose
    // mesh was never resolved.
    assert_eq!(
        generated.args[1].index_count, meshes[1].index_count,
        "the mesh range is written whether or not anything draws it"
    );
    assert_eq!(generated.args[1].first_index, meshes[1].base_index);

    teardown(headless, renderer, pool);
}

/// **A counter poisoned right up to the frame reaches both shaders at zero.**
///
/// This is what says the clearing pass *runs*, and it is the only shape that
/// can. Every one of these three buffers is written by a shader and by nothing
/// else, so a counter nothing poisoned reads as the zero its allocation came
/// with and an assertion against that passes whether or not anything zeroed it.
/// The engine's own unit suite used to make this check against a host write and
/// cannot any more: the counters are device-local now, and the backend that
/// suite runs on executes no shader.
///
/// [`SENTINEL`] is what the frame has to have overwritten. Every assertion below
/// is a number a single frame produces, and none of them survives the poison
/// leaking through: the survivor count would be `SENTINEL + 2`, each bucket's
/// instance count `SENTINEL + 1`, and a draw count would stay at `SENTINEL`
/// outright, because `draw_gen.slang` stores its `1` only for the invocation
/// that took slot zero and no invocation would.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_poisoned_counter_reaches_the_frame_at_zero() {
    let (headless, mut renderer, mut pool) = setup();
    renderer.set_pyramid(Some(Mat4::from_translation(PYRAMID_AT)));

    let camera = mesh_camera(Projection::default());
    let model = ForwardRenderer::spin(0.35);
    let generated = generate_with(
        &headless,
        &mut renderer,
        &mut pool,
        &camera,
        model,
        Some(SENTINEL),
    );

    assert_eq!(
        generated.visible_count, 2,
        "both instances survive, and the count is this frame's alone rather than \
         {SENTINEL:#010x} counted up from"
    );
    for (bucket, args) in generated.args.iter().enumerate() {
        assert_eq!(
            args.instance_count, 1,
            "bucket {bucket} draws its one instance: {:?}",
            generated.args
        );
    }
    assert_eq!(
        generated.counts,
        vec![1, 1],
        "and each bucket's draw count is the `1` the pass stored, not the poison it \
         was left holding"
    );

    teardown(headless, renderer, pool);
}

/// **Removing an instance empties its bucket, and putting it back fills it
/// again** — over consecutive frames, through the ring.
///
/// The frame the samples draw has no pyramid instance at all, and this is what
/// says the two states differ in the generated arguments rather than only in the
/// picture. Consecutive frames also exercise the thing a single frame cannot: a
/// counter that was not zeroed carries the previous frame's total, and the
/// second frame here would then claim two instances of a mesh that has one.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_bucket_fills_and_empties_as_its_instance_comes_and_goes() {
    let (headless, mut renderer, mut pool) = setup();
    let camera = mesh_camera(Projection::default());
    let model = ForwardRenderer::spin(0.35);

    // Four frames, so the ring wraps and every slot is used twice: the second
    // pass over a slot is the one that would carry a stale counter.
    for round in 0..2 {
        renderer.set_pyramid(None);
        let without = generate(&headless, &mut renderer, &mut pool, &camera, model);
        assert_eq!(
            (
                without.visible_count,
                without.args[0].instance_count,
                without.args[1].instance_count,
                without.counts[1]
            ),
            (1, 1, 0, 0),
            "round {round}: with no pyramid instance, only the cube survives and only \
             its bucket draws"
        );

        renderer.set_pyramid(Some(Mat4::from_translation(PYRAMID_AT)));
        let with = generate(&headless, &mut renderer, &mut pool, &camera, model);
        assert_eq!(
            (
                with.visible_count,
                with.args[0].instance_count,
                with.args[1].instance_count,
                with.counts[1]
            ),
            (2, 1, 1, 1),
            "round {round}: and putting it back fills its bucket again, exactly once"
        );
    }

    teardown(headless, renderer, pool);
}

/// **Both `GeometryPath` arms draw the same frame, byte for byte.**
///
/// `docs/plan/03-gpu-driven-rendering.md`'s design rule is that "the lesser path
/// is a constraint on data layout, not a separate renderer", and the exit
/// criterion is that every path renders the sandbox scene. This is that
/// criterion for the two indirect arms, on real hardware, in one process: the
/// same renderer, the same scene, the same camera, and the only difference is
/// which call the forward pass records.
///
/// `IndirectPerBatch` is what Metal is on — its API has multi-draw-indirect and
/// no GPU-side count — and no adapter this suite can see would ever select it,
/// so the device is opened without [`Features::DRAW_INDIRECT_COUNT`] on purpose.
/// Without that, the arm would be code no machine here runs.
///
/// Byte equality rather than a tolerance: this is one driver, one adapter and
/// one scene, so anything but identical output is a difference the two calls
/// made.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn both_geometry_paths_draw_the_same_frame() {
    let mut frames = Vec::new();
    for (optional, expected) in [
        (Features::GPU_DRIVEN, GeometryPath::IndirectCount),
        (
            Features::GPU_DRIVEN.difference(Features::DRAW_INDIRECT_COUNT),
            GeometryPath::IndirectPerBatch,
        ),
    ] {
        let headless = Headless::open_for_mesh_with(optional);
        assert_eq!(
            headless.device.caps().geometry_path(),
            expected,
            "the device must be on the path this arm is about, or the two halves of \
             this comparison are the same code twice"
        );
        let mut pool = TransientPool::new();
        let mut renderer =
            ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
                .expect("the forward renderer builds");
        renderer.set_pyramid(Some(Mat4::from_translation(PYRAMID_AT)));
        let frame = crate::mesh::render_mesh(
            &headless,
            &mut renderer,
            &mut pool,
            &mesh_camera(Projection::default()),
        );
        frames.push(frame.image);
        teardown(headless, renderer, pool);
    }

    let (counted, per_batch) = (&frames[0], &frames[1]);
    // Anti-vacuity: two blank frames match perfectly, so the frame has to have
    // something in it before the comparison means anything. The count is small
    // because every face of both meshes is flat-shaded — measured here, a
    // cleared frame is 1 and this scene's committed golden (the cube alone) is
    // 4, so anything at or below that is a frame the pyramid did not reach.
    let colours = counted.distinct_colors(64);
    assert!(
        colours > 4,
        "the frame is missing geometry ({colours} distinct colours), so comparing \
         it against another frame like it proves nothing"
    );
    assert_eq!(
        counted.pixels(),
        per_batch.pixels(),
        "the two geometry paths must draw the same pixels"
    );
}
