use crate::harness::Headless;
use crcbl_hal::{BufferDesc, BufferUsage, CommandEncoderDesc, MemoryLocation, SubmitInfo};

/// Destroy a resource **after recording but before submitting**, then submit.
///
/// Regression test for the deletion queue's retirement key. It used to be
/// `submissions()` — the count of submissions *already issued* — so an object
/// destroyed between `finish` and `submit` was freed by the very `poll_retire`
/// that runs at the end of that submit, while the batch reading it was
/// executing. Validation reports it as "destroyed while in use"; without
/// validation it is a silent use-after-free.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn destroying_a_resource_between_recording_and_submitting_is_safe() {
    let headless = Headless::open();
    let device = &headless.device;

    let buffer = |label, usage, memory| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: 4096,
                usage,
                memory,
            })
            .expect("a buffer")
    };
    let source = buffer(
        "recorded then destroyed (src)",
        BufferUsage::TRANSFER_SRC,
        MemoryLocation::HostUpload,
    );
    let destination = buffer(
        "recorded then destroyed (dst)",
        BufferUsage::TRANSFER_DST,
        MemoryLocation::DeviceLocal,
    );
    device.write_buffer(source, 0, &[3u8; 4096]).expect("write");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("copy"),
        queue: headless.queue,
    });
    encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
        src: source,
        src_offset: 0,
        dst: destination,
        dst_offset: 0,
        size: 4096,
    });
    let commands = encoder.finish().expect("recording succeeded");

    // The window that matters: recorded, not yet submitted.
    device.destroy_buffer(source);
    device.destroy_buffer(destination);

    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    headless.finish();
}

/// The deletion queue, exercised where it matters: destroy a resource while a
/// submission that used it is still in flight, and let the device tear down.
/// A backend freeing inline would trip "object destroyed while in use".
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn destroying_a_resource_mid_flight_is_safe() {
    let headless = Headless::open();
    let device = &headless.device;

    let source = device
        .create_buffer(&BufferDesc {
            label: Some("mid-flight source"),
            size: 4096,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a buffer");
    let destination = device
        .create_buffer(&BufferDesc {
            label: Some("mid-flight destination"),
            size: 4096,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a buffer");
    device.write_buffer(source, 0, &[9u8; 4096]).expect("write");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("copy"),
        queue: headless.queue,
    });
    encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
        src: source,
        src_offset: 0,
        dst: destination,
        dst_offset: 0,
        size: 4096,
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    // The seam is explicit: `destroy_*` means "this handle is dead now", not
    // "the GPU is finished". Both of these are still being read and written by
    // the submission above.
    device.destroy_buffer(source);
    device.destroy_buffer(destination);
    let error = device
        .write_buffer(source, 0, &[0; 4])
        .expect_err("the handle really is dead");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidHandle { .. }),
        "{error}"
    );

    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    headless.finish();
}

/// Two submissions referencing one destroyed buffer: the deletion queue must
/// keep the buffer parked until the **last** submission using it completes.
///
/// `park` keys a destroyed object at `submissions() + 1`, which is correct for
/// one future submission and a use-after-free for two: the timeline reaches the
/// first submission's value while the second is still queued or running, and
/// the buffer's `vkDestroyBuffer` runs under it. Recording buffer X into two
/// command buffers and submitting both is exactly the repro — the seam permits
/// record → destroy → submit, twice over.
///
/// Whether lavapipe has finished submission A when submit(B)'s poll runs is
/// timing-dependent, so this is the integration smoke test; the deterministic
/// proof is `RetireQueue`'s own unit tests for `extend_matching` and the
/// scan-all `retire`.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn two_submissions_referencing_one_destroyed_buffer_keep_it_alive() {
    let headless = Headless::open();
    let device = &headless.device;

    let source = device
        .create_buffer(&BufferDesc {
            label: Some("destroyed, still referenced"),
            size: 4096,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a source buffer");
    let record_copy = |label, dst| {
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some(label),
            queue: headless.queue,
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src: source,
            src_offset: 0,
            dst,
            dst_offset: 0,
            size: 4096,
        });
        encoder.finish().expect("recording succeeded")
    };
    let dst_a = device
        .create_buffer(&BufferDesc {
            label: Some("first submission (dst)"),
            size: 4096,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a destination buffer");
    let dst_b = device
        .create_buffer(&BufferDesc {
            label: Some("second submission (dst)"),
            size: 4096,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a destination buffer");
    let a = record_copy("copy to A", dst_a);
    let b = record_copy("copy to B", dst_b);

    // The seam's record → destroy → submit order. Both submissions reference
    // X, so X must stay parked until B completes — freeing it when A's value is
    // reached would destroy it while B is queued or running.
    device.destroy_buffer(source);
    device
        .submit(headless.queue, &SubmitInfo::new(&[a]))
        .expect("submit A");
    device
        .submit(headless.queue, &SubmitInfo::new(&[b]))
        .expect("submit B");

    device.wait_idle().expect("idle");
    device.destroy_command_buffer(a);
    device.destroy_command_buffer(b);
    device.destroy_buffer(dst_a);
    device.destroy_buffer(dst_b);
    headless.finish();
}
