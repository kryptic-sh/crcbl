//! [`ForwardRenderer::with_scene_serviced`]'s calls, counted on the null
//! backend.
//!
//! What the service is for — a window that keeps answering while its thread
//! builds a renderer — needs a desktop and a stopwatch to see. What *can* be
//! checked without either is where the calls land relative to the work: once
//! per mesh, once per page layer, and never so far apart that a run of
//! pipeline compilations sits between two of them.

use super::*;

/// The most raster and mesh pipelines one gap between two service calls may
/// create.
///
/// **A structural stand-in for a duration.** Pipeline creation is where a
/// build's time goes on a cold driver cache, so a count of pipelines between
/// calls is the no-GPU measure of the longest wait. The longest run is one
/// subsystem's own set, built back to back — the occlusion pair's, today — and
/// a new subsystem, or a mesh-pass or grass variant, that joins a run without
/// a call of its own is what this is here to catch.
///
/// **Compute pipelines are not counted**, and that is a measurement rather
/// than a convenience: `DrawGen::new` builds its whole set of compute
/// pipelines with no call between them, and on a cold cache that set cost
/// less than the water surface's pair of raster pipelines did. Counting them
/// would set this bound by the cheap run and let the expensive ones through.
const MAX_PIPELINES_PER_GAP: usize = 4;

/// The raster and mesh pipelines `recorder` has seen created — every one but
/// the compute ones, for [`MAX_PIPELINES_PER_GAP`]'s reason.
fn graphics_pipelines(recorder: &Recorder) -> usize {
    recorder
        .pipelines_created()
        .iter()
        .filter(|pipeline| {
            !pipeline
                .stages
                .iter()
                .any(|stage| stage.stage == ShaderStages::COMPUTE)
        })
        .count()
}

/// Builds `scene` through the serviced constructor, returning the number of
/// graphics pipelines the device had created at each call, then at the
/// build's end.
fn pipelines_at_each_call(scene: &SceneDesc<'_>) -> Vec<usize> {
    let (recorder, device, queue) = open();
    let mut seen = Vec::new();
    let renderer = ForwardRenderer::with_scene_serviced(
        device.as_ref(),
        queue,
        Format::Rgba8UnormSrgb,
        scene,
        &mut || seen.push(graphics_pipelines(&recorder)),
    )
    .expect("the null backend accepts every descriptor");
    seen.push(graphics_pipelines(&recorder));
    renderer.destroy(device.as_ref());
    recorder.assert_valid();
    seen
}

/// How many times `scene`'s build calls the service.
fn calls(scene: &SceneDesc<'_>) -> usize {
    pipelines_at_each_call(scene).len() - 1
}

/// Every mesh upload is followed by a call, so a scene of more meshes is a
/// build of at least that many more.
#[test]
fn each_mesh_upload_is_serviced() {
    const EXTRA: usize = 3;
    let base = scene::demo();
    let mut more = scene::demo();
    let cube = more.meshes[DEMO_CUBE].clone();
    more.meshes.extend(std::iter::repeat_n(cube, EXTRA));
    let added = calls(&more) - calls(&base);
    assert!(
        added >= EXTRA,
        "{EXTRA} more meshes added {added} service call(s)"
    );
}

/// Every page layer's mip chain is followed by a call, so a page of many
/// large layers is not one long gap.
#[test]
fn each_page_layer_is_serviced() {
    const EXTRA: usize = 3;
    let base = scene::demo();
    let mut more = scene::demo();
    let layer = more.page.layers(PageKind::BaseColor)[0].clone();
    for _ in 0..EXTRA {
        more.page.push_layer(PageKind::BaseColor, layer.clone());
    }
    let added = calls(&more) - calls(&base);
    assert!(
        added >= EXTRA,
        "{EXTRA} more page layers added {added} service call(s)"
    );
}

/// No gap between two calls — nor before the first or after the last —
/// creates more than [`MAX_PIPELINES_PER_GAP`] pipelines.
#[test]
fn no_gap_between_calls_holds_a_long_run_of_pipelines() {
    let seen = pipelines_at_each_call(&scene::demo());
    let total = *seen.last().expect("the end is always recorded");
    assert!(
        total > MAX_PIPELINES_PER_GAP,
        "the build created {total} pipelines, too few for the bound to mean anything"
    );
    let gaps: Vec<usize> = std::iter::once(0)
        .chain(seen.iter().copied())
        .collect::<Vec<_>>()
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect();
    let longest = gaps.iter().copied().max().expect("at least one gap");
    assert!(
        longest <= MAX_PIPELINES_PER_GAP,
        "a gap between service calls created {longest} pipelines (gaps: {gaps:?})"
    );
}
