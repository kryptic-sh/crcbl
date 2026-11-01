//! What a moved eye and a moved caster redraw in the shadow atlas, group by
//! group — and, the half a cache is for, what they leave alone.
//!
//! `each_thing_the_atlas_is_drawn_from_redraws_it` asks that every input redraws
//! the atlas; these ask that an input redraws **only the groups whose maps it
//! is in**. A light's map is drawn from the light, so the camera is no input of
//! it unless a DAG in the light's cull chooses its level from the eye, and a
//! caster is an input of the groups whose cull could keep it before or after it
//! moved.

use super::*;

/// Where the spot of [`shadow_cache_scene`] stands along `x`, and the point
/// light beside it.
pub(super) const SPOT: f32 = -1.0;
pub(super) const POINT: f32 = 12.0;

/// Somewhere no light of these scenes reaches: far along `+x`, past both
/// lights' radii.
const AWAY: Vec3 = Vec3::new(60.0, 0.0, 0.0);

/// A camera `step` small moves along `x` from the default one: enough to refit
/// both cascades, too little to change any light's tile.
fn stepped_camera(step: usize) -> Camera {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a handful of frames, and the step is what is wanted"
    )]
    let along = 0.05 * step as f32;
    Camera {
        eye: Camera::default().eye + Vec3::X * along,
        ..Camera::default()
    }
}

/// Draws one frame and answers which groups it redrew: both cascades, then
/// light slots 0 and 1.
fn redrawn(
    device: &dyn Device,
    renderer: &mut ForwardRenderer,
    queue: QueueHandle,
    camera: &Camera,
) -> [bool; 4] {
    let drawn = frame_seen_from(
        device,
        renderer,
        queue,
        camera,
        &DirectionalLight::default(),
    );
    drawn.release(device);
    [
        renderer.shadow_cascade_redrawn(0),
        renderer.shadow_cascade_redrawn(1),
        renderer.shadow_slot_redrawn(0),
        renderer.shadow_slot_redrawn(1),
    ]
}

/// The cache scene with a shadowed point light beside its spot, drawn until
/// it holds everything.
fn settled_scene(device: &dyn Device, queue: QueueHandle) -> (ForwardRenderer, InstanceHandle) {
    let (mut renderer, cube) = shadow_cache_scene(device, queue);
    renderer.set_lights(&[shadowable_spot(SPOT), shadowable_point(POINT)]);
    assert_eq!(
        redrawn(device, &mut renderer, queue, &Camera::default()),
        [true; 4],
        "the first frame of all draws every group"
    );
    assert!(
        renderer.shadow_lights().base_of(1).is_some(),
        "the point light must hold a slot for its group to be asked about"
    );
    assert_eq!(
        redrawn(device, &mut renderer, queue, &Camera::default()),
        [false; 4],
        "a still second frame holds every group"
    );
    (renderer, cube)
}

/// **A moving camera redraws the cascades, which are fitted to it, and holds
/// both lights' maps**, over a scene of flat meshes nothing moves — and a light
/// that does change afterwards redraws its own group alone.
#[test]
fn a_moving_camera_holds_the_lights_maps_and_redraws_the_cascades() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, _) = settled_scene(device, queue);

    for step in 1..=4 {
        assert_eq!(
            redrawn(device, &mut renderer, queue, &stepped_camera(step)),
            [true, true, false, false],
            "frame {step} of a moving camera: the cascades follow the eye and the lights do not"
        );
    }

    // What a light's own map is drawn from still redraws it: its radius, here,
    // with the camera standing still — and no other group moves with it.
    let still = stepped_camera(4);
    renderer.set_lights(&[
        Light::Spot(crate::light::SpotLight {
            radius: 9.0,
            ..match shadowable_spot(SPOT) {
                Light::Spot(spot) => spot,
                _ => unreachable!("a spot"),
            }
        }),
        shadowable_point(POINT),
    ]);
    assert_eq!(
        redrawn(device, &mut renderer, queue, &still),
        [false, false, true, false],
        "the spot's radius changed and only the spot's map was redrawn"
    );
    renderer.set_lights(&[shadowable_spot(SPOT), shadowable_point(POINT + 0.5)]);
    assert_eq!(
        redrawn(device, &mut renderer, queue, &still),
        [false, false, true, true],
        "the spot's radius went back and the point light moved"
    );
    assert_eq!(
        redrawn(device, &mut renderer, queue, &still),
        [false; 4],
        "and a still frame after it holds everything again"
    );

    renderer.destroy(device);
    recorder.assert_valid();
}

/// **A moved caster redraws the lights whose cull could keep it before or
/// after the move, and no other.**
///
/// Five moves of one cube, each followed by the frame that settles its
/// `previous_transform` — a write too, and one that reaches the same groups:
/// far away to further away, which no light reaches; into the spot's cone;
/// within it; out of it again, which only the box it had *before* the move can
/// see; and a removal from inside it.
#[test]
fn a_moved_caster_redraws_the_lights_it_was_or_is_in_and_no_other() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, _) = settled_scene(device, queue);
    let camera = Camera::default();
    // Placed before anything is asked: a new element above the pool's
    // high-water mark grows the instance count every cull is handed, which is
    // in every group's record and redraws the whole atlas once.
    let mover = place_cube(&mut renderer, Mat4::from_translation(AWAY));
    for _ in 0..3 {
        redrawn(device, &mut renderer, queue, &camera);
    }

    let spot_cone = Vec3::new(SPOT, 0.0, 0.0);
    let moves: [(&str, Vec3, [bool; 2]); 4] = [
        ("from away to further away", AWAY + Vec3::X, [false, false]),
        ("into the spot's cone", spot_cone, [true, false]),
        (
            "within the spot's cone",
            spot_cone + Vec3::Z * 0.25,
            [true, false],
        ),
        ("out of the spot's cone", AWAY, [true, false]),
    ];
    for (what, to, lights) in moves {
        renderer.set_instance(
            mover,
            &InstanceDesc {
                mesh: DEMO_CUBE,
                material: DEMO_UNTINTED,
                transform: Mat4::from_translation(to),
            },
        );
        let moved = redrawn(device, &mut renderer, queue, &camera);
        assert_eq!(
            [moved[2], moved[3]],
            lights,
            "a cube moved {what}: which lights redrew"
        );
        // The frame after settles the move's `previous_transform`, which is a
        // write of the same element at its new place.
        let settling = redrawn(device, &mut renderer, queue, &camera);
        let at_rest = redrawn(device, &mut renderer, queue, &camera);
        assert!(
            !settling[3] && at_rest == [false; 4],
            "a cube moved {what}: the frames after it did not settle ({settling:?}, \
             {at_rest:?})"
        );
    }

    // Back into the cone, and then removed from it: a dead record draws
    // nothing, and the box it had when it was live is what says the spot's map
    // held it.
    renderer.set_instance(
        mover,
        &InstanceDesc {
            mesh: DEMO_CUBE,
            material: DEMO_UNTINTED,
            transform: Mat4::from_translation(spot_cone),
        },
    );
    for _ in 0..3 {
        redrawn(device, &mut renderer, queue, &camera);
    }
    renderer.remove_instance(mover);
    let removed = redrawn(device, &mut renderer, queue, &camera);
    assert_eq!(
        [removed[2], removed[3]],
        [true, false],
        "a cube removed from the spot's cone"
    );

    renderer.destroy(device);
    recorder.assert_valid();
}

/// **A light whose cull holds a DAG redraws for the eye; one whose cull holds
/// flat meshes alone does not.**
///
/// `draw_gen.slang` chooses a DAG instance's level from the selection eye, so a
/// light map with a dunes patch in it is a different map from a different eye.
/// The patch starts far from both lights, moves into the spot's cone, and
/// leaves again.
#[test]
fn a_light_redraws_for_the_eye_only_while_a_dag_is_in_its_cull() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, _) = settled_scene(device, queue);
    let dunes = |at: Vec3| InstanceDesc {
        mesh: DEMO_DUNES,
        material: DEMO_UNTINTED,
        // Small, so where it stands decides which lights can keep it.
        transform: Mat4::from_translation(at) * Mat4::from_scale(Vec3::splat(0.02)),
    };
    let patch = renderer
        .add_instance(&dunes(AWAY))
        .expect("room for one more instance");
    let mut step = 0;
    let mut frame = |renderer: &mut ForwardRenderer| {
        step += 1;
        redrawn(device, renderer, queue, &stepped_camera(step))
    };
    for _ in 0..3 {
        frame(&mut renderer);
    }
    for round in 0..3 {
        let drew = frame(&mut renderer);
        assert_eq!(
            drew,
            [true, true, false, false],
            "round {round}: a DAG no light reaches put the eye into a light's record"
        );
    }

    renderer.set_instance(patch, &dunes(Vec3::new(SPOT, 0.0, 0.0)));
    for _ in 0..3 {
        frame(&mut renderer);
    }
    for round in 0..3 {
        let drew = frame(&mut renderer);
        assert_eq!(
            drew,
            [true, true, true, false],
            "round {round}: a DAG in the spot's cone selects from the eye, so a moved eye is \
             a different spot map — and the point light, which it is not in, holds"
        );
    }

    renderer.set_instance(patch, &dunes(AWAY));
    for _ in 0..3 {
        frame(&mut renderer);
    }
    let drew = frame(&mut renderer);
    assert_eq!(
        drew,
        [true, true, false, false],
        "the DAG left the spot's cone and the spot went on redrawing for the eye"
    );

    renderer.destroy(device);
    recorder.assert_valid();
}

/// Draws one **skinned** frame of `fixture` and answers which groups it
/// redrew, on [`redrawn`]'s terms: the fixture's one range is skinned, so every
/// object drawn out of its region is re-pointed.
fn skinned_redrawn(
    device: &dyn Device,
    fixture: &mut SkinnedFixture,
    queue: QueueHandle,
) -> [bool; 4] {
    fixture.begin(device);
    skinned_groups_drawn(device, fixture, queue)
}

/// [`skinned_redrawn`] with an **empty** skinning plan: no region is
/// dispatched, and the parity still alternates.
fn unplanned_redrawn(
    device: &dyn Device,
    fixture: &mut SkinnedFixture,
    queue: QueueHandle,
) -> [bool; 4] {
    fixture
        .renderer
        .begin_skinned_frame(
            device,
            &mut fixture.skinning,
            &[],
            &Camera::default(),
            &DirectionalLight::default(),
            TEST_EXTENT,
        )
        .expect("an empty plan is legal");
    skinned_groups_drawn(device, fixture, queue)
}

/// Records and runs the frame `fixture`'s renderer has just begun, and answers
/// which groups it redrew, on [`redrawn`]'s terms.
fn skinned_groups_drawn(
    device: &dyn Device,
    fixture: &mut SkinnedFixture,
    queue: QueueHandle,
) -> [bool; 4] {
    let imported = swapchain_image(device);
    let mut graph = crate::RenderGraph::new(queue);
    let target = graph.import_image("target", imported);
    let mut pool = crate::TransientPool::new();
    fixture
        .renderer
        .add_skinned_passes(&mut graph, &pool, target, TEST_EXTENT, &fixture.skinning);
    let compiled = graph.compile(&pool).expect("a legal frame");
    let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
        label: Some("skinned shadow reach"),
        queue,
    });
    compiled
        .execute(device, &mut pool, encoder.as_mut(), None)
        .expect("the graph executed");
    let commands = encoder.finish().expect("recording succeeded");
    Frame {
        imported,
        pool,
        commands,
    }
    .release(device);
    let renderer = &fixture.renderer;
    [
        renderer.shadow_cascade_redrawn(0),
        renderer.shadow_cascade_redrawn(1),
        renderer.shadow_slot_redrawn(0),
        renderer.shadow_slot_redrawn(1),
    ]
}

/// A unit model-space box around `centre`, for an object whose transform
/// is the identity.
pub(super) fn skinned_box(centre: Vec3) -> Aabb {
    Aabb {
        min: centre - Vec3::splat(0.5),
        max: centre + Vec3::splat(0.5),
    }
}

/// The skinning fixture with [`settled_scene`]'s two lights and one skinned
/// object placed with `bounds`, drawn for a few skinned frames so the first
/// frame's whole-atlas redraw is behind it.
pub(super) fn skinned_scene(
    device: &dyn Device,
    queue: QueueHandle,
    bounds: Option<Aabb>,
) -> (SkinnedFixture, InstanceHandle) {
    let mut fixture = SkinnedFixture::build(device, queue);
    // Pinned, on `shadow_cache_scene`'s terms.
    fixture
        .renderer
        .set_shadow_cadence(Some(shadow::Cadence::EVERY_FRAME));
    fixture
        .renderer
        .set_lights(&[shadowable_spot(SPOT), shadowable_point(POINT)]);
    let handle = fixture
        .renderer
        .add_skinned_instance(&SkinnedInstanceDesc {
            mesh: &fixture.skinned,
            material: DEMO_UNTINTED,
            transform: Mat4::IDENTITY,
            bounds,
        })
        .expect("room for one object");
    assert_eq!(
        skinned_redrawn(device, &mut fixture, queue),
        [true; 4],
        "the first frame of all draws every group"
    );
    assert!(
        fixture.renderer.shadow_lights().base_of(1).is_some(),
        "the point light must hold a slot for its group to be asked about"
    );
    (fixture, handle)
}

/// **A skinned object with a box redraws the lights its box reaches on every
/// frame it is skinned, and holds the others** — the skinned twin of
/// `a_moved_caster_redraws_the_lights_it_was_or_is_in_and_no_other`, with the
/// pose standing for the move: the dispatch writes new vertices every frame,
/// so the box is a write every frame.
#[test]
fn a_boxed_skinned_object_redraws_only_the_lights_its_box_reaches() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let in_spot = skinned_box(Vec3::new(SPOT, 0.0, 0.0));
    let (mut fixture, _) = skinned_scene(device, queue, Some(in_spot));

    for frame in 0..4 {
        let drew = skinned_redrawn(device, &mut fixture, queue);
        assert_eq!(
            [drew[2], drew[3]],
            [true, false],
            "skinned frame {frame}: a pose in the spot's cone redraws the spot and holds the \
             point light, which its box cannot reach"
        );
    }

    fixture.destroy(device);
    recorder.assert_valid();
}

/// **One skinned object with no box redraws every map**, whatever a boxed one
/// beside it reaches: nothing bounds its vertices, so its footprint is
/// everywhere.
#[test]
fn an_unboxed_skinned_object_redraws_every_light_beside_a_boxed_one() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let in_spot = skinned_box(Vec3::new(SPOT, 0.0, 0.0));
    let (mut fixture, _) = skinned_scene(device, queue, Some(in_spot));
    for _ in 0..2 {
        skinned_redrawn(device, &mut fixture, queue);
    }
    fixture
        .renderer
        .add_skinned_instance(&SkinnedInstanceDesc {
            mesh: &fixture.skinned,
            material: DEMO_UNTINTED,
            // Far from both lights, so only the missing box can put it in them.
            transform: Mat4::from_translation(AWAY),
            bounds: None,
        })
        .expect("room for a second object");

    for frame in 0..4 {
        assert_eq!(
            skinned_redrawn(device, &mut fixture, queue),
            [true; 4],
            "skinned frame {frame}: an object with no box held a map its pose could be in"
        );
    }

    fixture.destroy(device);
    recorder.assert_valid();
}

/// **A box that moves redraws the lights it reached before the move and the
/// ones it reaches after**, and the frame after the move only the latter:
/// from the spot's cone to the point light's reach, the frame of the move
/// redraws both, because the spot's map still holds the pose that left it.
#[test]
fn a_moved_skinned_box_redraws_the_lights_it_left_and_entered() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let in_spot = skinned_box(Vec3::new(SPOT, 0.0, 0.0));
    let (mut fixture, handle) = skinned_scene(device, queue, Some(in_spot));
    for _ in 0..2 {
        skinned_redrawn(device, &mut fixture, queue);
    }

    let at_point = skinned_box(Vec3::new(POINT, 0.0, 0.0));
    fixture
        .renderer
        .set_skinned_bounds(handle, Some(at_point))
        .expect("a finite, ordered box");
    let moved = skinned_redrawn(device, &mut fixture, queue);
    assert_eq!(
        [moved[2], moved[3]],
        [true, true],
        "the box moved from the spot's cone to the point light: which lights redrew"
    );
    let after = skinned_redrawn(device, &mut fixture, queue);
    assert_eq!(
        [after[2], after[3]],
        [false, true],
        "the frame after the move: the spot's map no longer holds the pose"
    );

    fixture.destroy(device);
    recorder.assert_valid();
}

/// **A skinned object is redrawn on every skinned frame whether or not its
/// region was in the plan**: the parity alternates either way, so it draws out
/// of the other half of its region — an older pose — and the re-point that
/// says so is the write that reaches the shadow cache. Both ways to be left
/// out: a plan naming another region, and an empty plan.
#[test]
fn a_boxed_skinned_object_left_out_of_the_plan_still_redraws_its_lights() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let in_spot = skinned_box(Vec3::new(SPOT, 0.0, 0.0));

    // A plan naming the fixture's region, and the object drawn out of another
    // one, which no range this test hands over names.
    let (mut fixture, placed) = skinned_scene(device, queue, None);
    fixture.renderer.remove_instance(placed);
    let other = fixture
        .renderer
        .reserve_skinned(DEMO_CUBE)
        .expect("room for a second region");
    fixture
        .renderer
        .add_skinned_instance(&SkinnedInstanceDesc {
            mesh: &other,
            material: DEMO_UNTINTED,
            transform: Mat4::IDENTITY,
            bounds: Some(in_spot),
        })
        .expect("room for one object");
    for _ in 0..2 {
        skinned_redrawn(device, &mut fixture, queue);
    }
    for frame in 0..3 {
        let drew = skinned_redrawn(device, &mut fixture, queue);
        assert_eq!(
            [drew[2], drew[3]],
            [true, false],
            "frame {frame} skinned another region: the object left out of the plan held the \
             spot's map, or redrew a light its box cannot reach"
        );
    }
    fixture.renderer.release_skinned(other);
    fixture.destroy(device);

    // An empty plan, with the object drawn out of the fixture's own region.
    let (mut fixture, _) = skinned_scene(device, queue, Some(in_spot));
    for _ in 0..2 {
        unplanned_redrawn(device, &mut fixture, queue);
    }
    for frame in 0..3 {
        let drew = unplanned_redrawn(device, &mut fixture, queue);
        assert_eq!(
            [drew[2], drew[3]],
            [true, false],
            "empty plan {frame}: the object held the spot's map, or redrew a light its box \
             cannot reach"
        );
    }
    fixture.destroy(device);
    recorder.assert_valid();
}
