//! **A description with two cluster DAGs, five meshes and capacities the caller
//! chose, drawn on the device this suite opens** — `docs/backlog.md`'s "no
//! device has drawn a description with two DAGs".
//!
//! # What this covers that the null backend cannot
//!
//! `crcbl-render`'s `a_second_dag_reaches_its_own_groups_and_not_the_first_s`
//! builds the same shape and asserts the three things a second DAG uniquely
//! decides — one mesh table id per description mesh, a `group_stride` that sums
//! both hierarchies, and every `ClusterSelect` record of the second DAG naming a
//! group in the second half of the concatenated `level_groups`. It reads all
//! three out of a recorder, on a backend that creates no memory and runs no
//! shader. Nothing it asserts has ever been in front of a driver.
//!
//! This runs the same description through a real one, and the difference is
//! everything the null backend answers `Ok` to without looking: the geometry and
//! cluster pools are sized from **non-default [`Capacities`]** and really
//! allocated, five meshes and two whole hierarchies are really uploaded into
//! them, the descriptor set really binds a selection buffer sized for both DAGs'
//! clusters, and a frame with an instance of each really dispatches. The
//! observable is [`ForwardRenderer::cluster_selection`] — the backlog names it,
//! and it needs no new golden image — read once per DAG over the run
//! [`ForwardRenderer::cluster_range`] gives that DAG. **Both runs must come out
//! of the frame with clusters chosen in them.** A second DAG whose bucket never
//! dispatched, whose clusters landed in the first's run, or whose selection
//! buffer was sized for one hierarchy leaves its own run at the zeroes nothing
//! wrote.
//!
//! # And what it still does not cover
//!
//! * **Not which groups the second DAG's records name.** The engine has one DAG,
//!   so a second one here is a second copy of it and the two runs' cuts are
//!   alike by construction; records that wrongly named the first DAG's groups
//!   would still produce a cut, and this would still be green. That is the
//!   assertion `a_second_dag_reaches_its_own_groups_and_not_the_first_s` makes
//!   and it stays the null backend's, because it reads the `ClusterSelect`
//!   records the host wrote rather than anything the frame produced.
//! * **Not the picture.** No golden is compared here — two dune patches side by
//!   side is a frame nobody has blessed — so this says both DAGs were *selected
//!   from*, not that either was shaded correctly.
//! * **Not two DAGs of different shapes.** Both hierarchies have the same depth
//!   and the same group count, so nothing here would catch an offset that
//!   happened to be right only for equal-sized runs.
//! * **Not `Capacities` sized to fit.** The two the description raises are
//!   doubled rather than measured, so this exercises a non-default value and not
//!   a tight one; `crcbl-render`'s
//!   `a_description_that_exactly_fits_its_capacities_is_built` is where the
//!   boundary lives, on the null backend.
//!
//! [`Capacities`]: crcbl::render::scene::Capacities

use crate::harness::{Headless, poisoned};
use crate::lod::selected_level;
use crate::mesh_scene::{place, place_cube, render_mesh};
use crcbl::hal::{
    Barriers, BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferUsage, CommandEncoderDesc,
    Features, MemoryLocation, ResourceState, SubmitInfo,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{DEMO_DUNES, DEMO_UNTINTED, SceneDesc};
use crcbl::render::{Camera, ClusterRange, ForwardRenderer, Projection, TransientPool};
use crcbl::shaders::dunes::DUNES_EXTENT;

/// [`crcbl::render::scene::demo`] with its DAG appended a second time, and the
/// description index the copy landed at.
///
/// **Five meshes, two DAGs and capacities that are not the default** — the three
/// things `docs/backlog.md` records as never having reached a device, in one
/// description. A second copy of the engine's own DAG rather than a second shape
/// because the engine has exactly one; see this module's header for what that
/// costs the assertions.
fn two_dag_scene() -> (SceneDesc<'static>, usize) {
    let mut scene = crcbl::render::scene::demo();
    let mut again = scene.meshes[DEMO_DUNES].clone();
    again.label = "dunes again".into();
    scene.meshes.push(again);
    // Room for the second copy. Doubling rather than measuring, because what
    // matters here is that a description carrying its own sizes reaches a real
    // allocator at all — the exact fit is `crcbl-render`'s to check.
    scene.capacities.vertices *= 2;
    scene.capacities.indices *= 2;
    let second = scene.meshes.len() - 1;
    (scene, second)
}

/// How far either patch stands from the origin along `x`.
///
/// One whole patch across plus a unit, so the two occupy disjoint ground rather
/// than interpenetrating — two instances of one mesh at one place would be a
/// scene where a wrong run is indistinguishable from the right one by position
/// as well as by geometry.
const PATCH_OFFSET: f32 = DUNES_EXTENT + 1.0;

/// Where the camera stands to have **both** patches in front of it.
///
/// Far enough back that the near edge of the pair is inside the horizontal field
/// of view: an instance the cull rejects launches no task group at all, and its
/// run would then hold whatever the buffer was created with rather than a cut.
/// Distance along `-z` and a little height, [`crate::lod`]'s shape.
fn both_patches_camera() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 8.0, -DUNES_EXTENT - 128.0),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// Opens the fixture on a geometry path that **has** an observable for which
/// clusters a DAG chose.
///
/// The mesh path when the device has an amplification stage, because
/// [`ForwardRenderer::cluster_selection`] is the buffer that stage writes. A
/// device with [`Features::MESH_SHADER`] and no [`Features::TASK_SHADER`] is a
/// real and supported state with **neither** observable — that buffer is `None`
/// and `level_buckets` is empty on the mesh path — so this withholds the mesh
/// features from it and takes the uniform cut, which every backend has. Skipping
/// instead would report "not supported here" as "passed", which is the trap
/// `docs/plan/12-testing.md` names and this suite's header opens with.
fn open_with_an_observable() -> Headless {
    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    if headless.device.caps().supports(Features::TASK_SHADER) {
        return headless;
    }
    headless.finish();
    Headless::open_for_mesh_with(Features::GPU_DRIVEN)
}

/// Copies one DAG's run of this frame's [`ForwardRenderer::cluster_selection`]
/// out and answers it word for word — `1` where the descent chose that cluster.
///
/// A submission of its own after the frame's: the graph leaves the buffer in
/// [`ResourceState::ShaderReadWrite`], which is where the next frame on that
/// slot expects it, so this moves it out and puts it straight back. Only `range`
/// is copied, so the two calls this test makes read two disjoint spans of one
/// buffer and neither can answer with the other's clusters.
fn chosen_run(headless: &Headless, renderer: &ForwardRenderer, range: ClusterRange) -> Vec<u32> {
    let device = headless.device.as_ref();
    let selection: BufferHandle = renderer
        .cluster_selection(renderer.frame())
        .expect("an amplification stage records its cut");
    let bytes = u64::from(range.count) * 4;

    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("two-dag selection readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("two-dag selection copy"),
        queue: headless.queue,
    });
    let barrier = |from: ResourceState, to: ResourceState| {
        [BufferBarrier {
            buffer: selection,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::ShaderReadWrite, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::ShaderReadWrite);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: selection,
        src_offset: u64::from(range.base) * 4,
        dst: staging,
        dst_offset: 0,
        size: bytes,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let mut words = poisoned(bytes as usize);
    headless.readback(staging, bytes, &mut words);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    words
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("four bytes")))
        .collect()
}

/// **Both DAGs of a five-mesh description come out of a real frame with
/// clusters chosen in them.**
///
/// The description is [`two_dag_scene`], the frame has an instance of each patch
/// side by side, and what is read back is the run
/// [`ForwardRenderer::cluster_range`] gives each DAG out of the buffer the
/// amplification stage wrote. Three assertions per run, and the first two are
/// what make the third worth making:
///
/// * **The two runs are disjoint**, which is what makes reading one an answer
///   about that DAG rather than about the pool.
/// * **Every word is a flag.** The buffer holds `0` or `1` and nothing else, so
///   a span copied out of some other buffer is caught here rather than counted
///   below as clusters that were chosen.
/// * **And each run has at least one cluster chosen.** A second DAG whose bucket
///   never dispatched, or whose clusters were selected into the first DAG's run,
///   leaves its own span at zero.
///
/// Off the mesh path there is no amplification stage and no such buffer, so the
/// same claim is made through the uniform cut's own observable: the bucket a
/// level drew through, per DAG, which [`selected_level`] reads out of the
/// indirect arguments. Every backend runs one branch or the other and neither is
/// a skip — see [`open_with_an_observable`].
///
/// See this module's header for what a device adds over
/// `crcbl-render`'s `a_second_dag_reaches_its_own_groups_and_not_the_first_s`,
/// and for the four things neither of them covers.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn both_dags_of_a_five_mesh_description_have_clusters_chosen() {
    let (scene, second_dag) = two_dag_scene();
    assert_eq!(
        scene.meshes.len(),
        5,
        "five meshes, and the fifth is the DAG"
    );

    let headless = open_with_an_observable();
    let mut renderer = ForwardRenderer::with_scene(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
        &scene,
    )
    .expect("a five-mesh description with two DAGs is one a device can make resident");
    let mut pool = TransientPool::new();

    // The cube first, this suite's insertion-order convention, then a patch of
    // each DAG on either side of it.
    place_cube(&mut renderer);
    let patch = |renderer: &mut ForwardRenderer, mesh: usize, x: f32| {
        place(
            renderer,
            mesh,
            DEMO_UNTINTED,
            Mat4::from_translation(Vec3::new(x, 0.0, 0.0)),
        );
    };
    patch(&mut renderer, DEMO_DUNES, -PATCH_OFFSET);
    patch(&mut renderer, second_dag, PATCH_OFFSET);

    let _ = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &both_patches_camera(),
        None,
    );

    let dags = [
        ("the first DAG", DEMO_DUNES),
        ("the second DAG", second_dag),
    ];
    if renderer.culls_clusters() {
        let runs = dags.map(|(name, mesh)| {
            let range = renderer
                .cluster_range(mesh)
                .expect("the mesh path has a cluster pool");
            (name, range)
        });
        assert!(
            runs[1].1.base >= runs[0].1.base + runs[0].1.count,
            "the two DAGs share pool clusters — {:?} against {:?} — so neither run is an \
             answer about one of them",
            runs[0].1,
            runs[1].1,
        );
        for (name, range) in runs {
            let words = chosen_run(&headless, &renderer, range);
            for (cluster, &word) in words.iter().enumerate() {
                assert!(
                    word <= 1,
                    "{name}'s cluster {cluster} of {} recorded {word:#010x}, which is not a \
                     flag — the span this read is not the one the amplification stage \
                     wrote its cut into",
                    range.count,
                );
            }
            let chosen = words.iter().filter(|&&word| word == 1).count();
            eprintln!(
                "{}: {name} — {chosen} of {} clusters chosen",
                crate::SUITE,
                range.count,
            );
            assert!(
                chosen > 0,
                "{name}'s run of {} clusters came out of the frame with none chosen at all. \
                 A DAG whose bucket never dispatched, or whose clusters were selected into \
                 another DAG's run, is what leaves a span at zero",
                range.count,
            );
        }
    } else {
        // The uniform cut: one level per instance, and the bucket whose
        // instance count came out non-zero is that level.
        for (name, mesh) in dags {
            let level = selected_level(&headless, &renderer, mesh).unwrap_or_else(|| {
                panic!(
                    "{name} scattered its instance into none of its buckets, so no level of \
                     it drew"
                )
            });
            eprintln!("{}: {name} — uniform cut at level {level}", crate::SUITE);
        }
    }

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}
