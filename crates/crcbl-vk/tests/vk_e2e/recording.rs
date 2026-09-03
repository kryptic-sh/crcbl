//! Failures a command encoder must report from `finish`, having never reached a
//! queue.
//!
//! Both tests are about `crcbl-vk`'s own encoder rather than about the seam,
//! which is why they are here and not in `crcbl-hal`: the null recorder already
//! bounds-checks the same query ranges, so it cannot reproduce either bug. An
//! out-of-range reset, timestamp or resolve used to be recorded and handed to
//! the driver — where the validation layer flags it — and a failing call inside
//! a labelled pass used to spin `finish`'s label-closing loop forever, an
//! infinite loop with no output at all.
//!
//! The query test records valid commands on the same set afterwards, so a green
//! run says the *range* was rejected rather than the set being poisoned.

use crate::harness::Headless;
use crcbl_hal::{
    BufferDesc, BufferUsage, ClearValue, ColorAttachment, CommandEncoderDesc, Features, Instance,
    LoadOp, MemoryLocation, QueryKind, QuerySetDesc, Rect2d, RenderPassDesc, StoreOp, SubmitInfo,
};

/// Query commands with caller-supplied ranges must bounds-check them against
/// the pool's count **at record time**, matching `Device::query_results` and
/// the null backend — an out-of-range reset, timestamp or resolve used to be
/// recorded and handed to the driver, which the validation layer flags.
///
/// The three failing encoders never reach a queue: `finish` reports the
/// `InvalidDescriptor` and drops the recording. The valid calls on the same
/// set afterwards record cleanly, so the failure is the range, not the set.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn out_of_range_query_commands_fail_recording_not_the_driver() {
    let headless = Headless::open();
    let device = &headless.device;
    let set = device.create_query_set(&QuerySetDesc {
        label: Some("vk e2e query bounds"),
        kind: QueryKind::Timestamp,
        count: 4,
    });
    let Ok(set) = set else {
        assert!(
            !device.caps().features.contains(Features::TIMESTAMP_QUERY),
            "a device reporting TIMESTAMP_QUERY must create a timestamp set"
        );
        headless.finish();
        return;
    };
    let dst = device
        .create_buffer(&BufferDesc {
            label: Some("vk e2e query resolve dst"),
            size: 4 * 8,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a resolve destination");

    let mut resolve = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("vk e2e out-of-range resolve"),
        queue: headless.queue,
    });
    resolve.resolve_query_set(set, 2..8, dst, 0);
    let error = resolve
        .finish()
        .expect_err("an out-of-range resolve must fail recording, not the driver");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidDescriptor { .. }),
        "the out-of-range resolve is a descriptor problem: {error}"
    );

    let mut reset = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("vk e2e out-of-range reset"),
        queue: headless.queue,
    });
    reset.reset_query_set(set, 3..9);
    let error = reset
        .finish()
        .expect_err("an out-of-range reset must fail recording, not the driver");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidDescriptor { .. }),
        "the out-of-range reset is a descriptor problem: {error}"
    );

    let mut timestamp = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("vk e2e out-of-range timestamp"),
        queue: headless.queue,
    });
    timestamp.begin_compute_pass(&crcbl_hal::ComputePassDesc {
        label: None,
        timestamp_writes: Some(crcbl_hal::PassTimestampWrites {
            set,
            beginning_of_pass: 3,
            end_of_pass: 4,
        }),
    });
    timestamp.end_compute_pass();
    let error = timestamp
        .finish()
        .expect_err("an out-of-range timestamp index must fail recording, not the driver");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidDescriptor { .. }),
        "the out-of-range timestamp is a descriptor problem: {error}"
    );

    // The valid calls on the same set still record cleanly. Every resolved
    // query must also have been issued, or the layer flags
    // `VUID-vkCmdCopyQueryPoolResults-None-08752` (a reset-but-never-issued
    // query) — so all four slots get a timestamp before the full-range resolve.
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("vk e2e in-range queries"),
        queue: headless.queue,
    });
    encoder.reset_query_set(set, 0..4);
    for pair in 0..2 {
        encoder.begin_compute_pass(&crcbl_hal::ComputePassDesc {
            label: None,
            timestamp_writes: Some(crcbl_hal::PassTimestampWrites {
                set,
                beginning_of_pass: pair * 2,
                end_of_pass: pair * 2 + 1,
            }),
        });
        encoder.end_compute_pass();
    }
    encoder.resolve_query_set(set, 0..4, dst, 0);
    let commands = encoder.finish().expect("in-range query recording succeeds");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");

    device.destroy_command_buffer(commands);
    device.destroy_buffer(dst);
    device.destroy_query_set(set);
    headless.finish();
}

/// A failing encoder must still `finish`, and must report the failure.
///
/// Regression test. `finish` used to close open debug labels with
/// `while self.label_depth > 0 { self.end_debug_label(); }`, and
/// `end_debug_label` used to return early once anything had failed — so the
/// first failing call inside a labelled pass produced an **infinite loop with
/// no output**. `begin_render_pass` opens a label, and `push_constants` cannot
/// succeed until P1.2, so those two lines are the whole reproduction.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_failed_recording_finishes_with_an_error_rather_than_hanging() {
    let mut headless = Headless::open();
    let device = &headless.device;

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("an image");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("doomed"),
        queue: headless.queue,
    });
    // Opens a debug label as a side effect of the pass label.
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("labelled"),
        color_attachments: &[ColorAttachment {
            view: acquired.view,
            resolve: None,
            load: LoadOp::DontCare,
            store: StoreOp::Store,
            clear: ClearValue::default(),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(acquired.extent.0, acquired.extent.1),
        timestamp_writes: None,
    });
    // Leave the pass deliberately open so `finish` returns the error
    // rather than a command buffer.
    let error = encoder
        .finish()
        .expect_err("a failed recording must not produce a command buffer");
    assert!(
        error.to_string().contains("render pass"),
        "the unfinished render pass is reported: {error}"
    );

    device.wait_idle().expect("idle");
    device.destroy_swapchain(headless.swapchain);
    headless.instance.destroy_surface(headless.surface);
    headless.device.destroy();
    headless.instance.validation_report().assert_clean();
}

/// A command buffer must be submitted to a queue of the family its pool was
/// created on, and `submit` must say so rather than hand the mismatch to the
/// driver.
///
/// Vulkan requires the two to match and defines nothing about what happens when
/// they do not, so this was the one handle misuse in this backend that was
/// undefined behaviour instead of an error: `CommandBufferEntry` recorded the
/// pool but not the family it came from, and `submit` looked at neither. Every
/// other foreign or stale handle here answers with a `HalError`.
///
/// **A second queue being `Some` is what makes the mismatch real.**
/// `adapter::Families` fills the compute slot only from a family carrying
/// `COMPUTE` and not `GRAPHICS`, and the transfer slot only from one carrying
/// `TRANSFER` and neither of the others — so a device that offers either has a
/// family genuinely distinct from its graphics family, and the test never has
/// to ask which index any of them landed on.
///
/// **The features have to be asked for.** `Headless::open`'s device requests
/// neither `ASYNC_COMPUTE_QUEUE` nor `TRANSFER_QUEUE`, so both slots are empty
/// there whatever the hardware offers, and a test built on it would skip on
/// every driver in the world while looking like it had run. This opens its own
/// device asking for both.
///
/// A device that really has neither cannot commit this misuse at all, and says
/// so rather than passing quietly — lavapipe is such a device, and a green run
/// there would be reporting on a submission that was always legal.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_command_buffer_is_refused_by_a_queue_of_another_family() {
    let mut headless = Headless::open_pinning_format(
        "vk e2e cross-family submit",
        Features::ASYNC_COMPUTE_QUEUE | Features::TRANSFER_QUEUE,
        (64, 64),
    );
    let device = &headless.device;

    let graphics = device
        .queue(crcbl_hal::QueueKind::Graphics)
        .expect("every device has a graphics queue");
    let transfer = device
        .queue(crcbl_hal::QueueKind::Compute)
        .or_else(|| device.queue(crcbl_hal::QueueKind::Transfer));

    let encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("recorded on graphics"),
        queue: graphics,
    });
    let commands = encoder.finish().expect("an empty recording finishes");

    if let Some(transfer) = transfer {
        let error = device
            .submit(
                transfer,
                &SubmitInfo {
                    command_buffers: &[commands],
                    waits: &[],
                    signals: &[],
                },
            )
            .expect_err("a command buffer from the graphics family must not submit to transfer");
        assert!(
            error.to_string().contains("family"),
            "the refusal names the family mismatch: {error}"
        );
    } else {
        eprintln!(
            "crcbl vk e2e: this device has neither an async-compute nor a dedicated transfer \
             family, so there is no cross-family submission to refuse — that is every device \
             whose every queue also carries graphics, lavapipe included"
        );
        device
            .submit(
                graphics,
                &SubmitInfo {
                    command_buffers: &[commands],
                    waits: &[],
                    signals: &[],
                },
            )
            .expect("the same family it was recorded on is always legal");
    }

    // **A refused submission still owns its recording.** `submit` returning an
    // error means the driver never took the command buffer, so nothing retires
    // it and the suite's teardown reporter counts it as an object outliving the
    // device — which is how this test first failed CI while every assertion in
    // it passed.
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    device.destroy_swapchain(headless.swapchain);
    headless.instance.destroy_surface(headless.surface);
    headless.device.destroy();
    headless.instance.validation_report().assert_clean();
}
