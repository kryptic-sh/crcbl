//! **A held shadow atlas, on a device** — topic 45's
//! static-caching rung, where its failure would actually be seen.
//!
//! A frame whose lights and casters have not moved does not draw the shadow
//! atlas again: it samples the maps an earlier frame left in the image. What
//! makes that worth a device test is the shape of getting it wrong — a map that
//! *should* have been redrawn and was not is a shadow under a caster that has
//! walked away from it, which is a perfectly plausible frame. No golden can
//! catch it, because every golden here is one frame.
//!
//! # What is asked here, and what is asked without a device
//!
//! **Which inputs invalidate the atlas** is host arithmetic and is asked by
//! `crcbl_render::forward`'s `each_thing_the_atlas_is_drawn_from_redraws_it`,
//! against the null backend's recorded command stream: it moves a caster, a
//! light and the camera in turn and reads back whether the pass was recorded.
//! **That a still frame skips the pass and its culls** is the same file's
//! `a_frame_over_a_still_scene_does_not_draw_the_shadow_atlas_again`.
//!
//! What is left for a device is the *picture*, and it is the half neither of
//! those can reach: whether the frame a cache decided not to redraw is the
//! frame it would have drawn. That is asked here by rendering the same scene
//! two ways — once through a renderer that has been made to move, and once
//! through a renderer meeting it for the first time — and comparing the two
//! images. A frozen atlas differs from the reference by exactly the shadow it
//! failed to move, and a cache that handed back the wrong rectangle differs by
//! more.

use crate::harness::Headless;
use crate::mesh_scene::{place, render_mesh_lit};
use crcbl::hal::Features;
use crcbl::math::{Mat4, Vec3};
use crcbl::render::shadow::Cadence;
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, InstanceDesc, InstanceHandle, Light, PointLight,
    Projection, SpotLight, TransientPool,
};

/// The frame this file renders at.
///
/// The suite's own extent: what is compared here is two whole images against
/// each other rather than a feature measured in pixels, so there is nothing to
/// gain from a larger frame and a readback to pay for.
const EXTENT: (u32, u32) = crate::mesh_scene::MESH_EXTENT;

/// The floor's side, in world units — wider than the camera sees, so the pool
/// and both shadows are inside the frame with room to spare.
const FLOOR: f32 = 24.0;

/// How far the lamp reaches.
const REACH: f32 = 12.0;

/// Where the caster stands before the move, and where it stands after it.
///
/// Far enough apart that its shadow lands somewhere else entirely: a move small
/// enough for the two shadows to overlap would make the comparison below a
/// question about a rim rather than about a shadow.
const CASTER_STEPS: [Vec3; 2] = [Vec3::new(-2.0, 0.0, 0.0), Vec3::new(2.5, 0.0, 1.5)];

/// How much the pyramid is scaled by to make the caster.
const CASTER: f32 = 1.6;

/// The default sun, turned a degree per `index` — a frame's worth of movement
/// for a test that needs the shadow atlas **redrawn**.
///
/// The other side of this file's subject, and it lives here because that is
/// where the reason is written down. A frame whose lights and casters have not
/// moved does not draw the atlas at all, so a suite that renders a run of
/// identical frames and then asks what the `shadow` pass cost, or which render
/// area it used, is asking about a pass that ran once. Turning the sun is the
/// smallest input the atlas depends on: the cascades are refitted, so every
/// frame is a redraw, and a degree is small enough that each frame draws the
/// same geometry through nearly the same cascade boxes.
///
/// `depth_only.rs`'s price and `resize.rs`'s storm are the callers.
pub(crate) fn turning_sun(index: usize) -> DirectionalLight {
    let sun = DirectionalLight::default();
    let turn = Mat4::from_rotation_y(
        f32::from(u16::try_from(index).expect("a hundred frames or so")) * std::f32::consts::PI
            / 180.0,
    );
    DirectionalLight {
        direction: turn.transform_vector3(sun.direction).normalize(),
        ..sun
    }
}

/// The camera: straight down over the floor, far enough back to hold the whole
/// pool.
fn floor_camera() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 14.0, 0.0),
        target: Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`.
        up: Vec3::Z,
        projection: Projection::default(),
    }
}

/// The one lamp, leaning in from `+z` so its casters throw their shadows along
/// `-z` — across the frame rather than under themselves.
fn lamp() -> Light {
    let position = Vec3::new(0.0, 5.0, 5.0);
    Light::Spot(SpotLight {
        position,
        // Near-white and bright, so a shadow is a large luma step rather than a
        // shade of a colour.
        color: Vec3::new(1.0, 0.97, 0.94) * 30.0,
        radius: REACH,
        direction: Vec3::ZERO - position,
        inner_angle: 0.5,
        outer_angle: 0.75,
        fill: false,
    })
}

/// The sun this frame is drawn under: none of it, and a floor of ambient.
///
/// Black, so the lamp's pool is the only lit thing and the shadow inside it is
/// the largest difference two frames can have. A sun would light the floor
/// everywhere and fill in the very pixels this file compares.
fn no_sun() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::Y,
        color: Vec3::ZERO,
        ambient: Vec3::splat(0.02),
    }
}

/// The floor and the lamp, with the caster standing at `at`.
///
/// Returns the caster's handle so the test can move it: the point of the
/// exercise is a renderer that has been made to move against one that has not.
fn lit_floor(headless: &Headless, at: Vec3) -> (ForwardRenderer, TransientPool, InstanceHandle) {
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    // The cube spans half a unit either side of its origin, so a floor scaled to
    // `FLOOR` and dropped by half of it has its `+Y` face on `y = 0`.
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::from_translation(Vec3::new(0.0, -0.5 * FLOOR, 0.0))
            * Mat4::from_scale(Vec3::splat(FLOOR)),
    );
    let caster = place(
        &mut renderer,
        crcbl::render::scene::DEMO_PYRAMID,
        crcbl::render::scene::DEMO_UNTINTED,
        caster_at(at),
    );
    renderer.set_lights(&[lamp()]);
    (renderer, TransientPool::new(), caster)
}

/// The caster's transform at `at`.
///
/// The pyramid's base is at `-0.4` in its own space, so lifting it by that much
/// of the scale stands it on the floor — which is what puts the contact point of
/// its shadow in the frame.
fn caster_at(at: Vec3) -> Mat4 {
    Mat4::from_translation(at + Vec3::new(0.0, 0.4 * CASTER, 0.0))
        * Mat4::from_scale(Vec3::splat(CASTER))
}

/// Releases everything in dependency order, then asks the device what it saw.
fn teardown(headless: Headless, renderers: Vec<(ForwardRenderer, TransientPool)>) {
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    for (renderer, mut pool) in renderers {
        renderer.destroy(device);
        pool.destroy(device);
    }
    headless.finish();
}

/// How many pixels of `left` and `right` differ by more than one step in any
/// channel.
///
/// One step rather than zero: the two images below are drawn by the same device
/// from the same commands, so an exact comparison is the right one — and this
/// is also the statistic the *first* assertion uses, where a tolerance of one
/// keeps a driver's own rounding out of a count that has to be large to mean
/// anything.
fn differing(left: &crcbl_golden::Image, right: &crcbl_golden::Image) -> usize {
    let (width, height) = EXTENT;
    let mut differing = 0;
    for y in 0..height {
        for x in 0..width {
            let a = left.pixel(x, y).expect("inside the frame");
            let b = right.pixel(x, y).expect("inside the frame");
            if a.iter().zip(b).any(|(a, b)| a.abs_diff(b) > 1) {
                differing += 1;
            }
        }
    }
    differing
}

/// How many pixels a moved caster has to change before the comparisons below
/// are about a shadow that moved.
///
/// **Measured rather than guessed**, on radv at [`EXTENT`]: the move in
/// [`CASTER_STEPS`] changes 1432 pixels. The same run with the atlas's record
/// forced to a constant — every frame cached, which is the failure this file
/// exists to catch — changes 992, and its moved frame differs from the
/// reference in 1023.
///
/// The floor is about half the measured figure and therefore under the frozen
/// number too, deliberately: what this guards is only that the move landed at
/// all, so that the two zero-difference comparisons cannot pass by comparing a
/// frame with itself. Catching the frozen atlas is the reference comparison's
/// job, and it reads a thousand wrong pixels there rather than a threshold.
const MOVED_PIXELS: usize = 700;

/// **A cached shadow atlas draws the frame it would have drawn.**
///
/// Three renders and two comparisons, and each comparison fails on its own:
///
/// * **The moved caster's frame against a renderer meeting the scene for the
///   first time.** The reference renderer has no history at all, so its one
///   frame draws every map from nothing; the frame under test belongs to a
///   renderer that drew the scene, was told to move the caster, and had to
///   decide whether its maps were still good. A cache that answered "still
///   good" leaves the old shadow in the new frame, and the two images differ by
///   it. This is the assertion that stops "cached everything, for ever".
/// * **The frame after it, which nothing changed, against that same frame.**
///   Here the renderer *does* cache — [`ForwardRenderer::shadow_atlas_cached`]
///   says so — and the claim is that the picture is unchanged by it. A cache
///   that held the wrong rectangle, or handed a tile back while a map was still
///   being sampled through it, differs here.
///
/// And [`MOVED_PIXELS`] guards the pair from underneath: a move that changed
/// nothing would satisfy both comparisons trivially, so the move is asserted to
/// have moved something first.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_cached_atlas_draws_the_frame_it_would_have_drawn() {
    let headless = Headless::open_at(EXTENT, Features::GPU_DRIVEN | Features::DEBUG_MARKERS);
    let camera = floor_camera();
    let sun = no_sun();

    // The renderer under test: it draws the scene, is told to move the caster,
    // and draws it again.
    let (mut moved, mut moved_pool, caster) = lit_floor(&headless, CASTER_STEPS[0]);
    let before = render_mesh_lit(&headless, &mut moved, &mut moved_pool, &camera, &sun, None);
    let drew_first = !moved.shadow_atlas_cached();
    moved.set_instance(
        caster,
        &InstanceDesc {
            mesh: crcbl::render::scene::DEMO_PYRAMID,
            material: crcbl::render::scene::DEMO_UNTINTED,
            transform: caster_at(CASTER_STEPS[1]),
        },
    );
    let after = render_mesh_lit(&headless, &mut moved, &mut moved_pool, &camera, &sun, None);
    let drew_after_the_move = !moved.shadow_atlas_cached();

    // The reference: a renderer with the caster already where the move put it,
    // so its first frame draws every map from an empty atlas.
    let (mut fresh, mut fresh_pool, _) = lit_floor(&headless, CASTER_STEPS[1]);
    let reference = render_mesh_lit(&headless, &mut fresh, &mut fresh_pool, &camera, &sun, None);

    // Two more frames from the renderer under test, with nothing changed.
    //
    // **Two, because the first of them still redraws**, and that is a property
    // of the instance pool rather than of this rung: `InstancePool`'s
    // carry-forward puts a moved record's `previous_transform` back at rest on
    // the frame *after* the move, which is a write, and a write is a change the
    // atlas's record cannot tell from any other. See
    // `InstancePool::take_written`, which says so. The second frame is the one
    // the cache actually answers.
    let settling = render_mesh_lit(&headless, &mut moved, &mut moved_pool, &camera, &sun, None);
    let drew_while_settling = !moved.shadow_atlas_cached();
    let held = render_mesh_lit(&headless, &mut moved, &mut moved_pool, &camera, &sun, None);
    let cached_the_still_frame = moved.shadow_atlas_cached();

    // Deferred on purpose, on this suite's terms: a test that panicked here
    // would leave the renderers, the pools and the device undestroyed, and the
    // resulting `Drop` warning would print on top of the message that says what
    // actually went wrong.
    let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(
            drew_first,
            "the first frame of all has to draw the atlas: nothing has put anything in it"
        );
        assert!(
            drew_after_the_move,
            "the caster moved and the renderer kept the maps it had, so the frame below is \
             holding a shadow of where the caster used to be"
        );
        assert!(
            drew_while_settling,
            "the frame that settles the moved caster's `previous_transform` did not redraw, so \
             the record has stopped seeing an instance write and the comment above it is wrong"
        );
        assert!(
            cached_the_still_frame,
            "the second still frame after the move drew the atlas again, so the comparison \
             below is not about a cached frame at all"
        );

        let moved_by = differing(&before, &after);
        eprintln!(
            "{}: the caster's move changed {moved_by} pixels",
            crate::SUITE
        );
        assert!(
            moved_by >= MOVED_PIXELS,
            "moving the caster changed {moved_by} pixels, under the {MOVED_PIXELS} this scene \
             draws — the move did not land, and both comparisons below would pass on a frozen \
             atlas"
        );

        let against_reference = differing(&after, &reference);
        eprintln!(
            "{}: the moved frame differs from the reference in {against_reference} pixels",
            crate::SUITE
        );
        assert_eq!(
            against_reference, 0,
            "the frame a renderer drew after moving its caster differs from the frame a \
             renderer meeting the same scene draws. The maps are the only state that carries \
             across a frame here, so this is a shadow left where the caster used to be"
        );

        let settled_by = differing(&after, &settling);
        assert_eq!(
            settled_by, 0,
            "the frame that settled the caster's previous transform drew a different picture \
             from the frame that moved it, which nothing in this scene explains"
        );

        let across_the_cache = differing(&after, &held);
        eprintln!(
            "{}: the cached frame differs from the drawn one in {across_the_cache} pixels",
            crate::SUITE
        );
        assert_eq!(
            across_the_cache, 0,
            "the frame that kept its atlas is not the frame that drew it, so the rectangles the \
             shader read are not the rectangles the maps were rendered into"
        );
    }));

    teardown(headless, vec![(moved, moved_pool), (fresh, fresh_pool)]);
    if let Err(panic) = verdict {
        std::panic::resume_unwind(panic);
    }
}

/// The cadence this file's second test pins: every map is due every frame, and
/// one tile is all a frame may redraw.
///
/// **The budget rather than the ladder**, because it is the shorter road to the
/// frame this test is about: the scene has the sun's two cascades and the lamp's
/// one map asking every frame, so a budget of one tile holds two of them on
/// every frame and rotates which. Pinned through
/// [`ForwardRenderer::set_shadow_cadence`] rather than set on the console, for
/// the reason that method gives.
const ONE_TILE_A_FRAME: Cadence = Cadence { hold: 1, faces: 1 };

/// How many frames the run below draws after the caster moves.
///
/// Long enough for the rotation to serve every map that is asking several times
/// over — the schedule's own tie-break turns by one group a frame, and there are
/// `crcbl_render::shadow::GROUPS` of them.
const CADENCE_FRAMES: usize = 12;

/// **A frame that kept a tile draws the map it redrew, and the tile it kept
/// stays kept.**
///
/// Topic 45's cadence rung, on a device, and the half no host
/// test can reach. A frame that holds a map cannot clear the attachment — the
/// only clear this seam has covers the whole image — so it **loads** it and
/// resets each tile it redraws with a primitive of its own. Two ways that goes
/// wrong, and both draw a plausible picture:
///
/// * **The tile is not reset.** The map keeps the previous frame's depths, and
///   under reversed-Z a caster that has walked away is still the nearest thing
///   to the light: its shadow goes on being cast from where it used to be, on
///   top of the shadow that is cast now.
/// * **The load erases what it was meant to keep.** A held tile reads as the
///   reversed-Z clear, which is "nothing stored" — so the light it belongs to
///   stops occluding altogether.
///
/// The comparison is the same one
/// [`a_cached_atlas_draws_the_frame_it_would_have_drawn`] makes: once the
/// rotation has served the lamp's map after the move, the frame under test has
/// to be the frame a renderer meeting the scene for the first time draws. The
/// sun is black here, so the only map in the picture is the lamp's — a held
/// cascade is a real lag and would be a different test.
///
/// And the hold is asserted to have happened at all: a run in which every frame
/// redrew every map would satisfy the comparison with nothing held, which is
/// exactly the cadence not running.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_frame_that_kept_a_tile_draws_the_map_it_redrew() {
    let headless = Headless::open_at(EXTENT, Features::GPU_DRIVEN | Features::DEBUG_MARKERS);
    let camera = floor_camera();
    let sun = no_sun();

    let (mut moved, mut moved_pool, caster) = lit_floor(&headless, CASTER_STEPS[0]);
    moved.set_shadow_cadence(Some(ONE_TILE_A_FRAME));
    let before = render_mesh_lit(&headless, &mut moved, &mut moved_pool, &camera, &sun, None);
    moved.set_instance(
        caster,
        &InstanceDesc {
            mesh: crcbl::render::scene::DEMO_PYRAMID,
            material: crcbl::render::scene::DEMO_UNTINTED,
            transform: caster_at(CASTER_STEPS[1]),
        },
    );

    // Each frame after the move, with what the renderer says it did: whether
    // the lamp's map was redrawn, and how many tiles the frame redrew at all.
    let mut run = Vec::with_capacity(CADENCE_FRAMES);
    for _ in 0..CADENCE_FRAMES {
        let image = render_mesh_lit(&headless, &mut moved, &mut moved_pool, &camera, &sun, None);
        run.push((
            image,
            moved.shadow_slot_redrawn(0),
            moved.shadow_faces_redrawn(),
        ));
    }

    // The reference: a renderer with the caster already where the move put it,
    // so its first frame lays the atlas out and draws every map from nothing.
    let (mut fresh, mut fresh_pool, _) = lit_floor(&headless, CASTER_STEPS[1]);
    let reference = render_mesh_lit(&headless, &mut fresh, &mut fresh_pool, &camera, &sun, None);

    let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let over_budget = run.iter().filter(|(_, _, faces)| *faces > 1).count();
        assert_eq!(
            over_budget, 0,
            "{over_budget} frame(s) of the run redrew more than the one tile the pinned \
             cadence allows, so nothing below is about a frame that kept anything"
        );
        let held = run.iter().filter(|(_, drew, _)| !*drew).count();
        assert!(
            held >= CADENCE_FRAMES / 2,
            "only {held} of {CADENCE_FRAMES} frames held the lamp's map: the budget is not \
             binding and this run has no load path in it"
        );

        let moved_by = differing(&before, &run[run.len() - 1].0);
        eprintln!(
            "{}: the caster's move changed {moved_by} pixels",
            crate::SUITE
        );
        assert!(
            moved_by >= MOVED_PIXELS,
            "moving the caster changed {moved_by} pixels, under the {MOVED_PIXELS} this scene \
             draws — the move did not land, and the comparison below would pass on an atlas \
             that never redrew anything"
        );

        // The last frame of the run: the rotation has served the lamp's map by
        // then, so its shadow is where the caster now stands and the picture is
        // the reference's.
        let (settled, _, _) = &run[run.len() - 1];
        let against_reference = differing(settled, &reference);
        eprintln!(
            "{}: the frame after the run differs from the reference in {against_reference} \
             pixels",
            crate::SUITE
        );
        assert_eq!(
            against_reference, 0,
            "a frame drawn under the cadence differs from the frame a renderer meeting the \
             same scene draws. The maps are the only state that carries across a frame here, \
             so this is either a tile the load path failed to reset — the old caster still \
             occluding — or a tile it erased"
        );

        // And the hold was visible while it lasted: the first frame after the
        // move still shows the shadow the lamp's map was drawn with.
        let (first, redrew_first, _) = &run[0];
        assert!(
            !redrew_first,
            "the first frame after the move served the lamp, so there is no held frame here \
             to compare"
        );
        let lagging = differing(first, &reference);
        eprintln!(
            "{}: the held frame lags the reference by {lagging} pixels",
            crate::SUITE
        );
        assert!(
            lagging > 0,
            "the frame that held the lamp's map drew the reference exactly, so the map was \
             redrawn after all and the cadence held nothing"
        );
    }));

    teardown(headless, vec![(moved, moved_pool), (fresh, fresh_pool)]);
    if let Err(panic) = verdict {
        std::panic::resume_unwind(panic);
    }
}

/// Pyramids along one side of the field the moving-camera price is drawn over.
const PRICE_FIELD: usize = 5;

/// How far apart the field's pyramids stand, in world units.
const PRICE_SPACING: f32 = 3.0;

/// How far the camera turns about the field each frame of the price, in
/// radians.
///
/// Small enough that no light's tile changes size from one frame to the next —
/// a relaid atlas redraws every map, which is a different question — and large
/// enough that the cascades, which are fitted to the eye, are out of date on
/// every frame.
const PRICE_TURN: f32 = 0.002;

/// What one run of [`moving_camera_price`] saw.
struct MovingCameraPrice {
    /// The `shadow` pass's p50 and p95 in nanoseconds, or [`None`] where the
    /// device cannot time a pass.
    price: Option<(u64, u64)>,
    /// Shadow groups — cascades and light slots — redrawn, summed over the
    /// recorded frames.
    groups: u64,
    /// Of those, the light slots' alone.
    light_groups: u64,
    /// Tiles redrawn, summed over the recorded frames.
    faces: u64,
}

/// The shadow pass under a camera that turns about a still field lit by three
/// shadowed spots and a shadowed point light, with the sun's cascades beside
/// them — and, with `stirred`, one pyramid inside the first spot's cone nudged
/// every frame.
///
/// What it prices is the atlas cache's reach: a light's maps do not depend on
/// the camera, so a cache keyed on what they are drawn from holds them while
/// the cascades, which are fitted to the eye, redraw.
fn moving_camera_price(extent: (u32, u32), frames: usize, stirred: bool) -> MovingCameraPrice {
    use crate::area_light::PRICE_WARMUP;
    use crcbl::hal::{CommandEncoderDesc, PresentInfo, ResourceState, SubmitInfo};

    let headless = Headless::open_at(
        extent,
        Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
    );
    let device = headless.device.as_ref();
    let timed = device.caps().features.contains(Features::TIMESTAMP_QUERY);
    let mut renderer =
        ForwardRenderer::new(device, headless.queue, headless.format).expect("the renderer builds");
    // Every map that is out of date redrawn on the frame it went out of date,
    // so the counts below are the cache's alone and not a budget's.
    renderer.set_shadow_cadence(Some(Cadence::EVERY_FRAME));
    let mut pool = TransientPool::new();
    place(
        &mut renderer,
        crcbl::render::scene::DEMO_CUBE,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::from_translation(Vec3::new(0.0, -0.5 * FLOOR, 0.0))
            * Mat4::from_scale(Vec3::splat(FLOOR)),
    );
    #[expect(
        clippy::cast_precision_loss,
        reason = "a field of a few pyramids a side"
    )]
    let first = -(PRICE_FIELD as f32 - 1.0) / 2.0 * PRICE_SPACING;
    let mut casters = Vec::new();
    for row in 0..PRICE_FIELD {
        for column in 0..PRICE_FIELD {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a field of a few pyramids a side"
            )]
            let at = Vec3::new(
                first + column as f32 * PRICE_SPACING,
                0.0,
                first + row as f32 * PRICE_SPACING,
            );
            casters.push((
                at,
                place(
                    &mut renderer,
                    crcbl::render::scene::DEMO_PYRAMID,
                    crcbl::render::scene::DEMO_UNTINTED,
                    caster_at(at),
                ),
            ));
        }
    }
    renderer.set_lights(&price_lights());
    // The pyramid in the field's first corner, which is inside the first spot's
    // cone.
    let (stir_at, stir) = casters[0];
    let sun = DirectionalLight::default();

    let mut timers = timed.then(|| {
        crcbl::render::PassTimers::new(
            device,
            crcbl::render::forward::FRAMES_IN_FLIGHT,
            crcbl::render::MAX_TIMED_PASSES,
        )
        .expect("a device reporting TIMESTAMP_QUERY gives out timer sets")
    });
    let mut stats = crcbl::render::PassStats::new();
    let mut recorded = Vec::new();
    let mut price = MovingCameraPrice {
        price: None,
        groups: 0,
        light_groups: 0,
        faces: 0,
    };
    for index in 0..PRICE_WARMUP + frames {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a few hundred frames, and the angle is what is wanted"
        )]
        let angle = index as f32 * PRICE_TURN;
        let camera = price_camera(angle);
        if stirred {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a few hundred frames against a drift measured in millimetres"
            )]
            let drift = Vec3::X * (index as f32 * 1.0e-3);
            renderer.set_instance(
                stir,
                &InstanceDesc {
                    mesh: crcbl::render::scene::DEMO_PYRAMID,
                    material: crcbl::render::scene::DEMO_UNTINTED,
                    transform: caster_at(stir_at + drift),
                },
            );
        }
        let acquired = device
            .acquire_next_frame(headless.swapchain)
            .expect("the ring always has an image");
        renderer
            .begin_frame(device, &camera, &sun, extent)
            .expect("the uniform buffer is writable");
        if index >= PRICE_WARMUP {
            let cascades = (0..crcbl::render::shadow::CASCADES)
                .filter(|cascade| renderer.shadow_cascade_redrawn(*cascade))
                .count();
            let lights = (0..crcbl::render::shadow::LIGHT_SLOTS)
                .filter(|slot| renderer.shadow_slot_redrawn(*slot))
                .count();
            price.groups += (cascades + lights) as u64;
            price.light_groups += lights as u64;
            price.faces += u64::from(renderer.shadow_faces_redrawn());
        }
        let compiled = {
            let mut graph = crcbl::render::RenderGraph::new(headless.queue);
            let target = graph.import_image(
                "swapchain",
                crcbl::render::ImportedImage {
                    image: acquired.image,
                    view: acquired.view,
                    format: headless.format,
                    extent,
                    initial: ResourceState::Undefined,
                    claim: crcbl::render::InitialClaim::Acquired,
                    final_state: ResourceState::Present,
                },
            );
            renderer.add_passes(&mut graph, &pool, target, extent);
            graph.compile(&pool).expect("a legal frame")
        };
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("moving camera shadow frame"),
            queue: headless.queue,
        });
        compiled
            .execute(device, &mut pool, encoder.as_mut(), timers.as_mut())
            .expect("the graph executed");
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device
            .present(
                headless.queue,
                &PresentInfo {
                    swapchain: headless.swapchain,
                    waits: acquired.present_semaphore.as_slice(),
                    present_id: None,
                },
            )
            .expect("present");
        recorded.push(commands);
        // Against the frame the timings came from, which is what `FrameTimings`
        // itself says — `shadow_tiles.rs`'s price explains the lag.
        if let Some(timers) = timers.as_ref() {
            let timings = timers.latest();
            let Some(drawn) = usize::try_from(timings.frame)
                .ok()
                .and_then(|frame| frame.checked_sub(1))
            else {
                continue;
            };
            if drawn >= PRICE_WARMUP {
                stats.record(timings);
            }
        }
    }

    device.wait_idle().expect("idle");
    price.price = timed.then(|| {
        stats
            .percentiles("shadow")
            .expect("the shadow pass is timed and the window is past its floor")
    });
    if let Some(mut timers) = timers.take() {
        timers.destroy(device);
    }
    for commands in recorded {
        device.destroy_command_buffer(commands);
    }
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    price
}

/// The camera for the moving-camera price and check: `angle` radians round
/// the field, up and back, looking at its middle.
fn price_camera(angle: f32) -> Camera {
    Camera {
        eye: Vec3::new(18.0 * angle.sin(), 12.0, 18.0 * angle.cos()),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// Three shadowed spots over three corners of the field and a shadowed point
/// light over the fourth — every light slot the atlas has, and both kinds of
/// light group.
fn price_lights() -> Vec<Light> {
    let spot = |pool: Vec3| {
        let position = pool + Vec3::new(0.0, 5.0, 1.0);
        Light::Spot(SpotLight {
            position,
            color: Vec3::new(1.0, 0.97, 0.94) * 30.0,
            radius: REACH,
            direction: pool - position,
            inner_angle: 0.4,
            outer_angle: 0.6,
            fill: false,
        })
    };
    vec![
        spot(Vec3::new(-6.0, 0.0, -6.0)),
        spot(Vec3::new(6.0, 0.0, -6.0)),
        spot(Vec3::new(-6.0, 0.0, 6.0)),
        Light::Point(PointLight {
            position: Vec3::new(6.0, 3.0, 6.0),
            radius: 6.0,
            color: Vec3::splat(20.0),
            fill: false,
        }),
    ]
}

/// **The price of a moving camera over shadowed local lights**: how many shadow
/// groups each frame redraws and what the `shadow` pass costs, over a still
/// field and over the same field with one caster nudged inside one spot's cone.
///
/// Prints rather than asserts a duration, on `shadow_tiles.rs`'s terms: a
/// millisecond figure is a property of the machine it was measured on.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_moving_camera_prices_the_shadow_pass() {
    let (extent, frames) = crate::area_light::price_frame();
    for stirred in [false, true] {
        let price = moving_camera_price(extent, frames, stirred);
        #[expect(
            clippy::cast_precision_loss,
            reason = "counts over a few hundred frames, printed as averages"
        )]
        let per_frame = |count: u64| count as f64 / frames as f64;
        let groups = per_frame(price.groups);
        let lights = per_frame(price.light_groups);
        let faces = per_frame(price.faces);
        let arm = if stirred { ", one caster stirred" } else { "" };
        match price.price {
            Some((p50, p95)) => eprintln!(
                "{}: moving camera{arm} at {extent:?} over {frames} frames — {groups:.2} groups \
                 ({lights:.2} light slots) and {faces:.2} tiles redrawn a frame, shadow pass p50 \
                 {:.3} ms p95 {:.3} ms",
                crate::SUITE,
                p50 as f64 / 1e6,
                p95 as f64 / 1e6,
            ),
            None => eprintln!(
                "{}: moving camera{arm} at {extent:?} over {frames} frames — {groups:.2} groups \
                 ({lights:.2} light slots) and {faces:.2} tiles redrawn a frame; this backend \
                 cannot time a pass",
                crate::SUITE,
            ),
        }
    }
}

/// How many frames the camera turns for in
/// [`a_moving_camera_holds_the_lights_maps_and_draws_the_frame_it_would_have_drawn`].
const TURNING_FRAMES: usize = 8;

/// **A moving camera redraws the cascades, holds every light's maps, and the
/// frame it then draws is the frame a renderer meeting the scene draws.**
///
/// A light's map is drawn from the light, so a cache keyed on what it is
/// drawn from holds it while the eye moves — and the picture is the proof that
/// holding it was right: the maps a turned camera samples were drawn from where
/// the camera stood on the first frame, and a renderer with no history draws
/// them from where it stands now. A light map that depended on the eye after
/// all — a cut chosen from it, a camera-fitted field the depth pass reads —
/// differs between the two.
///
/// A far caster is moved on every frame too, outside every light's reach, so
/// the lights holding is also a moved instance they cannot see redrawing
/// nothing.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_moving_camera_holds_the_lights_maps_and_draws_the_frame_it_would_have_drawn() {
    let headless = Headless::open_at(EXTENT, Features::GPU_DRIVEN | Features::DEBUG_MARKERS);
    let sun = DirectionalLight::default();
    let far_caster = |step: usize| {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a handful of frames, and the step is what is wanted"
        )]
        let along = step as f32 * 0.1;
        caster_at(Vec3::new(60.0 + along, 0.0, 0.0))
    };
    let scene = |headless: &Headless| {
        let mut renderer =
            ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
                .expect("the forward renderer builds");
        renderer.set_shadow_cadence(Some(Cadence::EVERY_FRAME));
        place(
            &mut renderer,
            crcbl::render::scene::DEMO_CUBE,
            crcbl::render::scene::DEMO_UNTINTED,
            Mat4::from_translation(Vec3::new(0.0, -0.5 * FLOOR, 0.0))
                * Mat4::from_scale(Vec3::splat(FLOOR)),
        );
        for at in [
            Vec3::new(-6.0, 0.0, -6.0),
            Vec3::new(6.0, 0.0, -6.0),
            Vec3::new(-6.0, 0.0, 6.0),
            Vec3::new(6.0, 0.0, 6.0),
            Vec3::ZERO,
        ] {
            place(
                &mut renderer,
                crcbl::render::scene::DEMO_PYRAMID,
                crcbl::render::scene::DEMO_UNTINTED,
                caster_at(at),
            );
        }
        renderer.set_lights(&price_lights());
        renderer
    };

    let mut moved = scene(&headless);
    let far = place(
        &mut moved,
        crcbl::render::scene::DEMO_PYRAMID,
        crcbl::render::scene::DEMO_UNTINTED,
        far_caster(0),
    );
    let mut moved_pool = TransientPool::new();
    // Two frames, so the atlas is drawn and the instance writes the placing
    // made have settled.
    for _ in 0..2 {
        render_mesh_lit(
            &headless,
            &mut moved,
            &mut moved_pool,
            &price_camera(0.0),
            &sun,
            None,
        );
    }
    let occupied = (0..crcbl::render::shadow::LIGHT_SLOTS)
        .filter(|slot| moved.shadow_lights().base_of(*slot).is_some())
        .count();
    let mut run = Vec::with_capacity(TURNING_FRAMES);
    for step in 1..=TURNING_FRAMES {
        moved.set_instance(
            far,
            &InstanceDesc {
                mesh: crcbl::render::scene::DEMO_PYRAMID,
                material: crcbl::render::scene::DEMO_UNTINTED,
                transform: far_caster(step),
            },
        );
        #[expect(
            clippy::cast_precision_loss,
            reason = "a handful of frames, and the angle is what is wanted"
        )]
        let camera = price_camera(step as f32 * 0.05);
        render_mesh_lit(&headless, &mut moved, &mut moved_pool, &camera, &sun, None);
        run.push((
            (0..crcbl::render::shadow::CASCADES)
                .all(|cascade| moved.shadow_cascade_redrawn(cascade)),
            (0..crcbl::render::shadow::LIGHT_SLOTS)
                .filter(|slot| moved.shadow_slot_redrawn(*slot))
                .count(),
        ));
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a handful of frames, and the angle is what is wanted"
    )]
    let last = price_camera(TURNING_FRAMES as f32 * 0.05);
    // One more frame at the last camera, so neither renderer's frame carries a
    // camera motion the other's does not.
    let held = render_mesh_lit(&headless, &mut moved, &mut moved_pool, &last, &sun, None);

    // The reference: the same scene met for the first time at the last camera,
    // with the far caster where the run left it.
    let mut fresh = scene(&headless);
    place(
        &mut fresh,
        crcbl::render::scene::DEMO_PYRAMID,
        crcbl::render::scene::DEMO_UNTINTED,
        far_caster(TURNING_FRAMES),
    );
    let mut fresh_pool = TransientPool::new();
    let reference = render_mesh_lit(&headless, &mut fresh, &mut fresh_pool, &last, &sun, None);

    let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_eq!(
            occupied,
            crcbl::render::shadow::LIGHT_SLOTS,
            "every light must hold a slot, or the holds below are about fewer maps than it says"
        );
        for (step, (cascades, lights)) in run.iter().enumerate() {
            assert!(
                *cascades,
                "frame {step} of the turn held a cascade, which is fitted to the eye"
            );
            assert_eq!(
                *lights, 0,
                "frame {step} of the turn redrew {lights} light map(s): nothing any light is \
                 drawn from moved"
            );
        }
        let against_reference = differing(&held, &reference);
        eprintln!(
            "{}: the turned camera's frame differs from the reference in {against_reference} \
             pixels",
            crate::SUITE
        );
        assert_eq!(
            against_reference, 0,
            "the frame drawn through light maps held across a turning camera differs from the \
             frame a renderer meeting the scene draws, so a held light map depended on the eye"
        );
    }));

    teardown(headless, vec![(moved, moved_pool), (fresh, fresh_pool)]);
    if let Err(panic) = verdict {
        std::panic::resume_unwind(panic);
    }
}
