//! Which settings-driven passes [`ForwardRenderer`]'s build leaves for a frame
//! to build, counted on the null backend.
//!
//! Pipeline creation is most of a cold build, and a pass no frame records is
//! a pipeline a player waits through for nothing — see `View::build_passes`
//! and `ForwardRenderer::frame_atlas_view`. What this pins is both halves: the
//! build makes none of them, and the first frame that asks for one makes it,
//! once — on the primary camera and on a view made later, and whichever tier
//! of the resolve a frame switches to.

use super::*;

/// One pipeline label from each pass the default build leaves unbuilt, and the
/// setting that asks for it: the contact march, the bloom chain and the cheap
/// antialiasing tier by their effect bits, the upscale by a render scale below
/// one, and the shadow atlas viewer by its debug view.
const LAZY: [&str; 5] = [
    "contact shadows",
    "bloom composite",
    "fxaa",
    "upscale",
    "shadow atlas view",
];

/// The passes the default stack draws on its first frame, which the build
/// still makes so that frame is not the one that pays for them: the occlusion
/// pair and the higher antialiasing tier.
const EXPECTED: [&str; 2] = ["ssao", "cmaa2-apply"];

/// The label of every pipeline `recorder` has seen created, in order.
fn pipelines(recorder: &Recorder) -> Vec<Option<String>> {
    recorder
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Created {
                kind: ObjectKind::GraphicsPipeline | ObjectKind::ComputePipeline,
                label,
            } => Some(label),
            _ => None,
        })
        .collect()
}

/// How many pipelines labelled `label` `recorder` has seen created.
fn created(recorder: &Recorder, label: &str) -> usize {
    pipelines(recorder)
        .iter()
        .filter(|created| created.as_deref() == Some(label))
        .count()
}

/// **The default build makes no pass its first frame does not record, and a
/// frame that asks for one makes it — once.**
#[test]
fn a_settings_driven_pass_is_built_by_the_first_frame_that_asks_for_it() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    recorder.clear();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let at_build = pipelines(&recorder).len();
    for label in LAZY {
        assert_eq!(
            created(&recorder, label),
            0,
            "the build made `{label}`, which no default frame records"
        );
    }
    for label in EXPECTED {
        assert_eq!(
            created(&recorder, label),
            1,
            "the build left `{label}` to the first default frame, which records it"
        );
    }

    // A default frame asks for none of them.
    frame(device, &mut renderer, queue).release(device);
    for label in LAZY {
        assert_eq!(
            created(&recorder, label),
            0,
            "a default frame built `{label}`, which it does not record"
        );
    }

    // A frame that asks for the four the effects and the scale reach.
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(
                RenderEffects::BLOOM
                    .union(RenderEffects::CONTACT_SHADOWS)
                    .union(RenderEffects::ANTIALIASING),
                Some(true),
            )
            .force(RenderEffects::CMAA2, Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_render_scale(0.5);
    let asked = renderer.resolved_effects();
    assert!(
        asked.contains(RenderEffects::ANTIALIASING) && !asked.contains(RenderEffects::CMAA2),
        "the request resolves to the cheap tier, or `fxaa` below asks about nothing"
    );
    for lap in 0..2 {
        frame(device, &mut renderer, queue).release(device);
        for label in &LAZY[..4] {
            assert_eq!(
                created(&recorder, label),
                1,
                "after frame {lap} asking for it, `{label}` was not built exactly once"
            );
        }
    }

    // And the viewer, by the debug view — which takes the resolve off, so it is
    // a frame of its own.
    renderer.set_atlas_view(true);
    frame(device, &mut renderer, queue).release(device);
    assert_eq!(
        created(&recorder, LAZY[4]),
        1,
        "a frame showing the atlas did not build its viewer"
    );
    let every = pipelines(&recorder).len();
    eprintln!("pipelines: {at_build} at build; {every} once every settings-driven pass is built");

    // Released with the renderer, every one of them: nothing is left alive.
    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}

/// The labels of the passes a frame records, drawn into a target of
/// [`TEST_EXTENT`] and — where `view` is `Some` — that view into one of the
/// same size, without executing it.
fn recorded(
    device: &dyn Device,
    queue: QueueHandle,
    renderer: &mut ForwardRenderer,
    view: Option<ViewId>,
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
    let images: Vec<ImportedImage> = (0..2)
        .map(|_| swapchain_image_at(device, TEST_EXTENT))
        .collect();
    let pool = crate::TransientPool::new();
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
    let labels = graph
        .compile(&pool)
        .expect("a legal frame")
        .passes()
        .iter()
        .map(|pass| pass.label().to_string())
        .collect();
    for image in images {
        device.destroy_image_view(image.view);
        device.destroy_image(image.image);
    }
    labels
}

/// A request whose resolve slot holds `tier`, and with the occlusion pair
/// switched `occlusion`.
fn request(tier: Antialiasing, occlusion: bool) -> EffectRequest {
    EffectRequest {
        antialiasing: Some(tier),
        programmatic: EffectOverride::none()
            .force(RenderEffects::AMBIENT_OCCLUSION, Some(occlusion)),
        ..EffectRequest::default()
    }
}

/// **The resolve's two tiers are each built by the first frame that switches
/// to it**, and switching back builds nothing: the primary camera starts with
/// CMAA2, the default stack's tier, and gains FXAA the frame it is chosen.
#[test]
fn switching_the_resolve_tier_builds_the_other_tier_once() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    assert_eq!(
        (
            created(&recorder, "cmaa2-apply"),
            created(&recorder, "fxaa")
        ),
        (1, 0),
        "the build makes the default stack's tier and not the other"
    );

    renderer.set_effect_request(request(Antialiasing::Fxaa, true));
    let labels = recorded(device, queue, &mut renderer, None);
    assert!(labels.iter().any(|label| label == "fxaa"), "{labels:?}");
    assert_eq!(
        created(&recorder, "fxaa"),
        1,
        "the switch built the cheap tier"
    );

    renderer.set_effect_request(request(Antialiasing::Cmaa2, true));
    let labels = recorded(device, queue, &mut renderer, None);
    assert!(!labels.iter().any(|label| label == "fxaa"), "{labels:?}");
    assert_eq!(
        (
            created(&recorder, "cmaa2-apply"),
            created(&recorder, "fxaa")
        ),
        (1, 1),
        "switching back rebuilt a tier the renderer already held"
    );

    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}

/// **A view made under a non-default stack builds what that stack draws**,
/// and the first frame that asks it for more builds the rest: made while the
/// occlusion pair is off and the cheap tier is chosen, it holds neither the
/// pair nor CMAA2, and gains each the frame they are switched on.
#[test]
fn a_view_made_later_builds_what_its_first_frames_switch_on() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    // The renderer's last frame is what a later view's build expects, so one
    // frame under the non-default stack first.
    renderer.set_effect_request(request(Antialiasing::Fxaa, false));
    let labels = recorded(device, queue, &mut renderer, None);
    assert!(!labels.iter().any(|label| label == "ssao"), "{labels:?}");
    let before = (
        created(&recorder, "ssao"),
        created(&recorder, "fxaa"),
        created(&recorder, "cmaa2-apply"),
    );
    assert_eq!(before, (1, 1, 1), "the primary camera's, and only those");

    let view = renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("a second view fits");
    assert_eq!(
        (
            created(&recorder, "ssao"),
            created(&recorder, "fxaa"),
            created(&recorder, "cmaa2-apply"),
        ),
        (1, 2, 1),
        "the view built the tier the last frame drew and nothing it did not draw"
    );

    renderer.set_effect_request(request(Antialiasing::Cmaa2, true));
    let labels = recorded(device, queue, &mut renderer, Some(view));
    assert_eq!(
        labels.iter().filter(|label| *label == "ssao").count(),
        2,
        "both cameras gather occlusion: {labels:?}"
    );
    assert_eq!(
        (
            created(&recorder, "ssao"),
            created(&recorder, "fxaa"),
            created(&recorder, "cmaa2-apply"),
        ),
        (2, 2, 2),
        "the view's first frame asking for the pair and CMAA2 built each once"
    );

    renderer.destroy_view(device, view);
    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}
