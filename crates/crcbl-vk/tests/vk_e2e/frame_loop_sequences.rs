//! Regressions from the 2026-08 code review: paths it identified as both wrong
//! and untested.
//!
//! The module exists because these are *sequences* rather than subjects — a
//! read-only depth attachment, a submit that fails, a readback whose wait
//! semaphore is destroyed under it, a reconfigure between acquire and present.
//! Each is something a real frame loop does and the offscreen suite never did,
//! and none of them belongs to any one of the milestone modules.
//!
//! Their failure modes are why they need a driver at all rather than the null
//! backend: `a_read_only_depth_pass_uses_the_read_only_layout` asserts nothing
//! but the validation report, and the wedged-retire-timeline one is observable
//! only as every later `request_readback` returning `Pending` forever.
//!
//! One is knowingly partial. `a_reconfigure_between_acquire_and_present_is_survivable`
//! proves the *handle* half headlessly and says in place why the fence half
//! needs a compositor: an offscreen ring has an implicit acquire and therefore
//! no fences to destroy. The sequence is written here anyway because it is the
//! same one the windowed sandbox runs drive.

use crate::harness::{CLEAR, EXTENT, Headless, poisoned};
use crcbl_hal::{
    Barriers, BufferDesc, BufferUsage, ClearValue, ColorAttachment, CommandEncoderDesc,
    CompositeAlpha, Extent3d, Format, ImageDesc, ImageSubresourceRange, ImageType, ImageUsage,
    ImageViewDesc, ImageViewType, LoadOp, MemoryLocation, PresentInfo, PresentMode, ReadbackDesc,
    ReadbackState, Rect2d, RenderPassDesc, ResourceState, SemaphoreDesc, SemaphoreKind,
    SemaphoreSignal, SemaphoreWait, StoreOp, SubmitInfo, SwapchainDesc,
};

/// A **read-only** depth attachment must begin rendering in the layout the
/// image is actually in.
///
/// `conv::state_masks(ResourceState::DepthStencilRead)` transitions the image to
/// `DEPTH_STENCIL_READ_ONLY_OPTIMAL`, and the encoder used to hardcode
/// `DEPTH_STENCIL_ATTACHMENT_OPTIMAL` in `VkRenderingAttachmentInfo::imageLayout`
/// regardless — so a depth-prepass-shaped pass began rendering with a layout
/// that did not match, which is
/// `VUID-vkCmdBeginRendering-pDepthAttachment-06135` and leaves the attachment
/// contents undefined. [`DepthStencilAttachment::read_only`] is what the seam
/// now carries to say which of the two states was declared, and this is the
/// sequence that exercises both: write depth, transition, read it.
///
/// The validation report is the assertion.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_read_only_depth_pass_uses_the_read_only_layout() {
    let headless = Headless::open();
    let device = &headless.device;

    const DEPTH: Format = Format::D32Float;
    let depth = device
        .create_image(&ImageDesc {
            label: Some("vk e2e depth"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(EXTENT.0, EXTENT.1),
            format: DEPTH,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
        })
        .expect("a depth target");
    let depth_view = device
        .create_image_view(&ImageViewDesc {
            label: Some("vk e2e depth view"),
            image: depth,
            view_type: ImageViewType::D2,
            format: DEPTH,
            range: ImageSubresourceRange::all(DEPTH),
        })
        .expect("a depth view");

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("vk e2e depth frame"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[
            crcbl_hal::ImageBarrier::new(
                acquired.image,
                ImageSubresourceRange::all(headless.format),
                ResourceState::Undefined,
                ResourceState::ColorAttachment,
            ),
            crcbl_hal::ImageBarrier::new(
                depth,
                ImageSubresourceRange::all(DEPTH),
                ResourceState::Undefined,
                ResourceState::DepthStencilWrite,
            ),
        ],
        ..Barriers::default()
    });

    // The prepass: depth is written, so the attachment layout must be
    // `DEPTH_STENCIL_ATTACHMENT_OPTIMAL`.
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("depth prepass"),
        color_attachments: &[],
        depth_stencil_attachment: Some(crcbl_hal::DepthStencilAttachment {
            view: depth_view,
            read_only: false,
            depth_load: LoadOp::Clear,
            depth_store: StoreOp::Store,
            // A `D32Float` view has no stencil plane; the encoder keys the
            // stencil attachment off the *format*, so these are inert.
            stencil_load: LoadOp::DontCare,
            stencil_store: StoreOp::Discard,
            clear: ClearValue::default(),
        }),
        render_area: Rect2d::from_size(EXTENT.0, EXTENT.1),
        timestamp_writes: None,
    });
    encoder.end_render_pass();

    // The transition the graph emits for `PassBuilder::depth_read`.
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            depth,
            ImageSubresourceRange::all(DEPTH),
            ResourceState::DepthStencilWrite,
            ResourceState::DepthStencilRead,
        )],
        ..Barriers::default()
    });

    // The pass the fix exists for: depth is tested, never written, and the
    // image is in `DEPTH_STENCIL_READ_ONLY_OPTIMAL`.
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("depth read"),
        color_attachments: &[ColorAttachment {
            view: acquired.view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(CLEAR),
        }],
        depth_stencil_attachment: Some(crcbl_hal::DepthStencilAttachment {
            view: depth_view,
            read_only: true,
            depth_load: LoadOp::Load,
            depth_store: StoreOp::Store,
            stencil_load: LoadOp::DontCare,
            stencil_store: StoreOp::Discard,
            clear: ClearValue::default(),
        }),
        render_area: Rect2d::from_size(EXTENT.0, EXTENT.1),
        timestamp_writes: None,
    });
    encoder.end_render_pass();

    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");
    device.wait_idle().expect("idle");

    device.destroy_command_buffer(commands);
    device.destroy_image_view(depth_view);
    device.destroy_image(depth);
    headless.finish();
}

/// A submission that **fails** must not consume a value on the retire timeline.
///
/// The counter used to be bumped before the signal semaphores were resolved and
/// before `vkQueueSubmit2` ran, so any failure left a value nothing would ever
/// signal. From that moment `poll_retire` sat below every later-parked object
/// forever: the deletion queue never drained again, and — the observable half,
/// and what this test asserts — every `request_readback` with no explicit wait
/// returned `Pending` for the rest of the process's life.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_failed_submit_does_not_wedge_the_retire_timeline() {
    let headless = Headless::open();
    let device = &headless.device;

    // A semaphore that is dead by the time `submit` resolves it. Nothing else
    // about the submission is wrong, so the failure lands exactly where the
    // counter used to have been bumped already.
    let dead = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("vk e2e dead signal"),
            kind: SemaphoreKind::Binary,
        })
        .expect("a semaphore");
    device.destroy_semaphore(dead);

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("vk e2e doomed"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        global: true,
        ..Barriers::default()
    });
    let doomed = encoder.finish().expect("recording succeeded");

    let error = device
        .submit(
            headless.queue,
            &SubmitInfo {
                command_buffers: &[doomed],
                waits: &[],
                signals: &[SemaphoreSignal {
                    semaphore: dead,
                    value: 1,
                }],
            },
        )
        .expect_err("a destroyed signal semaphore must fail the submit");
    assert!(
        matches!(
            error,
            crcbl_hal::HalError::InvalidHandle { .. } | crcbl_hal::HalError::ForeignObject { .. }
        ),
        "the failure must name the handle: {error}"
    );

    // Now prove the device is still usable. A readback with `after: None`
    // watches the retire timeline's *current* value, so it completes only if
    // the counter and the timeline still agree.
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("vk e2e after-failure readback"),
            size: 256,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("vk e2e recovery"),
        queue: headless.queue,
    });
    encoder.clear_buffer(staging, 0, 256);
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("the device is still usable after a failed submit");

    let readback = device
        .request_readback(&ReadbackDesc {
            label: Some("vk e2e recovery readback"),
            buffer: staging,
            offset: 0,
            size: 256,
            after: None,
        })
        .expect("a readback request");
    let mut bytes = poisoned(256);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match device
            .poll_readback(readback, &mut bytes)
            .expect("the readback did not fail")
        {
            ReadbackState::Ready => break,
            ReadbackState::Pending => assert!(
                std::time::Instant::now() < deadline,
                "the retire timeline never reached the readback's value, which is exactly \
                 what a submission counter bumped past a failed submit produces"
            ),
        }
        std::thread::yield_now();
    }

    device.wait_idle().expect("idle");
    device.destroy_readback(readback);
    device.destroy_buffer(staging);
    device.destroy_command_buffer(commands);
    device.destroy_command_buffer(doomed);
    headless.finish();
}

/// A readback whose explicit wait semaphore is destroyed before polling must
/// fail cleanly, never dereference the destroyed semaphore.
///
/// Regression test. The completion point used to be stored as the raw
/// `VkSemaphore` and dereferenced at poll time with no liveness check, so
/// destroying the semaphore between request and poll was undefined behaviour.
/// It is now stored as a generational handle and re-resolved through the pool
/// at poll time — exactly like the buffer — so a destroyed semaphore fails
/// lookup and reports [`HalError::InvalidHandle`] instead.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_readback_whose_wait_semaphore_is_destroyed_fails_cleanly() {
    let headless = Headless::open();
    let device = &headless.device;

    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some("vk e2e orphaned readback"),
            size: 64,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    let semaphore = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("vk e2e readback wait"),
            kind: SemaphoreKind::Timeline { initial_value: 0 },
        })
        .expect("timeline semaphores are a hard requirement of this backend");

    let readback = device
        .request_readback(&ReadbackDesc {
            label: Some("vk e2e orphaned readback"),
            buffer,
            offset: 0,
            size: 64,
            after: Some(SemaphoreWait {
                semaphore,
                value: 1,
            }),
        })
        .expect("a readback request");
    device.destroy_semaphore(semaphore);

    let mut bytes = poisoned(64);
    let error = device
        .poll_readback(readback, &mut bytes)
        .expect_err("a destroyed wait semaphore must fail the poll, not be UB");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidHandle { .. }),
        "the poll must report the destroyed semaphore: {error}"
    );

    device.destroy_readback(readback);
    device.destroy_buffer(buffer);
    headless.finish();
}

/// Acquire, then reconfigure **before ever presenting** — the ordinary shape of
/// a window resize, and the one that used to destroy a fence still in use.
///
/// `retire_swapchain` idles the device and then frees the acquire semaphores
/// and fences, but `vkDeviceWaitIdle` does **not** complete a pending
/// `vkAcquireNextImageKHR`: it is not queue work. So this sequence left one slot
/// with an unsignalled fence and a semaphore with an outstanding signal, and
/// destroyed both. `FrameSync::acquire_armed` records which slots are
/// outstanding and `drain_pending_acquires` now waits on them first.
///
/// The offscreen ring has an implicit acquire and therefore no fences, so what
/// this test proves headlessly is the *handle* half — every image and view a
/// reconfigure reissues, and the old ones going dead. The fence half needs a
/// compositor and is covered by the windowed sandbox runs; the sequence is the
/// same one, which is why it is written here.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_reconfigure_between_acquire_and_present_is_survivable() {
    let headless = Headless::open();

    for extent in [(80u32, 60u32), (48, 32), EXTENT] {
        let acquired = headless
            .device
            .acquire_next_frame(headless.swapchain)
            .expect("the ring always has an image");
        let stale_view = acquired.view;

        // The resize event arrives between the acquire and any present. No
        // submission has touched the image, and none will.
        headless
            .device
            .reconfigure_swapchain(
                headless.swapchain,
                &SwapchainDesc {
                    label: Some("vk e2e ring"),
                    surface: headless.surface,
                    format: headless.format,
                    extent,
                    image_count: 2,
                    present_mode: PresentMode::Fifo,
                    composite_alpha: CompositeAlpha::Opaque,
                },
            )
            .expect("a reconfigure after an acquire must succeed");

        // The handle the reconfigure invalidated must be dead rather than
        // stale: a caller holding one across a resize gets an error, never an
        // object that names freed memory.
        let mut encoder = headless.device.create_command_encoder(&CommandEncoderDesc {
            label: Some("vk e2e stale view"),
            queue: headless.queue,
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("stale"),
            color_attachments: &[ColorAttachment {
                view: stale_view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(CLEAR),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(1, 1),
            timestamp_writes: None,
        });
        encoder.end_render_pass();
        let error = encoder
            .finish()
            .expect_err("a view from before the reconfigure no longer resolves");
        assert!(
            matches!(
                error,
                crcbl_hal::HalError::InvalidHandle { .. }
                    | crcbl_hal::HalError::ForeignObject { .. }
            ),
            "detected, never undefined: {error}"
        );

        // And the fresh ring works, at the new size.
        let fresh = headless
            .device
            .acquire_next_frame(headless.swapchain)
            .expect("the reconfigured ring hands out images");
        assert_eq!(
            fresh.extent, extent,
            "an offscreen ring has no window system to clamp against"
        );
        headless
            .device
            .present(
                headless.queue,
                &PresentInfo {
                    swapchain: headless.swapchain,
                    waits: fresh.present_semaphore.as_slice(),
                    present_id: None,
                },
            )
            .expect("present");
        headless.device.wait_idle().expect("idle");
    }

    headless.finish();
}
