//! `docs/plan/18-render-features.md`'s light list, on a real device.
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

use crcbl_hal::Features;
use crcbl_render::{Light, PointLight};
use crcbl_shaders::light::{CLUSTER_LIGHT_CAPACITY, CLUSTER_OVERFLOW_WORD};
use glam::Vec3;

use crate::harness::Headless;
use crate::mesh::{MESH_EXTENT, mesh_camera, read_stats_word, render_mesh};

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
fn camera() -> crcbl_render::Camera {
    mesh_camera(crcbl_render::Projection::default())
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
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_froxel_that_runs_out_of_budget_counts_what_it_refused() {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    let device = headless.device.as_ref();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("a forward renderer");
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
        "vk e2e: lights — {lights} lights over {} froxels refused {overflowed} assignment(s)",
        grid.froxels()
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
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_point_light_brightens_the_face_it_is_beside_and_not_the_frame() {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    let device = headless.device.as_ref();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("a forward renderer");
    let camera = camera();

    let dark = render_mesh(&headless, &mut renderer, &mut pool, &camera).image;

    renderer.set_lights(&[Light::Point(PointLight {
        position: LIGHT_AT,
        radius: LIGHT_REACH,
        // Bright enough to be unmistakable against the sun, which
        // `DirectionalLight::default` already puts above 1.0.
        color: Vec3::new(4.0, 1.0, 1.0),
    })]);
    let lit = render_mesh(&headless, &mut renderer, &mut pool, &camera).image;

    // The cube's centre, which faces the camera and is on the light's side.
    let (x, y) = (MESH_EXTENT.0 / 2, MESH_EXTENT.1 / 2);
    let before = dark.pixel(x, y).expect("inside the frame");
    let after = lit.pixel(x, y).expect("inside the frame");
    eprintln!("vk e2e: lights — cube centre {before:?} → {after:?}");
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
