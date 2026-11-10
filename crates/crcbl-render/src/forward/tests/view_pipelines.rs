//! That a view made after the renderer compiles its froxel volume's and
//! reflection march's pipelines as the build's mesh batch lists them, and
//! through a batch of its own, observed on the null backend.
//!
//! Whether those compiles overlap needs a driver that compiles side by side
//! and a stopwatch; the null backend creates a batch's members one after
//! another, in the jobs' order — see `crate::pipeline_batch`. What it can see
//! is that order, which is the member list's: the costly pair first, the
//! rest after it.

use super::*;

/// The label of every pipeline [`ViewPipelines`] holds.
const VIEW_PIPELINES: [&str; 5] = [
    "volumetric scatter",
    "volumetric integrate",
    "volumetric composite",
    "ssr",
    "ssr blur",
];

/// The labels of the [`VIEW_PIPELINES`] `recorder` has seen created since it
/// was last cleared, in the order the device created them.
fn view_pipelines_created(recorder: &Recorder) -> Vec<String> {
    recorder
        .pipelines_created()
        .into_iter()
        .filter_map(|pipeline| pipeline.label)
        .filter(|label| VIEW_PIPELINES.contains(&label.as_str()))
        .collect()
}

/// **A view compiles its two sets in the order the build's batch does** —
/// out of the one member list, `ViewPipelineLayouts`, rather than one set and
/// then the other.
#[test]
fn a_view_lists_its_pipelines_as_the_build_does() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    recorder.clear();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    let built = view_pipelines_created(&recorder);
    assert_eq!(
        built.len(),
        VIEW_PIPELINES.len(),
        "the build compiled each view pipeline once: {built:?}"
    );

    recorder.clear();
    let view = renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("a second view fits");
    assert_eq!(
        view_pipelines_created(&recorder),
        built,
        "the view compiled its sets in another order than the build's batch"
    );

    renderer.destroy_view(device, view);
    frame(device, &mut renderer, queue).release(device);
    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}

/// **A view's sets come out of a [`crate::pipeline_batch`]**, not out of
/// their members called in a row: on the null backend a batch is serial, and a
/// serial batch runs `service` before every member — the order alone cannot
/// tell those two apart.
#[test]
fn a_views_pipelines_are_one_serviced_batch() {
    let (recorder, device, _) = open();
    let device = device.as_ref();
    let mut services = 0;
    let pipelines = ViewPipelines::new(device, &mut || services += 1).expect("built");
    assert_eq!(
        services,
        VIEW_PIPELINES.len(),
        "a serial batch services before each of its members"
    );
    pipelines.ssr.destroy(device);
    pipelines.volumetric.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the sets leaked");
}
