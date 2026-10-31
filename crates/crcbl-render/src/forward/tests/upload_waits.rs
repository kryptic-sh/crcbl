//! How often [`ForwardRenderer`]'s build and [`ForwardRenderer::create_view`]
//! wait for the device, counted on the null backend — and that the uploads
//! they no longer wait for keep their staging until the GPU is past it.
//!
//! The build's small uploads — its placeholders and tables, every view's and
//! shadow cull's start-up fills, the calm wind layers — share one
//! [`UploadBatch`], submitted with no wait at all. Each used to be a submit
//! and a `wait_idle` of its own, and on a real driver every round trip is time
//! a player waits through before the first frame, so a new subsystem that
//! submits its own start-up copy instead of joining the batch is what this is
//! here to catch.
//!
//! The null backend signals a submission's semaphore at submit, so the first
//! frame is the completion point here; a real device reaches it a frame or two
//! later. It cannot see a buffer destroyed under a submitted copy, which is
//! why these assert on liveness and order rather than on `assert_valid` alone.

use super::*;
use crcbl_hal::null::Command;

/// The build's waits, for a scene with no probes, now that its batch waits for
/// nothing: one per page kind, from `upload_page`, which keeps its own
/// submission so that every page's staging buffer is not alive at once — and
/// the probe table's clear in `ProbeTable::new`. That one stays out because a
/// scene with probes refills the rows straight after it from `ShaderRead`, and
/// a clear in a batch submitted at the build's end would land after that fill.
/// `open`'s device has timeline semaphores, so the geometry pools retire
/// their uploads without one.
const WAITS_OUTSIDE_THE_BATCH: usize = PageKind::ALL.len() + 1;

/// The label [`UploadBatch::new`] gives the build's encoder and semaphore: its
/// stem, as `build_on_path` passes it, plus the batch's own suffix.
const BUILD_BATCH: &str = "forward upload";

/// [`BUILD_BATCH`] for a batch [`ForwardRenderer::create_view`] submits.
const VIEW_BATCH: &str = "view upload";

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

/// The command buffers finished under `label`.
fn finished_as(recorder: &Recorder, label: &str) -> Vec<crcbl_hal::CommandBufferHandle> {
    recorder
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::Finished {
                label: Some(finished),
                command_buffer,
            } if finished == label => Some(*command_buffer),
            _ => None,
        })
        .collect()
}

/// How many submissions carried a command buffer finished under `label`, and
/// how many of those signalled [`PendingUpload::LANDED`].
fn submissions_of(recorder: &Recorder, label: &str) -> (usize, usize) {
    let finished = finished_as(recorder, label);
    let events = recorder.events();
    let mut submitted = 0;
    let mut signalled = 0;
    for event in &events {
        if let Event::Submitted {
            command_buffers,
            signals,
            ..
        } = event
            && command_buffers.iter().any(|cb| finished.contains(cb))
        {
            submitted += 1;
            if signals
                .iter()
                .any(|signal| signal.value == PendingUpload::LANDED)
            {
                signalled += 1;
            }
        }
    }
    (submitted, signalled)
}

/// Every buffer the copies and clears recorded under `label` read: the staging
/// and zero buffers the batch's [`PendingUpload`] owns.
fn staging_of(recorder: &Recorder, label: &str) -> Vec<BufferHandle> {
    let finished = finished_as(recorder, label);
    let mut buffers: Vec<BufferHandle> = recorder
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::Command {
                command_buffer,
                command,
            } if finished.contains(command_buffer) => match command {
                Command::CopyBufferToImage(copy) => Some(copy.buffer),
                Command::CopyBufferToBuffer(copy) => Some(copy.src),
                Command::ClearBuffer { buffer, .. } => Some(*buffer),
                _ => None,
            },
            _ => None,
        })
        .collect();
    buffers.sort_unstable_by_key(|buffer| buffer.to_bits());
    buffers.dedup();
    buffers
}

/// Whether, in `recorder`'s stream, a host wait on an upload's semaphore came
/// before the first object was destroyed — the order
/// [`ForwardRenderer::destroy`] and [`ForwardRenderer::destroy_view`] promise,
/// because what they destroy includes what the uploads' copies write.
fn waited_before_anything_went(recorder: &Recorder) -> bool {
    let events = recorder.events();
    let waited = events.iter().position(|event| {
        matches!(
            event,
            Event::SemaphoresWaited { waits, reached: true }
                if !waits.is_empty()
                    && waits.iter().all(|wait| wait.value == PendingUpload::LANDED)
        )
    });
    let gone = events
        .iter()
        .position(|event| matches!(event, Event::Destroyed { .. }));
    matches!((waited, gone), (Some(waited), Some(gone)) if waited < gone)
}

/// One frame of the primary camera's blocks, which is where the uploads are
/// retired.
fn begin(device: &dyn Device, renderer: &mut ForwardRenderer) {
    renderer
        .begin_frame(
            device,
            &Camera::default(),
            &DirectionalLight::default(),
            TEST_EXTENT,
        )
        .expect("the null backend writes every block");
}

/// **The batch is not waited for**: everything the build waits for is a page
/// or the probe table, however many views, shadow culls and frames in flight
/// recorded into the batch — which is submitted once, signalling the value
/// its retirement waits for. One fewer `wait_idle` than when the batch ended
/// in one.
#[test]
fn the_build_does_not_wait_for_its_batch() {
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
        waits, WAITS_OUTSIDE_THE_BATCH,
        "the pages and the probe table, and nothing for the batch"
    );
    assert_eq!(
        submissions_of(&recorder, BUILD_BATCH),
        (1, 1),
        "the build's batch is submitted once, signalling its semaphore"
    );
    renderer.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **The build's staging outlives the build**: alive when `new` returns,
/// destroyed by the first frame that finds the batch's semaphore signalled,
/// and destroyed once.
#[test]
fn the_build_batch_staging_lives_until_the_first_frame() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let before = recorder.total_live_objects();
    recorder.clear();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb)
        .expect("the null backend accepts every descriptor");
    let staging = staging_of(&recorder, BUILD_BATCH);
    assert!(
        staging.len() >= 5,
        "the placeholders and tables at least: {staging:?}"
    );
    for buffer in &staging {
        assert!(
            recorder.buffer_size(*buffer).is_some(),
            "{buffer:?} was destroyed before the GPU was known to be past it"
        );
    }
    assert_eq!(renderer.uploads_in_flight.len(), 1);

    begin(device, &mut renderer);
    for buffer in &staging {
        assert_eq!(
            recorder.buffer_size(*buffer),
            None,
            "{buffer:?} outlived the frame that saw its copy land"
        );
    }
    assert!(renderer.uploads_in_flight.is_empty());

    begin(device, &mut renderer);
    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), before);
    recorder.assert_valid();
}

/// **A renderer destroyed before its first frame waits for its upload**
/// before destroying anything — nothing else has waited for it, and its
/// copies write the placeholders and fills `destroy` releases.
#[test]
fn destroy_before_the_first_frame_waits_for_the_upload_first() {
    let (recorder, device, queue) = open();
    let before = recorder.total_live_objects();
    let renderer = ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
        .expect("the null backend accepts every descriptor");
    recorder.clear();
    renderer.destroy(device.as_ref());
    assert!(
        waited_before_anything_went(&recorder),
        "destroy released something before waiting for the upload that writes it"
    );
    assert_eq!(recorder.total_live_objects(), before);
    recorder.assert_valid();
}

/// **A view made after the build is one submission and no wait**: its cull
/// state, its exposure ring and its reflection table share a batch of its own,
/// whose staging lives until a frame retires it.
#[test]
fn a_late_view_does_not_wait() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb)
        .expect("the null backend accepts every descriptor");
    begin(device, &mut renderer);
    recorder.clear();
    let view = renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("a second view fits");
    assert_eq!(waits_and_submits(&recorder), (0, 1));
    assert_eq!(submissions_of(&recorder, VIEW_BATCH), (1, 1));
    let staging = staging_of(&recorder, VIEW_BATCH);
    assert!(!staging.is_empty(), "the view's fills have staging");
    for buffer in &staging {
        assert!(recorder.buffer_size(*buffer).is_some());
    }
    begin(device, &mut renderer);
    for buffer in &staging {
        assert_eq!(recorder.buffer_size(*buffer), None, "retired by the frame");
    }
    renderer.destroy_view(device, view);
    renderer.destroy(device);
    recorder.assert_valid();
}

/// **A view destroyed before any frame waits for its upload first**, on
/// [`destroy_before_the_first_frame_waits_for_the_upload_first`]'s terms.
#[test]
fn destroying_a_view_before_a_frame_waits_for_its_upload_first() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb)
        .expect("the null backend accepts every descriptor");
    begin(device, &mut renderer);
    let view = renderer
        .create_view(device, queue, &ViewDesc::default())
        .expect("a second view fits");
    recorder.clear();
    renderer.destroy_view(device, view);
    assert!(
        waited_before_anything_went(&recorder),
        "destroy_view released the view before waiting for the fills that write it"
    );
    assert!(renderer.uploads_in_flight.is_empty());
    renderer.destroy(device);
    recorder.assert_valid();
}

/// **New wind layers are one submission and no wait**, retired by a frame.
#[test]
fn setting_wind_layers_does_not_wait() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb)
        .expect("the null backend accepts every descriptor");
    begin(device, &mut renderer);
    recorder.clear();
    renderer
        .set_wind_layers(device, queue, None)
        .expect("the calm layers upload");
    assert_eq!(waits_and_submits(&recorder), (0, 1));
    assert_eq!(submissions_of(&recorder, "wind upload"), (1, 1));
    assert_eq!(renderer.uploads_in_flight.len(), 1);
    begin(device, &mut renderer);
    assert!(renderer.uploads_in_flight.is_empty());
    renderer.destroy(device);
    recorder.assert_valid();
}

/// **The build's last creation is its batch's semaphore**, and failing it
/// leaves nothing alive and submits no batch.
///
/// The submit is the last fallible step of the build, so there is no failure
/// *after* it for a rollback to wait through: this is the latest a build can
/// fail, and it must fail before anything is pending.
#[test]
fn a_build_failing_at_its_batch_semaphore_leaks_nothing() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let before = recorder.total_live_objects();
    recorder.clear();
    let renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb)
        .expect("the null backend accepts every descriptor");
    // What `Recorder::fail_create_after` counts: a command buffer is created
    // by `finish`, which cannot fail, so it is recorded and not counted.
    let created: Vec<_> = recorder
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::Created { kind, label } if *kind != ObjectKind::CommandBuffer => {
                Some((*kind, label.clone()))
            }
            _ => None,
        })
        .collect();
    renderer.destroy(device);
    assert_eq!(
        created.last(),
        Some(&(ObjectKind::Semaphore, Some(BUILD_BATCH.to_owned()))),
        "the batch's semaphore is the build's last creation"
    );

    recorder.clear();
    recorder.fail_create_after(u32::try_from(created.len() - 1).expect("fits"));
    let error = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb)
        .expect_err("the semaphore's creation was failed");
    assert!(
        !recorder.create_failure_pending(),
        "the build failed before reaching its last creation: {error}"
    );
    assert!(matches!(error, HalError::OutOfDeviceMemory), "{error}");
    assert!(
        !recorder.events().iter().any(|event| matches!(
            event,
            Event::Created { kind: ObjectKind::Semaphore, label: Some(label) } if label == BUILD_BATCH
        )),
        "the failed creation was the batch's semaphore"
    );
    assert_eq!(submissions_of(&recorder, BUILD_BATCH), (0, 0));
    assert_eq!(
        recorder.total_live_objects(),
        before,
        "the failed build leaked"
    );
    recorder.assert_valid();
}

/// **A queue other than the graphics queue is refused**, before anything is
/// created: the uploads are not waited for, and only the frames' own queue
/// orders their copies before them.
#[test]
fn a_renderer_on_another_queue_is_refused() {
    let (recorder, device, graphics) = open();
    let device = device.as_ref();
    let before = recorder.total_live_objects();
    let mut refused = 0;
    for kind in [QueueKind::Compute, QueueKind::Transfer] {
        let Some(queue) = device.queue(kind) else {
            continue;
        };
        assert_ne!(queue, graphics);
        match ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb) {
            Err(HalError::InvalidDescriptor(message)) => {
                assert!(message.contains("graphics queue"), "{message}");
            }
            other => panic!("a {kind:?} queue was not refused: {:?}", other.err()),
        }
        assert_eq!(recorder.total_live_objects(), before);
        let mut renderer = ForwardRenderer::new(device, graphics, Format::Rgba8UnormSrgb)
            .expect("the graphics queue builds");
        assert!(matches!(
            renderer.create_view(device, queue, &ViewDesc::default()),
            Err(HalError::InvalidDescriptor(_))
        ));
        assert!(matches!(
            renderer.set_wind_layers(device, queue, None),
            Err(crate::grass::WindLayerError::Hal(
                HalError::InvalidDescriptor(_)
            ))
        ));
        renderer.destroy(device);
        refused += 1;
    }
    assert!(refused > 0, "the null adapter models no second queue");
    assert_eq!(recorder.total_live_objects(), before);
    recorder.assert_valid();
}
