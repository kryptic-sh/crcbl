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
const SPOT: f32 = -1.0;
const POINT: f32 = 12.0;

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
