//! Topic 03 §3.6's culling-stats readback, against a real driver: the number on
//! the panel is the cull's own answer, and it is late by exactly the ring.
//!
//! Every other reader of `DrawGen::visible_count` in this suite copies it back
//! by hand, outside the frame loop, with its own barriers — see
//! [`crate::mesh::read_stats_word`]. This one reads what
//! [`ForwardRenderer::counters`](crcbl_render::ForwardRenderer::counters)
//! reports **from inside the loop**, off `crcbl_render::cull_stats`'s ring, with
//! no fence, no `wait_idle` and no poll loop anywhere in the frame.
//!
//! # The observable
//!
//! A scene whose survivor count is *known* and *different* from the submitted
//! count: the cube at the origin, and two pyramids parked far behind the camera.
//! A readback that returned a number would pass a test asserting only that a
//! number arrived; what makes this evidence is that the number is 1 while three
//! instances were submitted, and that the hand copy of the very same buffer
//! agrees with it.
//!
//! The device is opened asking for the mesh and amplification stages, so a
//! machine that has them takes the geometry path where the cluster word is a
//! count and a machine that has not takes an indirect tail where it is unknown.
//! Both arms are asserted below; between radv and lavapipe locally, and wgpu in
//! `crcbl`'s `render_e2e`, both are actually reached.

use crate::harness::Headless;
use crate::mesh::{mesh_camera, read_stats_word, render_mesh};

/// How many frames behind [`ForwardRenderer::counters`]' culling half is.
///
/// [`crcbl_render::forward::FRAMES_IN_FLIGHT`] slots plus the one that makes a
/// reused slot's submission certainly complete — the ring's own length, taken
/// from the renderer's constant rather than written down, so a change to either
/// moves this with it.
///
/// [`ForwardRenderer::counters`]: crcbl_render::ForwardRenderer::counters
const RING_LATENCY: u64 = crcbl_render::forward::FRAMES_IN_FLIGHT as u64 + 1;

/// The tonemap's full-screen triangle: one instance, submitted and drawn, and
/// the one draw in a forward frame the CPU knows the count of. It is on both
/// sides of the submitted/drawn pair, which is what makes them comparable.
const TONEMAP_INSTANCE: u64 = 1;

/// **The culling counters come back off the GPU, and they are the cull's own
/// answer.**
///
/// Three instances go into the pool and two of them are parked 500 units behind
/// the camera, so the frustum test has something to reject and the answer is
/// known before the frame runs. Then:
///
/// * nothing is reported while the ring has not come round — a number here would
///   be one invented before any readback landed;
/// * the number that does arrive is the survivor count, not the submitted count,
///   and not the pool's size;
/// * it agrees with a hand copy of the same buffer, which is what makes it *the
///   cull's* answer rather than a plausible number from somewhere;
/// * and it is stamped with the frame it came from, which is the oldest frame in
///   the ring rather than the one just recorded.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_culling_counters_come_back_off_the_gpu_and_are_the_culls_own_answer() {
    let headless = Headless::open_for_mesh_with(
        crcbl_hal::Features::GPU_DRIVEN
            | crcbl_hal::Features::MESH_SHADER
            | crcbl_hal::Features::TASK_SHADER,
    );
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    let camera = mesh_camera(crcbl_render::Projection::default());

    // The cube is always in the pool. These two are put where the camera cannot
    // see them: 500 units along +Z, which is behind an eye at z = 2.2 looking at
    // the origin. Two rather than one so a survivor count that reported the
    // *culled* half instead would read 2 and not 1.
    let far_behind = |offset: f32| {
        Some(glam::Mat4::from_translation(glam::Vec3::new(
            offset, 0.0, 500.0,
        )))
    };
    renderer.set_pyramid(far_behind(0.0));
    renderer.set_tinted_pyramid(far_behind(2.0));

    // Frame 1, and every frame up to the ring's length: the copy is in the
    // graph, the readback is in flight, and there is nothing to report.
    for frame in 1..=RING_LATENCY {
        render_mesh(&headless, &mut renderer, &mut pool, &camera);
        let counters = renderer.counters();
        assert_eq!(
            counters.drawn, None,
            "frame {frame} is inside the ring's latency, so the row must say `indirect`",
        );
        assert_eq!(counters.cull_frame, None);
    }

    // And the frame the ring comes round on.
    render_mesh(&headless, &mut renderer, &mut pool, &camera);
    let counters = renderer.counters();

    let submitted = counters.instances;
    assert_eq!(
        submitted,
        3 + TONEMAP_INSTANCE,
        "the cube, two pyramids and the tonemap's triangle",
    );
    assert_eq!(
        counters.drawn,
        Some(1 + TONEMAP_INSTANCE),
        "one instance survives the frustum test, and the tonemap's own triangle is drawn \
         whatever the cull decided",
    );
    assert!(
        counters.drawn < Some(submitted),
        "a culling win that is not a win is a counter reporting the pool's size: {counters:?}",
    );

    // The same buffer, copied by hand outside the frame loop: if the ring's
    // number and this disagree, the ring is reading the wrong word, the wrong
    // slot or the wrong buffer.
    let by_hand = read_stats_word(
        &headless,
        &renderer,
        crcbl_shaders::cull::INSTANCE_SURVIVOR_WORD,
    );
    assert_eq!(
        u64::from(by_hand) + TONEMAP_INSTANCE,
        counters.drawn.expect("the ring came round"),
        "the ring's number must be the one in the counter buffer",
    );

    // Printed unconditionally, like the graph dump beside it: on a green run
    // this is the only place the numbers the panel would show are visible, and
    // which geometry path produced them decides whether the cluster word is a
    // count at all.
    eprintln!(
        "vk e2e: counters on {:?} (amplification stage: {}) — {counters:?}",
        renderer.geometry_path(),
        renderer.culls_clusters(),
    );

    // The stamp: the oldest frame in the ring, not the frame just recorded.
    let stats = renderer.cull_stats().expect("the ring came round");
    assert_eq!(
        stats.frame, 1,
        "the report is the first frame's, {RING_LATENCY} frames later",
    );
    assert_eq!(counters.cull_frame, Some(stats.frame));
    assert_eq!(u64::from(by_hand), stats.instances);

    // The indirect tails have no amplification stage, so the cluster word is
    // unknown rather than the zero the clearing pass left in it. A device with
    // mesh *and* task shaders has one, and then it is a count.
    if renderer.culls_clusters() {
        assert!(
            stats.clusters.is_some_and(|clusters| clusters > 0),
            "the amplification stage kept some clusters: {stats:?}",
        );
    } else {
        assert_eq!(
            stats.clusters, None,
            "nothing on this path counts a cluster, and zero would say every one was rejected",
        );
    }

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **The counters keep moving, and each frame's report is the next frame's.**
///
/// A ring that resolved once and then stopped — a slot never released, a request
/// never made again — would pass the test above and report the same frame
/// forever. So the scene changes underneath it: the second pyramid comes back
/// into view, and the survivor count has to follow it up, still late by the same
/// ring.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_ring_keeps_turning_and_the_count_follows_the_scene() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    let camera = mesh_camera(crcbl_render::Projection::default());
    renderer.set_pyramid(Some(glam::Mat4::from_translation(glam::Vec3::new(
        0.0, 0.0, 500.0,
    ))));

    // Long enough for the ring to come round on the culled scene.
    for _ in 0..=RING_LATENCY {
        render_mesh(&headless, &mut renderer, &mut pool, &camera);
    }
    let culled = renderer.cull_stats().expect("the ring came round");
    assert_eq!(
        culled.instances, 1,
        "only the cube is in front of the camera"
    );

    // Now bring it back where the camera can see it. The count must not change
    // on the very next frame — the ring is still carrying the old frames — and
    // must have changed once the ring has turned over.
    renderer.set_pyramid(Some(glam::Mat4::from_translation(glam::Vec3::new(
        0.9, 0.0, 0.0,
    ))));
    render_mesh(&headless, &mut renderer, &mut pool, &camera);
    let straight_after = renderer.cull_stats().expect("still reporting");
    assert_eq!(
        straight_after.instances, 1,
        "the next frame's report is still an old frame's: {straight_after:?}",
    );
    assert!(
        straight_after.frame > culled.frame,
        "and it is a different frame, so the ring did advance",
    );

    for _ in 0..=RING_LATENCY {
        render_mesh(&headless, &mut renderer, &mut pool, &camera);
    }
    let visible = renderer.cull_stats().expect("still reporting");
    assert_eq!(
        visible.instances, 2,
        "both instances are in front of the camera now: {visible:?}",
    );
    assert_eq!(
        renderer.counters().drawn,
        Some(2 + TONEMAP_INSTANCE),
        "and the row follows it",
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}
