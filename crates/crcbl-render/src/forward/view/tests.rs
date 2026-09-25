use super::*;
use crate::forward::tests::{
    open, place_cube, ssao_blur_switch, ssao_split_switch, swapchain_image_at,
};

/// A renderer over the demo scene, and a view of it that asks for every effect.
fn renderer_with_view(device: &dyn Device, queue: QueueHandle) -> (ForwardRenderer, ViewId) {
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let view = renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("the null backend accepts every descriptor");
    (renderer, view)
}

/// Every pass label of a frame that draws the primary camera into a target at
/// `extent` and, when `view` is `Some`, that view into a 512 × 512 one.
fn frame_labels(
    device: &dyn Device,
    queue: QueueHandle,
    renderer: &mut ForwardRenderer,
    view: Option<ViewId>,
) -> Vec<String> {
    const EXTENT: (u32, u32) = (256, 192);
    const VIEW_EXTENT: (u32, u32) = (512, 512);
    renderer
        .begin_frame(
            device,
            &Camera::default(),
            &DirectionalLight::default(),
            EXTENT,
        )
        .expect("write");
    if let Some(view) = view {
        renderer
            .begin_view(device, view, &Camera::default(), VIEW_EXTENT)
            .expect("write");
    }
    let pool = crate::TransientPool::new();
    let mut graph = RenderGraph::new(queue);
    let target = graph.import_image("target", swapchain_image_at(device, EXTENT));
    let targets: Vec<ViewTarget> = view
        .map(|view| ViewTarget {
            view,
            target: graph.import_image("view target", swapchain_image_at(device, VIEW_EXTENT)),
            extent: VIEW_EXTENT,
        })
        .into_iter()
        .collect();
    renderer.add_passes_with_views(
        &mut graph,
        &pool,
        FrameTargets {
            target,
            extent: EXTENT,
            skinning: None,
            views: &targets,
        },
        |_, _| {},
    );
    graph
        .compile(&pool)
        .expect("a legal frame")
        .passes()
        .iter()
        .map(|pass| pass.label().to_string())
        .collect()
}

#[test]
fn a_view_is_released_by_destroy_view_and_by_the_renderer_and_leaks_nothing() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let before = recorder.total_live_objects();
    let (mut renderer, view) = renderer_with_view(device, queue);
    let with_view = recorder.total_live_objects();

    renderer.destroy_view(device, view);
    let without = recorder.total_live_objects();
    assert!(
        without < with_view,
        "releasing the view must release what it built"
    );
    let again = renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("built");
    assert_eq!(again, view, "a released id is the next one handed out");
    assert_eq!(
        recorder.total_live_objects(),
        with_view,
        "and a view costs the same every time it is built"
    );

    renderer.destroy(device);
    assert_eq!(
        recorder.total_live_objects(),
        before,
        "the renderer releases a live view with everything else"
    );
    recorder.assert_valid();
}

#[test]
fn a_renderer_draws_at_most_max_views_and_reuses_the_lowest_free_id() {
    let (_, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let views: Vec<ViewId> = (1..MAX_VIEWS)
        .map(|_| {
            renderer
                .create_view(device, queue, &ViewDesc::default())
                .expect("room for every view but the primary")
        })
        .collect();
    assert_eq!(
        views.iter().map(|view| view.index()).collect::<Vec<_>>(),
        (1..MAX_VIEWS).collect::<Vec<_>>(),
        "ids are handed out in order, after the primary camera's"
    );
    assert!(
        matches!(
            renderer.create_view(device, queue, &ViewDesc::default()),
            Err(HalError::InvalidDescriptor(_))
        ),
        "a view past MAX_VIEWS has no bit to cull on and is refused"
    );
    renderer.destroy_view(device, views[2]);
    assert_eq!(
        renderer
            .create_view(device, queue, &ViewDesc::default())
            .expect("a freed id"),
        views[2]
    );
    renderer.destroy(device);
}

/// A view's frame block is its own camera's, and the shadow matrices in it are
/// the frame's — the ones the primary camera's block carries.
#[test]
fn a_view_writes_its_own_camera_and_samples_the_frames_shadow_maps() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, view) = renderer_with_view(device, queue);
    let primary_camera = Camera::default();
    let view_camera = Camera {
        eye: Vec3::new(4.0, 2.0, -3.0),
        ..Camera::default()
    };
    renderer
        .begin_frame(
            device,
            &primary_camera,
            &DirectionalLight::default(),
            (256, 192),
        )
        .expect("write");
    renderer
        .begin_view(device, view, &view_camera, (512, 512))
        .expect("write");

    let slot = renderer.frame;
    let built = renderer.views[view.index() - 1].as_ref().expect("built");
    let primary_block = recorder
        .buffer_bytes(renderer.primary.uniforms[slot])
        .expect("live");
    let view_block = recorder.buffer_bytes(built.uniforms[slot]).expect("live");
    let matrix_bytes = |matrix: Mat4| -> Vec<u8> {
        matrix
            .to_cols_array()
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    };
    assert_eq!(
        view_block[..64],
        matrix_bytes(view_camera.view_projection(1.0))[..],
        "the view's block carries its own camera at its own target's aspect"
    );
    assert_eq!(
        primary_block[..64],
        matrix_bytes(primary_camera.view_projection(256.0 / 192.0))[..],
        "and the primary camera's block is untouched by it"
    );
    // `view_proj`, `camera_position` and `ambient` come first — see
    // `mesh::FrameUniforms::to_bytes` — and the cascades' matrices follow,
    // fitted to the primary camera and not to the view's.
    let cascades = 96..96 + 64 * shadow::CASCADES;
    let fitted: Vec<u8> = Cascades::new(
        &primary_camera,
        DirectionalLight::default().direction.normalize_or_zero(),
    )
    .view_proj
    .iter()
    .flat_map(|matrix| matrix_bytes(*matrix))
    .collect();
    assert_eq!(
        primary_block[cascades.clone()],
        fitted[..],
        "the primary camera's block carries the cascades fitted to it"
    );
    assert_eq!(
        view_block[cascades],
        fitted[..],
        "and the view samples those same maps, through the same matrices"
    );

    let cull = recorder
        .buffer_bytes(renderer.view_draws(view).cull_params(slot))
        .expect("live");
    assert_eq!(
        u32::from_le_bytes(cull[104..108].try_into().expect("4")),
        1 << (mesh::GpuInstance::HIDDEN_VIEWS_SHIFT + 1),
        "the first secondary view's cull rejects on bit 9 of the flags word"
    );
    renderer.destroy(device);
}

/// A frame with a view records the scene's passes once and the view's frame
/// whole, before the primary camera draws anything.
#[test]
fn a_view_records_its_own_frame_and_the_scenes_passes_run_once() {
    let (_, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, view) = renderer_with_view(device, queue);
    place_cube(&mut renderer, Mat4::IDENTITY);

    let alone = frame_labels(device, queue, &mut renderer, None);
    let with_view = frame_labels(device, queue, &mut renderer, Some(view));
    let count =
        |labels: &[String], label: &str| labels.iter().filter(|each| *each == label).count();

    for label in ["forward", "depth-prepass", "tonemap", "light-cluster"] {
        assert_eq!(
            count(&with_view, label),
            2 * count(&alone, label),
            "the view records its own `{label}`"
        );
    }
    for label in alone
        .iter()
        .filter(|label| label.starts_with("shadow") || label.starts_with("probe"))
    {
        assert_eq!(
            count(&with_view, label),
            count(&alone, label),
            "`{label}` is the scene's and runs once whatever draws it"
        );
    }
    let first = |label: &str| {
        with_view
            .iter()
            .position(|each| each == label)
            .expect("recorded")
    };
    let last = |label: &str| {
        with_view
            .iter()
            .rposition(|each| each == label)
            .expect("recorded")
    };
    assert!(
        last("tonemap") > first("tonemap") && first("tonemap") < last("depth-prepass"),
        "the view's frame is finished before the primary camera's prepass: {with_view:?}"
    );
    renderer.destroy(device);
}

/// **Water is drawn in every view, and a renderer with no bodies records none
/// of it** — [`crate::water`]'s off position, as the passes a frame records.
///
/// The frame before any body and the frame after the bodies are removed record
/// the same list, label for label: no pass, and nothing moved to make room for
/// one.
#[test]
fn every_view_draws_the_water_and_no_body_records_no_pass() {
    // Keep process-wide SSAO settings fixed across the compared frames.
    let _blurs = ssao_blur_switch();
    let _split = ssao_split_switch();
    let (_, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, view) = renderer_with_view(device, queue);
    place_cube(&mut renderer, Mat4::IDENTITY);
    let count =
        |labels: &[String], label: &str| labels.iter().filter(|each| *each == label).count();

    let dry = frame_labels(device, queue, &mut renderer, Some(view));
    assert_eq!(count(&dry, "water-copy") + count(&dry, "water"), 0);

    renderer
        .set_water(&[crcbl_water::WaterBody {
            outline: vec![[-2.0, -2.0], [2.0, -2.0], [2.0, 2.0], [-2.0, 2.0]],
            level: 0.25,
            medium: crcbl_water::Medium {
                absorption: [0.4, 0.1, 0.05],
                scattering: [0.01, 0.01, 0.01],
            },
        }])
        .expect("a square meshes");
    let alone = frame_labels(device, queue, &mut renderer, None);
    let wet = frame_labels(device, queue, &mut renderer, Some(view));
    for label in ["water-copy", "water"] {
        assert_eq!(
            count(&alone, label),
            1,
            "the primary camera records `{label}`"
        );
        assert_eq!(count(&wet, label), 2, "the view records its own `{label}`");
    }

    renderer.set_water(&[]).expect("an empty set is a set");
    // Two frames, so both slots of the ring have come round since the bodies
    // were removed.
    frame_labels(device, queue, &mut renderer, Some(view));
    let removed = frame_labels(device, queue, &mut renderer, Some(view));
    assert_eq!(
        removed, dry,
        "removing the water left the frame a different shape"
    );
    renderer.destroy(device);
}

/// **Grass is generated and drawn in every view, and a renderer with no field
/// records none of it** — [`crate::grass`]'s off position, as the passes a frame
/// records.
///
/// The frame before any field and the frame after the field is removed record
/// the same list, label for label: no pass, and nothing moved to make room for
/// one.
#[test]
fn every_view_draws_the_grass_and_no_field_records_no_pass() {
    // Keep process-wide SSAO settings fixed across the compared frames.
    let _blurs = ssao_blur_switch();
    let _split = ssao_split_switch();
    let (_, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, view) = renderer_with_view(device, queue);
    place_cube(&mut renderer, Mat4::IDENTITY);
    let count =
        |labels: &[String], label: &str| labels.iter().filter(|each| *each == label).count();
    const LABELS: [&str; 3] = ["grass-clear", "grass-generate", "grass"];

    let bare = frame_labels(device, queue, &mut renderer, Some(view));
    assert_eq!(
        LABELS
            .iter()
            .map(|label| count(&bare, label))
            .sum::<usize>(),
        0
    );

    let field = crate::grass::GrassField::new(
        [1, 1],
        4.0,
        [-2.0, -2.0],
        50.0,
        crate::grass::Heightfield {
            texels: [4, 4],
            metres_per_texel: 1.0,
            origin: [-2.0, -2.0],
            heights: vec![0.0; 16],
        },
        crate::grass::CoverMap {
            texels: [4, 4],
            metres_per_texel: 1.0,
            origin: [-2.0, -2.0],
            cover: vec![[255, 0]; 16],
        },
        vec![crate::grass::BladeType {
            root_color: [0.05, 0.1, 0.02],
            tip_color: [0.3, 0.5, 0.1],
            height: 0.3,
            half_width: 0.03,
            height_spread: 0.3,
            width_spread: 0.2,
            look: crate::grass::BladeLook::Cards,
            style: crate::grass::BladeStyle::PLAIN,
            shape: crate::grass::BladeShape::STRAIGHT,
            clumping: crate::grass::Clumping::NONE,
        }],
    )
    .expect("a real field");
    renderer
        .set_grass(device, queue, Some(&field))
        .expect("the null backend uploads every map");
    let alone = frame_labels(device, queue, &mut renderer, None);
    let grown = frame_labels(device, queue, &mut renderer, Some(view));
    for label in LABELS {
        assert_eq!(
            count(&alone, label),
            1,
            "the primary camera records `{label}`"
        );
        assert_eq!(
            count(&grown, label),
            2,
            "the view records its own `{label}`"
        );
    }
    // And the three are in the order the module's header draws them.
    let at = |label: &str| {
        grown
            .iter()
            .position(|each| each == label)
            .unwrap_or_else(|| panic!("`{label}` is in the frame"))
    };
    assert!(at("grass-clear") < at("grass-generate"));
    assert!(at("grass-generate") < at("grass"));
    assert!(
        at("forward") < at("grass"),
        "the cards must draw over the opaque frame, not under it"
    );

    renderer
        .set_grass(device, queue, None)
        .expect("removing is a set");
    // Two frames, so both slots of the ring have come round since the field was
    // removed.
    frame_labels(device, queue, &mut renderer, Some(view));
    let removed = frame_labels(device, queue, &mut renderer, Some(view));
    assert_eq!(
        removed, bare,
        "removing the grass left the frame a different shape"
    );
    renderer.destroy(device);
}

/// A view's visibility reaches the instance record, survives the caller
/// rewriting the object, and is handed back when the view is released.
#[test]
fn an_instances_views_survive_a_rewrite_and_reset_with_the_view() {
    let (_, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, view) = renderer_with_view(device, queue);
    let cube = place_cube(&mut renderer, Mat4::IDENTITY);
    assert_eq!(renderer.instance_views(cube), Some(ViewMask::ALL));

    let hidden = ViewMask::ALL.without(view);
    renderer.set_instance_views(cube, hidden);
    assert_eq!(renderer.instance_views(cube), Some(hidden));
    let record = renderer.instances.get(cube).expect("live");
    assert_eq!(
        record.flags & mesh::GpuInstance::HIDDEN_VIEWS_MASK,
        1 << (mesh::GpuInstance::HIDDEN_VIEWS_SHIFT + 1),
        "the one bit its view culls on, and no other"
    );

    renderer.set_instance(
        cube,
        &crate::scene::InstanceDesc {
            mesh: crate::scene::DEMO_CUBE,
            material: crate::scene::DEMO_UNTINTED,
            transform: Mat4::from_translation(Vec3::X),
        },
    );
    assert_eq!(
        renderer.instance_views(cube),
        Some(hidden),
        "moving an object does not put it back in the view it was hidden from"
    );

    renderer.destroy_view(device, view);
    assert_eq!(
        renderer.instance_views(cube),
        Some(ViewMask::ALL),
        "a released view's bit is cleared, so the next view under its id starts from ALL"
    );
    renderer.destroy(device);
}

#[test]
#[should_panic(expected = "was not begun this frame")]
fn a_view_nothing_began_this_frame_is_refused() {
    let (_, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, view) = renderer_with_view(device, queue);
    renderer
        .begin_frame(
            device,
            &Camera::default(),
            &DirectionalLight::default(),
            (64, 48),
        )
        .expect("write");
    let pool = crate::TransientPool::new();
    let mut graph = RenderGraph::new(queue);
    let target = graph.import_image("target", swapchain_image_at(device, (64, 48)));
    let view_target = graph.import_image("view", swapchain_image_at(device, (64, 64)));
    renderer.add_passes_with_views(
        &mut graph,
        &pool,
        FrameTargets {
            target,
            extent: (64, 48),
            skinning: None,
            views: &[ViewTarget {
                view,
                target: view_target,
                extent: (64, 64),
            }],
        },
        |_, _| {},
    );
}

/// **A transparent view takes neither antialiasing tier**, alone or together,
/// and is refused by name rather than drawn with a fringe; the description
/// [`ViewDesc::transparent`] hands out is one it accepts.
#[test]
fn a_transparent_view_refuses_both_antialiasing_tiers() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let before = recorder.total_live_objects();
    for tier in [
        RenderEffects::ANTIALIASING,
        RenderEffects::CMAA2,
        ViewBackground::REFUSED_ON_TRANSPARENT,
    ] {
        let refused = renderer.create_view(
            device,
            queue,
            &ViewDesc {
                effects: ViewDesc::transparent().effects.union(tier),
                background: ViewBackground::Transparent,
                lighting: ViewLighting::Scene,
            },
        );
        assert!(
            matches!(&refused, Err(HalError::InvalidDescriptor(message))
                if message.contains("transparent view")),
            "{tier:?}: {refused:?}"
        );
    }
    assert_eq!(
        recorder.total_live_objects(),
        before,
        "a refused view builds nothing"
    );
    assert!(
        ViewDesc::transparent()
            .effects
            .intersection(ViewBackground::REFUSED_ON_TRANSPARENT)
            .is_empty()
    );
    renderer
        .create_view(device, queue, &ViewDesc::transparent())
        .expect("the transparent description is one a transparent view takes");
    renderer
        .create_view(
            device,
            queue,
            &ViewDesc {
                effects: RenderEffects::all(),
                background: ViewBackground::Scene,
                lighting: ViewLighting::Scene,
            },
        )
        .expect("and an opaque view still takes every effect");
    renderer.destroy(device);
}

/// **A transparent view draws no sky, no resolve and no upscale, and asks the
/// tonemap for coverage** — while the primary camera in the same frame draws
/// all three and writes the opaque alpha it always wrote.
///
/// The sky is set and the render scale halved so that each of the three is a
/// pass the primary camera does record: a view that skipped them only because
/// the frame had none would pass nothing.
#[test]
fn a_transparent_view_draws_no_sky_resolve_or_upscale_and_writes_coverage() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    place_cube(&mut renderer, Mat4::IDENTITY);
    renderer.set_sky(crate::camera::Sky {
        zenith: Vec3::new(0.2, 0.3, 0.6),
        horizon: Vec3::new(0.5, 0.5, 0.5),
        ground: Vec3::new(0.1, 0.1, 0.1),
    });
    renderer.set_render_scale(0.5);
    renderer.set_effect_request(crate::effects::EffectRequest {
        camera: RenderEffects::DEFAULT_STACK.union(RenderEffects::CMAA2),
        ..crate::effects::EffectRequest::default()
    });
    let view = renderer
        .create_view(device, queue, &ViewDesc::transparent())
        .expect("built");
    let count =
        |labels: &[String], label: &str| labels.iter().filter(|each| *each == label).count();

    let alone = frame_labels(device, queue, &mut renderer, None);
    let with_view = frame_labels(device, queue, &mut renderer, Some(view));
    for label in ["sky", "cmaa2-apply", "upscale"] {
        assert_eq!(
            count(&alone, label),
            1,
            "the primary camera records `{label}`"
        );
        assert_eq!(
            count(&with_view, label),
            1,
            "and the transparent view records no `{label}` of its own"
        );
    }
    assert_eq!(
        count(&with_view, "tonemap"),
        2,
        "the view still tonemaps its own frame"
    );

    let slot = renderer.frame;
    let lane = |block: BufferHandle| {
        let bytes = recorder.buffer_bytes(block).expect("begin_frame wrote it");
        u32::from_le_bytes(bytes[12..16].try_into().expect("4"))
    };
    let built = renderer.views[view.index() - 1].as_ref().expect("built");
    assert_eq!(
        lane(built.tonemap_uniforms[slot]),
        1,
        "the view's tonemap writes coverage"
    );
    assert_eq!(
        lane(renderer.primary.tonemap_uniforms[slot]),
        0,
        "and the primary camera's the opaque alpha"
    );
    renderer.destroy(device);
}

/// **Every shadow cull rejects on the no-shadow bit and every camera cull on
/// its own view bit**, so an instance told not to cast is dropped from each
/// cascade's and each light's survivors and from no camera's.
#[test]
fn every_shadow_cull_rejects_on_the_no_shadow_bit_and_no_camera_cull_does() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, view) = renderer_with_view(device, queue);
    // A shadowed spot and a shadowed point light beside the sun, so the light
    // slots' culls — the point light's six-face one included — run this frame
    // alongside the cascades'.
    renderer.set_lights(&[
        crate::light::Light::Spot(crate::light::SpotLight {
            position: Vec3::new(0.0, 3.0, 0.0),
            radius: 6.0,
            color: Vec3::ONE,
            direction: Vec3::NEG_Y,
            inner_angle: 0.2,
            outer_angle: 0.4,
            fill: false,
        }),
        crate::light::Light::Point(crate::light::PointLight {
            position: Vec3::new(2.0, 1.0, 0.0),
            radius: 4.0,
            color: Vec3::ONE,
            fill: false,
        }),
    ]);
    renderer
        .begin_frame(
            device,
            &Camera::default(),
            &DirectionalLight::default(),
            (64, 48),
        )
        .expect("write");
    renderer
        .begin_view(device, view, &Camera::default(), (64, 64))
        .expect("write");
    let slot = renderer.frame;
    let hidden_view = |draws: &DrawGen| {
        let bytes = recorder
            .buffer_bytes(draws.cull_params(slot))
            .expect("live");
        u32::from_le_bytes(bytes[104..108].try_into().expect("4"))
    };
    // A slot no light holds this frame writes no parameters and dispatches no
    // cull, so its block is still the zeroes it was created with; every cull
    // the frame does run carries the bit.
    let written: Vec<(usize, u32)> = renderer
        .shadow_draws
        .iter()
        .map(hidden_view)
        .enumerate()
        .filter(|(_, bit)| *bit != 0)
        .collect();
    assert!(
        written.len() >= shadow::CASCADES + 2,
        "every cascade and both lights cull this frame: {written:?}"
    );
    for (index, bit) in written {
        assert_eq!(
            bit,
            mesh::GpuInstance::CASTS_NO_SHADOW,
            "shadow cull {index}"
        );
    }
    for camera in [ViewId::PRIMARY, view] {
        assert_eq!(
            hidden_view(renderer.view_draws(camera)),
            camera.hidden_bit(),
            "{camera:?} culls on its own bit and not on the no-shadow one"
        );
    }
    renderer.destroy(device);
}

/// **Whether an instance casts is its own, and a rewrite keeps it** — on
/// [`ForwardRenderer::set_instance_views`]' terms: moving an object does not
/// start it casting again, and its views and its shadow are independent bits.
#[test]
fn an_instance_keeps_casting_no_shadow_through_a_rewrite() {
    let (_, device, queue) = open();
    let device = device.as_ref();
    let (mut renderer, view) = renderer_with_view(device, queue);
    let cube = renderer
        .add_instance(&crate::scene::InstanceDesc {
            mesh: crate::scene::DEMO_CUBE,
            material: crate::scene::DEMO_UNTINTED,
            transform: Mat4::IDENTITY,
        })
        .expect("room");
    assert_eq!(
        renderer.instance_casts_shadow(cube),
        Some(true),
        "every instance starts casting"
    );

    renderer.set_instance_casts_shadow(cube, false);
    renderer.set_instance_views(cube, ViewMask::only(view));
    assert_eq!(renderer.instance_casts_shadow(cube), Some(false));
    assert_eq!(
        renderer.instance_views(cube),
        Some(ViewMask::only(view)),
        "the views are a bit field of their own"
    );
    renderer.set_instance(
        cube,
        &crate::scene::InstanceDesc {
            mesh: crate::scene::DEMO_CUBE,
            material: crate::scene::DEMO_UNTINTED,
            transform: Mat4::from_translation(Vec3::X),
        },
    );
    assert_eq!(
        renderer.instance_casts_shadow(cube),
        Some(false),
        "moving an object does not start it casting"
    );
    assert_eq!(renderer.instance_views(cube), Some(ViewMask::only(view)));

    let revision = renderer.instances.revision();
    renderer.set_instance_casts_shadow(cube, true);
    assert_eq!(renderer.instance_casts_shadow(cube), Some(true));
    assert_ne!(
        renderer.instances.revision(),
        revision,
        "a change moves the revision every cached shadow map is keyed on"
    );

    renderer.remove_instance(cube);
    assert_eq!(renderer.instance_casts_shadow(cube), None, "a stale handle");
    renderer.set_instance_casts_shadow(cube, false);
    renderer.destroy(device);
}

/// The key light a fixed view's tests light it with: nothing like
/// [`DirectionalLight::default`] in any of its three terms, so a block carrying
/// the frame's light instead of this one differs in every lane checked.
const KEY: DirectionalLight = DirectionalLight {
    direction: Vec3::new(-0.3, 0.9, 0.2),
    color: Vec3::new(0.7, 0.9, 1.3),
    ambient: Vec3::new(0.05, 0.02, 0.11),
};

/// The environment a fixed view's tests light it with: three distinct
/// channels, none of them the frame's sky's, so a block carrying the frame's
/// sky in its place, or a swapped channel, differs in every lane checked.
const ENVIRONMENT: Vec3 = Vec3::new(0.3, 0.6, 0.9);

/// [`KEY`] and [`ENVIRONMENT`] as a view's lighting.
const FIXED: ViewLighting = ViewLighting::Fixed {
    key: KEY,
    environment: ENVIRONMENT,
};

/// A renderer whose frame carries every term a [`ViewLighting::Fixed`] view
/// ignores — a probe grid, a sky, fog and a point light beside the sun — and
/// asks for every effect that would bring one of them back in.
fn renderer_lit_by_everything(device: &dyn Device, queue: QueueHandle) -> ForwardRenderer {
    let mut desc = crate::scene::demo();
    desc.probes = crate::scene::ProbeGrid {
        volume: crcbl_shaders::probe::ProbeVolume {
            origin: [-1.0, 0.0, -1.0],
            inv_spacing: [0.5, 0.5, 0.5],
            counts: [1, 1, 1],
            levels: 1,
            steps: crcbl_shaders::probe::ProbeSteps::default(),
        },
        update: crate::scene::ProbeUpdate::Authored,
        probes: vec![crcbl_shaders::probe::GpuProbe {
            sh_r: [0.1, 0.2, 0.3, 0.4],
            sh_g: [0.1, 0.2, 0.3, 0.4],
            sh_b: [0.1, 0.2, 0.3, 0.4],
        }],
    };
    desc.capacities.probes = desc.probes.volume.total();
    let mut renderer =
        ForwardRenderer::with_scene(device, queue, Format::Rgba8UnormSrgb, &desc).expect("built");
    place_cube(&mut renderer, Mat4::IDENTITY);
    renderer.set_sky(crate::camera::Sky {
        zenith: Vec3::new(0.2, 0.3, 0.6),
        horizon: Vec3::new(0.5, 0.5, 0.5),
        ground: Vec3::new(0.1, 0.1, 0.1),
    });
    renderer.set_fog(Fog {
        density: 0.05,
        color: Vec3::new(0.4, 0.5, 0.6),
        ..Fog::NONE
    });
    renderer.set_lights(&[crate::light::Light::Point(crate::light::PointLight {
        position: Vec3::new(0.0, 1.0, 0.0),
        radius: 5.0,
        color: Vec3::ONE,
        fill: false,
    })]);
    renderer.set_effect_request(crate::effects::EffectRequest {
        camera: RenderEffects::DEFAULT_STACK.union(ViewLighting::SCENE_EFFECTS),
        ..crate::effects::EffectRequest::default()
    });
    renderer
}

/// **A fixed view's frame block and light list are its key light's and nothing
/// of the frame's**: the fill as the ambient, no sky, no probes, no fog, no
/// shadow map, and one row — the key — where the primary camera's list holds the
/// sun and the point light.
///
/// The primary camera's block, written in the same frame, is the control: it
/// carries every term the view's lacks, so each equality below is a term the
/// view was handed and dropped rather than one the frame never had.
#[test]
fn a_fixed_view_is_lit_by_its_key_light_and_nothing_of_the_frames() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = renderer_lit_by_everything(device, queue);
    let view = renderer
        .create_view(
            device,
            queue,
            &ViewDesc {
                lighting: FIXED,
                ..ViewDesc::transparent()
            },
        )
        .expect("built");
    renderer
        .begin_frame(
            device,
            &Camera::default(),
            &DirectionalLight::default(),
            (256, 192),
        )
        .expect("write");
    renderer
        .begin_view(device, view, &Camera::default(), (128, 128))
        .expect("write");

    let slot = renderer.frame;
    let built = renderer.views[view.index() - 1].as_ref().expect("built");
    let primary = recorder
        .buffer_bytes(renderer.primary.uniforms[slot])
        .expect("live");
    let fixed = recorder.buffer_bytes(built.uniforms[slot]).expect("live");
    let floats = |values: &[f32]| -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    };

    // `ambient` follows `view_proj` and `camera_position`; the probe header,
    // the LOD row and the two fog rows sit in front of the sky's — see
    // `mesh::FrameUniforms::to_bytes`.
    let ambient = 80..92;
    let sky = mesh::SKY_SH_R_OFFSET..mesh::SKY_SH_R_OFFSET + 48;
    let fog = mesh::SKY_SH_R_OFFSET - 32..mesh::SKY_SH_R_OFFSET;
    let probes_at = mesh::SKY_SH_R_OFFSET - 48 - crcbl_shaders::probe::PROBE_VOLUME_SIZE;
    let probes = probes_at..probes_at + crcbl_shaders::probe::PROBE_VOLUME_SIZE;
    // The atlas rectangles are the block's last rows but the filter's one.
    let atlas = mesh::FRAME_UNIFORMS_SIZE - 16 - 16 * shadow::TILES..mesh::FRAME_UNIFORMS_SIZE - 16;
    let zeroes = |range: &std::ops::Range<usize>| vec![0u8; range.len()];

    assert_eq!(
        primary[ambient.clone()],
        floats(&DirectionalLight::default().ambient.to_array())[..],
        "the primary camera's ambient is the frame's"
    );
    assert_eq!(
        fixed[ambient],
        floats(&KEY.ambient.to_array())[..],
        "and the fixed view's is its fill"
    );
    for (term, range) in [
        ("sky", &sky),
        ("fog", &fog),
        ("probe grid", &probes),
        ("shadow map", &atlas),
    ] {
        assert_ne!(
            primary[range.clone()],
            zeroes(range)[..],
            "the frame carries a {term}, or the check below proves nothing"
        );
        assert_eq!(
            fixed[range.clone()],
            zeroes(range)[..],
            "the fixed view's block carries none of the frame's {term}"
        );
    }

    let stride = crcbl_shaders::light::LIGHT_STRIDE;
    let primary_rows = recorder
        .buffer_bytes(renderer.primary.lights.lights(slot))
        .expect("live");
    let fixed_rows = recorder
        .buffer_bytes(built.lights.lights(slot))
        .expect("live");
    assert_eq!(
        primary_rows[..stride],
        crate::light::sun_row(&DirectionalLight::default()).to_bytes()[..],
        "the primary camera's list starts with the frame's sun"
    );
    assert_ne!(
        primary_rows[stride..2 * stride],
        vec![0u8; stride][..],
        "and holds the point light after it"
    );
    assert_eq!(
        fixed_rows[..stride],
        crate::light::sun_row(&KEY).to_bytes()[..],
        "the fixed view's list is its key light"
    );
    assert_eq!(
        fixed_rows[stride..2 * stride],
        vec![0u8; stride][..],
        "and nothing else: the point light never reaches it"
    );

    // The probe table its groups name: one zeroed row where the primary
    // camera's name the scene's, whose first row is the grid's lit probe.
    let binds = |entries: &[BindGroupEntry], buffer: BufferHandle| {
        entries.iter().any(|entry| {
            matches!(entry.resource, BindingResource::Buffer { buffer: bound, .. } if bound == buffer)
        })
    };
    let scene_probes = renderer.probes.buffer(slot);
    let zero_row = built.fixed_probes.expect("a fixed view owns a zero row");
    assert!(
        binds(&renderer.primary.mesh_group_entries[slot], scene_probes),
        "the primary camera's group binds the scene's probe table"
    );
    assert!(
        binds(&built.mesh_group_entries[slot], zero_row)
            && !binds(&built.mesh_group_entries[slot], scene_probes),
        "and the fixed view's binds its zero row in its place"
    );
    assert_eq!(
        recorder.buffer_bytes(zero_row).expect("live"),
        crcbl_shaders::probe::GpuProbe::ZERO.to_bytes().to_vec(),
        "which holds exactly one zero probe"
    );
    renderer.destroy(device);
}

/// **A fixed view's reflection march sees its environment and nothing of the
/// frame's**: the sky rows are the environment in all three bands, the
/// atmosphere arm is off and the probe header is empty — where the primary
/// camera's block, written in the same frame, carries the frame's gradient, its
/// atmosphere and its probe grid.
///
/// And the forward pass's L1 sky stays zero, so the environment is the
/// specular half alone and the fill the diffuse half alone.
#[test]
fn a_fixed_view_s_reflections_see_its_environment_and_nothing_of_the_frames() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = renderer_lit_by_everything(device, queue);
    renderer.set_atmosphere(Some(crate::camera::Atmosphere::NOON));
    let view = renderer
        .create_view(
            device,
            queue,
            &ViewDesc {
                lighting: FIXED,
                ..ViewDesc::transparent()
            },
        )
        .expect("built");
    renderer
        .begin_frame(
            device,
            &Camera::default(),
            &DirectionalLight::default(),
            (256, 192),
        )
        .expect("write");
    renderer
        .begin_view(device, view, &Camera::default(), (128, 128))
        .expect("write");

    let slot = renderer.frame;
    let built = renderer.views[view.index() - 1].as_ref().expect("built");
    let primary = recorder
        .buffer_bytes(renderer.primary.ssr.uniforms(slot))
        .expect("live");
    let fixed = recorder
        .buffer_bytes(built.ssr.uniforms(slot))
        .expect("live");
    assert_eq!(
        fixed.len(),
        crcbl_shaders::ssr::PARAMS_SIZE,
        "the whole block"
    );

    // The sky's three rows and the atmosphere row close the block; the probe
    // header follows the three matrices — see `ssr::SsrParams::to_bytes`.
    let air = crcbl_shaders::ssr::PARAMS_SIZE - 16..crcbl_shaders::ssr::PARAMS_SIZE;
    let sky = air.start - 48..air.start;
    let probes = 192..192 + crcbl_shaders::probe::PROBE_VOLUME_SIZE;
    let band = [ENVIRONMENT.x, ENVIRONMENT.y, ENVIRONMENT.z, 0.0];
    let uniform: Vec<u8> = [band; 3]
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect();

    assert_ne!(
        primary[sky.clone()],
        uniform[..],
        "the frame's gradient is not the environment, or the check below proves nothing"
    );
    assert_eq!(
        fixed[sky],
        uniform[..],
        "the fixed view's march sees the environment along every band"
    );
    assert_eq!(
        f32::from_le_bytes(primary[air.end - 4..].try_into().expect("four bytes")),
        crcbl_shaders::sky::ATMOSPHERE_ON,
        "the frame's march reads the atmosphere, or the check below proves nothing"
    );
    assert_eq!(
        fixed[air],
        [0.0f32, 0.0, 0.0, crcbl_shaders::sky::ATMOSPHERE_OFF]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>()[..],
        "and the fixed view's reads the gradient alone"
    );
    assert_ne!(
        primary[probes.clone()],
        vec![0u8; probes.len()][..],
        "the frame's march reads a probe grid, or the check below proves nothing"
    );
    assert_eq!(
        fixed[probes.clone()],
        vec![0u8; probes.len()][..],
        "and the fixed view's reads none"
    );

    let frame_block = recorder.buffer_bytes(built.uniforms[slot]).expect("live");
    let l1_sky = mesh::SKY_SH_R_OFFSET..mesh::SKY_SH_R_OFFSET + 48;
    assert_eq!(
        frame_block[l1_sky.clone()],
        vec![0u8; l1_sky.len()][..],
        "the environment is not the forward pass's diffuse sky"
    );
    renderer.destroy(device);
}

/// **A fixed view records none of [`ViewLighting::SCENE_EFFECTS`]' passes**,
/// whatever its description asked for — while a scene-lit view with the same
/// effects, in the same frame setup, records every one of them. **It still
/// records the reflection pair**, which is how its environment reaches a
/// surface.
#[test]
fn a_fixed_view_records_no_pass_that_brings_the_frames_light_in() {
    let (_, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = renderer_lit_by_everything(device, queue);
    let scene_lit = renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("built");
    let fixed = renderer
        .create_view(
            device,
            queue,
            &ViewDesc {
                lighting: FIXED,
                ..ViewDesc::default()
            },
        )
        .expect("built");
    let count =
        |labels: &[String], label: &str| labels.iter().filter(|each| *each == label).count();

    let alone = frame_labels(device, queue, &mut renderer, None);
    let with_scene_lit = frame_labels(device, queue, &mut renderer, Some(scene_lit));
    let with_fixed = frame_labels(device, queue, &mut renderer, Some(fixed));
    for label in ["ssr", "ssr-blur"] {
        assert!(
            count(&alone, label) > 0,
            "the frame records `{label}`, or its presence below proves nothing"
        );
        assert_eq!(
            count(&with_fixed, label),
            2 * count(&alone, label),
            "a fixed view records its own `{label}`"
        );
    }
    for label in [
        "contact-shadows",
        "volumetric-scatter",
        "volumetric-composite",
    ] {
        assert!(
            count(&alone, label) > 0,
            "the frame records `{label}`, or its absence below proves nothing"
        );
        assert_eq!(
            count(&with_scene_lit, label),
            2 * count(&alone, label),
            "a scene-lit view records its own `{label}`"
        );
        assert_eq!(
            count(&with_fixed, label),
            count(&alone, label),
            "and a fixed view records none"
        );
    }
    assert_eq!(
        count(&with_fixed, "forward"),
        2 * count(&alone, "forward"),
        "the fixed view still draws its own frame"
    );
    renderer.destroy(device);
}

/// **A fixed view's zero probe row is released with it**, by `destroy_view`
/// and by the renderer, like everything else a view builds.
#[test]
fn a_fixed_view_releases_its_probe_row() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let before = recorder.total_live_objects();
    let fixed = ViewDesc {
        lighting: FIXED,
        ..ViewDesc::transparent()
    };
    let view = renderer.create_view(device, queue, &fixed).expect("built");
    assert!(
        renderer.views[view.index() - 1]
            .as_ref()
            .expect("built")
            .fixed_probes
            .is_some(),
        "the view owns a row to release"
    );
    renderer.destroy_view(device, view);
    assert_eq!(
        recorder.total_live_objects(),
        before,
        "destroy_view releases it"
    );
    renderer.create_view(device, queue, &fixed).expect("built");
    renderer.destroy(device);
    assert_eq!(
        recorder.live_objects(crcbl_hal::null::ObjectKind::Buffer),
        0,
        "and so does the renderer"
    );
    recorder.assert_valid();
}
