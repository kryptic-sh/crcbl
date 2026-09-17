use super::*;
use crate::forward::tests::{open, place_cube, swapchain_image_at};

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
