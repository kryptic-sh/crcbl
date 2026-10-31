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

/// Every filter pipeline the frame bound since command `from`, as the pair the
/// `ssao-blur` and `ssao-upsample` passes ran — the same question
/// `gathers_bound` asks of the marches.
fn filters_bound(recorder: &Recorder, from: usize) -> Vec<crate::ssao::Filters> {
    use crcbl_hal::null::Command;

    let mut inside = None;
    let (mut blurs, mut upsamples) = (Vec::new(), Vec::new());
    for command in &recorder.commands()[from..] {
        if let Some((_, label)) = command.opens_pass() {
            inside = match label {
                Some("ssao-blur") => Some(true),
                Some("ssao-upsample") => Some(false),
                _ => None,
            };
            continue;
        }
        if matches!(command, Command::EndRenderPass | Command::EndComputePass) {
            inside = None;
            continue;
        }
        match (inside, command) {
            (Some(true), Command::BindGraphicsPipeline(pipeline)) => blurs.push(*pipeline),
            (Some(false), Command::BindGraphicsPipeline(pipeline)) => upsamples.push(*pipeline),
            _ => {}
        }
    }
    assert_eq!(
        blurs.len(),
        upsamples.len(),
        "the frame recorded a blur without a reconstruction or the other way round"
    );
    blurs
        .into_iter()
        .zip(upsamples)
        .map(|(blur, upsample)| crate::ssao::Filters { blur, upsample })
        .collect()
}

/// Moves `r_ssao_bent_normals` and puts it back at its default when dropped.
///
/// Taken only while [`ssao_technique_switch`]'s guard is held, which is the
/// lock `crate::ssao`'s own tests move this variable under.
struct BentSwitch;

impl BentSwitch {
    fn set(&self, on: bool) {
        crate::ssao::r_ssao_bent_normals
            .set(&crcbl_console::Value::Bool(on))
            .expect("`r_ssao_bent_normals` is a writable bool");
    }
}

impl Drop for BentSwitch {
    fn drop(&mut self) {
        let shipped = matches!(
            crate::ssao::r_ssao_bent_normals.default(),
            crcbl_console::Value::Bool(true)
        );
        self.set(shipped);
    }
}

/// **The bent-free filters are built by the first frame whose gathers write no
/// direction, and run only on such frames.**
///
/// Read off the pipelines the `ssao-blur` and `ssao-upsample` passes bound:
/// the default frame measures a direction and runs the filters that carry it,
/// a frame with `r_ssao_bent_normals` off builds the bent-free pair once and
/// runs it, a comparing frame runs the carrying pair again because its far side
/// gathers what ships, and the cheap tier runs the bent-free pair with the
/// switch back on, since `ssao_hemisphere.slang` measures no direction either.
#[test]
fn the_filters_skip_the_bent_direction_only_when_no_gather_wrote_one() {
    const BLUR: &str = "ssao blur bentless";
    const UPSAMPLE: &str = "ssao upsample bentless";
    let _blurs = ssao_blur_switch();
    let seam = ssao_split_switch();
    let technique = ssao_technique_switch();
    let bent = BentSwitch;
    bent.set(true);

    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let built = |recorder: &Recorder| (created(recorder, BLUR), created(recorder, UPSAMPLE));
    let filters = |renderer: &ForwardRenderer| {
        renderer
            .primary
            .ssao
            .as_ref()
            .expect("the default stack builds the occlusion pair")
            .filters()
    };
    assert_eq!(
        built(&recorder),
        (0, 0),
        "the build made the bent-free pair"
    );
    let (carrying, _) = filters(&renderer);

    // One frame, and the filters it bound, under whatever the console now holds.
    let bound = |renderer: &mut ForwardRenderer| {
        let from = recorder.commands().len();
        frame(device, renderer, queue).release(device);
        filters_bound(&recorder, from)
    };

    assert_eq!(bound(&mut renderer), vec![carrying], "the default frame");
    assert_eq!(
        built(&recorder),
        (0, 0),
        "a frame measuring a direction built the bent-free pair"
    );

    bent.set(false);
    let first = bound(&mut renderer);
    let bentless = filters(&renderer)
        .1
        .expect("the first frame without a direction built the bent-free pair");
    assert_ne!(bentless, carrying);
    assert_eq!(first, vec![bentless], "a frame with the direction off");
    assert_eq!(bound(&mut renderer), vec![bentless], "and the next one");
    assert_eq!(
        built(&recorder),
        (1, 1),
        "built once, by the first such frame"
    );

    seam.set(0.5);
    assert_eq!(
        bound(&mut renderer),
        vec![carrying],
        "a comparing frame's far side gathers what ships, which measures a direction"
    );
    seam.set(0.0);

    bent.set(true);
    assert_eq!(
        bound(&mut renderer),
        vec![carrying],
        "the direction back on"
    );
    technique.set(other_technique_name());
    assert_eq!(
        bound(&mut renderer),
        vec![bentless],
        "the cheap tier measures no direction, switch or no switch"
    );
    assert_eq!(built(&recorder), (1, 1), "and nothing was built again");

    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}

/// **The filters follow the switch the gather's block was written with, not
/// the console at record time.**
///
/// `begin_frame` writes the occlusion block, and with it the switch the
/// gather reads; `add_passes` records the filters later. A console turned off
/// between the two, with the bent-free pair already built, must still run the
/// filters that carry a direction — the gather was told to measure one, and the
/// bent-free pair would overwrite it with the sentinel.
#[test]
fn the_filters_follow_the_switch_the_frame_was_begun_with() {
    let _blurs = ssao_blur_switch();
    let _seam = ssao_split_switch();
    let _technique = ssao_technique_switch();
    let bent = BentSwitch;

    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    bent.set(false);
    frame(device, &mut renderer, queue).release(device);
    let (carrying, bentless) = renderer
        .primary
        .ssao
        .as_ref()
        .expect("the default stack builds the occlusion pair")
        .filters();
    let bentless = bentless.expect("a frame without a direction built the bent-free pair");

    // Begun with the direction on, recorded with it off.
    bent.set(true);
    renderer
        .begin_frame(
            device,
            &Camera::default(),
            &DirectionalLight::default(),
            TEST_EXTENT,
        )
        .expect("write");
    bent.set(false);
    let from = recorder.commands().len();
    let imported = swapchain_image(device);
    let mut graph = crate::RenderGraph::new(queue);
    let target = graph.import_image("target", imported);
    let mut pool = crate::TransientPool::new();
    renderer.add_passes(&mut graph, &pool, target, TEST_EXTENT);
    let compiled = graph.compile(&pool).expect("a legal frame");
    let mut encoder = device.create_command_encoder(&crcbl_hal::CommandEncoderDesc {
        label: Some("forward frame"),
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
    let switched = filters_bound(&recorder, from);

    // The control: begun with it off, the same frame runs the bent-free pair.
    let off = {
        let from = recorder.commands().len();
        frame(device, &mut renderer, queue).release(device);
        filters_bound(&recorder, from)
    };

    assert_eq!(
        switched,
        vec![carrying],
        "the console moved between begin_frame and add_passes and the frame ran the filters \
         the console asked for, over a gather its block told to measure a direction"
    );
    assert_eq!(off, vec![bentless], "a frame begun with the direction off");

    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}
