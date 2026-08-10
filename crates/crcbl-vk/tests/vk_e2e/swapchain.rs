use std::time::{Duration, Instant};

use crate::harness::{CLEAR, EXTENT, Headless};
use crcbl_hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, ClearValue, ColorAttachment,
    CommandEncoderDesc, CompositeAlpha, Extent3d, Format, ImageAspect, ImageSubresourceLayers,
    ImageSubresourceRange, ImageViewDesc, ImageViewType, LoadOp, MemoryLocation, PresentInfo,
    PresentMode, ReadbackDesc, ReadbackState, Rect2d, RenderPassDesc, ResourceState, SemaphoreDesc,
    SemaphoreKind, SemaphoreSignal, SemaphoreWait, StoreOp, SubmitInfo, SwapchainDesc,
};

/// Milestone 1, end to end and *verified*: acquire an image, clear it through a
/// real render pass, copy it out, read the pixels back and check them.
///
/// A `vkCmdClearColorImage` would put the same bytes there while exercising
/// none of the attachment, load-op or layout machinery every later milestone is
/// built on — so this goes through `begin_render_pass` with
/// [`LoadOp::Clear`], which is what "clear colour through the graph" means.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_vulkan_render_pass_clear_reaches_memory_with_the_colour_it_was_given() {
    let headless = Headless::open();
    let device = &headless.device;

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert!(
        acquired.acquire_semaphore.is_none() && acquired.present_semaphore.is_none(),
        "an offscreen ring has an implicit acquire, like WebGPU's"
    );

    // The swapchain hands over its own view and the extent it was configured
    // at. Neither is derived here — that is the whole point of the two fields.
    assert_eq!(
        acquired.extent, EXTENT,
        "an offscreen ring has no window system to clamp against, so the \
         configured extent is the requested one"
    );

    let pixels = (EXTENT.0 * EXTENT.1 * 4) as u64;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("vk e2e readback"),
            size: pixels,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("vk e2e frame"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            acquired.image,
            ImageSubresourceRange::all(headless.format),
            ResourceState::Undefined,
            ResourceState::ColorAttachment,
        )],
        ..Barriers::default()
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("clear"),
        color_attachments: &[ColorAttachment {
            view: acquired.view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(acquired.extent.0, acquired.extent.1),
    });
    encoder.end_render_pass();
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            acquired.image,
            ImageSubresourceRange::all(headless.format),
            ResourceState::ColorAttachment,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl_hal::Offset3d::default(),
        image_extent: Extent3d::d2(EXTENT.0, EXTENT.1),
    });
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

    let readback = device
        .request_readback(&ReadbackDesc {
            label: Some("vk e2e pixels"),
            buffer: staging,
            offset: 0,
            size: pixels,
            after: None,
        })
        .expect("a readback request");

    // Poll with a deadline, never a fixed sleep — `docs/plan/12-testing.md`.
    let mut bytes = vec![0u8; pixels as usize];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match device
            .poll_readback(readback, &mut bytes)
            .expect("the readback did not fail")
        {
            ReadbackState::Ready => break,
            ReadbackState::Pending => assert!(
                std::time::Instant::now() < deadline,
                "the readback never completed"
            ),
        }
        std::thread::yield_now();
    }

    // The swapchain format is sRGB, so the clear's linear values are encoded on
    // write. Rather than reimplement the transfer function, assert the two
    // properties that catch the bugs this test is for: every pixel is identical
    // (so the whole attachment really was cleared, not just part of it), and
    // the channels are ordered and distinct (so no channel swap or all-zero
    // "nothing happened" result slipped through).
    let first: [u8; 4] = bytes[0..4].try_into().expect("four bytes");
    assert!(
        bytes.chunks_exact(4).all(|pixel| pixel == first),
        "the whole render area must be cleared uniformly; got {first:?} then {:?}",
        &bytes[4..8]
    );
    assert_ne!(first, [0, 0, 0, 0], "an all-zero result means nothing ran");
    assert_eq!(first[3], 255, "alpha 1.0 must survive");
    let (r, g, b) = match headless.format {
        // The channel order in memory follows the format, which is the point of
        // checking it: a backend that ignored it would pass a "not all zero"
        // assertion and produce a blue window.
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => (first[2], first[1], first[0]),
        _ => (first[0], first[1], first[2]),
    };
    assert!(
        r < g && g < b,
        "the clear was {CLEAR:?}, so red < green < blue must survive into memory; \
         got r={r} g={g} b={b} in {:?}",
        headless.format
    );

    device.destroy_readback(readback);
    device.destroy_buffer(staging);
    headless.finish();
}

/// Coming back round the offscreen ring is ordered against the frame that had
/// the image last — the dependency `vkAcquireNextImageKHR` and its semaphore
/// give the windowed path, which a ring hand-built out of plain images does not
/// get for free.
///
/// [`AcquiredFrame::acquire_semaphore`] is `None` here and the seam calls that
/// an "implicit acquire". Implicit was true of the *semaphores* and false of the
/// ordering. A frame's last act on a headless ring is
/// `vkCmdCopyImageToBuffer` — a read — and the next frame's first act is a
/// layout transition out of `ResourceState::Undefined`, which is a write that
/// discards the contents. `Undefined` maps to `srcStageMask = NONE`, correct on
/// a WSI image because the acquire semaphore carries the dependency and there is
/// nothing left to wait for, and wrong on a ring image because there is no such
/// semaphore. Nothing ordered the two, and CI's layer reported exactly that:
/// `SYNC-HAZARD-WRITE-AFTER-READ`, with `read_barriers: VkPipelineStageFlags2(0)`
/// naming the empty mask.
///
/// # Why one command buffer, when the bug spans two frames
///
/// It is the same missing dependency at a distance a developer's layer can see.
/// `synchronisation_validation_catches_a_missing_barrier` measures that reach
/// and prints it: a machine reporting `cross-submission=no` — which is what an
/// Arch box running layer 1.4.357 reports — sees nothing at all in the
/// two-submission shape, so a test written that way is green there whether the
/// bug is present or not, which is the failure mode that whole test exists to
/// warn about. A one-image ring makes the reuse real (the second acquire hands
/// back the same image) and recording both trips into one command buffer puts
/// the hazard where **record-time** validation finds it, which every build has.
///
/// So this is a narrower gate than CI's, deliberately: it catches the mechanism
/// on any machine, and the two-submission instance stays CI's job.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn reusing_an_offscreen_vulkan_ring_image_is_ordered_against_the_frame_that_had_it() {
    // One image, so the second acquire is genuinely the first image again
    // rather than the other half of a ring.
    let headless = Headless::open_with(EXTENT, 1);
    let device = headless.device.as_ref();
    let pixels = u64::from(EXTENT.0) * u64::from(EXTENT.1) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("ring reuse readback"),
            size: pixels,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let first = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("ring reuse"),
        queue: headless.queue,
    });
    let image_barrier = |image, from, to| {
        [crcbl_hal::ImageBarrier::new(
            image,
            ImageSubresourceRange::all(headless.format),
            from,
            to,
        )]
    };

    // Trip one: draw into the image, then read it back out. The copy is the
    // read the next trip must not run over.
    let open = image_barrier(
        first.image,
        ResourceState::Undefined,
        ResourceState::ColorAttachment,
    );
    encoder.pipeline_barrier(&Barriers {
        images: &open,
        ..Barriers::default()
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("ring reuse clear"),
        color_attachments: &[ColorAttachment {
            view: first.view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(first.extent.0, first.extent.1),
    });
    encoder.end_render_pass();
    let to_source = image_barrier(
        first.image,
        ResourceState::ColorAttachment,
        ResourceState::TransferSrc,
    );
    encoder.pipeline_barrier(&Barriers {
        images: &to_source,
        ..Barriers::default()
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: first.image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl_hal::Offset3d::default(),
        image_extent: Extent3d::d2(EXTENT.0, EXTENT.1),
    });

    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: first.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");
    let second = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(
        second.index, first.index,
        "a one-image ring must hand the same image back, which is the whole \
         premise of this test"
    );

    // Trip two's opening move, on the image trip one just copied out of. This
    // is the write; the copy above is the read.
    let reopen = image_barrier(
        second.image,
        ResourceState::Undefined,
        ResourceState::ColorAttachment,
    );
    encoder.pipeline_barrier(&Barriers {
        images: &reopen,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");

    let report = headless.instance.validation_report();
    assert!(
        report.enabled,
        "this assertion is meaningless without the layer"
    );
    let hazard: Vec<_> = report
        .messages
        .iter()
        .filter(|message| {
            message.id.contains("SYNC-HAZARD-WRITE-AFTER-READ")
                || message.text.contains("WRITE_AFTER_READ")
        })
        .collect();
    assert!(
        hazard.is_empty(),
        "the transition that opens the reused image discarded its layout with \
         nothing ordering it against the copy that read the image on the \
         previous trip round the ring. `ResourceState::Undefined` must widen to \
         an execution dependency on a ring image, because no acquire semaphore \
         carries one there.\n{}",
        report.summary()
    );

    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: second.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);
    headless.finish();
}

/// The sandbox's frame, run against real Vulkan for as many frames as the ring
/// has images plus a few — enough that per-image sync, the deletion queue and
/// the retire timeline all wrap around at least once.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn many_frames_of_the_sandboxs_loop_leave_validation_silent() {
    let headless = Headless::open_with(EXTENT, 3);
    let device = &headless.device;

    let timeline = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("frames in flight"),
            kind: SemaphoreKind::Timeline { initial_value: 0 },
        })
        .expect("timeline semaphores are a hard requirement of this backend");

    let mut in_flight: Vec<(u64, crcbl_hal::CommandBufferHandle)> = Vec::new();
    // Views seen so far, per ring slot. Not a cache the caller has to maintain
    // any more — the swapchain hands one over — but a *check* that the ring
    // really does hand back the same view for the same slot, which is what
    // makes "no per-frame allocation" true rather than merely claimed.
    let mut seen: Vec<Option<crcbl_hal::ImageViewHandle>> = vec![None; 8];

    for frame in 1..=12u64 {
        let acquired = device
            .acquire_next_frame(headless.swapchain)
            .expect("the ring always has an image");
        let slot = &mut seen[acquired.index as usize];
        match slot {
            Some(previous) => assert_eq!(
                *previous, acquired.view,
                "a ring slot must hand back the same view every time round"
            ),
            None => *slot = Some(acquired.view),
        }

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("vk e2e frame"),
            queue: headless.queue,
        });
        encoder.pipeline_barrier(&Barriers {
            images: &[crcbl_hal::ImageBarrier::new(
                acquired.image,
                ImageSubresourceRange::all(headless.format),
                ResourceState::Undefined,
                ResourceState::ColorAttachment,
            )],
            ..Barriers::default()
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("clear"),
            color_attachments: &[ColorAttachment {
                view: acquired.view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(CLEAR),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(acquired.extent.0, acquired.extent.1),
        });
        encoder.end_render_pass();
        encoder.pipeline_barrier(&Barriers {
            images: &[crcbl_hal::ImageBarrier::new(
                acquired.image,
                ImageSubresourceRange::all(headless.format),
                ResourceState::ColorAttachment,
                ResourceState::Present,
            )],
            ..Barriers::default()
        });
        let commands = encoder.finish().expect("recording succeeded");

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
        in_flight.push((frame, commands));

        // A two-deep ring, exactly like `apps/sandbox`.
        while in_flight.len() > 2 {
            let (value, buffer) = in_flight.remove(0);
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
            device.destroy_command_buffer(buffer);
        }
    }

    device.wait_idle().expect("idle");
    assert_eq!(
        device.semaphore_value(timeline).expect("a timeline value"),
        12,
        "every submission signalled its frame number"
    );
    for (_, buffer) in in_flight {
        device.destroy_command_buffer(buffer);
    }
    device.destroy_semaphore(timeline);
    headless.finish();
}

/// The seam's present wait, on a ring that has no display behind it.
///
/// An offscreen "swapchain" is `VK_NULL_HANDLE` — there is no `VkSwapchainKHR`
/// for `vkWaitForPresentKHR` to take — and its present is an index bump, so
/// every wait is answered at once. Same for an id that was never presented.
///
/// **This is only "it returned `Ok`" on a driver that lacks the extensions**,
/// and the run says which it was. Where `Features::PRESENT_FEEDBACK` came back
/// — radv does, lavapipe does not — `VK_KHR_present_wait` is *enabled* on this
/// device, and a `wait_until_presented` that forgot either guard hands the
/// driver a null swapchain or an id nothing presented. The first is what
/// `Headless::finish`'s `assert_clean` catches; the second is a wait that runs
/// out the timeout, which the deadline below catches.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_offscreen_ring_answers_a_present_wait_with_no_swapchain_to_wait_on() {
    let headless = Headless::open();
    let device = &headless.device;
    let feedback = device
        .caps()
        .features
        .contains(crcbl_hal::Features::PRESENT_FEEDBACK);
    eprintln!(
        "vk e2e: present feedback is {} on this driver",
        if feedback { "ENABLED" } else { "absent" }
    );

    // Generous enough never to fire on a slow machine, and far below the
    // timeout asked for below — so a wait that actually blocked shows up here
    // rather than as a test that merely took a while.
    let budget = Duration::from_secs(5);
    // Long enough that reaching it would be unmistakably a block, and it is
    // never reached: nothing offscreen can be waited on.
    let timeout = Duration::from_secs(60);

    let started = Instant::now();
    for id in 1..=4u64 {
        device
            .acquire_next_frame(headless.swapchain)
            .expect("the ring always has an image");
        device
            .present(
                headless.queue,
                &PresentInfo {
                    swapchain: headless.swapchain,
                    waits: &[],
                    present_id: Some(id),
                },
            )
            .expect("present");
        device
            .wait_until_presented(headless.swapchain, id, timeout)
            .expect("an offscreen present is as complete as it will ever be");
    }
    // Never presented at all, and never will be: the frame after the last one,
    // and one far beyond anything this run numbered.
    device
        .wait_until_presented(headless.swapchain, 5, timeout)
        .expect("an id with no record behind it answers at once");
    device
        .wait_until_presented(headless.swapchain, u64::MAX, timeout)
        .expect("an id with no record behind it answers at once");
    let elapsed = started.elapsed();
    assert!(
        elapsed < budget,
        "the waits took {elapsed:?}; nothing offscreen may block"
    );

    // And an invalid handle is still refused — the immediate `Ok(())` is the
    // answer for a *device* without the capability, not for a bad argument.
    let stale = headless.swapchain;
    device.destroy_swapchain(stale);
    assert!(
        matches!(
            device.wait_until_presented(stale, 1, timeout),
            Err(crcbl_hal::SurfaceError::Hal(
                crcbl_hal::HalError::InvalidHandle { .. }
            ))
        ),
        "a destroyed swapchain is an invalid handle, not a frame that is up"
    );

    // `destroy_swapchain` on a handle already destroyed is a no-op, so the
    // canonical teardown still runs and still checks the layer.
    headless.finish();
}

/// The offscreen counterpart for `display_timing`, and the same guard.
///
/// An offscreen ring is `VK_NULL_HANDLE` with no display behind it, so the only
/// honest answer is [`DisplayTiming::Unknown`] — and reaching for the handle
/// anyway would hand `vkGetSwapchainTimingPropertiesEXT` a null swapchain,
/// which is what `Headless::finish`'s `assert_clean` catches.
///
/// **What this proves and what it does not.** Where `Features::PRESENT_TIMING`
/// came back, the whole `VK_EXT_present_timing` chain is *enabled* on this
/// device — that alone is worth having, because a wrong extension name or an
/// ungranted feature bit fails `vkCreateDevice` and this test would never
/// reach its first line. What it cannot reach is any arm other than `Unknown`:
/// only a real windowed swapchain on a real display returns `Fixed`,
/// `Variable` or `Stepped`, and this suite is headless by construction. The run
/// prints which driver it was so a green result is not mistaken for coverage of
/// the other three arms.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_offscreen_ring_reports_no_display_timing_and_never_takes_a_null_swapchain() {
    let headless = Headless::open();
    let device = &headless.device;
    let timing_enabled = device
        .caps()
        .features
        .contains(crcbl_hal::Features::PRESENT_TIMING);
    eprintln!(
        "vk e2e: present timing is {} on this driver; the offscreen suite can only \
         reach DisplayTiming::Unknown either way",
        if timing_enabled { "ENABLED" } else { "absent" }
    );

    // Before any present, and again after one: the extension's own proposal
    // says some platforms answer `VK_NOT_READY` until an image has been
    // presented, so both sides of that boundary must be `Unknown` here rather
    // than one of them erroring.
    assert_eq!(
        device
            .display_timing(headless.swapchain)
            .expect("an offscreen ring answers rather than failing"),
        crcbl_hal::DisplayTiming::Unknown,
        "an offscreen ring has no display, so it can only report Unknown"
    );
    device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: &[],
                present_id: Some(1),
            },
        )
        .expect("present");
    assert_eq!(
        device
            .display_timing(headless.swapchain)
            .expect("an offscreen ring answers rather than failing"),
        crcbl_hal::DisplayTiming::Unknown,
        "presenting to a ring does not give it a display"
    );

    // A destroyed handle is still refused. `Ok(Unknown)` is the answer for a
    // device without the capability, never for a bad argument — the seam's
    // obligation 3, which a stub that skipped the lookup would silently drop.
    let stale = headless.swapchain;
    device.destroy_swapchain(stale);
    assert!(
        matches!(
            device.display_timing(stale),
            Err(crcbl_hal::SurfaceError::Hal(
                crcbl_hal::HalError::InvalidHandle { .. }
            ))
        ),
        "a destroyed swapchain is an invalid handle, not an unknown display"
    );

    headless.finish();
}

/// Reconfigure is the resize path, and the seam promises the *handle* survives
/// it. Doing it in a tight loop is the "resize storm" the promise exists for.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_resize_storm_keeps_the_swapchain_handle_and_invalidates_the_old_images() {
    let headless = Headless::open();
    let device = &headless.device;

    let first = device
        .acquire_next_frame(headless.swapchain)
        .expect("an image");
    let (stale_image, stale_view) = (first.image, first.view);
    assert_eq!(first.extent, EXTENT);
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: &[],
                present_id: None,
            },
        )
        .expect("present");

    for (width, height) in [(32, 32), (128, 96), (17, 5), (64, 48)] {
        device
            .reconfigure_swapchain(
                headless.swapchain,
                &SwapchainDesc {
                    label: Some("vk e2e ring"),
                    surface: headless.surface,
                    format: headless.format,
                    extent: (width, height),
                    image_count: 2,
                    present_mode: PresentMode::Fifo,
                    composite_alpha: CompositeAlpha::Opaque,
                },
            )
            .expect("reconfigure keeps the handle valid");
        // And the handle really is still usable, which is the promise.
        let frame = device
            .acquire_next_frame(headless.swapchain)
            .expect("an image after the reconfigure");
        // Obligation 3: the frame reports the size it was configured at, and a
        // reconfigure is visible in it on the very next acquire. An offscreen
        // ring pins no range, so here the answer equals the request — which is
        // itself worth asserting, because it is the property every platform
        // *except* X11 has.
        assert_eq!(
            frame.extent,
            (width, height),
            "the ring configures at exactly what it was asked for"
        );
        device
            .present(
                headless.queue,
                &PresentInfo {
                    swapchain: headless.swapchain,
                    waits: frame.present_semaphore.as_slice(),
                    present_id: None,
                },
            )
            .expect("present");
    }

    // Handles from before the first reconfigure must be dead, not
    // stale-but-usable: that is the generational handle doing its job. Both the
    // image *and* the view the swapchain handed out, since a reconfigure
    // reissues each.
    let error = device
        .create_image_view(&ImageViewDesc {
            label: None,
            image: stale_image,
            view_type: ImageViewType::D2,
            format: headless.format,
            range: ImageSubresourceRange::all(headless.format),
        })
        .expect_err("an image handle does not survive a reconfigure");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidHandle { .. }),
        "{error}"
    );
    let latest = device
        .acquire_next_frame(headless.swapchain)
        .expect("an image");
    assert_ne!(
        latest.view, stale_view,
        "a reconfigure reissues the swapchain's views too"
    );

    headless.finish();
}

/// The swapchain's own image and view survive a caller trying to destroy them.
///
/// Regression test. The seam says the swapchain owns both, so `destroy_image`
/// and `destroy_image_view` on one must do *nothing* — including not removing
/// the pool row. `destroy_image` used to remove the row before checking, so one
/// stray call (a plausible mistake, given the matching view call is a
/// documented no-op) left every later `AcquiredFrame` handing out a handle that
/// no longer resolved, and the swapchain was unusable until it was recreated.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_caller_cannot_destroy_the_swapchains_own_image_or_view() {
    let headless = Headless::open();
    let device = &headless.device;

    let first = device
        .acquire_next_frame(headless.swapchain)
        .expect("an image");
    // Both are the swapchain's. Both must be no-ops.
    device.destroy_image(first.image);
    device.destroy_image_view(first.view);

    // The ring still works, and still hands back the same handles for the same
    // slot — which is only true if neither row was removed.
    let mut wrapped = device
        .acquire_next_frame(headless.swapchain)
        .expect("acquire");
    while wrapped.index != first.index {
        device
            .present(
                headless.queue,
                &PresentInfo {
                    swapchain: headless.swapchain,
                    waits: wrapped.present_semaphore.as_slice(),
                    present_id: None,
                },
            )
            .expect("present");
        wrapped = device
            .acquire_next_frame(headless.swapchain)
            .expect("acquire");
    }
    assert_eq!(wrapped.image, first.image);
    assert_eq!(wrapped.view, first.view);

    // And the handles still resolve, which is the property a barrier needs.
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("after the stray destroys"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            wrapped.image,
            ImageSubresourceRange::all(headless.format),
            ResourceState::Undefined,
            ResourceState::ColorAttachment,
        )],
        ..Barriers::default()
    });
    let commands = encoder
        .finish()
        .expect("the swapchain's image handle still resolves");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    headless.finish();
}
