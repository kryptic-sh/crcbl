//! **The mesh tail draws a field of many buckets byte for byte as it does a
//! call per bucket**, whether its runs are long enough to become one
//! multi-draw call or are kept a call per bucket.
//!
//! `render_e2e`'s `a_call_per_range_draws_every_scene_as_a_call_per_bucket_does`
//! draws the builtin scenes, whose runs of buckets are all shorter than the
//! mesh tail's threshold behind a task stage — so on that arm it compares the
//! split path against the call per bucket, and never draws a multi-draw
//! `draw_mesh_tasks_indirect` through a task stage at all. This is the field
//! that does: [`MANY`] buckets in one run, and [`FEW`] beside it for the short
//! run, each drawn with and without `Features::DRAW_INDEX`.
//!
//! The meshes alternate between the demo's cube, pyramid and open box, so a
//! draw of a range that read another bucket's clusters, run or arguments puts
//! the wrong shape somewhere in the frame rather than an identical one.

use crate::SUITE;
use crate::harness::Headless;
use crate::mesh_scene::{place, render_mesh};
use crcbl::hal::{Features, GeometryPath};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{DEMO_CUBE, DEMO_OPEN_BOX, DEMO_PYRAMID, DEMO_UNTINTED};
use crcbl::render::{Camera, ForwardRenderer, Projection, TransientPool};

/// A run long enough to become one call on every tail — well past the mesh
/// tail's task-stage threshold, which is private to `crcbl-render`.
const MANY: u32 = 64;

/// A run short enough to stay a call per bucket behind a task stage, and to
/// become one call without one.
const FEW: u32 = 4;

/// Instances per bucket, so every draw of a range has more than one.
const PER_BUCKET: u32 = 2;

/// Instances a row of the field holds.
const ROW: u32 = 16;

/// `buckets` meshes, cycling through three distinct demo shapes.
fn scene(buckets: u32) -> crcbl::render::scene::SceneDesc<'static> {
    let mut scene = crcbl::render::scene::demo();
    let shapes = [DEMO_CUBE, DEMO_PYRAMID, DEMO_OPEN_BOX];
    scene.meshes = (0..buckets as usize)
        .map(|bucket| scene.meshes[shapes[bucket % shapes.len()]].clone())
        .collect();
    let most = |counts: [usize; 3]| counts.into_iter().max().unwrap_or(0) as u32;
    scene.capacities.meshes = buckets;
    scene.capacities.vertices = buckets
        * most([
            crcbl::shaders::mesh::CUBE_VERTEX_COUNT,
            crcbl::shaders::mesh::PYRAMID_VERTEX_COUNT,
            crcbl::shaders::mesh::OPEN_BOX_VERTEX_COUNT,
        ]);
    scene.capacities.indices = buckets
        * most([
            crcbl::shaders::mesh::CUBE_INDEX_COUNT,
            crcbl::shaders::mesh::PYRAMID_INDEX_COUNT,
            crcbl::shaders::mesh::OPEN_BOX_INDEX_COUNT,
        ]);
    scene
}

/// Where instance `index` stands: a field of [`ROW`] a row, centred on the
/// origin, each shape turned so three of its faces show.
fn placed(index: u32, instances: u32) -> Mat4 {
    let rows = instances.div_ceil(ROW);
    #[expect(clippy::cast_precision_loss, reason = "a field of a hundred or so")]
    let (x, z) = (
        (index % ROW) as f32 - (ROW as f32 - 1.0) / 2.0,
        (index / ROW) as f32 - (rows as f32 - 1.0) / 2.0,
    );
    Mat4::from_translation(Vec3::new(x * 0.7, 0.0, z * 0.7))
        * Mat4::from_rotation_y(0.6)
        * Mat4::from_scale(Vec3::splat(0.3))
}

/// The camera: above and in front of the field, looking into it.
fn camera() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 5.0, 7.0),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// One frame of the `buckets` field on the mesh tail, on a device asked for
/// `asked`, and the calls it recorded.
fn draw(buckets: u32, asked: Features) -> (crcbl_golden::Image, u64, Features) {
    let headless = Headless::open_for_mesh_with(asked);
    let device = headless.device.as_ref();
    let granted = device.caps().features;
    let mut renderer = ForwardRenderer::with_scene_on_path(
        device,
        headless.queue,
        headless.format,
        &scene(buckets),
        GeometryPath::MeshShader,
    )
    .expect("the field builds on the mesh tail");
    let instances = buckets * PER_BUCKET;
    for index in 0..instances {
        place(
            &mut renderer,
            (index % buckets) as usize,
            DEMO_UNTINTED,
            placed(index, instances),
        );
    }
    let mut pool = TransientPool::new();
    let image = render_mesh(&headless, &mut renderer, &mut pool, &camera(), None);
    let calls = renderer.counters().draws;
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    (image, calls, granted)
}

/// **A long run drawn as one call and a short one kept a call per bucket both
/// draw the frame the call per bucket draws**, with and without a task stage.
///
/// Per arm the two frames differ only in [`Features::DRAW_INDEX`]. The call
/// counts say which path each took: [`MANY`] buckets always draw in fewer
/// calls with a draw index, and [`FEW`] do too without a task stage, but not
/// behind one — that run is shorter than the threshold and stays a call per
/// bucket. A device without mesh shaders or a draw index has nothing to
/// compare, and says so.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_ranged_mesh_field_draws_as_a_call_per_bucket_does() {
    let offered = {
        let headless = Headless::open_for_mesh_with(Features::all());
        let features = headless.device.caps().features;
        headless.finish();
        features
    };
    let needs = Features::MESH_SHADER | Features::DRAW_INDEX | Features::MULTI_DRAW_INDIRECT;
    if !offered.contains(needs) {
        eprintln!("{SUITE}: this device has no ranged mesh tail, so there is nothing to compare");
        return;
    }
    let base = crcbl::screenshot::OffscreenSetup::OPTIONAL_FEATURES;
    let mut compared = 0;
    for task in [true, false] {
        if task && !offered.contains(Features::TASK_SHADER) {
            eprintln!("{SUITE}: no task stage on this device, so that arm is not drawn");
            continue;
        }
        let asked = if task {
            base
        } else {
            base.difference(Features::TASK_SHADER)
        };
        for buckets in [FEW, MANY] {
            let (per_bucket, per_bucket_calls, granted) =
                draw(buckets, asked.difference(Features::DRAW_INDEX));
            assert!(!granted.contains(Features::DRAW_INDEX));
            let (ranged, ranged_calls, granted) = draw(buckets, asked);
            assert!(granted.contains(Features::DRAW_INDEX));
            assert_eq!(
                granted.contains(Features::TASK_SHADER),
                task,
                "the device was not granted the task stage the arm asked for"
            );
            let case = format!("{buckets} buckets, task stage {task}");
            if task && buckets == FEW {
                assert_eq!(
                    ranged_calls, per_bucket_calls,
                    "{case}: a run shorter than the threshold stays a call per bucket"
                );
            } else {
                assert!(
                    ranged_calls < per_bucket_calls,
                    "{case}: {ranged_calls} calls with a draw index against {per_bucket_calls} \
                     without, so nothing was drawn as a range"
                );
            }
            let mut colours: Vec<[u8; 4]> = per_bucket.pixels().as_chunks::<4>().0.to_vec();
            colours.sort_unstable();
            colours.dedup();
            assert!(
                colours.len() > 16,
                "{case}: the frame holds {} colours, which is not a field of shapes",
                colours.len()
            );
            let differing = per_bucket
                .pixels()
                .iter()
                .zip(ranged.pixels())
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(
                differing, 0,
                "{case}: the ranged frame differs from the call-per-bucket one in {differing} \
                 bytes"
            );
            eprintln!(
                "{SUITE}: {case}: {ranged_calls} calls against {per_bucket_calls}, frames \
                 identical"
            );
            compared += 1;
        }
    }
    assert!(compared > 0, "no arm was drawn");
}
