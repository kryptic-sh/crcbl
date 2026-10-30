//! Which content passes [`ForwardRenderer`]'s build leaves for a frame to
//! build, counted on the null backend.
//!
//! Water and grass are content a caller sets after the build —
//! `ForwardRenderer::set_water` and `ForwardRenderer::set_grass` — so a build
//! that made their pipelines made them for a scene that may never have either.
//! What this pins is both halves: the build makes none of them, and the first
//! frame with a body or a field makes that pass's, once per camera — on the
//! primary camera and on a view made later.

use super::*;

/// Every pipeline the water passes create: the copy and the surface.
const WATER: [&str; 2] = ["water copy", "water"];

/// Every pipeline the grass passes create: the dispatches and each draw
/// but the cards, whose entry point's name another pass shares.
const GRASS: [&str; 6] = [
    "grass clear",
    "grass generate",
    "shellVertexMain",
    "finVertexMain",
    "bladeNearVertexMain",
    "bladeFarVertexMain",
];

/// How many pipelines labelled `label` `recorder` has seen created.
fn created(recorder: &Recorder, label: &str) -> usize {
    recorder
        .events()
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::Created {
                    kind: ObjectKind::GraphicsPipeline | ObjectKind::ComputePipeline,
                    label: Some(created),
                } if created == label
            )
        })
        .count()
}

/// How many of each of `labels` `recorder` has seen created.
fn counts<const N: usize>(recorder: &Recorder, labels: [&str; N]) -> [usize; N] {
    labels.map(|label| created(recorder, label))
}

/// A body of water the null backend meshes.
fn pond() -> crcbl_water::WaterBody {
    crcbl_water::WaterBody {
        outline: vec![[-2.0, -2.0], [2.0, -2.0], [2.0, 2.0], [-2.0, 2.0]],
        level: 0.25,
        medium: crcbl_water::Medium {
            absorption: [0.4, 0.1, 0.05],
            scattering: [0.01, 0.01, 0.01],
        },
    }
}

/// A one-tile field of cards.
fn lawn() -> crate::grass::GrassField {
    crate::grass::GrassField::new(
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
    .expect("a real field")
}

/// Draws one frame of the primary camera and, where `view` is `Some`, that
/// view, executed on the null backend, and returns its pass labels.
///
/// `between` runs after the frame's `begin_frame` and before its passes are
/// recorded — where a caller's late `set_grass` would land.
fn drawn(
    device: &dyn Device,
    queue: QueueHandle,
    renderer: &mut ForwardRenderer,
    view: Option<ViewId>,
    between: impl FnOnce(&mut ForwardRenderer),
) -> Vec<String> {
    let (camera, sun) = (Camera::default(), DirectionalLight::default());
    renderer
        .begin_frame(device, &camera, &sun, TEST_EXTENT)
        .expect("write");
    if let Some(view) = view {
        renderer
            .begin_view(device, view, &camera, TEST_EXTENT)
            .expect("write");
    }
    between(renderer);
    let images: Vec<ImportedImage> = (0..2)
        .map(|_| swapchain_image_at(device, TEST_EXTENT))
        .collect();
    let mut pool = crate::TransientPool::new();
    let mut graph = crate::RenderGraph::new(queue);
    let target = graph.import_image("target", images[0]);
    let targets: Vec<ViewTarget> = view
        .map(|view| ViewTarget {
            view,
            target: graph.import_image("view target", images[1]),
            extent: TEST_EXTENT,
        })
        .into_iter()
        .collect();
    renderer.add_passes_with_views(
        &mut graph,
        &pool,
        FrameTargets {
            target,
            extent: TEST_EXTENT,
            skinning: None,
            views: &targets,
        },
        |_, _| {},
    );
    let compiled = graph.compile(&pool).expect("a legal frame");
    let labels = compiled
        .passes()
        .iter()
        .map(|pass| pass.label().to_string())
        .collect();
    let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
        label: Some("content frame"),
        queue,
    });
    compiled
        .execute(device, &mut pool, encoder.as_mut(), None)
        .expect("the graph executed");
    let commands = encoder.finish().expect("recording succeeded");
    device.destroy_command_buffer(commands);
    pool.destroy(device);
    for image in images {
        device.destroy_image_view(image.view);
        device.destroy_image(image.image);
    }
    labels
}

/// How many passes labelled `label` `labels` holds.
fn passes(labels: &[String], label: &str) -> usize {
    labels.iter().filter(|each| *each == label).count()
}

/// **The build makes no water or grass pipeline, and the first frame with a
/// body or a field makes that pass's — once per camera.**
#[test]
fn water_and_grass_are_built_by_the_first_frame_that_has_them() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    recorder.clear();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    place_cube(&mut renderer, Mat4::IDENTITY);
    assert_eq!(counts(&recorder, WATER), [0; 2], "the build made water");
    assert_eq!(counts(&recorder, GRASS), [0; 6], "the build made grass");

    // A frame with neither builds neither.
    let dry = drawn(device, queue, &mut renderer, None, |_| {});
    assert_eq!(passes(&dry, "water") + passes(&dry, "grass"), 0, "{dry:?}");
    assert_eq!(counts(&recorder, WATER), [0; 2]);
    assert_eq!(counts(&recorder, GRASS), [0; 6]);

    // Setting the bodies builds nothing; the first frame with them builds the
    // water passes, and the frames after build nothing more.
    renderer.set_water(&[pond()]).expect("a square meshes");
    assert_eq!(counts(&recorder, WATER), [0; 2], "`set_water` built a pass");
    for lap in 0..2 {
        let wet = drawn(device, queue, &mut renderer, None, |_| {});
        assert_eq!(passes(&wet, "water"), 1, "frame {lap}: {wet:?}");
        assert_eq!(
            counts(&recorder, WATER),
            [1; 2],
            "after frame {lap} with water"
        );
    }
    assert_eq!(counts(&recorder, GRASS), [0; 6], "water built grass");

    // A field set **between** a frame's two halves is drawn from the next
    // frame on: the frame froze the field it began with, so it neither draws
    // grass nor reaches for passes it did not build.
    let late = drawn(device, queue, &mut renderer, None, |renderer| {
        renderer
            .set_grass(device, queue, Some(&lawn()))
            .expect("the null backend uploads every map");
    });
    assert_eq!(passes(&late, "grass"), 0, "{late:?}");
    assert_eq!(
        counts(&recorder, GRASS),
        [0; 6],
        "a frame without grass built it"
    );
    for lap in 0..2 {
        let grown = drawn(device, queue, &mut renderer, None, |_| {});
        assert_eq!(passes(&grown, "grass"), 1, "frame {lap}: {grown:?}");
        assert_eq!(
            counts(&recorder, GRASS),
            [1; 6],
            "after frame {lap} with grass"
        );
    }

    // A view made now builds neither; its first frame builds both, once.
    let view = renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("a second view fits");
    assert_eq!(
        counts(&recorder, WATER),
        [1; 2],
        "the view's build made water"
    );
    assert_eq!(
        counts(&recorder, GRASS),
        [1; 6],
        "the view's build made grass"
    );
    for lap in 0..2 {
        let both = drawn(device, queue, &mut renderer, Some(view), |_| {});
        assert_eq!(passes(&both, "water"), 2, "frame {lap}: {both:?}");
        assert_eq!(passes(&both, "grass"), 2, "frame {lap}: {both:?}");
        assert_eq!(
            counts(&recorder, WATER),
            [2; 2],
            "after the view's frame {lap}"
        );
        assert_eq!(
            counts(&recorder, GRASS),
            [2; 6],
            "after the view's frame {lap}"
        );
    }

    // Removing both keeps the passes for the frame that sets them again, and
    // records none of them.
    renderer.set_water(&[]).expect("an empty set is a set");
    renderer
        .set_grass(device, queue, None)
        .expect("removing is a set");
    for _ in 0..2 {
        drawn(device, queue, &mut renderer, Some(view), |_| {});
    }
    let removed = drawn(device, queue, &mut renderer, Some(view), |_| {});
    assert_eq!(
        passes(&removed, "water") + passes(&removed, "grass"),
        0,
        "{removed:?}"
    );
    assert_eq!(counts(&recorder, WATER), [2; 2]);
    assert_eq!(counts(&recorder, GRASS), [2; 6]);

    // Released with the renderer, every one of them.
    renderer.destroy_view(device, view);
    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}
