//! Milestones 3, 4 and 5, against four checked-in references: the depth-tested
//! lit cube, the orthographic camera, a second mesh at a non-zero base vertex,
//! and a mesh of several clusters.
//!
//! Every test here opens the device with `Features::GPU_DRIVEN` and nothing
//! else, so it runs on whichever geometry path the adapter reports. That is the
//! difference between this file and the mesh-shader tests that stayed in
//! `vk_e2e/mesh.rs`: nothing below names a path, and each one is the same claim
//! on all four backends.

use crate::harness::Headless;
use crate::mesh_scene::{MESH_EXTENT, mesh_camera, place, place_cube, render_mesh};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::{ForwardRenderer, Projection, TransientPool};

/// Compares an image against a checked-in reference and **returns** the verdict
/// rather than asserting it.
///
/// [`crcbl_golden::Golden::new`]'s own tolerance, which is
/// [`Tolerance::RASTERISER`](crcbl_golden::Tolerance::RASTERISER) — the bound
/// `tests/render_e2e.rs` holds four backends to, sized against measured driver
/// disagreement rather than against what would make a backend pass. These
/// references were blessed on Vulkan; a backend that cannot meet them here is a
/// finding, not a reason to re-bless.
///
/// Deferred on purpose, on `tests/sprite_e2e/`'s terms: a test that panicked
/// here would leave the renderer, the pool and the device undestroyed, and the
/// resulting `Drop` warning and out-of-band device error would print on top of
/// the message that says what actually went wrong. Every caller tears down
/// first and unwraps last.
pub(crate) fn mesh_golden(name: &str, image: &crcbl_golden::Image) -> Result<String, String> {
    let reference =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.png"));
    crcbl_golden::Golden::new(reference)
        .check(image)
        .expect("the reference is readable")
        .into_result()
        .map(|comparison| format!("golden {name} — {}", comparison.summary()))
}

/// A renderer and a pool on `headless`, with the demo cube already in the frame
/// at the spin every golden here was blessed at.
fn cube_scene(headless: &Headless) -> (ForwardRenderer, TransientPool) {
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    place_cube(&mut renderer);
    (renderer, TransientPool::new())
}

/// Releases everything in dependency order, then asks the device what it saw.
fn teardown(headless: Headless, renderer: ForwardRenderer, mut pool: TransientPool) {
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}

/// Milestones 3 and 4: a depth-tested, lit, spinning cube drawn through the
/// render graph, against a checked-in reference.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_lit_mesh_through_the_graph_matches_its_golden_image() {
    let headless = Headless::open_for_mesh();
    let (mut renderer, mut pool) = cube_scene(&headless);
    let image = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        None,
    );

    // Something drew, and it is not the whole frame: the clear must still be
    // visible in the corners, or the "cube" is a full-screen quad.
    let corner = image.pixel(1, 1).expect("inside");
    assert!(
        corner[0] < 40 && corner[1] < 40 && corner[2] < 50,
        "the corner must still be the clear colour, got {corner:?}"
    );
    let centre = image.pixel(128, 96).expect("inside");
    assert!(
        u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 60,
        "the centre must be the cube, not the clear, got {centre:?}"
    );

    let verdict = mesh_golden("mesh", &image);
    teardown(headless, renderer, pool);
    eprintln!(
        "{}: {}",
        crate::SUITE,
        verdict.unwrap_or_else(|m| panic!("{m}"))
    );
}

/// Milestone 5: the orthographic camera is a **projection-matrix swap and
/// nothing else**.
///
/// The assertion is in two halves, and both matter. The golden proves the
/// orthographic frame is the one that was reviewed; comparing it against the
/// perspective frame proves the swap actually did something, so a
/// `Projection::Orthographic` that silently fell through to perspective could
/// not pass.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_orthographic_camera_is_a_projection_swap_and_matches_its_golden() {
    let headless = Headless::open_for_mesh();
    let (mut renderer, mut pool) = cube_scene(&headless);

    let ortho = Projection::Orthographic {
        half_height: 0.9,
        near: 0.1,
        far: 100.0,
    };
    // The *same* renderer, the same pipeline, the same geometry, the same
    // shader, the same graph. One field differs.
    let perspective = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        None,
    );
    let image = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(ortho),
        None,
    );

    let differing = (0..MESH_EXTENT.1)
        .flat_map(|y| (0..MESH_EXTENT.0).map(move |x| (x, y)))
        .filter(|(x, y)| image.pixel(*x, *y) != perspective.pixel(*x, *y))
        .count();
    assert!(
        differing > (MESH_EXTENT.0 * MESH_EXTENT.1) as usize / 100,
        "swapping the projection must change the picture; only {differing} pixels moved"
    );

    let verdict = mesh_golden("mesh_ortho", &image);
    teardown(headless, renderer, pool);
    eprintln!(
        "{}: {}",
        crate::SUITE,
        verdict.unwrap_or_else(|m| panic!("{m}"))
    );
}

/// Where the second-mesh golden puts the pyramid, in world units.
///
/// Left of the cube from this camera and clear of it: the two meshes have to be
/// separable by eye, because the failure this golden exists to catch is one mesh
/// drawing the *other's* vertices. Measured against [`two_mesh_camera`] rather
/// than guessed — at [`mesh_camera`]'s distance the cube hides the pyramid
/// entirely, which was the first thing this golden showed.
const PYRAMID_AT: Vec3 = Vec3::new(-2.0, 0.0, 0.0);

/// [`mesh_camera`] pulled back far enough for two meshes to fit side by side.
///
/// The same eye direction, the same target, the same projection — only the
/// distance differs, because a frame that has to hold the cube *and* a mesh
/// beside it needs about twice the width the cube alone does. Keeping the
/// direction is what makes this frame comparable to the cube golden's: three
/// cube faces are visible in both.
fn two_mesh_camera() -> crcbl::render::Camera {
    let mut camera = mesh_camera(Projection::default());
    camera.eye *= 1.7;
    camera
}

/// `docs/plan/03-gpu-driven-rendering.md` §3.1's pool, drawn with **two**
/// residents in it — which is the first frame in which a base vertex means
/// anything at all.
///
/// The pyramid is the pool's second mesh, so it is at a non-zero
/// `MeshRange::base_vertex`. The assertions below are what make it evidence
/// rather than a picture:
///
/// * The cube must still be where the cube golden has it, so this cannot pass by
///   drawing anything anywhere.
/// * The pyramid's pixels must carry a colour **no cube face has**. That is the
///   half that fails when the base vertex is lost: the draw then reads the
///   cube's first sixteen vertices, and the frame comes out with a piece of a
///   second cube in the pyramid's place — which is exactly what it did before
///   `mesh.slang` grew its `DrawConstants` block, on Vulkan and not on WebGPU.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_mesh_at_a_non_zero_base_vertex_draws_its_own_geometry() {
    let headless = Headless::open_for_mesh();
    let (mut renderer, mut pool) = cube_scene(&headless);
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_PYRAMID,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::from_translation(PYRAMID_AT),
    );

    let image = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &two_mesh_camera(),
        None,
    );

    // The cube is still there, in the middle where the camera points.
    let centre = image.pixel(128, 96).expect("inside");
    assert!(
        u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 60,
        "the centre must still be the cube, got {centre:?}"
    );

    // And somewhere in the left third there is a hue the cube does not own. The
    // check is on the *ratios* between channels rather than on an exact colour,
    // because the value that reaches the swapchain has been through Lambert, a
    // GGX lobe and a tonemap; what survives all three is which channel leads.
    // `PYRAMID_SIDE_COLORS`' visible sides are the `+Z` violet and the `+X`
    // teal, and no cube face is either.
    let violet = (0..MESH_EXTENT.1)
        .flat_map(|y| (0..MESH_EXTENT.0 / 3).map(move |x| (x, y)))
        .filter_map(|(x, y)| image.pixel(x, y))
        .filter(|pixel| {
            let (r, g, b) = (
                u32::from(pixel[0]),
                u32::from(pixel[1]),
                u32::from(pixel[2]),
            );
            b > 100 && r > g + 30 && b > g + 30
        })
        .count();
    assert!(
        violet > 200,
        "the pyramid's violet side is missing: only {violet} texel(s) of it. A frame \
         that lost the base vertex draws the cube's own vertices here instead, and no \
         cube face is violet"
    );

    let verdict = mesh_golden("mesh_second", &image);
    teardown(headless, renderer, pool);
    eprintln!(
        "{}: {}",
        crate::SUITE,
        verdict.unwrap_or_else(|m| panic!("{m}"))
    );
}

/// Where the multi-cluster golden puts the open box — [`PYRAMID_AT`]'s place,
/// because no frame here holds both and the reasoning about it is the same:
/// left of the cube at [`two_mesh_camera`]'s distance, and clear of it.
const OPEN_BOX_AT: Vec3 = PYRAMID_AT;

/// How many pixels of the frame's left third the given channel leads by a clear
/// margin.
///
/// Ratios between channels rather than an exact colour, for the reason
/// [`a_mesh_at_a_non_zero_base_vertex_draws_its_own_geometry`] gives: what
/// survives Lambert, the GGX lobe and the tonemap is which channel leads.
fn leading_channel_texels(image: &crcbl_golden::Image, channel: usize, margin: u32) -> usize {
    (0..MESH_EXTENT.1)
        .flat_map(|y| (0..MESH_EXTENT.0 / 3).map(move |x| (x, y)))
        .filter_map(|(x, y)| image.pixel(x, y))
        .filter(|pixel| {
            let value = u32::from(pixel[channel]);
            (0..3)
                .filter(|other| *other != channel)
                .all(|other| value > u32::from(pixel[other]) + margin)
        })
        .count()
}

/// **A mesh of several clusters draws one face per cluster**, on whichever path
/// this adapter selects.
///
/// `docs/plan/03-gpu-driven-rendering.md` §3.5's unit of work is a cluster, and
/// until this frame no rendered frame had more than one of them per mesh: the
/// cube is 24 vertices and the pyramid 16, against a bound of 64. So a geometry
/// path that ignored [`Meshlet::vertex_offset`] and [`Meshlet::triangle_offset`]
/// *within* a mesh drew a correct picture of both, and
/// `crcbl::shaders::meshlet::open_box_clusters` is the geometry that stops that
/// being true — five clusters, one per face, four of them at offsets that are
/// not zero.
///
/// [`Meshlet::vertex_offset`]: crcbl::shaders::meshlet::Meshlet::vertex_offset
/// [`Meshlet::triangle_offset`]: crcbl::shaders::meshlet::Meshlet::triangle_offset
///
/// # What each assertion rules out
///
/// * **Three hues, in the box's own third of the frame.** The visible faces are
///   the floor, the `-X` wall and the `-Z` wall — grey, red-led and blue-led,
///   and no two clusters of this mesh share a colour. A path that lost the
///   per-cluster offsets emits face zero five times over, which is a grey square
///   with no red and no blue anywhere in it: the same triangle count, the same
///   buffer sizes, a different picture. That failure is invisible on the cube.
/// * **The cube is still where the camera points**, so nothing here passes by
///   drawing the box over the whole frame.
/// * **The golden**, which is the picture that was reviewed.
///
/// # What stayed in `vk_e2e`
///
/// The other half of the original test compared this frame against the *same*
/// frame drawn through a second geometry path, and then against a third arm with
/// `Features::TASK_SHADER`'s per-cluster cull in front of it. Both arms name a
/// path — `MeshShader` reached by adding features to an adapter that reports
/// them — so both are claims about radv rather than about the seam, and both
/// stayed in `vk_e2e/mesh.rs`. They read this directory's `mesh_clusters.png`,
/// not a second copy of it.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_multi_cluster_mesh_draws_one_face_per_cluster_and_matches_its_golden() {
    let headless = Headless::open_for_mesh();
    let (mut renderer, mut pool) = cube_scene(&headless);
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_OPEN_BOX,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::from_translation(OPEN_BOX_AT),
    );
    let image = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &two_mesh_camera(),
        None,
    );

    let centre = image.pixel(128, 96).expect("inside");
    assert!(
        u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 60,
        "the centre must still be the cube, got {centre:?}"
    );

    let red = leading_channel_texels(&image, 0, 30);
    let blue = leading_channel_texels(&image, 2, 30);
    eprintln!(
        "{}: open box — {red} red-led and {blue} blue-led texel(s) in its third",
        crate::SUITE
    );
    assert!(
        red > 200 && blue > 200,
        "the open box's `-X` wall is red-led and its `-Z` wall blue-led, and only \
         {red} and {blue} texel(s) of them are here — a geometry path that drew cluster \
         zero five times leaves this third one flat grey"
    );

    let verdict = mesh_golden("mesh_clusters", &image);
    teardown(headless, renderer, pool);
    eprintln!(
        "{}: {}",
        crate::SUITE,
        verdict.unwrap_or_else(|m| panic!("{m}"))
    );
}
