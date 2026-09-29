//! **Behind a task stage a pass records one flat call per pipeline partition,
//! and it draws and sizes what one workgroup per pair did.**
//!
//! `draw_gen.slang` scans each closed draw region's task chunks into a run of
//! chunk starts and sizes one `draw_mesh_tasks_indirect` per flat segment —
//! a partition's run of buckets — from them, and `mesh_cluster.slang`'s
//! `taskMain` finds each workgroup's bucket by searching those starts. What can
//! go wrong that `task_chunks.rs`' one-mode field does not show: a segment
//! sized from the wrong run, a search that lands one bucket off at a boundary,
//! a region scanned before its counts are final, and a scan whose lanes each
//! own several buckets.
//!
//! * [`a_two_mode_flat_frame_draws_what_one_workgroup_per_pair_draws`] draws a
//!   field in two material modes — two depth pipelines, so two segments in
//!   every depth pass — with the sun's cascades and a shadowed point light's
//!   faces, and compares every frame of a camera slide against the one
//!   `meshMain` draws on a device without a task stage, byte for byte, with
//!   and without the occlusion cull.
//! * [`the_flat_work_list_is_every_closed_region_s_chunks`] reads back the
//!   camera's, a cascade's and the point light's chunk starts and flat
//!   dispatches after every frame and holds them to
//!   `crcbl_shaders::meshlet`'s host twins over the instance counts the
//!   generator wrote beside them — on a table of nine buckets and on one of
//!   450, past the one bucket a lane of the scans owns.
//!
//! A device without a task stage has no flat calls to test, and says so.

use crate::SUITE;
use crate::harness::Headless;
use crate::mesh_scene::{place, render_mesh};
use crate::occlusion_cull::{Readable, read_back};
use crcbl::hal::{Features, GeometryPath, ResourceState};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{DEMO_CUBE, DEMO_OPEN_BOX, DEMO_PYRAMID, DEMO_UNTINTED, SceneDesc};
use crcbl::render::{
    Camera, DrawGen, ForwardRenderer, Light, OcclusionCulling, PointLight, Projection,
    TransientPool,
};
use crcbl::shaders::draw_gen::{
    DRAW_ARGS_WORDS, DrawMode, EARLY_REGION, FACE_REGION_BASE, FLAT_SEGMENTS, LATE_REGION,
    chunk_start_word, flat_args_word, flat_segments,
};
use crcbl::shaders::mesh::GpuMaterial;
use crcbl::shaders::meshlet::{TASK_LANES, chunk_starts, task_chunks, task_dispatch};

/// The field's meshes, cycled through in bucket order within each mode.
const SHAPES: [usize; 3] = [DEMO_CUBE, DEMO_PYRAMID, DEMO_OPEN_BOX];

/// Instances of each of [`SHAPES`] per mode on the small table: 70
/// single-cluster cubes are two chunks and six lanes of a third, 32 pyramids
/// exactly one chunk, and 13 five-cluster open boxes one lane into a third.
const COUNTS: [u32; 3] = [70, 32, 13];

/// Instances a row of the field holds.
const ROW: u32 = 16;

/// Frames each arm draws: enough for the occlusion cull to have a previous
/// frame's pyramid, and for the camera to move past the wall.
const FRAMES: u32 = 6;

/// The field's scene: [`SHAPES`] repeated `laps` times, and two materials of
/// two modes — the demo's untinted row made double-sided, and a copy of it that
/// also alpha-masks, appended — so every mesh the field draws is two buckets and
/// every depth pass binds two pipelines. The demo's other rows are opaque, so
/// the table holds a third mode's buckets too, which nothing draws.
///
/// **Both double-sided**, for `task_chunks.rs`' reason: single-sided, the task
/// path and `meshMain` already differed by a few bytes before chunking, so the
/// normal cone is not a pixel-neutral reference. The mask cuts nothing, since
/// the untinted row is opaque, and is there for the second pipeline.
fn scene(laps: usize) -> SceneDesc<'static> {
    let mut scene = crcbl::render::scene::demo();
    scene.materials[DEMO_UNTINTED].flags |= GpuMaterial::DOUBLE_SIDED;
    let mut masked = scene.materials[DEMO_UNTINTED];
    masked.flags |= GpuMaterial::ALPHA_MODE_MASK;
    scene.materials.push(masked);
    let meshes: Vec<_> = (0..laps * SHAPES.len())
        .map(|index| scene.meshes[SHAPES[index % SHAPES.len()]].clone())
        .collect();
    let most = [
        crcbl::shaders::mesh::CUBE_VERTEX_COUNT,
        crcbl::shaders::mesh::PYRAMID_VERTEX_COUNT,
        crcbl::shaders::mesh::OPEN_BOX_VERTEX_COUNT,
    ]
    .into_iter()
    .max()
    .unwrap_or(0) as u32;
    let most_indices = [
        crcbl::shaders::mesh::CUBE_INDEX_COUNT,
        crcbl::shaders::mesh::PYRAMID_INDEX_COUNT,
        crcbl::shaders::mesh::OPEN_BOX_INDEX_COUNT,
    ]
    .into_iter()
    .max()
    .unwrap_or(0) as u32;
    let count = u32::try_from(meshes.len()).expect("a few hundred meshes");
    scene.capacities.meshes = count;
    scene.capacities.vertices = scene.capacities.vertices.max(count * most);
    scene.capacities.indices = scene.capacities.indices.max(count * most_indices);
    scene.meshes = meshes;
    scene
}

/// The material row of mode `mode` in [`scene`]: 0 the double-sided one, 1 the
/// masked copy appended behind the demo's rows.
fn material(mode: usize) -> usize {
    if mode == 0 {
        DEMO_UNTINTED
    } else {
        crcbl::render::scene::demo().materials.len()
    }
}

/// Every instance of the field on a table of `laps` laps, as (mesh, material,
/// transform): [`COUNTS`] of each shape per mode on one lap, two of each mesh
/// per mode on more.
fn field(laps: usize) -> Vec<(usize, usize, Mat4)> {
    let mut placed = Vec::new();
    for mode in 0..2 {
        for mesh in 0..laps * SHAPES.len() {
            let count = if laps == 1 {
                COUNTS[mesh % SHAPES.len()]
            } else {
                2
            };
            for _ in 0..count {
                placed.push((mesh, material(mode)));
            }
        }
    }
    let total = u32::try_from(placed.len()).expect("a field of thousands");
    let rows = total.div_ceil(ROW);
    (0u32..)
        .zip(placed)
        .map(|(index, (mesh, material))| {
            #[expect(clippy::cast_precision_loss, reason = "a field of thousands")]
            let (x, z, turn) = (
                (index % ROW) as f32 - (ROW as f32 - 1.0) / 2.0,
                (index / ROW) as f32 - (rows as f32 - 1.0) / 2.0,
                index as f32,
            );
            (
                mesh,
                material,
                Mat4::from_translation(Vec3::new(x * 0.75, 0.0, z * 0.75 - 1.5))
                    * Mat4::from_rotation_y(turn * 0.61)
                    * Mat4::from_rotation_x(turn * 0.23)
                    * Mat4::from_scale(Vec3::splat(0.28)),
            )
        })
        .collect()
}

/// A wall in front of the field's left half, so the occlusion cull has
/// something to hide and a moving camera something to rescue.
fn wall() -> Mat4 {
    Mat4::from_translation(Vec3::new(-1.5, 0.6, 2.0)) * Mat4::from_scale(Vec3::new(3.5, 1.2, 0.2))
}

/// The shadowed point light over the field on frame `step`, whose six faces are
/// the [`DrawMode::Faces`] generator's regions. A step along its path a frame,
/// so its faces are culled and drawn every frame rather than kept from the
/// last.
fn lamp(step: u32) -> Light {
    #[expect(clippy::cast_precision_loss, reason = "a handful of frames")]
    let along = step as f32 * 0.05;
    Light::Point(PointLight {
        position: Vec3::new(0.5 + along, 2.5, -1.0),
        radius: 9.0,
        color: Vec3::splat(20.0),
        fill: false,
    })
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
    (
        headless,
        granted.contains(Features::MESH_SHADER | Features::TASK_SHADER),
    )
}

/// The field's renderer on the mesh tail, walled and lit by [`lamp`].
fn renderer(headless: &Headless, laps: usize, culling: OcclusionCulling) -> ForwardRenderer {
    let device = headless.device.as_ref();
    let mut renderer = ForwardRenderer::with_scene_on_path(
        device,
        headless.queue,
        headless.format,
        &scene(laps),
        GeometryPath::MeshShader,
    )
    .expect("the field builds on the mesh tail");
    renderer.set_occlusion_culling(culling);
    // Every cascade and the light's faces culled and drawn every frame, so
    // every frame's shadow generators hold that frame's work lists.
    renderer.set_shadow_cadence(Some(crcbl::render::shadow::Cadence::EVERY_FRAME));
    renderer.set_lights(&[lamp(0)]);
    for (mesh, material, transform) in field(laps) {
        place(&mut renderer, mesh, material, transform);
    }
    place(&mut renderer, 0, DEMO_UNTINTED, wall());
    renderer
}

/// Every frame of the slide, on the mesh tail with or without the task stage.
fn slide(task: bool, culling: OcclusionCulling) -> Option<Vec<crcbl_golden::Image>> {
    let (headless, has) = open(task);
    if task && !has {
        headless.finish();
        return None;
    }
    let device = headless.device.as_ref();
    let mut renderer = renderer(&headless, 1, culling);
    let mut pool = TransientPool::new();
    let frames = (0..FRAMES)
        .map(|step| {
            renderer.set_lights(&[lamp(step)]);
            render_mesh(&headless, &mut renderer, &mut pool, &camera(step), None)
        })
        .collect();
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    Some(frames)
}

/// **Flat, the task stage draws every frame of a two-mode slide byte for byte
/// as `meshMain` draws it one workgroup per pair**, shadows included, with the
/// occlusion cull off and on.
///
/// Two modes are two depth pipelines, so the depth prepass, every cascade and
/// every face of the point light record two flat calls where one mode records
/// one, each searching its own segment of the table; the colour pass records
/// one, over both. A segment sized from another's run, or a search that
/// stepped across a segment's end, drops or doubles a bucket's instances under
/// a pipeline, which moves pixels here. `meshMain` reads the per-bucket
/// extents on a device with no task stage and culls nothing, so it is the
/// reference on the same device.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_two_mode_flat_frame_draws_what_one_workgroup_per_pair_draws() {
    for culling in [
        OcclusionCulling::OFF,
        OcclusionCulling {
            occlusion: true,
            small_feature_pixels: None,
        },
    ] {
        let Some(flat) = slide(true, culling) else {
            eprintln!("{SUITE}: no task stage on this device, so there are no flat calls");
            return;
        };
        let per_pair = slide(false, culling).expect("the task-free arm always draws");
        for (step, (flat, per_pair)) in flat.iter().zip(&per_pair).enumerate() {
            let mut colours: Vec<[u8; 4]> = per_pair.pixels().as_chunks::<4>().0.to_vec();
            colours.sort_unstable();
            colours.dedup();
            assert!(
                colours.len() > 16,
                "occlusion {}, frame {step}: {} colours is not a field of shapes",
                culling.occlusion,
                colours.len()
            );
            let differing = flat
                .pixels()
                .iter()
                .zip(per_pair.pixels())
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(
                differing, 0,
                "occlusion {}, frame {step}: the flat frame differs from one workgroup per pair \
                 in {differing} bytes",
                culling.occlusion
            );
        }
        eprintln!(
            "{SUITE}: occlusion {}: {FRAMES} two-mode flat frames identical to one workgroup per \
             pair",
            culling.occlusion
        );
    }
}

/// The bucket table a generator draws: each bucket's cluster count, and the
/// flat segments `crcbl_shaders::draw_gen::flat_segments` lays down for it.
struct Table {
    clusters: Vec<u32>,
    segments: [(u32, u32); FLAT_SEGMENTS as usize],
}

/// What the closed regions of the generators were held to.
#[derive(Default)]
struct Checked {
    /// Regions whose chunk starts and flat dispatches were compared.
    regions: u32,
    /// Buckets more than one chunk long, so a boundary inside a bucket counted.
    long_buckets: u32,
    /// Segments of more than one non-empty bucket, so the search had a choice.
    shared_segments: u32,
    /// Instances the late region drew, so its scan in `lateFinishMain` held
    /// something.
    late_instances: u32,
    /// Face regions compared, so the point light's six-region scan ran.
    faces: u32,
}

/// Reads back `generator`'s chunk starts and flat dispatches for `frame` and
/// holds every region the frame closed to the host twins over the instance
/// counts it wrote beside them, adding what it saw to `checked`.
fn check_generator(
    headless: &Headless,
    pool: &mut TransientPool,
    (generator, frame): (&DrawGen, usize),
    table: &Table,
    what: &str,
    checked: &mut Checked,
) {
    let Table { clusters, segments } = table;
    let buckets = generator.bucket_count();
    let capacity = generator.visible_capacity();
    let regions = generator.mode().regions();
    let closed: Vec<u32> = match generator.frame_mode(frame) {
        DrawMode::Plain => vec![0],
        DrawMode::Occlusion => vec![0, EARLY_REGION, LATE_REGION],
        DrawMode::Faces => {
            checked.faces += 6;
            (FACE_REGION_BASE..FACE_REGION_BASE + 6).collect()
        }
    };
    let mut copied = read_back(
        headless,
        pool,
        &[
            Readable::Buffer {
                buffer: generator.runs(frame),
                state: ResourceState::ShaderRead,
                bytes: generator.runs_size(),
            },
            Readable::Buffer {
                buffer: generator.args(frame),
                state: ResourceState::IndirectArgument,
                bytes: u64::from(regions * buckets) * DRAW_ARGS_WORDS as u64 * 4,
            },
            Readable::Buffer {
                buffer: generator.counts(frame),
                state: ResourceState::IndirectArgument,
                bytes: u64::from(flat_args_word(buckets, regions, regions, 0, 0)) * 4,
            },
        ],
    )
    .into_iter();
    let runs = copied.next().expect("the runs").words();
    let args = copied.next().expect("the arguments").words();
    let counts = copied.next().expect("the counts and extents").words();
    for region in closed {
        let chunks: Vec<u32> = (0..buckets)
            .map(|bucket| {
                let instances = args[((region * buckets + bucket) as usize) * DRAW_ARGS_WORDS + 1];
                if region == LATE_REGION && generator.mode() == DrawMode::Occlusion {
                    checked.late_instances += instances;
                }
                task_chunks(clusters[bucket as usize], instances, TASK_LANES)
            })
            .collect();
        let starts: Vec<u32> = (0..=buckets)
            .map(|bucket| runs[chunk_start_word(capacity, buckets, region, bucket) as usize])
            .collect();
        let relative: Vec<u32> = starts.iter().map(|start| start - starts[0]).collect();
        assert_eq!(
            relative,
            chunk_starts(0, &chunks),
            "{what}, region {region}: the chunk starts are not the running sum of {chunks:?}"
        );
        for (segment, (first, end)) in (0u32..).zip(segments) {
            let at = flat_args_word(buckets, regions, region, segment, 0) as usize;
            let written = [counts[at], counts[at + 1], counts[at + 2]];
            let total: u32 = chunks[*first as usize..*end as usize].iter().sum();
            assert_eq!(
                written,
                task_dispatch(total),
                "{what}, region {region}, segment {segment} over buckets {first}..{end}"
            );
            if chunks[*first as usize..*end as usize]
                .iter()
                .filter(|count| **count > 0)
                .count()
                > 1
            {
                checked.shared_segments += 1;
            }
        }
        checked.long_buckets +=
            u32::try_from(chunks.iter().filter(|count| **count > 1).count()).expect("small");
        checked.regions += 1;
    }
}

/// **Every region a frame closes holds its buckets' task chunks as a running
/// sum, and every flat segment's dispatch is its buckets' chunks** — for the
/// camera's three occlusion regions, a cascade's region 0 and the point
/// light's six faces, on every frame of the slide, on a table of nine buckets
/// and on one of 450.
///
/// Read back from the words `draw_gen.slang` wrote and held to
/// `crcbl_shaders::meshlet::chunk_starts` and `task_dispatch` over the
/// instance counts it wrote beside them. Past 256 buckets every lane of
/// `startsMain`'s and `lateFinishMain`'s scans owns more than one, so a scan
/// that summed a lane's own buckets wrong, or wrote one bucket's start at
/// another's word, shows here as well as the flat call's segment sums.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_flat_work_list_is_every_closed_region_s_chunks() {
    let (headless, has) = open(true);
    if !has {
        eprintln!("{SUITE}: no task stage on this device, so there is no flat work list");
        headless.finish();
        return;
    }
    let device = headless.device.as_ref();
    let mut pool = TransientPool::new();
    for laps in [1, 50] {
        let mut renderer = renderer(
            &headless,
            laps,
            OcclusionCulling {
                occlusion: true,
                small_feature_pixels: None,
            },
        );
        let buckets = renderer.draws().bucket_count();
        // The modes the scene's materials hold, in the table's mode-major
        // order: the demo's other rows are opaque, so that mode's buckets are
        // there and draw nothing.
        let materials = scene(laps).materials;
        let held: Vec<u32> = [
            0,
            GpuMaterial::ALPHA_MODE_MASK,
            GpuMaterial::DOUBLE_SIDED,
            GpuMaterial::MODE_MASK,
        ]
        .into_iter()
        .filter(|mode| materials.iter().any(|material| material.mode() == *mode))
        .collect();
        let meshes = laps * SHAPES.len();
        assert_eq!(
            buckets as usize,
            held.len() * meshes,
            "a bucket per mesh per mode the scene holds"
        );
        let clusters: Vec<u32> = (0..buckets as usize)
            .map(|bucket| {
                renderer
                    .cluster_range(bucket % meshes)
                    .expect("every mesh is resident")
                    .count
            })
            .collect();
        let modes: Vec<u32> = (0..buckets as usize)
            .map(|bucket| held[bucket / meshes])
            .collect();
        let table = Table {
            clusters,
            segments: flat_segments(&modes),
        };
        let mut checked = Checked::default();
        for step in 0..FRAMES {
            renderer.set_lights(&[lamp(step)]);
            render_mesh(&headless, &mut renderer, &mut pool, &camera(step), None);
            let frame = renderer.frame();
            check_generator(
                &headless,
                &mut pool,
                (renderer.draws(), frame),
                &table,
                &format!("{buckets} buckets, frame {step}, camera"),
                &mut checked,
            );
            for (cull, what) in [
                (0, "cascade 0"),
                (crcbl::render::shadow::CASCADES, "light slot 0"),
            ] {
                check_generator(
                    &headless,
                    &mut pool,
                    (renderer.shadow_generator(cull), frame),
                    &table,
                    &format!("{buckets} buckets, frame {step}, {what}"),
                    &mut checked,
                );
            }
        }
        eprintln!(
            "{SUITE}: {buckets} buckets: {} regions held to the host twins, {} buckets past one \
             chunk, {} segments of several buckets, {} late instances",
            checked.regions, checked.long_buckets, checked.shared_segments, checked.late_instances
        );
        assert!(
            checked.late_instances > 0,
            "{buckets} buckets: the slide rescued nothing, so the late region's scan held zeroes"
        );
        assert!(
            checked.faces > 0,
            "{buckets} buckets: the point light never culled by face, so no face region was read"
        );
        assert!(
            checked.shared_segments > 0,
            "{buckets} buckets: no segment had two buckets to search between"
        );
        if laps == 1 {
            assert!(
                checked.long_buckets > 0,
                "no bucket was more than one chunk long, so no boundary inside one was counted"
            );
        }
        renderer.destroy(device);
    }
    pool.destroy(device);
    headless.finish();
}
