//! The skinning cull: a boxed skinned object nothing this frame draws is not
//! dispatched, and everything else still is — see
//! `ForwardRenderer::cull_skinned_regions`.
//!
//! Every frame here is recorded and executed on the null backend, so what is
//! counted is the dispatches the skinning pass really recorded, and what is
//! read of a record is the bytes the frame's instance ring holds.

use super::shadow_reach::{SPOT, skinned_box, skinned_scene};
use super::*;
use crcbl_hal::null::Command;

/// Behind the default camera and far from both lights of
/// [`skinned_scene`]: out of the camera's frustum, every cascade's and every
/// light's. [`a_boxed_object_nothing_draws_is_not_skinned`] checks that
/// against the culls a frame actually fits rather than trusting it.
const UNSEEN: Vec3 = Vec3::new(0.0, -400.0, 400.0);

/// In front of the default camera and far past every cascade and light: the
/// camera's frustum is the only cull that reaches it.
const CAMERA_ONLY: Vec3 = Vec3::new(0.0, 0.0, -2000.0);

/// How many dispatches the recorder has seen inside a `skinning` compute pass,
/// over every command buffer so far.
fn skinning_dispatches(recorder: &Recorder) -> usize {
    let mut inside = false;
    let mut dispatches = 0;
    for command in recorder.commands() {
        match command {
            Command::BeginComputePass { label, .. } => {
                inside = label.as_deref() == Some("skinning");
            }
            Command::EndComputePass => inside = false,
            Command::Dispatch { .. } if inside => dispatches += 1,
            _ => {}
        }
    }
    dispatches
}

/// What one recorded frame did: its skinning dispatches, and which groups it
/// redrew — both cascades, then light slots 0 and 1.
#[derive(Debug, PartialEq, Eq)]
struct Drawn {
    dispatches: usize,
    redrawn: [bool; 4],
}

/// Begins one skinned frame of `fixture` with its single range, begins `view`
/// through `view_camera` where one is given, and records, executes and
/// releases the frame.
fn skinned_frame(
    recorder: &Recorder,
    device: &dyn Device,
    queue: QueueHandle,
    fixture: &mut SkinnedFixture,
    view: Option<(ViewId, &Camera)>,
) -> Drawn {
    const VIEW_EXTENT: (u32, u32) = (64, 64);
    let before = skinning_dispatches(recorder);
    fixture.begin(device);
    if let Some((view, camera)) = view {
        fixture
            .renderer
            .begin_view(device, view, camera, VIEW_EXTENT)
            .expect("the view's blocks are written");
    }
    let imported = swapchain_image(device);
    let view_image = swapchain_image_at(device, VIEW_EXTENT);
    let mut graph = crate::RenderGraph::new(queue);
    let target = graph.import_image("target", imported);
    let targets: Vec<ViewTarget> = view
        .map(|(view, _)| ViewTarget {
            view,
            target: graph.import_image("view target", view_image),
            extent: VIEW_EXTENT,
        })
        .into_iter()
        .collect();
    let mut pool = crate::TransientPool::new();
    fixture.renderer.add_passes_with_views(
        &mut graph,
        &pool,
        FrameTargets {
            target,
            extent: TEST_EXTENT,
            skinning: Some(&fixture.skinning),
            views: &targets,
        },
        |_, _| {},
    );
    let compiled = graph.compile(&pool).expect("a legal frame");
    let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
        label: Some("skinning cull"),
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
    device.destroy_image_view(view_image.view);
    device.destroy_image(view_image.image);
    let renderer = &fixture.renderer;
    Drawn {
        dispatches: skinning_dispatches(recorder) - before,
        redrawn: [
            renderer.shadow_cascade_redrawn(0),
            renderer.shadow_cascade_redrawn(1),
            renderer.shadow_slot_redrawn(0),
            renderer.shadow_slot_redrawn(1),
        ],
    }
}

/// [`skinned_frame`] for the primary camera alone.
fn primary_frame(
    recorder: &Recorder,
    device: &dyn Device,
    queue: QueueHandle,
    fixture: &mut SkinnedFixture,
) -> Drawn {
    skinned_frame(recorder, device, queue, fixture, None)
}

/// Whether any cull the frame just begun fitted — the primary camera's or a
/// shadow's — could keep a box at `centre` by the host's own reckoning.
fn reached_by_the_frame(renderer: &ForwardRenderer, centre: Vec3) -> bool {
    let bounds = skinned_box(centre);
    core::iter::once(Frustum::from_view_projection(
        renderer.primary.camera_view_proj,
    ))
    .chain(renderer.skin_shadow_reach.iter().copied())
    .any(|frustum| frustum.intersects(&bounds))
}

/// **A boxed object outside every view and every shadow cull is not
/// skinned**, and the setter that turns the cull off skins it again.
///
/// The second half is what makes the first worth anything: a frame that
/// recorded no skinning pass for some other reason would pass the zero.
#[test]
fn a_boxed_object_nothing_draws_is_not_skinned() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let (mut fixture, _) = skinned_scene(device, queue, Some(skinned_box(UNSEEN)));
    assert!(
        !fixture.renderer.skin_shadow_reach.is_empty(),
        "the scene fits shadow culls, or the test below asks about the camera alone"
    );
    assert!(
        !reached_by_the_frame(&fixture.renderer, UNSEEN),
        "the box is meant to be outside every cull the frame fitted"
    );

    for frame in 0..3 {
        let drawn = primary_frame(&recorder, device, queue, &mut fixture);
        assert_eq!(
            (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
            (0, 1),
            "frame {frame}: an object no view or shadow can draw was skinned"
        );
    }

    fixture.renderer.set_skinning_culls(false);
    let drawn = primary_frame(&recorder, device, queue, &mut fixture);
    assert_eq!(
        (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
        (1, 0),
        "with the cull off every planned region is dispatched"
    );

    fixture.destroy(device);
    recorder.assert_valid();
}

/// **A boxed object that any one cull reaches is skinned**: the primary
/// camera's alone, a light's alone, and a secondary view's alone.
#[test]
fn a_boxed_object_in_any_view_or_shadow_is_skinned() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();

    // The camera's frustum and nothing else.
    let (mut fixture, handle) = skinned_scene(device, queue, Some(skinned_box(CAMERA_ONLY)));
    let in_shadows = fixture
        .renderer
        .skin_shadow_reach
        .iter()
        .any(|frustum| frustum.intersects(&skinned_box(CAMERA_ONLY)));
    assert!(
        !in_shadows,
        "the box is meant to be out of every shadow cull, so the camera alone keeps it"
    );
    let drawn = primary_frame(&recorder, device, queue, &mut fixture);
    assert_eq!(
        (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
        (1, 0),
        "an object only the camera sees was not skinned"
    );

    // A light's cull and not the camera's: the spot's cone, straight below
    // its lamp, which is out of the default camera's sideways view.
    let in_spot = skinned_box(Vec3::new(SPOT, -4.0, 0.0));
    fixture
        .renderer
        .set_skinned_bounds(handle, Some(in_spot))
        .expect("a finite, ordered box");
    let drawn = primary_frame(&recorder, device, queue, &mut fixture);
    let camera = Frustum::from_view_projection(fixture.renderer.primary.camera_view_proj);
    assert!(
        !camera.intersects(&in_spot),
        "the box is meant to be out of the camera's frustum, so a shadow alone keeps it"
    );
    assert_eq!(
        (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
        (1, 0),
        "an object only a shadow cull reaches was not skinned"
    );
    fixture.destroy(device);

    // A secondary view's frustum and nothing else: the box nothing of the
    // primary frame reaches, with a second camera turned round to face it.
    let (mut fixture, _) = skinned_scene(device, queue, Some(skinned_box(UNSEEN)));
    let view = fixture
        .renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("the null backend accepts every descriptor");
    let facing = Camera {
        eye: UNSEEN + Vec3::new(0.0, 0.0, 10.0),
        target: UNSEEN,
        ..Camera::default()
    };
    let drawn = skinned_frame(
        &recorder,
        device,
        queue,
        &mut fixture,
        Some((view, &facing)),
    );
    assert_eq!(
        (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
        (1, 0),
        "an object only a secondary view sees was not skinned"
    );
    let drawn = primary_frame(&recorder, device, queue, &mut fixture);
    assert_eq!(
        (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
        (0, 1),
        "a view the frame does not record keeps nothing"
    );
    fixture.renderer.destroy_view(device, view);
    fixture.destroy(device);
    recorder.assert_valid();
}

/// **An object with no box is skinned wherever it is**, as it always was.
#[test]
fn an_unboxed_object_is_always_skinned() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut fixture = SkinnedFixture::build(device, queue);
    fixture
        .renderer
        .add_skinned_instance(&SkinnedInstanceDesc {
            mesh: &fixture.skinned,
            material: DEMO_UNTINTED,
            transform: Mat4::from_translation(UNSEEN),
            bounds: None,
        })
        .expect("room for one object");

    for frame in 0..3 {
        let drawn = primary_frame(&recorder, device, queue, &mut fixture);
        assert_eq!(
            (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
            (1, 0),
            "frame {frame}: an object with no box was left unskinned"
        );
    }

    fixture.destroy(device);
    recorder.assert_valid();
}

/// **An object back in view after being skipped is at rest for one frame**,
/// then subtracts against the half its return filled.
///
/// Its halves hold poses from before the skip, so naming the other half on
/// the frame it returns is a motion vector against a pose frames old. Read
/// out of the instance ring the frame draws from.
#[test]
fn an_object_back_in_view_is_at_rest_for_one_frame() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let (mut fixture, handle) = skinned_scene(device, queue, Some(skinned_box(Vec3::ZERO)));

    // Seen, so it is subtracting against the other half like any skinned
    // object — the state the skip has to take it out of.
    primary_frame(&recorder, device, queue, &mut fixture);
    fixture.begin(device);
    let seen = instance_record(&recorder, &fixture.renderer, handle);
    assert_ne!(
        seen.previous_base_vertex, seen.base_vertex,
        "a skinned object in view subtracts against the other half"
    );

    fixture
        .renderer
        .set_skinned_bounds(handle, Some(skinned_box(UNSEEN)))
        .expect("a finite, ordered box");
    for _ in 0..2 {
        let drawn = primary_frame(&recorder, device, queue, &mut fixture);
        assert_eq!(drawn.dispatches, 0, "the object out of view is skipped");
    }

    fixture
        .renderer
        .set_skinned_bounds(handle, Some(skinned_box(Vec3::ZERO)))
        .expect("a finite, ordered box");
    fixture.begin(device);
    let back = instance_record(&recorder, &fixture.renderer, handle);
    let parity = fixture.skinning.parity();
    assert_eq!(
        back.base_vertex,
        fixture.skinned.region().base(parity),
        "the frame it returns on draws the half this frame fills"
    );
    assert_eq!(
        back.previous_base_vertex, back.base_vertex,
        "the frame it returns on must be at rest: the other half holds a pose from before \
         the skip"
    );
    let drawn = record_begun(&recorder, device, queue, &mut fixture);
    assert_eq!(drawn, 1, "and it is skinned on that frame");

    fixture.begin(device);
    let after = instance_record(&recorder, &fixture.renderer, handle);
    let parity = fixture.skinning.parity();
    assert_eq!(
        after.previous_base_vertex,
        fixture.skinned.region().previous_base(parity),
        "the frame after its return subtracts against the half the return filled"
    );
    assert_ne!(after.previous_base_vertex, after.base_vertex);

    fixture.destroy(device);
    recorder.assert_valid();
}

/// Records and executes the frame `fixture` has already begun, and answers its
/// skinning dispatches — [`skinned_frame`] without the begin, for a test that
/// reads the record between the two.
fn record_begun(
    recorder: &Recorder,
    device: &dyn Device,
    queue: QueueHandle,
    fixture: &mut SkinnedFixture,
) -> usize {
    let before = skinning_dispatches(recorder);
    let imported = swapchain_image(device);
    let mut graph = crate::RenderGraph::new(queue);
    let target = graph.import_image("target", imported);
    let mut pool = crate::TransientPool::new();
    fixture
        .renderer
        .add_skinned_passes(&mut graph, &pool, target, TEST_EXTENT, &fixture.skinning);
    let compiled = graph.compile(&pool).expect("a legal frame");
    let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
        label: Some("skinning cull"),
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
    skinning_dispatches(recorder) - before
}

/// **A skipped object that has just left a light still redraws that light's
/// map**, and the frame after it holds the map: the re-point is still an
/// instance write, and the box it left is that write's *before*.
#[test]
fn a_skipped_object_leaving_a_light_redraws_its_map() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let in_spot = skinned_box(Vec3::new(SPOT, -4.0, 0.0));
    let (mut fixture, handle) = skinned_scene(device, queue, Some(in_spot));
    for _ in 0..2 {
        let drawn = primary_frame(&recorder, device, queue, &mut fixture);
        assert_eq!(
            (drawn.dispatches, drawn.redrawn[2]),
            (1, true),
            "in the spot's cone the object is skinned and redraws the spot"
        );
    }

    fixture
        .renderer
        .set_skinned_bounds(handle, Some(skinned_box(UNSEEN)))
        .expect("a finite, ordered box");
    let left = primary_frame(&recorder, device, queue, &mut fixture);
    assert_eq!(
        (left.dispatches, left.redrawn[2]),
        (0, true),
        "the frame the box left the spot: skipped, and the spot's map redrawn without it"
    );
    let after = primary_frame(&recorder, device, queue, &mut fixture);
    assert_eq!(
        (after.dispatches, after.redrawn),
        (0, [false; 4]),
        "the frame after: still skipped, and nothing it can reach to redraw"
    );

    fixture.destroy(device);
    recorder.assert_valid();
}

/// **A region one seen object and one unseen object are drawn out of is
/// skinned**: the seen one reads it. And with both out of view it is not, so
/// the first half is not a region the cull could never skip.
#[test]
fn a_region_shared_by_a_seen_and_an_unseen_object_is_skinned() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let (mut fixture, seen) = skinned_scene(device, queue, Some(skinned_box(Vec3::ZERO)));
    fixture
        .renderer
        .add_skinned_instance(&SkinnedInstanceDesc {
            mesh: &fixture.skinned,
            material: DEMO_UNTINTED,
            transform: Mat4::IDENTITY,
            bounds: Some(skinned_box(UNSEEN)),
        })
        .expect("room for a second object");

    for frame in 0..2 {
        let drawn = primary_frame(&recorder, device, queue, &mut fixture);
        assert_eq!(
            (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
            (1, 0),
            "frame {frame}: a region the camera sees one of its objects through was not \
             skinned"
        );
    }

    fixture
        .renderer
        .set_skinned_bounds(seen, Some(skinned_box(UNSEEN)))
        .expect("a finite, ordered box");
    let drawn = primary_frame(&recorder, device, queue, &mut fixture);
    assert_eq!(
        (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
        (0, 1),
        "with both of its objects out of view the region is skipped"
    );

    fixture.destroy(device);
    recorder.assert_valid();
}

/// **A shadow group the cadence holds keeps nothing skinned**: it draws no
/// vertex this frame. An object only the spot reaches is skinned on exactly
/// the frames the spot's map is redrawn.
///
/// A cube moved inside the point light every frame keeps the point light's
/// six faces asking beside the spot's one, and the budget is pinned to six
/// faces: the two cannot both be redrawn, so the cadence's rotation redraws
/// the spot on some frames and holds it on others. Both kinds of frame have to
/// occur, or the test is about only one of them.
#[test]
fn an_object_only_a_held_shadow_reaches_is_not_skinned() {
    const FRAMES: u16 = 8;
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut fixture = far_spot_scene(
        &recorder,
        device,
        queue,
        &crate::scene::demo(),
        EffectRequest::default(),
    );
    let in_point = Vec3::new(super::shadow_reach::POINT, -2.0, 0.0);
    let cube = place_cube(&mut fixture.renderer, Mat4::from_translation(in_point));
    fixture.renderer.set_shadow_cadence(Some(shadow::Cadence {
        hold: 1,
        faces: shadow::POINT_FACES,
    }));

    let mut kinds = [false; 2];
    for frame in 0..FRAMES {
        let step = Vec3::new(0.0, 0.0, 0.01 * f32::from(frame + 1));
        fixture.renderer.set_instance(
            cube,
            &InstanceDesc {
                mesh: DEMO_CUBE,
                material: DEMO_UNTINTED,
                transform: Mat4::from_translation(in_point + step),
            },
        );
        let drawn = primary_frame(&recorder, device, queue, &mut fixture);
        let spot = drawn.redrawn[2 + spot_slot(&fixture.renderer)];
        kinds[usize::from(spot)] = true;
        assert_eq!(
            drawn.dispatches,
            usize::from(spot),
            "frame {frame}: the spot was {} and the object only it reaches was skinned {} \
             times",
            if spot { "redrawn" } else { "held" },
            drawn.dispatches
        );
    }
    assert_eq!(
        kinds, [true; 2],
        "the spot must be both held and redrawn across the run (held, redrawn)"
    );

    fixture.destroy(device);
    recorder.assert_valid();
}

/// **The shadow culls a frame fits are that frame's alone**: an object only a
/// light reached is skipped on the first frame after the light is gone.
#[test]
fn a_light_gone_since_the_last_frame_keeps_nothing_skinned() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut fixture = far_spot_scene(
        &recorder,
        device,
        queue,
        &crate::scene::demo(),
        EffectRequest::default(),
    );
    fixture
        .renderer
        .set_lights(&[shadowable_point(super::shadow_reach::POINT)]);
    let drawn = primary_frame(&recorder, device, queue, &mut fixture);
    assert_eq!(
        (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
        (0, 1),
        "the spot is gone, so an object only it reached must be skipped"
    );

    fixture.destroy(device);
    recorder.assert_valid();
}

/// **With shadows off, the probe updater's light culls keep an object
/// skinned**: its punctual producer draws the spot's face for the bounce. And
/// on a renderer whose probes are not updated the same frame skips it, so it
/// is the updater that keeps it.
#[test]
fn an_object_only_the_probe_updater_reaches_is_skinned() {
    let _blurs = ssao_blur_switch();
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let shadows_off = EffectRequest {
        programmatic: EffectOverride::none().force(RenderEffects::SHADOWS, Some(false)),
        ..EffectRequest::default()
    };

    for updated in [false, true] {
        let mut scene = crate::scene::demo();
        if updated {
            scene.capacities.probes = 1;
            scene.probes = crate::scene::ProbeGrid {
                volume: crcbl_shaders::probe::ProbeVolume {
                    origin: [0.0; 3],
                    inv_spacing: [1.0; 3],
                    counts: [1, 1, 1],
                    levels: 1,
                    steps: crcbl_shaders::probe::ProbeSteps::default(),
                },
                probes: vec![crcbl_shaders::probe::GpuProbe::ZERO],
                update: ProbeUpdate::EveryFrame,
            };
        }
        let mut fixture = far_spot_scene(&recorder, device, queue, &scene, shadows_off);
        for frame in 0..2 {
            let drawn = primary_frame(&recorder, device, queue, &mut fixture);
            assert!(
                !fixture
                    .renderer
                    .frame_effects
                    .contains(RenderEffects::SHADOWS),
                "the frame is meant to draw with shadows off"
            );
            assert_eq!(
                fixture.renderer.probe_update_runs(),
                updated,
                "the probe updater runs exactly where the scene asks for it"
            );
            assert_eq!(
                (drawn.dispatches, fixture.renderer.skinned_regions_skipped()),
                if updated { (1, 0) } else { (0, 1) },
                "frame {frame}, probes updated {updated}: with shadows off only the \
                 updater's spot cull reaches the object"
            );
        }
        fixture.destroy(device);
    }
    recorder.assert_valid();
}

/// Where [`far_spot_scene`] hangs its spot: far enough along `-x` that
/// neither cascade reaches below it, which [`SPOT`] is not.
const FAR_SPOT: f32 = -40.0;

/// Below [`FAR_SPOT`]'s lamp: inside its cone and, [`only_the_spot_reaches`]
/// checks, no other cull's.
const IN_SPOT_ONLY: Vec3 = Vec3::new(FAR_SPOT, -4.0, 0.0);

/// [`skinned_scene`] with the spot at [`FAR_SPOT`], built over `scene`, and
/// one object at [`IN_SPOT_ONLY`] — through its first frame, which redraws
/// every group, so [`only_the_spot_reaches`] reads all four culls. The cadence
/// is pinned on `skinned_scene`'s terms, and the frame's effects are
/// `effects`.
fn far_spot_scene(
    recorder: &Recorder,
    device: &dyn Device,
    queue: QueueHandle,
    scene: &crate::scene::SceneDesc<'_>,
    effects: EffectRequest,
) -> SkinnedFixture {
    let mut fixture = SkinnedFixture::build_with_scene(device, queue, scene);
    fixture
        .renderer
        .set_shadow_cadence(Some(shadow::Cadence::EVERY_FRAME));
    fixture.renderer.set_lights(&[
        shadowable_spot(FAR_SPOT),
        shadowable_point(super::shadow_reach::POINT),
    ]);
    fixture.renderer.set_effect_request(effects);
    fixture
        .renderer
        .add_skinned_instance(&SkinnedInstanceDesc {
            mesh: &fixture.skinned,
            material: DEMO_UNTINTED,
            transform: Mat4::IDENTITY,
            bounds: Some(skinned_box(IN_SPOT_ONLY)),
        })
        .expect("room for one object");
    let first = primary_frame(recorder, device, queue, &mut fixture);
    if fixture
        .renderer
        .frame_effects
        .contains(RenderEffects::SHADOWS)
    {
        assert_eq!(
            first,
            Drawn {
                dispatches: 1,
                redrawn: [true; 4],
            },
            "the first frame of all draws every group, and the spot keeps the object"
        );
        only_the_spot_reaches(&fixture.renderer, IN_SPOT_ONLY);
    }
    fixture
}

/// The light slot [`far_spot_scene`]'s spot — its first light — holds.
fn spot_slot(renderer: &ForwardRenderer) -> usize {
    renderer
        .shadow_lights()
        .slots()
        .iter()
        .position(|held| held.is_some_and(|held| held.light == 0))
        .expect("the spot holds a slot")
}

/// Asserts that of the culls a frame that redrew every group fitted — the
/// camera's, both cascades' and the two light slots', in that order — only the
/// spot's reaches a box at `centre`.
fn only_the_spot_reaches(renderer: &ForwardRenderer, centre: Vec3) {
    let bounds = skinned_box(centre);
    let mut wanted = [false; 5];
    wanted[3 + spot_slot(renderer)] = true;
    let reached: Vec<bool> = core::iter::once(Frustum::from_view_projection(
        renderer.primary.camera_view_proj,
    ))
    .chain(renderer.skin_shadow_reach.iter().copied())
    .map(|frustum| frustum.intersects(&bounds))
    .collect();
    assert_eq!(
        reached, wanted,
        "camera, cascade 0, cascade 1, slot 0, slot 1: the spot's cull alone is meant to \
         reach {centre}"
    );
}
