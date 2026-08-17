//! The one draw-generation claim that is **this adapter's** rather than the
//! seam's: every `GeometryPath` draws the same frame.
//!
//! The rest of this module moved to `crates/crcbl/tests/draw_gen_e2e/`, which
//! names no backend type and therefore runs on Vulkan, Metal, D3D12 and wgpu
//! from one binary — see that suite's header. The buffer-level checks it took
//! with it (the generated arguments against the CPU oracle, a culled bucket's
//! zero draw count, the poisoned counters the clearing pass has to undo, and a
//! bucket filling and emptying across the ring) are exactly the ones that had
//! only ever run on Vulkan.
//!
//! **This test could not go with them**, and the reason is what makes it worth
//! keeping here. Its three arms each assert a *named* [`GeometryPath`]:
//! `MeshShader`, `IndirectCount` and `IndirectPerBatch`, reached by subtracting
//! features from an adapter that reports all of them. That is a claim about
//! radv and lavapipe, not about the seam — Metal's API has no GPU-side draw
//! count, so its `IndirectCount` arm cannot exist, and a backend with no mesh
//! stage cannot reach `MeshShader` at all. The cross-backend form of the claim
//! is `crates/crcbl/tests/render_e2e.rs`'s
//! `the_*_scene_draws_the_same_frame_on_every_geometry_path`, which derives the
//! two paths it compares from what the adapter reports instead of writing them
//! down.
//!
//! `both_geometry_paths_draw_the_same_frame` opens its device *without*
//! `Features::DRAW_INDIRECT_COUNT` on purpose. `IndirectPerBatch` is the arm
//! Metal is on and no adapter this suite can see would ever select it, so
//! without that subtraction the arm would be code no machine here runs.
//!
//! [`GeometryPath`]: crcbl_hal::GeometryPath

use crate::harness::Headless;
use crate::mesh::mesh_camera;
use crcbl_hal::{Features, GeometryPath};
use crcbl_render::{ForwardRenderer, InstanceHandle, Projection, TransientPool};
use glam::{Mat4, Vec3};

/// Where the pyramid sits when the frame is meant to contain it. Beside the
/// cube and inside the frustum, like `crcbl screenshot --scene cube`.
const PYRAMID_AT: Vec3 = Vec3::new(-1.05, 0.0, 0.0);

/// Puts the demo pyramid in the frame at `at`, **after the cube**, so it is
/// element 1 of the instance array.
fn place_pyramid(renderer: &mut ForwardRenderer, at: Vec3) -> InstanceHandle {
    crate::mesh::place(
        renderer,
        crcbl_render::scene::DEMO_PYRAMID,
        crcbl_render::scene::DEMO_UNTINTED,
        Mat4::from_translation(at),
    )
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

/// **Every `GeometryPath` draws the same frame, byte for byte.**
///
/// `docs/plan/03-gpu-driven-rendering.md`'s design rule is that "the lesser path
/// is a constraint on data layout, not a separate renderer", and the exit
/// criterion is that every path renders the sandbox scene. This is that
/// criterion, on real hardware, in one process: the same renderer, the same
/// scene, the same camera, and the only difference is what the forward pass
/// records — a mesh dispatch, an indirect-count call, or a per-batch indirect
/// call.
///
/// **Two of the three arms have to be asked for by subtraction**, because this
/// adapter reports everything. `IndirectPerBatch` is what Metal is on — its API
/// has multi-draw-indirect and no GPU-side count — so its device is opened
/// without [`Features::DRAW_INDIRECT_COUNT`]; `IndirectCount` is what a device
/// with no mesh shaders selects, so its device is opened without
/// [`Features::MESH_SHADER`]. Without those subtractions two of the three arms
/// would be code no machine here runs.
///
/// Byte equality rather than a tolerance: this is one driver, one adapter and
/// one scene, so anything but identical output is a difference the three calls
/// made. The mesh arm is the interesting one — it emits the same triangles from
/// a mesh stage reading cluster records, where the other two read the index
/// pool through a vertex stage — and it is exactly what §3.5 means by the
/// fallback not being second-class.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn every_geometry_path_draws_the_same_frame() {
    let mut frames = Vec::new();
    for (optional, expected) in [
        (
            Features::GPU_DRIVEN | Features::MESH_SHADER,
            GeometryPath::MeshShader,
        ),
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
            "the device must be on the path this arm is about, or two halves of \
             this comparison are the same code twice"
        );
        let mut pool = TransientPool::new();
        let mut renderer =
            ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
                .expect("the forward renderer builds");
        crate::mesh::place_cube(&mut renderer);
        assert_eq!(
            renderer.geometry_path(),
            expected,
            "and the renderer must have *built* that path: a mesh-shader device that \
             degraded to an indirect tail would draw an identical frame and make this \
             comparison vacuous"
        );
        place_pyramid(&mut renderer, PYRAMID_AT);
        let frame = crate::mesh::render_mesh(
            &headless,
            &mut renderer,
            &mut pool,
            &mesh_camera(Projection::default()),
        );
        frames.push((expected, frame.image));
        teardown(headless, renderer, pool);
    }

    // Anti-vacuity: two blank frames match perfectly, so the frame has to have
    // something in it before the comparison means anything. The count is small
    // because every face of both meshes is flat-shaded — measured here, a
    // cleared frame is 1 and this scene's committed golden (the cube alone) is
    // 4, so anything at or below that is a frame the pyramid did not reach.
    let (first_path, first) = &frames[0];
    let colours = first.distinct_colors(64);
    assert!(
        colours > 4,
        "the frame is missing geometry ({colours} distinct colours), so comparing \
         it against another frame like it proves nothing"
    );
    for (path, image) in &frames[1..] {
        assert_eq!(
            first.pixels(),
            image.pixels(),
            "{first_path:?} and {path:?} must draw the same pixels"
        );
    }
}
