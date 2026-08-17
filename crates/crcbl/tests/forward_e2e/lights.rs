//! `docs/plan/18-render-features.md`'s light list, on whichever backend
//! `CRCBL_GPU` names.
//!
//! Two things a golden image cannot say, and this module exists for both:
//!
//! * **A froxel's budget refusing lights is counted.** A grid that silently
//!   dropped every light past the sixteenth would draw a plausible frame — a
//!   little dark in the busy places — and match any golden blessed from it. The
//!   overflow counter is what tells the two apart, and a counter is only a check
//!   once something has read a non-zero out of it *and* a zero out of it.
//! * **A point light actually lights something.** A light that exists, uploads,
//!   clusters and contributes nothing is the failure mode of every step in this
//!   slice at once, and only pixels can see it.

use crcbl::hal::{
    Barriers, BufferDesc, BufferUsage, CommandEncoderDesc, Features, MemoryLocation, ResourceState,
    SubmitInfo,
};
use crcbl::math::Vec3;
use crcbl::render::{Light, PointLight, SpotLight};
use crcbl::shaders::light::{CLUSTER_LIGHT_CAPACITY, CLUSTER_OVERFLOW_WORD, CLUSTER_STRIDE};

use crate::harness::{Headless, poisoned};
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place_cube, read_stats_word, render_mesh};

/// Where the cube is, and where a light has to be to reach it.
///
/// `mesh_camera` looks at the origin from `+Z`, and the cube is a unit cube
/// there — so a light off to one side at this distance lights the faces the
/// camera can see without being inside the geometry.
const LIGHT_AT: Vec3 = Vec3::new(1.6, 0.9, 2.0);

/// A radius comfortably larger than the distance above, so the falloff at the
/// cube is well clear of the window's zero.
const LIGHT_REACH: f32 = 8.0;

/// A radius no scene is larger than, for the lights whose only job is to be in
/// every froxel.
const EVERYWHERE: f32 = 1.0e4;

/// How many lights the overflow test puts in the frame beside the sun.
///
/// Chosen so the total is over [`CLUSTER_LIGHT_CAPACITY`] by a margin that is
/// not one: an off-by-one in the append loop would still overflow, and would
/// overflow by a different amount.
const CROWD: u32 = 20;

/// A camera whose frame is the one `MESH_EXTENT` sizes.
fn camera() -> crcbl::render::Camera {
    mesh_camera(crcbl::render::Projection::default())
}

/// A light that covers the whole frustum, so every froxel lists it.
fn everywhere(index: u32) -> Light {
    Light::Point(PointLight {
        // Spread along `+X` so no two rows are identical bytes — a clustering
        // pass that read one row and counted it many times would otherwise be
        // indistinguishable from one that read them all.
        position: Vec3::new(index as f32 * 0.01, 0.0, 0.0),
        radius: EVERYWHERE,
        // Dim, because twenty of them are in the frame and this test is about
        // the counter rather than about the picture.
        color: Vec3::splat(0.01),
    })
}

/// **The budget refusing lights is a number, and this is that number.**
///
/// The count is exact rather than "greater than zero", and it has to be: the
/// pass assigns froxel by froxel, in light-list order, keeping a prefix — so for
/// a scene where every light reaches every froxel the total is
/// `froxels × (lights − capacity)` and nothing else. A pass that dropped a light
/// without counting it, counted a froxel rather than an assignment, or ran its
/// loop once too often lands on a different number, and each of those is a
/// separate wrong answer this arithmetic distinguishes.
///
/// The zero case is asserted first and in the same frame shape, because a
/// counter wired to a constant passes the interesting half on its own.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_froxel_that_runs_out_of_budget_counts_what_it_refused() {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    let device = headless.device.as_ref();
    let mut pool = crcbl::render::TransientPool::new();
    let mut renderer = crcbl::render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("a forward renderer");
    place_cube(&mut renderer);
    let camera = camera();

    // A frame no froxel could overflow: the sun and two lights, well inside the
    // budget. If this is not zero, the number below says nothing about overflow.
    renderer.set_lights(&[everywhere(0), everywhere(1)]);
    let _ = render_mesh(&headless, &mut renderer, &mut pool, &camera);
    let quiet = read_stats_word(&headless, &renderer, CLUSTER_OVERFLOW_WORD);
    assert_eq!(
        quiet, 0,
        "three lights fit in a budget of {CLUSTER_LIGHT_CAPACITY}, so nothing was refused"
    );

    let crowd: Vec<Light> = (0..CROWD).map(everywhere).collect();
    renderer.set_lights(&crowd);
    let _ = render_mesh(&headless, &mut renderer, &mut pool, &camera);
    let grid = renderer.grid();
    let overflowed = read_stats_word(&headless, &renderer, CLUSTER_OVERFLOW_WORD);

    // The grid this extent produces, pinned: the total below is a product with
    // it, so a grid that quietly changed shape would move the total and the
    // formula would still "agree" with itself.
    assert_eq!(
        (grid.x, grid.y, grid.slices),
        (4, 3, 24),
        "a {MESH_EXTENT:?} perspective frame is four tiles by three by every slice"
    );
    assert_eq!(grid.froxels(), 288);

    // The sun is row 0 of every frame's list, so the frame holds one more light
    // than was set.
    let lights = CROWD + 1;
    let refused_per_froxel = lights - CLUSTER_LIGHT_CAPACITY;
    assert_eq!(refused_per_froxel, 5);
    assert_eq!(
        overflowed,
        grid.froxels() * refused_per_froxel,
        "{lights} lights reaching every one of {} froxels, each keeping {CLUSTER_LIGHT_CAPACITY}",
        grid.froxels()
    );
    assert_eq!(overflowed, 1440, "and that product is this number");

    eprintln!(
        "crcbl forward e2e: lights — {lights} lights over {} froxels refused {overflowed} assignment(s)",
        grid.froxels()
    );

    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}

/// The half-angles of [`the_cone_bound_lists_a_spot_in_fewer_froxels_than_its_sphere`]'s
/// spot, in radians.
///
/// Narrow, because that is the case the cone bound is *for*: a spot whose cone
/// opens as wide as its sphere reaches is one the two bounds agree about, and
/// the comparison below would be a tautology. At [`LIGHT_AT`]'s distance from
/// the cube this outer angle still covers it, which is what makes the picture
/// half of the claim available at all.
///
/// [`the_cone_bound_lists_a_spot_in_fewer_froxels_than_its_sphere`]: fn@the_cone_bound_lists_a_spot_in_fewer_froxels_than_its_sphere
const SPOT_INNER: f32 = 0.10;
const SPOT_OUTER: f32 = 0.22;

/// How many light-to-froxel assignments the clustering pass made in the frame
/// just drawn.
///
/// The sum of every froxel's count word, out of the grid the pass wrote — the
/// **whole** grid rather than a sample of it, because the number this produces
/// is a total and a total over some froxels is a different total.
///
/// The copy is its own submission after the frame's, on `read_stats_word`'s
/// terms exactly: the graph leaves the grid in [`ResourceState::ShaderRead`],
/// which is where the next frame on that slot expects it, so this moves it out
/// and puts it straight back.
fn assignments(headless: &Headless, renderer: &crcbl::render::ForwardRenderer) -> u32 {
    let device = headless.device.as_ref();
    let grid = renderer.light_grid_buffer(renderer.frame());
    let words = renderer.grid().froxels() * CLUSTER_STRIDE;
    let size = u64::from(words) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("froxel grid readback"),
            size,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("froxel grid copy"),
        queue: headless.queue,
    });
    let barrier = |from: ResourceState, to: ResourceState| {
        [crcbl::hal::BufferBarrier {
            buffer: grid,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::ShaderRead, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::ShaderRead);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
        src: grid,
        src_offset: 0,
        dst: staging,
        dst_offset: 0,
        size,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let mut bytes = poisoned(size as usize);
    headless.readback(staging, size, &mut bytes);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    // The count is the first word of each froxel's record; the rest are the
    // indices it kept.
    bytes
        .chunks_exact(CLUSTER_STRIDE as usize * 4)
        .map(|froxel| u32::from_le_bytes(froxel[..4].try_into().expect("four bytes")))
        .sum()
}

/// **A narrow spot is listed in fewer froxels than its sphere covers, and still
/// lights what it points at.**
///
/// `light_cluster.slang` bounds a spot by its cone as well as by its radius, and
/// the two halves of that are separate claims that need separate evidence:
///
/// * The cone is **tighter**, which is the point of having it — and it is
///   measured against a point light at the same place with the same radius, in
///   the same frame shape, so the difference between the two totals is the cone
///   test and nothing else. A cone test that quietly accepted everything would
///   land on the same number as the sphere and fail here.
/// * The cone is still **conservative**, which is what a wrong one costs. A
///   bound that is too tight leaves froxels that should list the light without
///   it, and that is a hard seam across a lit surface — so the frame has to
///   still light the cube the spot is aimed at. The `spot` golden in `crcbl`'s
///   render suite is where that claim is made across a whole surface; this is
///   the half of it that lives beside the number.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_cone_bound_lists_a_spot_in_fewer_froxels_than_its_sphere() {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    let device = headless.device.as_ref();
    let mut pool = crcbl::render::TransientPool::new();
    let mut renderer = crcbl::render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("a forward renderer");
    place_cube(&mut renderer);
    let camera = camera();

    let dark = render_mesh(&headless, &mut renderer, &mut pool, &camera);
    let sun_only = assignments(&headless, &renderer);

    renderer.set_lights(&[Light::Point(PointLight {
        position: LIGHT_AT,
        radius: LIGHT_REACH,
        color: Vec3::new(4.0, 1.0, 1.0),
    })]);
    let _ = render_mesh(&headless, &mut renderer, &mut pool, &camera);
    let with_sphere = assignments(&headless, &renderer);

    renderer.set_lights(&[Light::Spot(SpotLight {
        position: LIGHT_AT,
        radius: LIGHT_REACH,
        color: Vec3::new(4.0, 1.0, 1.0),
        // At the cube, which is at the origin — see `mesh_camera`.
        direction: -LIGHT_AT,
        inner_angle: SPOT_INNER,
        outer_angle: SPOT_OUTER,
    })]);
    let lit = render_mesh(&headless, &mut renderer, &mut pool, &camera);
    let with_cone = assignments(&headless, &renderer);

    let froxels = renderer.grid().froxels();
    eprintln!(
        "crcbl forward e2e: lights — {froxels} froxels list {sun_only} assignment(s) for the sun alone, \
         {with_sphere} with a point light beside it and {with_cone} with the same light as a \
         narrow spot"
    );

    // The sun reaches every froxel by construction, so this is the baseline both
    // totals below sit on — and it is asserted rather than assumed, because a
    // grid that was never written reads as zeroes and every difference below
    // would then be a difference between two nothings.
    assert_eq!(
        sun_only, froxels,
        "the sun is in every froxel, so a frame with only the sun in its list has one \
         assignment per froxel"
    );
    // Everything above that baseline is the second light's, so these two are the
    // froxel counts the two bounds produce for one light at one place.
    let by_sphere = with_sphere - sun_only;
    let by_cone = with_cone - sun_only;
    assert!(
        by_sphere > 0,
        "the point light reached no froxel at all, so there is nothing for the cone bound \
         to be tighter than"
    );
    assert!(
        by_cone > 0,
        "the cone bound listed the spot in no froxel, which is not a tighter bound but a \
         light switched off — and it is what a cone test with its sign the wrong way round \
         produces"
    );
    // A fraction rather than a pinned count: the exact number is geometry, and a
    // scene or a grid that legitimately moved would fail a pinned one with
    // nothing to say about why. Measured at 91 froxels against the sphere's 144
    // on radv and on lavapipe, so a bound that rejected nothing — the same 144 —
    // fails this by a wide margin.
    assert!(
        by_cone * 4 <= by_sphere * 3,
        "the cone bound listed the spot in {by_cone} of the {by_sphere} froxels its sphere \
         reaches, which is not a tighter bound"
    );

    // And the picture: the cube's centre is what the spot is aimed at, so a
    // bound that culled the froxels the cube is in leaves it as dark as the
    // frame with no light in the list at all.
    let (x, y) = (MESH_EXTENT.0 / 2, MESH_EXTENT.1 / 2);
    let before = dark.pixel(x, y).expect("inside the frame");
    let after = lit.pixel(x, y).expect("inside the frame");
    eprintln!("crcbl forward e2e: lights — cube centre under the spot {before:?} → {after:?}");
    assert!(
        after[0] > before[0],
        "the spot is aimed at the cube and must brighten it: {before:?} → {after:?}"
    );

    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}

/// **A point light lights something**, asserted in pixels rather than by the
/// light existing.
///
/// The sun is fixed and the camera is fixed, so the only difference between the
/// two frames below is one row of the light list — and the face the light is on
/// the side of has to get brighter. Both halves are needed: a shader that
/// ignored the list entirely leaves the frame unchanged, and one that added the
/// light everywhere regardless of the froxel leaves the *far* side changed too,
/// which the second assertion is what refuses.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_point_light_brightens_the_face_it_is_beside_and_not_the_frame() {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    let device = headless.device.as_ref();
    let mut pool = crcbl::render::TransientPool::new();
    let mut renderer = crcbl::render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("a forward renderer");
    place_cube(&mut renderer);
    let camera = camera();

    let dark = render_mesh(&headless, &mut renderer, &mut pool, &camera);

    renderer.set_lights(&[Light::Point(PointLight {
        position: LIGHT_AT,
        radius: LIGHT_REACH,
        // Bright enough to be unmistakable against the sun, which
        // `DirectionalLight::default` already puts above 1.0.
        color: Vec3::new(4.0, 1.0, 1.0),
    })]);
    let lit = render_mesh(&headless, &mut renderer, &mut pool, &camera);

    // The cube's centre, which faces the camera and is on the light's side.
    let (x, y) = (MESH_EXTENT.0 / 2, MESH_EXTENT.1 / 2);
    let before = dark.pixel(x, y).expect("inside the frame");
    let after = lit.pixel(x, y).expect("inside the frame");
    eprintln!("crcbl forward e2e: lights — cube centre {before:?} → {after:?}");
    assert!(
        after[0] > before[0],
        "a red point light beside the cube must brighten its red channel: \
         {before:?} → {after:?}"
    );

    // The clear colour in the corner, which no geometry covers. A light that
    // reached this changed something that is not a surface.
    let corner_before = dark.pixel(1, 1).expect("inside the frame");
    let corner_after = lit.pixel(1, 1).expect("inside the frame");
    assert_eq!(
        corner_before, corner_after,
        "the background is not a lit surface, so no light may touch it"
    );

    // The sun, unchanged: a light list that lost its first row while gaining a
    // second would brighten the cube too, and would be a regression rather than
    // this feature.
    assert!(
        before.iter().take(3).any(|channel| *channel > 0),
        "the sun must still be lighting the cube in the first frame, or the \
         comparison above is between two unlit pictures"
    );

    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}
