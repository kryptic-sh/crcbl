//! The command-pool free list, against a real driver.
//!
//! `crcbl-vk` keeps the pool of a retired command buffer and resets it for the
//! next encoder of its queue family. The rules — nothing is reset before the
//! submission that last used it has completed, the list is bounded — are unit
//! tests in `src/command_pools.rs`; what only a driver can say is whether the
//! pools this device actually resets, keeps and frees are ones it may. The
//! observable is the layer: `vkResetCommandPool` under a pending buffer is
//! `VUID-vkResetCommandPool-commandPool-00040`, and a kept pool nobody frees is
//! reported when the device is destroyed. Every test ends in
//! [`Headless::finish`], which asserts a clean report after the device is
//! gone, so a leaked pool fails the test that leaked it.

use crate::harness::Headless;
use crcbl_hal::{
    CommandBufferHandle, CommandEncoderDesc, Device, QueueHandle, SemaphoreDesc, SemaphoreKind,
    SemaphoreSignal, SemaphoreWait, SubmitInfo,
};

/// Records a command buffer that touches no memory, so consecutive
/// submissions have no hazard between them for sync validation to report —
/// what is under test is the pool, not the work — and finishes it.
fn record(device: &dyn Device, queue: QueueHandle) -> CommandBufferHandle {
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("pool reuse"),
        queue,
    });
    encoder.begin_debug_label("pool reuse");
    encoder.end_debug_label();
    encoder.finish().expect("recording succeeded")
}

/// **A command buffer released while its submission is still pending does not
/// have its pool reset under it.**
///
/// The seam says `destroy_command_buffer` waits for the submission to
/// complete, and this breaks that on purpose, deterministically: the first
/// submission waits on a timeline value only the host signals, so it is
/// pending for certain when its buffer is destroyed and the next encoder
/// begins. The pool must not be the one that encoder resets. A free list that
/// trusted the seam, or ignored the timeline, resets it there — which the
/// layer reports as `VUID-vkResetCommandPool-commandPool-00040`.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_released_command_buffer_is_not_reset_while_its_submission_is_pending() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    let gate = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("pool reuse gate"),
            kind: SemaphoreKind::Timeline { initial_value: 0 },
        })
        .expect("a timeline semaphore");
    let held = record(device, headless.queue);
    device
        .submit(
            headless.queue,
            &SubmitInfo {
                command_buffers: &[held],
                waits: &[SemaphoreWait {
                    semaphore: gate,
                    value: 1,
                }],
                signals: &[],
            },
        )
        .expect("submit");
    // Pending: nothing has signalled the gate.
    device.destroy_command_buffer(held);

    let behind = record(device, headless.queue);
    device
        .submit(headless.queue, &SubmitInfo::new(&[behind]))
        .expect("submit");

    device
        .signal_semaphore(gate, 1)
        .expect("the host opens the gate");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(behind);

    // Both submissions are done, so both pools may be reset now; one is.
    let after = record(device, headless.queue);
    device
        .submit(headless.queue, &SubmitInfo::new(&[after]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(after);

    device.destroy_semaphore(gate);
    headless.finish();
}

/// **A frame loop with two frames in flight recycles its pools cleanly.**
///
/// The engine's shape: submit a frame signalling its number on a timeline,
/// then wait for the frame two back and destroy its command buffer, so every
/// encoder after the warm-up begins on a pool the timeline has released. More
/// frames than the free list holds, so a pool is reset many times over.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_frame_loop_recycles_its_pools_with_validation_silent() {
    const FRAMES: u64 = 40;
    const IN_FLIGHT: usize = 2;

    let headless = Headless::open();
    let device = headless.device.as_ref();
    let timeline = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("pool reuse frames"),
            kind: SemaphoreKind::Timeline { initial_value: 0 },
        })
        .expect("a timeline semaphore");

    let mut in_flight: Vec<(u64, CommandBufferHandle)> = Vec::new();
    for frame in 1..=FRAMES {
        let commands = record(device, headless.queue);
        device
            .submit(
                headless.queue,
                &SubmitInfo {
                    command_buffers: &[commands],
                    waits: &[],
                    signals: &[SemaphoreSignal {
                        semaphore: timeline,
                        value: frame,
                    }],
                },
            )
            .expect("submit");
        in_flight.push((frame, commands));
        while in_flight.len() > IN_FLIGHT {
            let (value, retired) = in_flight.remove(0);
            assert!(
                device
                    .wait_semaphores(
                        &[SemaphoreWait {
                            semaphore: timeline,
                            value
                        }],
                        u64::MAX
                    )
                    .expect("the wait did not fail"),
                "an infinite wait cannot time out"
            );
            device.destroy_command_buffer(retired);
        }
    }

    device.wait_idle().expect("idle");
    for (_, commands) in in_flight {
        device.destroy_command_buffer(commands);
    }
    device.destroy_semaphore(timeline);
    headless.finish();
}

/// **An encoder dropped without finishing gives its pool back.**
///
/// Its buffer is still recording and never reached a queue, so the pool is
/// kept and reset for the next encoder at once. A pool that went nowhere would
/// be reported as leaked when the device is destroyed.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn an_encoder_dropped_unfinished_gives_its_pool_back() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    let mut abandoned = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("pool reuse (abandoned)"),
        queue: headless.queue,
    });
    // Left open on purpose: the buffer is mid-recording when it is dropped.
    abandoned.begin_debug_label("abandoned");
    drop(abandoned);

    let commands = record(device, headless.queue);
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    headless.finish();
}
