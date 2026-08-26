//! Topic 03 §3.6's culling-stats readback, against a real driver: the number on
//! the panel is the cull's own answer, a few frames after the frame it is about.
//!
//! Every other reader of `DrawGen::visible_count` copies it back by hand,
//! outside the frame loop, with its own barriers — see
//! [`crate::cull_readback::read_stats_word`]. This one reads what
//! [`ForwardRenderer::counters`](crcbl::render::ForwardRenderer::counters)
//! reports **from inside the loop**, off `crcbl::render::cull_stats`'s ring,
//! with no fence, no `wait_idle` and no poll loop anywhere in the frame.
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
//! Both arms are asserted below, and which one a run reaches is a property of
//! the backend and adapter `CRCBL_GPU` and `CRCBL_ADAPTER` named — which is why
//! the run prints the path it took rather than assuming one.

use crate::cull_readback::read_stats_word;
use crate::harness::Headless;
use crate::mesh_scene::{mesh_camera, place, place_cube, render_mesh};
use crcbl::hal::Features;
use crcbl::math::{Mat4, Vec3};
use crcbl::render::{ForwardRenderer, InstanceDesc, InstanceHandle, Projection, TransientPool};

/// The frames [`ForwardRenderer::counters`]' culling half cannot have arrived
/// in.
///
/// Two: a frame records the copy, the next requests the readback for it — a
/// readback covers work already *submitted* — and the third is the first that
/// can poll anything. Which frame the answer actually lands on past that is the
/// device's, which is why this is a floor and [`REPORT_BOUND`] is the other end.
///
/// [`ForwardRenderer::counters`]: crcbl::render::ForwardRenderer::counters
const REPORT_FLOOR: u64 = 2;

/// Frames a report is given to arrive before this suite calls it a failure.
///
/// The number that would have caught the bug this file now guards: a ring that
/// polls a readback once and releases it whatever it answered reports nothing
/// **ever** on a backend that cannot answer a first poll, and a test with no
/// upper bound would have looped rather than gone red. Generous enough that no
/// working device reaches it — a poll answered across a browser's command stream
/// costs a frame's round trip, not a dozen.
const REPORT_BOUND: u64 = 32;

/// The full-screen triangles a forward frame draws: `ssao`'s, `ssao-blur`'s,
/// `ssr`'s, `ssr-blur`'s, the tonemap's and the antialiasing resolve's.
///
/// One instance each, submitted and drawn, and the only draws in a forward frame
/// the CPU knows the count of. They are on both sides of the submitted/drawn
/// pair, which is what makes the two comparable — see
/// `crcbl::render::ForwardRenderer::counters`.
const FULLSCREEN_INSTANCES: u64 = 6;

/// **The culling counters come back off the GPU, and they are the cull's own
/// answer.**
///
/// Three instances go into the pool and two of them are parked 500 units behind
/// the camera, so the frustum test has something to reject and the answer is
/// known before the frame runs. Then:
///
/// * nothing is reported before a readback has been polled at all — a number
///   there would be one invented before any readback landed — and one does
///   arrive, within a bounded number of frames rather than never;
/// * the number that does arrive is the survivor count, not the submitted count,
///   and not the pool's size;
/// * it agrees with a hand copy of the same buffer, which is what makes it *the
///   cull's* answer rather than a plausible number from somewhere;
/// * and it is stamped with the frame it came from rather than the one just
///   recorded.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn the_culling_counters_come_back_off_the_gpu_and_are_the_culls_own_answer() {
    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    place_cube(&mut renderer);
    let camera = mesh_camera(Projection::default());

    // The cube is in the pool, placed above. These two are put where the camera cannot
    // see them: 500 units along +Z, which is behind an eye at z = 2.2 looking at
    // the origin. Two rather than one so a survivor count that reported the
    // *culled* half instead would read 2 and not 1.
    let far_behind = |offset: f32| Mat4::from_translation(Vec3::new(offset, 0.0, 500.0));
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_PYRAMID,
        crcbl::render::scene::DEMO_UNTINTED,
        far_behind(0.0),
    );
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_PYRAMID,
        crcbl::render::scene::DEMO_TINTED,
        far_behind(2.0),
    );

    // Frame 1 and frame 2: the copy is in the graph and the request has only
    // just been made, so nothing can have been polled and the row must say
    // `indirect`.
    for frame in 1..=REPORT_FLOOR {
        render_mesh(&headless, &mut renderer, &mut pool, &camera, None);
        let counters = renderer.counters();
        assert_eq!(
            counters.drawn, None,
            "frame {frame} is before the first poll, so the row must say `indirect`",
        );
        assert_eq!(counters.cull_frame, None);
    }

    // And then it has to arrive, within a bounded number of frames rather than
    // on a frame this test names: how many polls a readback needs is the
    // device's business, and that a report arrives at all is the assertion.
    let mut rendered = REPORT_FLOOR;
    while renderer.cull_stats().is_none() {
        assert!(
            rendered < REPORT_FLOOR + REPORT_BOUND,
            "no culling report arrived in {REPORT_BOUND} frames; the readback is being polled \
             and thrown away rather than answered",
        );
        render_mesh(&headless, &mut renderer, &mut pool, &camera, None);
        rendered += 1;
    }
    let counters = renderer.counters();

    let submitted = counters.instances;
    assert_eq!(
        submitted,
        3 + FULLSCREEN_INSTANCES,
        "the cube, two pyramids and one triangle per full-screen pass",
    );
    assert_eq!(
        counters.drawn,
        Some(1 + FULLSCREEN_INSTANCES),
        "one instance survives the frustum test, and the full-screen triangles are drawn \
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
        crcbl::shaders::cull::INSTANCE_SURVIVOR_WORD,
    );
    assert_eq!(
        u64::from(by_hand) + FULLSCREEN_INSTANCES,
        counters.drawn.expect("the ring came round"),
        "the ring's number must be the one in the counter buffer",
    );

    // Printed unconditionally, like the graph dump beside it: on a green run
    // this is the only place the numbers the panel would show are visible, and
    // which geometry path produced them decides whether the cluster word is a
    // count at all.
    eprintln!(
        "crcbl draw gen e2e: counters on {:?} (amplification stage: {}) — {counters:?}",
        renderer.geometry_path(),
        renderer.culls_clusters(),
    );

    // The stamp: a frame whose copy really was recorded and answered, and not
    // the frame just rendered. A ring that read the slot it had only just
    // requested would stamp the current frame, and one that never stamped
    // anything would have had to say so with a `None` this test has already
    // passed.
    let stats = renderer.cull_stats().expect("a readback answered");
    assert!(
        stats.frame >= 1 && stats.frame < rendered,
        "the report names frame {} on a run that has drawn {rendered}",
        stats.frame,
    );
    assert_eq!(counters.cull_frame, Some(stats.frame));
    assert_eq!(u64::from(by_hand), stats.instances);

    // The indirect tails have no amplification stage, so the cluster word is
    // unknown rather than the zero the clearing pass left in it. A device with
    // mesh *and* task shaders has one, and then it is a count.
    if renderer.culls_clusters() {
        let clusters = stats
            .clusters
            .expect("a path with an amplification stage counts what it tested");
        assert!(
            clusters.survivors > 0,
            "the amplification stage kept some clusters: {stats:?}",
        );
        // **Each of the three came off its own word**, checked the same way the
        // instance count above is: the buffer copied by hand, outside the frame
        // loop. The scene and the camera do not move, so the words hold the same
        // numbers this frame that the ring read a few frames ago. Three counters
        // in one buffer is exactly the arrangement where an off-by-one offset
        // reports a neighbour's total and reads as entirely plausible — word 2
        // between them is the light grid's, which is a real number and not a
        // zero. Whether the three *add up to the cut* needs a cut to compare
        // against, and that is `apps/quarry`'s device suite.
        for (word, name, reported) in [
            (
                crcbl::shaders::cull::CLUSTER_SURVIVOR_WORD,
                "survivors",
                clusters.survivors,
            ),
            (
                crcbl::shaders::cull::CLUSTER_FRUSTUM_REJECT_WORD,
                "frustum rejections",
                clusters.frustum_rejects,
            ),
            (
                crcbl::shaders::cull::CLUSTER_CONE_REJECT_WORD,
                "cone rejections",
                clusters.cone_rejects,
            ),
        ] {
            assert_eq!(
                u64::from(read_stats_word(&headless, &renderer, word)),
                reported,
                "the ring reported {reported} {name}, and word {word} of the counter buffer \
                 says otherwise",
            );
        }
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
/// A ring that reported once and then stopped — a slot never released, a request
/// never made again — would pass the test above and report the same frame
/// forever. So the scene changes underneath it: the second pyramid comes back
/// into view, and the survivor count has to follow it up, a few frames later.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn the_ring_keeps_turning_and_the_count_follows_the_scene() {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    place_cube(&mut renderer);
    let camera = mesh_camera(Projection::default());
    // One slot for the pyramid, moved rather than re-inserted below: a second
    // insert would leave the culled copy live and the survivor count would never
    // come back down.
    let pyramid_at = |at: Vec3| InstanceDesc {
        mesh: crcbl::render::scene::DEMO_PYRAMID,
        material: crcbl::render::scene::DEMO_UNTINTED,
        transform: Mat4::from_translation(at),
    };
    let pyramid: InstanceHandle = renderer
        .add_instance(&pyramid_at(Vec3::new(0.0, 0.0, 500.0)))
        .expect("an instance pool of thousands has room for the pyramid");

    // Long enough for the first readback to answer on the culled scene.
    let mut rendered = 0;
    while renderer.cull_stats().is_none() {
        assert!(
            rendered < REPORT_FLOOR + REPORT_BOUND,
            "no culling report arrived in {REPORT_BOUND} frames",
        );
        render_mesh(&headless, &mut renderer, &mut pool, &camera, None);
        rendered += 1;
    }
    let culled = renderer.cull_stats().expect("a readback answered");
    assert_eq!(
        culled.instances, 1,
        "only the cube is in front of the camera"
    );

    // Now bring it back where the camera can see it. The count must not change
    // on the very next frame — the report in hand is an older frame's — and must
    // have changed a few frames later.
    renderer.set_instance(pyramid, &pyramid_at(Vec3::new(0.9, 0.0, 0.0)));
    render_mesh(&headless, &mut renderer, &mut pool, &camera, None);
    let straight_after = renderer.cull_stats().expect("still reporting");
    assert_eq!(
        straight_after.instances, 1,
        "the next frame's report is still an old frame's: {straight_after:?}",
    );

    // And it does change, which is the half a ring that reported once and froze
    // fails: a slot never released, or a request never made again, leaves the
    // count on 1 for ever.
    let mut visible = straight_after;
    for frame in 0..REPORT_BOUND {
        render_mesh(&headless, &mut renderer, &mut pool, &camera, None);
        visible = renderer.cull_stats().expect("still reporting");
        assert!(
            visible.instances == 1 || visible.instances == 2,
            "the survivor count is neither scene's: {visible:?}",
        );
        if visible.instances == 2 {
            break;
        }
        assert!(
            frame + 1 < REPORT_BOUND,
            "the count never followed the scene: {visible:?}",
        );
    }
    assert!(
        visible.frame > culled.frame,
        "and the reports are different frames, so the ring did keep turning",
    );
    assert_eq!(
        renderer.counters().drawn,
        Some(2 + FULLSCREEN_INSTANCES),
        "and the row follows it",
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}
