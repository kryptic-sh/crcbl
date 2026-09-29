//! **The task stage decides a chunk of pairs per workgroup, and draws, counts
//! and sizes exactly what one workgroup per pair did.**
//!
//! `mesh_cluster.slang`'s `taskMain` takes `TASK_LANES` (cluster, instance
//! slot) pairs a workgroup, compacts the kept ones and launches them with one
//! `DispatchMesh`, and `draw_gen.slang` sizes each bucket's dispatch in those
//! chunks. Three things can go wrong that no single-cluster, single-instance
//! golden shows: a pair decoded to the wrong cluster or slot, a pair past a
//! chunk boundary skipped or drawn twice, and an extent short of the last
//! chunk. So the field here holds a bucket two chunks and six lanes long, one
//! exactly a chunk long, and a bucket of five-cluster open boxes whose 65 pairs
//! end one lane into a third chunk.
//!
//! * [`the_chunked_task_stage_draws_what_one_workgroup_per_pair_draws`]
//!   compares the frame against the one `meshMain` draws on a device without a
//!   task stage — one workgroup per pair from the unchunked extents — byte for
//!   byte, with and without the occlusion cull. The per-cluster cull removes no
//!   pixel, so any difference is a pair drawn wrong.
//! * [`the_chunked_task_stage_counts_every_pair_once`] reads the three cluster
//!   words back and holds them to `crcbl_render::cull`'s oracle over every
//!   pair of the field, and a DAG instance's words to the size of its cut.
//! * [`the_task_extents_are_each_bucket_s_chunks`] reads back the extents
//!   `draw_gen.slang` wrote for the camera's three occlusion regions and holds
//!   them to `crcbl_shaders::meshlet::task_extents`.
//!
//! A device without a task stage has no chunks to test, and says so.

use crate::SUITE;
use crate::harness::Headless;
use crate::mesh_scene::{MESH_EXTENT, place, render_mesh};
use crate::occlusion_cull::{Readable, read_back};
use crcbl::hal::{Features, GeometryPath, ResourceState};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::cull::{ClusterVerdict, cluster_cull_verdict};
use crcbl::render::scene::{
    DEMO_CUBE, DEMO_DUNES, DEMO_OPEN_BOX, DEMO_PYRAMID, DEMO_UNTINTED, SceneDesc,
};
use crcbl::render::{
    Camera, ClusterCull, CullStats, ForwardRenderer, Frustum, OcclusionCulling, Projection,
    TransientPool,
};
use crcbl::shaders::draw_gen::{DRAW_ARGS_WORDS, EARLY_REGION, LATE_REGION, MESH_ARGS_WORDS};
use crcbl::shaders::meshlet::{TASK_LANES, task_extents};

/// The field's meshes, one bucket each, in bucket order.
const SHAPES: [usize; 3] = [DEMO_CUBE, DEMO_PYRAMID, DEMO_OPEN_BOX];

/// Instances of each of [`SHAPES`]: 70 single-cluster cubes are two full chunks
/// and six lanes of a third, 32 pyramids exactly one chunk, and 13 open boxes of
/// five clusters each 65 pairs, one lane into a third chunk.
const COUNTS: [u32; 3] = [70, 32, 13];

/// Instances a row of the field holds.
const ROW: u32 = 12;

/// Frames each arm draws: enough for the occlusion cull to have a previous
/// frame's pyramid to reproject, and for the camera to move past the wall.
const FRAMES: u32 = 6;

/// The field's scene: the demo's meshes cut to [`SHAPES`], so bucket `b` draws
/// shape `b`, with the material every instance takes made double-sided where
/// `double_sided`.
///
/// Double-sided in place rather than a second material, so the scene holds one
/// material mode and one bucket per shape either way.
fn scene(double_sided: bool) -> SceneDesc<'static> {
    let mut scene = crcbl::render::scene::demo();
    if double_sided {
        scene.materials[DEMO_UNTINTED].flags |= crcbl::shaders::mesh::GpuMaterial::DOUBLE_SIDED;
    }
    scene.meshes = SHAPES
        .iter()
        .map(|shape| scene.meshes[*shape].clone())
        .collect();
    scene.capacities.meshes = SHAPES.len() as u32;
    scene
}

/// Every instance of the field, as (bucket, transform), in insertion order.
///
/// Turned differently each, so the open boxes' faces meet the normal cone from
/// every side and some are refused by it.
fn field() -> Vec<(usize, Mat4)> {
    let total: u32 = COUNTS.iter().sum();
    let rows = total.div_ceil(ROW);
    let mut instances = Vec::new();
    let mut index = 0u32;
    for (bucket, count) in COUNTS.iter().enumerate() {
        for _ in 0..*count {
            #[expect(clippy::cast_precision_loss, reason = "a field of a hundred or so")]
            let (x, z, turn) = (
                (index % ROW) as f32 - (ROW as f32 - 1.0) / 2.0,
                (index / ROW) as f32 - (rows as f32 - 1.0) / 2.0,
                index as f32,
            );
            instances.push((
                bucket,
                Mat4::from_translation(Vec3::new(x * 0.75, 0.0, z * 0.75 - 1.5))
                    * Mat4::from_rotation_y(turn * 0.61)
                    * Mat4::from_rotation_x(turn * 0.23)
                    * Mat4::from_scale(Vec3::splat(0.28)),
            ));
            index += 1;
        }
    }
    instances
}

/// A wall in front of the field's left half, so the occlusion cull has
/// something to hide and a moving camera something to rescue. A cube, so it
/// lands in the cube bucket and lengthens that run by one.
fn wall() -> Mat4 {
    Mat4::from_translation(Vec3::new(-1.5, 0.6, 2.0)) * Mat4::from_scale(Vec3::new(3.5, 1.2, 0.2))
}

/// The camera on frame `step`: in front of the field, sliding right past the
/// wall so instances it hid come into view.
fn camera(step: u32) -> Camera {
    #[expect(clippy::cast_precision_loss, reason = "a handful of frames")]
    let x = -3.0 + step as f32;
    Camera {
        eye: Vec3::new(x, 2.4, 7.5),
        target: Vec3::new(0.0, 0.0, -1.5),
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// Opens the ring on the mesh tail, with or without the task stage, and says
/// whether the stage was granted.
fn open(task: bool) -> (Headless, bool) {
    let asked = crcbl::screenshot::OffscreenSetup::OPTIONAL_FEATURES;
    let asked = if task {
        asked
    } else {
        asked.difference(Features::TASK_SHADER)
    };
    let headless = Headless::open_for_mesh_with(asked);
    let granted = headless.device.caps().features;
    assert!(
        task || !granted.contains(Features::TASK_SHADER),
        "the arm dropped the task stage and the device granted it anyway"
    );
    let has = granted.contains(Features::MESH_SHADER | Features::TASK_SHADER);
    (headless, has)
}

/// The field's renderer on the mesh tail, with the wall where `walled` and
/// [`scene`]'s double-sided material where `double_sided`.
fn renderer(
    headless: &Headless,
    walled: bool,
    double_sided: bool,
    culling: OcclusionCulling,
) -> ForwardRenderer {
    let device = headless.device.as_ref();
    let mut renderer = ForwardRenderer::with_scene_on_path(
        device,
        headless.queue,
        headless.format,
        &scene(double_sided),
        GeometryPath::MeshShader,
    )
    .expect("the field builds on the mesh tail");
    renderer.set_occlusion_culling(culling);
    for (bucket, transform) in field() {
        place(&mut renderer, bucket, DEMO_UNTINTED, transform);
    }
    if walled {
        place(&mut renderer, 0, DEMO_UNTINTED, wall());
    }
    renderer
}

/// Every frame of the camera's slide, drawn on the mesh tail with or without
/// the task stage.
fn slide(task: bool, culling: OcclusionCulling) -> Option<Vec<crcbl_golden::Image>> {
    let (headless, has) = open(task);
    if task && !has {
        headless.finish();
        return None;
    }
    let device = headless.device.as_ref();
    let mut renderer = renderer(&headless, true, true, culling);
    let mut pool = TransientPool::new();
    let frames = (0..FRAMES)
        .map(|step| render_mesh(&headless, &mut renderer, &mut pool, &camera(step), None))
        .collect();
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    Some(frames)
}

/// **Chunked, the task stage draws every frame of the slide byte for byte as
/// `meshMain` draws it one workgroup per pair**, with the occlusion cull off
/// and on.
///
/// `meshMain` culls nothing and reads the `(clusters, instances, 1)` extents a
/// generator without a task stage leaves, so it is the unchunked reference on
/// the same device. **The field is double-sided here**, so the normal cone
/// refuses nothing and the frustum only what no view rasterises. Drawn
/// single-sided, the two paths already differed by 29 bytes on an RX 7900 XTX
/// before chunking existed — measured on the commit before it, cause not
/// traced — so the cone is not a pixel-neutral reference. With it out of the
/// way, a decode that named another cluster or
/// slot, a compaction that lost or repeated a pair, or an extent short of a
/// bucket's last chunk moves pixels here. The single-sided cone is
/// [`the_chunked_task_stage_counts_every_pair_once`]'s. Under occlusion the
/// camera's three regions are each sized by `draw_gen.slang` separately, and
/// the late one only after the second phase.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_chunked_task_stage_draws_what_one_workgroup_per_pair_draws() {
    for culling in [
        OcclusionCulling::OFF,
        OcclusionCulling {
            occlusion: true,
            small_feature_pixels: None,
        },
    ] {
        let Some(chunked) = slide(true, culling) else {
            eprintln!("{SUITE}: no task stage on this device, so there are no chunks to compare");
            return;
        };
        let per_pair = slide(false, culling).expect("the task-free arm always draws");
        for (step, (chunked, per_pair)) in chunked.iter().zip(&per_pair).enumerate() {
            let mut colours: Vec<[u8; 4]> = per_pair.pixels().as_chunks::<4>().0.to_vec();
            colours.sort_unstable();
            colours.dedup();
            assert!(
                colours.len() > 16,
                "occlusion {}, frame {step}: {} colours is not a field of shapes",
                culling.occlusion,
                colours.len()
            );
            let differing = chunked
                .pixels()
                .iter()
                .zip(per_pair.pixels())
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(
                differing, 0,
                "occlusion {}, frame {step}: the chunked frame differs from one workgroup per \
                 pair in {differing} bytes",
                culling.occlusion
            );
        }
        eprintln!(
            "{SUITE}: occlusion {}: {FRAMES} chunked frames identical to one workgroup per pair",
            culling.occlusion
        );
    }
}

/// Draws frames of `camera` until the camera's cull statistics carry cluster
/// words for a frame past the first, and answers them.
fn settled_stats(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    camera: &Camera,
) -> (CullStats, ClusterCull) {
    for _ in 0..16 {
        render_mesh(headless, renderer, pool, camera, None);
        if let Some(stats) = renderer.cull_stats()
            && stats.frame >= 2
        {
            let clusters = stats
                .clusters
                .expect("the mesh tail behind a task stage counts its clusters");
            return (stats, clusters);
        }
    }
    panic!("the culling statistics never came round in sixteen frames");
}

/// **Chunked, the task stage counts every pair of the field exactly once, into
/// the word the oracle names**, and a DAG instance's words sum to its cut.
///
/// The field's instances all survive the instance cull, so the three words are
/// over every (cluster, instance) pair of it, and
/// `crcbl_render::cull::cluster_cull_verdict` says which word each belongs in.
/// A lane past a bucket's pairs counted, or a pair counted by two lanes, moves
/// the survivors; a pair decoded onto another cluster moves a rejection between
/// the frustum and the cone.
///
/// The DAG half is `apps/quarry`'s identity at a chunk scale: the dunes patch
/// is hundreds of clusters, so one instance is many chunks, and the words must
/// sum to the clusters its cut selected.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_chunked_task_stage_counts_every_pair_once() {
    let (headless, has) = open(true);
    if !has {
        eprintln!("{SUITE}: no task stage on this device, so there are no chunks to count");
        headless.finish();
        return;
    }
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();

    let mut renderer = renderer(&headless, false, false, OcclusionCulling::OFF);
    let view = Camera {
        eye: Vec3::new(0.0, 5.0, 11.0),
        ..camera(3)
    };
    let (stats, clusters) = settled_stats(&headless, &mut renderer, &mut pool, &view);
    renderer.destroy(device);

    let total: u32 = COUNTS.iter().sum();
    assert_eq!(
        stats.instances,
        u64::from(total),
        "every instance of the field is in front of the camera"
    );
    #[expect(clippy::cast_precision_loss, reason = "a small extent")]
    let aspect = MESH_EXTENT.0 as f32 / MESH_EXTENT.1 as f32;
    let frustum = Frustum::from_view_projection(view.view_projection(aspect));
    let scene = scene(false);
    let mode = scene.materials[DEMO_UNTINTED].mode();
    let mut expected = ClusterCull::default();
    for (bucket, transform) in field() {
        let crcbl::render::scene::Geometry::Flat {
            clusters: mesh_clusters,
            ..
        } = &scene.meshes[bucket].geometry
        else {
            panic!("the field's shapes are flat meshes");
        };
        for cluster in &mesh_clusters.clusters {
            match cluster_cull_verdict(&frustum, view.eye, transform, mode, &cluster.bounds) {
                ClusterVerdict::Kept => expected.survivors += 1,
                ClusterVerdict::RejectedByFrustum => expected.frustum_rejects += 1,
                ClusterVerdict::RejectedByCone => expected.cone_rejects += 1,
            }
        }
    }
    assert!(
        expected.cone_rejects > 0 && expected.survivors > 0,
        "the field must put pairs in more than one word: {expected:?}"
    );
    assert_eq!(
        clusters, expected,
        "the chunked task stage counted {clusters:?} where the oracle says {expected:?}"
    );
    eprintln!("{SUITE}: field {clusters:?} over {total} instances, as the oracle says");

    // The DAG: one dunes patch, from far enough back that its cut is coarse and
    // still many chunks long.
    let mut dunes = ForwardRenderer::with_scene_on_path(
        device,
        headless.queue,
        headless.format,
        &crcbl::render::scene::demo(),
        GeometryPath::MeshShader,
    )
    .expect("the demo scene builds on the mesh tail");
    place(&mut dunes, DEMO_DUNES, DEMO_UNTINTED, Mat4::IDENTITY);
    let far = Camera {
        eye: Vec3::new(0.0, 24.0, -crcbl::shaders::dunes::DUNES_EXTENT - 64.0),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    };
    let (stats, clusters) = settled_stats(&headless, &mut dunes, &mut pool, &far);
    let range = dunes
        .cluster_range(DEMO_DUNES)
        .expect("the dunes are resident");
    let selection = dunes
        .cluster_selection(dunes.frame())
        .expect("the task stage writes the cut");
    let cut: u64 = read_back(
        &headless,
        &mut pool,
        &[Readable::Buffer {
            buffer: selection,
            state: ResourceState::ShaderReadWrite,
            bytes: u64::from(range.base + range.count) * 4,
        }],
    )
    .into_iter()
    .next()
    .expect("the selection")
    .words()[range.base as usize..]
        .iter()
        .map(|word| u64::from(*word))
        .sum();
    eprintln!(
        "{SUITE}: dunes frame {} tested {} clusters of a {cut}-cluster cut",
        stats.frame,
        clusters.tested()
    );
    assert_eq!(
        stats.instances, 1,
        "the dunes patch is in front of the camera"
    );
    assert!(
        cut > u64::from(TASK_LANES),
        "a cut of {cut} clusters is not several chunks, so this proves nothing about them"
    );
    assert_eq!(
        clusters.tested(),
        cut,
        "the chunked task stage tested {} clusters of a {cut}-cluster cut",
        clusters.tested()
    );
    dunes.destroy(device);
    pool.destroy(device);
    headless.finish();
}

/// **`draw_gen.slang` sizes every bucket of the camera's three occlusion
/// regions in whole chunks of its pairs** — the extents it wrote, read back and
/// held to `crcbl_shaders::meshlet::task_extents` over the instance counts it
/// wrote beside them, on every frame of the slide.
///
/// The early region is sized in `startsMain`, region 0 and the late region in
/// `lateFinishMain` once the second phase has counted, so a frame that rescued
/// nothing would leave the late half of that unchecked; the slide past the
/// wall has to rescue something, and the test says so if it did not.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_task_extents_are_each_bucket_s_chunks() {
    let (headless, has) = open(true);
    if !has {
        eprintln!("{SUITE}: no task stage on this device, so there are no chunks to size");
        headless.finish();
        return;
    }
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    let mut renderer = renderer(
        &headless,
        true,
        false,
        OcclusionCulling {
            occlusion: true,
            small_feature_pixels: None,
        },
    );
    let buckets = SHAPES.len();
    let clusters: Vec<u32> = (0..buckets)
        .map(|bucket| {
            renderer
                .cluster_range(bucket)
                .expect("every shape is resident")
                .count
        })
        .collect();
    let regions = LATE_REGION as usize + 1;
    let mut late_drawn = 0;
    let mut long_buckets = 0;
    for step in 0..FRAMES {
        render_mesh(&headless, &mut renderer, &mut pool, &camera(step), None);
        let frame = renderer.frame();
        let mut copied = read_back(
            &headless,
            &mut pool,
            &[
                Readable::Buffer {
                    buffer: renderer.draw_args(frame),
                    state: ResourceState::IndirectArgument,
                    bytes: (buckets * regions * DRAW_ARGS_WORDS * 4) as u64,
                },
                Readable::Buffer {
                    buffer: renderer.draws().counts(frame),
                    state: ResourceState::IndirectArgument,
                    bytes: (buckets * regions * (1 + MESH_ARGS_WORDS) * 4) as u64,
                },
            ],
        )
        .into_iter();
        let args = copied.next().expect("arguments").words();
        let counts = copied.next().expect("extents").words();
        for region in [0, EARLY_REGION, LATE_REGION] {
            let region = region as usize;
            for bucket in 0..buckets {
                let instances = args[(region * buckets + bucket) * DRAW_ARGS_WORDS + 1];
                let at =
                    region * (1 + MESH_ARGS_WORDS) * buckets + buckets + bucket * MESH_ARGS_WORDS;
                let written = [counts[at], counts[at + 1], counts[at + 2]];
                let want = task_extents(clusters[bucket], instances, TASK_LANES);
                assert_eq!(
                    written, want,
                    "frame {step}, region {region}, bucket {bucket}: {} clusters x {instances} \
                     instances",
                    clusters[bucket]
                );
                if region == LATE_REGION as usize {
                    late_drawn += instances;
                }
                if region == 0 && want[0] > 1 {
                    long_buckets += 1;
                }
            }
        }
    }
    assert!(
        late_drawn > 0,
        "the slide rescued nothing, so the late region's extents were never non-zero"
    );
    assert!(
        long_buckets > 0,
        "no bucket was more than one chunk long, so no boundary was sized"
    );
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}
