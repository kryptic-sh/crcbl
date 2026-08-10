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
    timestamp.write_timestamp(set, 4);
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
    for index in 0..4 {
        encoder.write_timestamp(set, index);
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
    let headless = Headless::open();
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
    drop(headless.device);
    headless.instance.validation_report().assert_clean();
}
