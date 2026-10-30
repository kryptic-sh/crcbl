//! That every [`DrawGen`] a [`ForwardRenderer`] makes dispatches one shared
//! [`DrawGenPipelines`], counted on the null backend.
//!
//! The camera's generator and each shadow cull's used to compile the same
//! compute pipelines once apiece; measured cold on an RX 7900 XTX under radv in
//! 2026-10, a set was about 13 ms of the build. What this pins is that the
//! build compiles each of them once, that a view made later compiles none, and
//! that the set outlives every generator that dispatches it — a view's release
//! included — and no longer.

use super::*;

/// The label of every pipeline [`DrawGenPipelines`] compiles: the clearing
/// pass, the cull, `draw_gen.slang`'s three entry points, and the occlusion
/// phases a camera's generator is built for.
const DRAW_GEN_PIPELINES: [&str; 9] = [
    "clear counters",
    "cull",
    "draw args bin",
    "draw args starts",
    "draw args scatter",
    "occlusion early cull",
    "occlusion late cull",
    "draw args late scatter",
    "draw args late finish",
];

/// How many objects of `kind` labelled `label` `recorder` has seen created.
fn created(recorder: &Recorder, kind: ObjectKind, label: &str) -> usize {
    recorder
        .events()
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::Created { kind: created, label: Some(created_label) }
                    if *created == kind && created_label == label
            )
        })
        .count()
}

/// **The build compiles each draw-generation pipeline once, for the camera and
/// every shadow cull together, and a view made later compiles none.**
///
/// The generators are counted as well as the pipelines, by the cull bind group
/// each makes per frame in flight, so "once" is once across many generators
/// rather than once because only one was built.
#[test]
fn every_draw_generator_shares_one_set_of_pipelines() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    recorder.clear();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");

    let generators = 1 + SHADOW_CULLS;
    assert_eq!(
        created(&recorder, ObjectKind::BindGroup, "cull"),
        generators * FRAMES_IN_FLIGHT,
        "the build made a cull group per frame for the camera and each shadow cull"
    );
    for label in DRAW_GEN_PIPELINES {
        assert_eq!(
            created(&recorder, ObjectKind::ComputePipeline, label),
            1,
            "the build compiled `{label}` other than once across {generators} generators"
        );
    }

    // A view builds a generator of its own and compiles nothing for it.
    recorder.clear();
    let view = renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("a second view fits");
    assert_eq!(
        created(&recorder, ObjectKind::BindGroup, "cull"),
        FRAMES_IN_FLIGHT,
        "the view built a generator"
    );
    for label in DRAW_GEN_PIPELINES {
        assert_eq!(
            created(&recorder, ObjectKind::ComputePipeline, label),
            0,
            "the view compiled `{label}` again rather than sharing the renderer's"
        );
    }

    // Releasing the view leaves the set to the generators still using it: a
    // frame dispatching them would record a dead handle otherwise.
    renderer.destroy_view(device, view);
    frame(device, &mut renderer, queue).release(device);
    recorder.assert_valid();

    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}

/// **A build that fails after the set exists releases it**, with everything
/// else it had made.
///
/// A device whose limits refuse the mesh path's three-target colour pipeline,
/// which the camera builds after its generator — so the shared set and the
/// generator holding it are both in the rollback when the refusal arrives.
#[test]
fn a_failed_build_releases_the_shared_pipelines() {
    let recorder = Recorder::new();
    let mut limits = crcbl_hal::Limits::desktop();
    limits.max_color_attachments = 2;
    let features = Features::COMPUTE | Features::MESH_SHADER;
    let instance = NullInstance::new(crcbl_hal::DeviceCaps { features, limits })
        .with_recorder(recorder.clone());
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: crcbl_hal::AdapterId(0),
            required_features: features,
            optional_features: Features::empty(),
            compatible_surface: None,
        })
        .expect("the null backend always opens");
    let queue = device.queue(QueueKind::Graphics).expect("always present");
    let before = recorder.total_live_objects();
    ForwardRenderer::with_scene_on_path(
        device.as_ref(),
        queue,
        Format::Rgba8UnormSrgb,
        &scene::demo(),
        GeometryPath::MeshShader,
    )
    .expect_err("three-target colour pipelines must be refused");
    for label in DRAW_GEN_PIPELINES {
        assert_eq!(
            created(&recorder, ObjectKind::ComputePipeline, label),
            1,
            "the refusal came before `{label}` was compiled, so this releases nothing of it"
        );
    }
    assert_eq!(
        recorder.total_live_objects(),
        before,
        "the failed build leaked"
    );
    recorder.assert_valid();
}
