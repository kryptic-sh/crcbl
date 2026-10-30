//! How often [`ForwardRenderer`]'s build and [`ForwardRenderer::create_view`]
//! wait for the device, counted on the null backend.
//!
//! The build's small uploads — its placeholders and tables, every view's and
//! shadow cull's start-up fills, the calm wind layers — share one
//! [`UploadBatch`] and one wait. Each used to be a submit and a `wait_idle` of
//! its own, and on a real driver every round trip is time a player waits
//! through before the first frame, so a new subsystem that submits its own
//! start-up copy instead of joining the batch is what this is here to catch.

use super::*;

/// The build's waits that are not the batch's, for a scene with no probes:
/// one per page kind, from `upload_page`, which keeps its own submission so
/// that every page's staging buffer is not alive at once — and the probe
/// table's clear in `ProbeTable::new`. That one stays out because a scene with
/// probes refills the rows straight after it from `ShaderRead`, and a clear
/// in a batch submitted at the build's end would land after that fill.
/// `open`'s device has timeline semaphores, so the geometry pools retire
/// their uploads without one.
const WAITS_OUTSIDE_THE_BATCH: usize = PageKind::ALL.len() + 1;

/// The label [`UploadBatch::new`] gives the build's encoder: its stem, as
/// `build_on_path` passes it, plus the batch's own suffix.
const BUILD_BATCH: &str = "forward upload";

/// How many `wait_idle`s and submissions `recorder` has seen.
fn waits_and_submits(recorder: &Recorder) -> (usize, usize) {
    let events = recorder.events();
    let waits = events
        .iter()
        .filter(|event| matches!(event, Event::WaitedIdle))
        .count();
    let submits = events
        .iter()
        .filter(|event| matches!(event, Event::Submitted { .. }))
        .count();
    (waits, submits)
}

/// How many submissions carried a command buffer finished under `label`.
fn submissions_of(recorder: &Recorder, label: &str) -> usize {
    let events = recorder.events();
    let finished: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Finished {
                label: Some(finished),
                command_buffer,
            } if finished == label => Some(*command_buffer),
            _ => None,
        })
        .collect();
    events
        .iter()
        .filter(|event| match event {
            Event::Submitted {
                command_buffers, ..
            } => command_buffers.iter().any(|cb| finished.contains(cb)),
            _ => false,
        })
        .count()
}

/// **The batch is one wait**: everything the build waits for is a page, the
/// probe table, or the one batch — however many views, shadow culls and
/// frames in flight recorded into it.
#[test]
fn the_build_waits_once_for_its_batch() {
    assert!(
        scene::demo().probes.probes.is_empty(),
        "WAITS_OUTSIDE_THE_BATCH counts no probe-table fill"
    );
    let (recorder, device, queue) = open();
    recorder.clear();
    let renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
        .expect("the null backend accepts every descriptor");
    let (waits, _) = waits_and_submits(&recorder);
    assert_eq!(
        waits,
        WAITS_OUTSIDE_THE_BATCH + 1,
        "the pages, the probe table and one batch"
    );
    assert_eq!(
        submissions_of(&recorder, BUILD_BATCH),
        1,
        "the build's batch is submitted, and once"
    );
    renderer.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **A view made after the build is one submission and one wait**: its cull
/// state, its exposure ring and its reflection table share a batch of its own.
#[test]
fn a_late_view_waits_once() {
    let (recorder, device, queue) = open();
    let mut renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
        .expect("the null backend accepts every descriptor");
    recorder.clear();
    let view = renderer
        .create_view(device.as_ref(), queue, &ViewDesc::default())
        .expect("a second view fits");
    assert_eq!(waits_and_submits(&recorder), (1, 1));
    renderer.destroy_view(device.as_ref(), view);
    renderer.destroy(device.as_ref());
    recorder.assert_valid();
}
