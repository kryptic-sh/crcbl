//! §3.3's second half: the compacted visible list becomes
//! `draw_indexed_indirect` records and a count buffer.
//!
//! Separate from `cull` because it checks against a different oracle — the draw
//! calls `crcbl::render::forward` used to record itself — which is the only
//! comparison that can tell a *translation* from a picture that happens to look
//! right. Every number here is copied out of the buffers the frame actually
//! drew from, after it drew from them: a golden image cannot say the arguments
//! are right, because a wrong `first_index` that names the same range and a
//! right one produce the same pixels.
//!
//! The expectation is assembled here from `crcbl::shaders::mesh`'s own constants
//! rather than read back, because the mesh table and the instance array are
//! `HostUpload` buffers with no transfer usage — and reading the shader's own
//! input would only check that two consumers agree.
//!
//! **This is the file the draw-args repack most needs read on every backend.**
//! That pass went from fourteen storage buffers to eight: five host-static
//! tables merged into one word buffer addressed by offsets in `Params`, the
//! survivor list sharing a buffer with the per-bucket runs at
//! [`GeneratedDraws::bucket_base`](crcbl::render::GeneratedDraws::bucket_base),
//! and the counts sharing one with the mesh-dispatch extents. Every one of those
//! is an offset a backend can compute differently, and none of them changes a
//! pixel.

use crate::harness::{Headless, poisoned};
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place, place_cube_at};
use crcbl::hal::{
    Barriers, BufferCopy, BufferDesc, BufferHandle, BufferUsage, CommandEncoderDesc,
    MemoryLocation, PresentInfo, ResourceState, SubmitInfo,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::cull::{Aabb, Frustum, visible_instances};
use crcbl::render::{
    Camera, ForwardRenderer, InstanceHandle, Projection, RenderGraph, TransientPool,
};
use crcbl::shaders::draw_gen::{DRAW_ARGS_SIZE, DrawIndexedArgs};
use crcbl::shaders::mesh::{
    CUBE_INDEX_COUNT, CUBE_VERTEX_COUNT, GpuInstance, GpuMesh, PYRAMID_INDEX_COUNT, VERTEX_STRIDE,
};

// The pool suballocates in upload order, so the cube is at base vertex 0 and
// the pyramid starts where it ends. An allocator that stopped doing that fails
// here rather than silently drawing the other mesh.

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
/// **The two the scenes below instance**, and not every resident: the renderer
/// also uploads the open box into a third bucket, which nothing here puts in a
/// frame. A reference covering it would have to model a mesh no scene draws.
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
            &crcbl::shaders::mesh::cube_vertex_bytes(),
        ),
        entry(
            CUBE_VERTEX_COUNT,
            CUBE_INDEX_COUNT,
            PYRAMID_INDEX_COUNT,
            &crcbl::shaders::mesh::pyramid_vertex_bytes(),
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

/// Where every test here puts the cube.
///
/// `setup` places it and nothing moves it again, so a frame's cube is at this
/// transform whichever test drew it — which is what the CPU reference below
/// models it at.
fn cube_model() -> Mat4 {
    ForwardRenderer::spin(0.35)
}

/// The instance array as the tests here fill it: the cube at element 0, placed
/// by [`setup`], and the pyramid at element 1 from the first [`place_pyramid`].
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
    camera: &Camera,
) -> Generated {
    generate_with(headless, renderer, pool, camera, None)
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
/// the frame itself hand-writes a barrier — `crcbl::render::draw_gen` declares
/// its accesses and the graph computes the transitions.
fn generate_with(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    camera: &Camera,
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
            &crcbl::render::DirectionalLight::default(),
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
    // Where the poison comes from, when there is one. **A copy, not a
    // `fill_buffer`**: `crcbl-wgpu` refuses a fill with a non-zero value —
    // wgpu's `clear_buffer` is a zero fill and it has nothing else — so the
    // Vulkan original's fill was a backend-specific mechanism wearing a seam
    // call's name. This puts the same bytes in the same three buffers on every
    // backend, and every assertion below reads them unchanged.
    let poison_source = poison.map(|value| {
        let size = args_bytes.max(counts_bytes).max(4);
        let source = device
            .create_buffer(&BufferDesc {
                label: Some("draw gen poison"),
                size,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a poison source");
        let words = usize::try_from(size / 4).expect("a small buffer");
        device
            .write_buffer(source, 0, &value.to_le_bytes().repeat(words))
            .expect("write");
        source
    });
    if let Some(source) = poison_source {
        // The three the frame's clearing pass owns, each with the state this
        // slot's imports declare it arrives in and the bytes it holds.
        let counters = [
            (visible_count, ResourceState::ShaderRead, 4u64),
            (args, ResourceState::IndirectArgument, args_bytes),
            (counts, ResourceState::IndirectArgument, counts_bytes),
        ];
        let around = |into_transfer: bool| -> Vec<crcbl::hal::BufferBarrier> {
            counters
                .iter()
                .map(|(buffer, state, _)| {
                    let (from, to) = if into_transfer {
                        (*state, ResourceState::TransferDst)
                    } else {
                        (ResourceState::TransferDst, *state)
                    };
                    crcbl::hal::BufferBarrier {
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
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: source,
                src_offset: 0,
                dst: buffer,
                dst_offset: 0,
                size,
            });
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
            crcbl::render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                claim: crcbl::render::InitialClaim::Acquired,
                final_state: ResourceState::Present,
            },
        );
        let _ = renderer.add_passes(&mut graph, &*pool, target, MESH_EXTENT);
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
    let transitions = |to_transfer: bool| -> Vec<crcbl::hal::BufferBarrier> {
        resting
            .iter()
            .map(|(buffer, state)| {
                let (from, to) = if to_transfer {
                    (*state, ResourceState::TransferSrc)
                } else {
                    (ResourceState::TransferSrc, *state)
                };
                crcbl::hal::BufferBarrier {
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

    let mut bytes = poisoned(total as usize);
    headless.readback(staging, total, &mut bytes);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);
    if let Some(source) = poison_source {
        device.destroy_buffer(source);
    }

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
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    place_cube_at(&mut renderer, cube_model());
    (headless, renderer, TransientPool::new())
}

/// Puts the demo pyramid in the frame at `at`, **after the cube [`setup`]
/// placed**, so it is element 1 of the instance array — which is where
/// [`scene`]'s CPU reference models it.
fn place_pyramid(renderer: &mut ForwardRenderer, at: Vec3) -> InstanceHandle {
    place(
        renderer,
        crcbl::render::scene::DEMO_PYRAMID,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::from_translation(at),
    )
}

/// Releases everything in dependency order.
fn teardown(headless: Headless, renderer: ForwardRenderer, mut pool: TransientPool) {
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
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
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn the_generated_arguments_are_the_draws_the_cpu_would_have_recorded() {
    let (headless, mut renderer, mut pool) = setup();
    place_pyramid(&mut renderer, PYRAMID_AT);

    let camera = mesh_camera(Projection::default());
    let model = cube_model();
    let generated = generate(&headless, &mut renderer, &mut pool, &camera);

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
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn a_culled_bucket_generates_no_draw_and_says_so_in_the_count() {
    let (headless, mut renderer, mut pool) = setup();
    place_pyramid(&mut renderer, PYRAMID_AWAY);

    let camera = mesh_camera(Projection::default());
    let model = cube_model();
    let generated = generate(&headless, &mut renderer, &mut pool, &camera);

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
/// leaking through: the survivor count would be `SENTINEL + 2`, an occupied
/// bucket's instance count `SENTINEL + 1`, and a draw count would stay at
/// `SENTINEL` outright, because `draw_gen.slang` stores its `1` only for the
/// invocation that took slot zero and no invocation would.
///
/// **The empty buckets are the sharpest numbers here**, not gaps. Only the cube
/// and the pyramid are in this scene, so every bucket after theirs — the open
/// box's, and one per level of the dunes patch's DAG where the path takes
/// `docs/plan/25-lod.md`'s uniform cut — has nothing to add to `SENTINEL`, and a
/// clearing pass that never ran leaves them holding the poison undisguised.
///
/// How many of them there are is read off the arguments rather than written
/// down, because the bucket count is a property of the geometry path: the mesh
/// path gives the dunes patch one bucket and the indirect tails give it one per
/// level. What is asserted is the shape — the first two full, every one after
/// them empty, and at least one after them to be empty.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn a_poisoned_counter_reaches_the_frame_at_zero() {
    let (headless, mut renderer, mut pool) = setup();
    place_pyramid(&mut renderer, PYRAMID_AT);

    let camera = mesh_camera(Projection::default());
    let generated = generate_with(&headless, &mut renderer, &mut pool, &camera, Some(SENTINEL));

    assert_eq!(
        generated.visible_count, 2,
        "both instances survive, and the count is this frame's alone rather than \
         {SENTINEL:#010x} counted up from"
    );
    // The cube's bucket, the pyramid's, then the open box's and the dunes
    // patch's — which this scene puts nothing in. See the note above on why
    // those zeroes are the evidence.
    let instances: Vec<u32> = generated
        .args
        .iter()
        .map(|args| args.instance_count)
        .collect();
    for (what, counted) in [("instance", &instances), ("draw", &generated.counts)] {
        assert!(
            counted.len() > 2,
            "this scene fills two buckets and the table has {} of them, so nothing \
             here is left empty to hold the poison",
            counted.len()
        );
        assert_eq!(
            &counted[..2],
            &[1, 1],
            "the cube's and the pyramid's {what} counts are this frame's alone, rather \
             than {SENTINEL:#010x} counted up from: {:?}",
            generated.args
        );
        assert!(
            counted[2..].iter().all(|&count| count == 0),
            "a bucket this scene put nothing in reports a {what} count of {counted:?}, \
             which is the poison the clearing pass did not remove"
        );
    }

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
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn a_bucket_fills_and_empties_as_its_instance_comes_and_goes() {
    let (headless, mut renderer, mut pool) = setup();
    let camera = mesh_camera(Projection::default());

    // Four frames, so the ring wraps and every slot is used twice: the second
    // pass over a slot is the one that would carry a stale counter.
    // The pyramid's slot, held across rounds: removing it frees element 1 and
    // the next placement takes it back, which is what keeps the CPU reference
    // in `scene` describing the array both halves of a round see.
    let mut pyramid: Option<InstanceHandle> = None;
    for round in 0..2 {
        if let Some(handle) = pyramid.take() {
            renderer.remove_instance(handle);
        }
        let without = generate(&headless, &mut renderer, &mut pool, &camera);
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

        pyramid = Some(place_pyramid(&mut renderer, PYRAMID_AT));
        let with = generate(&headless, &mut renderer, &mut pool, &camera);
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
