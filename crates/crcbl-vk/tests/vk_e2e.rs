//! End-to-end suite against a **real Vulkan implementation**.
//!
//! ```text
//! crates/crcbl-vk/tests/run-vk-e2e.sh [extra nextest args…]
//! ```
//!
//! Feature-gated *and* `#[ignore]`d, exactly like `crcbl-shell`'s two
//! window-system suites: `cargo nextest run --workspace --all-features` on a
//! machine with no Vulkan loader must stay green, and the harness script is the
//! only thing that turns these on — and it fails when the suite reports zero
//! tests run, because `docs/plan/12-testing.md` calls a silently-skipped e2e job
//! a known trap.
//!
//! Everything here runs **headless**, through
//! [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen). That is
//! deliberate: it is the only Vulkan CI can run without a compositor, it is the
//! path `crcbl screenshot` and the P1 golden-image e2e need, and it goes through
//! the *same* acquire/present code as a window rather than a second,
//! less-exercised one. The windowed paths are covered by the sandbox runs in
//! `run-wayland-e2e.sh` and `run-x11-e2e.sh`.
//!
//! # Every test asserts a clean validation report
//!
//! [`ValidationReport::assert_clean`] fails on any error *or* warning, and also
//! fails when the layer was never loaded — so a green run means the layer looked
//! and found nothing, not that nobody looked. That is what makes
//! `docs/plan/02-vulkan-backend.md`'s "zero validation errors/warnings" exit
//! criterion a test result.

#![cfg(feature = "vk-e2e")]

use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, ClearValue, ColorAttachment,
    CommandEncoderDesc, CompositeAlpha, Device, DeviceDesc, Extent3d, Features, Format,
    ImageAspect, ImageDesc, ImageSubresourceLayers, ImageSubresourceRange, ImageType, ImageUsage,
    ImageViewDesc, ImageViewType, Instance, LoadOp, MemoryLocation, PresentInfo, PresentMode,
    QueryKind, QuerySetDesc, ReadbackDesc, ReadbackState, Rect2d, RenderPassDesc, ResourceState,
    SemaphoreDesc, SemaphoreKind, SemaphoreSignal, SemaphoreWait, StoreOp, SubmitInfo,
    SurfaceError, SwapchainDesc,
};
use crcbl_vk::{OpenError, VkInstance};

/// The size every offscreen test renders at. Small enough that lavapipe is
/// fast, large enough that a row-pitch mistake shows up.
const EXTENT: (u32, u32) = (64, 48);

/// A distinctive clear colour. Chosen so every channel differs and none is 0 or
/// 1: a channel-swap or an sRGB round-trip bug is then visible in the bytes.
const CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// Opens an instance, or explains why the suite cannot run.
///
/// A missing loader is a hard failure here, not a skip: this suite is only ever
/// started by the harness, which has already established that Vulkan is present.
fn instance() -> VkInstance {
    match VkInstance::open() {
        Ok(instance) => {
            let (major, minor, patch) = instance.loader_version();
            eprintln!("vk e2e: loader {major}.{minor}.{patch}");
            for adapter in instance.adapters() {
                eprintln!(
                    "vk e2e: adapter {:?} ({:?}) driver {:?} tier {:?}",
                    adapter.name,
                    adapter.device_type,
                    adapter.driver,
                    adapter.caps.tier()
                );
            }
            instance
        }
        Err(error @ OpenError::NoLoader(_)) => panic!(
            "the harness starts this suite only when Vulkan is available, so a \
             missing loader here is a real failure: {error}"
        ),
        Err(error) => panic!("could not open the Vulkan backend: {error}"),
    }
}

/// An offscreen surface, a device, and a swapchain-shaped image ring.
struct Headless {
    instance: VkInstance,
    device: Box<dyn Device>,
    surface: crcbl_hal::SurfaceHandle,
    swapchain: crcbl_hal::SwapchainHandle,
    queue: crcbl_hal::QueueHandle,
    format: Format,
}

impl Headless {
    fn open() -> Self {
        Self::open_with(EXTENT, 2)
    }

    fn open_with(extent: (u32, u32), image_count: u32) -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);

        let target = SurfaceTarget::Offscreen;
        // SAFETY: `Offscreen` names no platform object at all, so there is
        // nothing to outlive the surface. `destroy` below tears the swapchain
        // down before the surface regardless, which is the general rule.
        let surface = unsafe { instance.create_surface(&target) }.expect("offscreen always works");

        let caps = instance
            .surface_caps(surface, adapter.id)
            .expect("the offscreen ring reports its own caps");
        assert_eq!(
            caps.current_extent, None,
            "an offscreen ring has no opinion about its size, exactly like Wayland"
        );
        let format = caps.preferred_format().expect("some format is offered");

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e"),
                adapter: adapter.id,
                // Never `TIER_A`: lavapipe and radv genuinely differ, and this
                // suite exists partly to find out how.
                required_features: Features::empty(),
                optional_features: Features::TIER_A
                    | Features::TIMESTAMP_QUERY
                    | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e ring"),
                surface,
                format,
                extent,
                image_count,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");

        Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
        }
    }

    /// Tears down in the order `crcbl-hal`'s obligation 2 requires, then
    /// asserts the layer saw nothing.
    fn finish(self) {
        self.device.wait_idle().expect("idle");
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        drop(self.device);
        self.instance.validation_report().assert_clean();
    }
}

/// Milestone 1, end to end and *verified*: acquire an image, clear it through a
/// real render pass, copy it out, read the pixels back and check them.
///
/// A `vkCmdClearColorImage` would put the same bytes there while exercising
/// none of the attachment, load-op or layout machinery every later milestone is
/// built on — so this goes through `begin_render_pass` with
/// [`LoadOp::Clear`], which is what "clear colour through the graph" means.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_render_pass_clear_reaches_memory_with_the_colour_it_was_given() {
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

/// The other half of the validation gate: prove the messenger is **wired**, not
/// merely quiet.
///
/// Every other test asserts a clean report, and every one of them would pass
/// just as happily against a messenger that was never created — which is the
/// failure mode that turns "zero validation errors" into a slogan. So this test
/// commits a deliberate specification violation and asserts the layer
/// *noticed*.
///
/// The violation is chosen to be caught at **record** time and never submitted:
/// a copy whose size exceeds both buffers. An earlier version of this test used
/// an oversized render area instead, which the layer also catches — and which
/// **segfaults lavapipe**, because a spec violation is undefined behaviour and
/// a software rasteriser is under no obligation to survive one. That is the
/// first place radv and lavapipe disagreed, and it is a good argument for
/// provoking validation with something the driver never executes.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_deliberate_violation_is_caught_by_the_layer() {
    let headless = Headless::open();
    let device = &headless.device;

    let small = |label| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: 64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a small buffer")
    };
    let source = small("deliberately too small (src)");
    let destination = small("deliberately too small (dst)");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("deliberately wrong"),
        queue: headless.queue,
    });
    // `VUID-vkCmdCopyBuffer-size-00225`/`-00224`: the region must fit in both
    // buffers. Recorded, reported, and then thrown away — the command buffer is
    // never submitted, so no driver ever sees it.
    encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
        src: source,
        src_offset: 0,
        dst: destination,
        dst_offset: 0,
        size: 4096,
    });
    let commands = encoder.finish().expect("the backend records it regardless");

    let report = headless.instance.validation_report();
    assert!(
        report.enabled,
        "this suite is meaningless without the validation layer"
    );
    assert!(
        !report.is_clean(),
        "the layer must have caught an out-of-bounds copy; if it did not, every \
         other test in this file is proving nothing"
    );
    assert!(
        report
            .messages
            .iter()
            .any(|message| message.id.contains("CopyBuffer") || message.text.contains("size")),
        "the message should name the copy:\n{}",
        report.summary()
    );

    // Deliberately *not* `headless.finish()`: the report is dirty on purpose.
    device.destroy_command_buffer(commands);
    device.destroy_buffer(source);
    device.destroy_buffer(destination);
    device.wait_idle().expect("idle");
    device.destroy_swapchain(headless.swapchain);
    headless.instance.destroy_surface(headless.surface);
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

/// Obligation 4: a zero extent is the caller's problem, and the error says so
/// rather than the backend guessing a size.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_zero_extent_swapchain_is_refused_with_a_reason() {
    let instance = instance();
    let adapter = instance.adapters().remove(0);
    // SAFETY: `Offscreen` names no platform object.
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }.expect("offscreen");
    let device = instance
        .create_device(&DeviceDesc {
            label: Some("vk e2e"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: Some(surface),
        })
        .expect("a device opens");

    let error = device
        .create_swapchain(&SwapchainDesc {
            label: None,
            surface,
            format: Format::Rgba8UnormSrgb,
            extent: (0, 0),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        })
        .expect_err("a minimized window means 'not yet', not 'guess'");
    let SurfaceError::Hal(crcbl_hal::HalError::InvalidDescriptor(message)) = error else {
        panic!("wrong variant");
    };
    assert!(message.contains("do not create one yet"), "{message}");

    instance.destroy_surface(surface);
    drop(device);
    instance.validation_report().assert_clean();
}

/// Obligation 3: handles do not cross devices, and the failure is detected
/// rather than undefined.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_buffer_from_one_device_is_foreign_to_another() {
    let instance = instance();
    let adapter = instance.adapters().remove(0);
    let open = || {
        instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .or_else(|_| {
                // `for_adapter` demands Tier A, which lavapipe may not have.
                instance.create_device(&DeviceDesc {
                    label: None,
                    adapter: adapter.id,
                    required_features: Features::empty(),
                    optional_features: Features::empty(),
                    compatible_surface: None,
                })
            })
            .expect("a headless device opens")
    };
    let first = open();
    let second = open();

    let buffer = first
        .create_buffer(&BufferDesc {
            label: Some("foreign"),
            size: 256,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a buffer");

    let error = second
        .write_buffer(buffer, 0, &[1, 2, 3, 4])
        .expect_err("a buffer from another device must be refused");
    // `ForeignObject` specifically, not "either error will do". Every object
    // table is per-device, so this used to be answered by the handle happening
    // to miss the second device's pool — which stops being true the moment two
    // devices allocate in step, and at that point device A accepted device B's
    // handle outright. A handle now carries the tag of the device that issued
    // it, so the answer is decided rather than lucky.
    assert!(
        matches!(
            error,
            crcbl_hal::HalError::ForeignObject { kind: "buffer", .. }
        ),
        "a live handle from another device is foreign, not merely unresolvable: {error}"
    );

    first.destroy_buffer(buffer);
    drop(second);
    drop(first);
    instance.validation_report().assert_clean();
}

/// Obligation 1: a `Device` may outlive its `Instance`. Getting this wrong is a
/// use-after-free inside the driver, which is exactly the class of bug the
/// validation layer reports — so the report is the assertion.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_device_outlives_the_instance_that_made_it() {
    let instance = instance();
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: Some("outlives"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: None,
        })
        .expect("a headless device opens");

    // A clone so the report survives the instance object being dropped; the
    // *underlying* `VkInstance` must stay alive because the device does.
    let report_source = instance.clone();
    drop(instance);

    // The device still works, which is the property under test.
    let buffer = device
        .create_buffer(&BufferDesc {
            label: Some("after the instance went away"),
            size: 64,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostUpload,
        })
        .expect("the device outlived its instance");
    device.write_buffer(buffer, 0, &[7; 64]).expect("write");
    device.destroy_buffer(buffer);
    device.wait_idle().expect("idle");
    drop(device);

    report_source.validation_report().assert_clean();
}

/// The tier determination, against whatever this machine actually is. The
/// assertion is not "Tier A" — lavapipe may not be — but that the *report is
/// consistent*, which is the property a renderer branches on.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_reported_tier_agrees_with_the_reported_features() {
    let instance = instance();
    for adapter in instance.adapters() {
        let caps = adapter.caps;
        assert_eq!(
            caps.tier().is_a(),
            caps.supports(Features::TIER_A),
            "{}: tier and features must agree",
            adapter.name
        );
        if caps.tier().is_a() {
            assert!(caps.limits.max_bindless_descriptors > 0, "{}", adapter.name);
            assert!(caps.limits.max_draw_indirect_count > 1, "{}", adapter.name);
        } else {
            eprintln!(
                "vk e2e: {} is Tier B, missing {:?}",
                adapter.name,
                caps.missing(Features::TIER_A)
            );
        }
        if caps.features.contains(Features::TIMESTAMP_QUERY) {
            assert!(caps.limits.timestamp_period_ns > 0.0, "{}", adapter.name);
        } else {
            assert_eq!(caps.limits.timestamp_period_ns, 0.0, "{}", adapter.name);
        }
    }
}

/// Timestamp queries, if the device has them: the profiler HUD's foundation,
/// and the seam says it degrades rather than breaks without them.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn timestamps_either_work_or_are_refused_cleanly() {
    let headless = Headless::open();
    let device = &headless.device;
    let has_timestamps = device.caps().features.contains(Features::TIMESTAMP_QUERY);

    let set = device.create_query_set(&QuerySetDesc {
        label: Some("frame timers"),
        kind: QueryKind::Timestamp,
        count: 2,
    });
    let Ok(set) = set else {
        assert!(
            !has_timestamps,
            "a device reporting TIMESTAMP_QUERY must create a timestamp set"
        );
        headless.finish();
        return;
    };
    assert!(
        has_timestamps,
        "a set was created on a device claiming none"
    );

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("timers"),
        queue: headless.queue,
    });
    encoder.reset_query_set(set, 0..2);
    encoder.write_timestamp(set, 0);
    encoder.write_timestamp(set, 1);
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");

    let mut results = [0u64; 2];
    device
        .query_results(set, 0, &mut results)
        .expect("timestamps read back");
    assert!(
        results[1] >= results[0],
        "the GPU clock does not run backwards: {results:?}"
    );

    device.destroy_command_buffer(commands);
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

// --- milestone 2: the triangle ---------------------------------------------

/// The size the triangle suite renders at.
///
/// Larger than [`EXTENT`] on purpose: the golden image's structural metric
/// works on 8×8 blocks, and a 64×48 frame is only 48 of them — few enough that
/// one block of edge disagreement moves the mean SSIM more than it should. It
/// is still small enough that lavapipe renders it in milliseconds.
const TRIANGLE_EXTENT: (u32, u32) = (256, 192);

/// The clear behind the triangle. Dark, and none of the vertex primaries, so
/// "the triangle did not draw" and "the triangle drew" are not confusable.
const TRIANGLE_CLEAR: [f32; 4] = [0.02, 0.03, 0.06, 1.0];

impl Headless {
    /// Opens a ring at a pinned format, so a golden image means the same thing
    /// on every driver.
    ///
    /// `preferred_format()` is what the sandbox uses and is right there — but a
    /// golden image compared across two drivers must not depend on which format
    /// each of them happened to prefer, or a format change would look like a
    /// rendering regression.
    fn open_for_triangle() -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);
        // SAFETY: `Offscreen` names no platform object at all.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e triangle"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::TIER_A | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");
        let format = Format::Rgba8UnormSrgb;
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e triangle ring"),
                surface,
                format,
                extent: TRIANGLE_EXTENT,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");
        Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
        }
    }
}

/// Everything milestone 2 needs, built through the seam.
struct TriangleResources {
    vertices: crcbl_hal::BufferHandle,
    bind_group_layout: crcbl_hal::BindGroupLayoutHandle,
    bind_group: crcbl_hal::BindGroupHandle,
    pipeline_layout: crcbl_hal::PipelineLayoutHandle,
    pipeline: crcbl_hal::GraphicsPipelineHandle,
}

impl TriangleResources {
    /// Builds the pipeline and stages the geometry in.
    ///
    /// Deliberately the *same* shape as `apps/sandbox`'s `Triangle`, and the
    /// same geometry constant, so this suite is evidence about the code the
    /// sandbox runs rather than about a second triangle that resembles it.
    fn new(headless: &Headless) -> Self {
        let device = headless.device.as_ref();
        let bytes = crcbl_shaders::triangle::vertex_bytes();
        let size = bytes.len() as u64;

        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("triangle staging"),
                size,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        device.write_buffer(staging, 0, &bytes).expect("write");
        let vertices = device
            .create_buffer(&BufferDesc {
                label: Some("triangle vertices"),
                size,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a vertex buffer");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("triangle upload"),
            queue: headless.queue,
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src: staging,
            src_offset: 0,
            dst: vertices,
            dst_offset: 0,
            size,
        });
        encoder.pipeline_barrier(&Barriers {
            buffers: &[crcbl_hal::BufferBarrier {
                buffer: vertices,
                from: ResourceState::TransferDst,
                to: ResourceState::ShaderRead,
                queue_transfer: None,
            }],
            ..Barriers::default()
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
        device.destroy_buffer(staging);

        let layout_entries = [crcbl_hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl_hal::ShaderStages::VERTEX,
            kind: crcbl_hal::BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: crcbl_hal::BindingFlags::empty(),
        }];
        let bind_group_layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("triangle vertices"),
                entries: &layout_entries,
            })
            .expect("a layout with no descriptor-indexing flags works on both tiers");

        let group_entries = [crcbl_hal::BindGroupEntry {
            binding: 0,
            array_index: 0,
            resource: crcbl_hal::BindingResource::whole_buffer(vertices),
        }];
        let bind_group = device
            .create_bind_group(&crcbl_hal::BindGroupDesc {
                label: Some("triangle vertices"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
                label: Some("triangle"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        let module = device
            .create_shader_module(&crcbl_hal::ShaderModuleDesc {
                label: Some("triangle.slang"),
                spirv: crcbl_shaders::TRIANGLE.spirv(),
                wgsl: crcbl_shaders::TRIANGLE.wgsl(),
            })
            .expect("the committed SPIR-V is accepted");

        let color_targets = [crcbl_hal::ColorTargetState::opaque(headless.format)];
        let pipeline = device
            .create_graphics_pipeline(&crcbl_hal::GraphicsPipelineDesc {
                label: Some("triangle"),
                layout: pipeline_layout,
                vertex: crcbl_hal::ShaderEntry {
                    module,
                    entry_point: "vertexMain",
                },
                fragment: Some(crcbl_hal::ShaderEntry {
                    module,
                    entry_point: "fragmentMain",
                }),
                primitive: crcbl_hal::PrimitiveState::default(),
                depth_stencil: None,
                multisample: crcbl_hal::MultisampleState::default(),
                color_targets: &color_targets,
            })
            .expect("a graphics pipeline");
        // The seam promises pipelines built from a module stay valid once it is
        // destroyed, and this is where that promise is actually exercised: every
        // draw below happens after the module is gone.
        device.destroy_shader_module(module);

        Self {
            vertices,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            pipeline,
        }
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.vertices);
    }
}

/// Renders the triangle into the ring and reads the pixels back.
///
/// The whole `crcbl screenshot` path in one function: acquire, barrier, render
/// pass, draw, barrier to `TransferSrc`, copy to a host-readable buffer, poll.
fn render_triangle(headless: &Headless, resources: &TriangleResources) -> crcbl_golden::Image {
    let device = headless.device.as_ref();
    let (width, height) = TRIANGLE_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, TRIANGLE_EXTENT);

    let byte_count = u64::from(width) * u64::from(height) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("triangle readback"),
            size: byte_count,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let range = ImageSubresourceRange::all(headless.format);
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("triangle frame"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            acquired.image,
            range,
            ResourceState::Undefined,
            ResourceState::ColorAttachment,
        )],
        ..Barriers::default()
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("clear + triangle"),
        color_attachments: &[ColorAttachment {
            view: acquired.view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(TRIANGLE_CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(width, height),
    });
    encoder.set_viewport(&crcbl_hal::Viewport::from_size(width, height));
    encoder.set_scissor(&Rect2d::from_size(width, height));
    encoder.bind_graphics_pipeline(resources.pipeline);
    encoder.bind_group(0, resources.bind_group, &[], resources.pipeline_layout);
    // Three vertices, and no geometry bound to the pipeline at all.
    encoder.draw(0..3, 0..1);
    encoder.end_render_pass();
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            acquired.image,
            range,
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
        image_extent: Extent3d::d2(width, height),
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
            },
        )
        .expect("present");

    let readback = device
        .request_readback(&ReadbackDesc {
            label: Some("triangle pixels"),
            buffer: staging,
            offset: 0,
            size: byte_count,
            after: None,
        })
        .expect("a readback request");
    let mut bytes = vec![0u8; byte_count as usize];
    // Poll with a deadline, never a fixed sleep — `docs/plan/12-testing.md`.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
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
    device.destroy_readback(readback);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &bytes, order)
        .expect("the readback is exactly one image")
}

/// Milestone 2, end to end and verified: a triangle whose vertices came out of a
/// storage buffer, with **no vertex input state anywhere**.
///
/// The assertions are about geometry rather than exact colour, because that is
/// what distinguishes "the triangle drew" from "something drew": each corner of
/// the frame holds a different vertex's colour, and the centre holds a blend of
/// all three. A pipeline that ignored the storage buffer, a vertex order that
/// was mirrored, or a Y flip would each break at least one of them.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_triangle_pulled_from_a_storage_buffer_reaches_memory() {
    let headless = Headless::open_for_triangle();
    let resources = TriangleResources::new(&headless);
    let image = render_triangle(&headless, &resources);

    let (width, height) = TRIANGLE_EXTENT;

    // Sample points are derived from the geometry rather than guessed at as
    // fractions of the frame: 75% of the way from the centroid to each vertex is
    // comfortably inside the triangle whatever its shape, and near enough to a
    // corner that one vertex colour dominates. Picking round fractions of the
    // frame instead put the "apex" sample *above* the apex, which is a test bug
    // that looks exactly like a Y flip.
    let vertices = crcbl_shaders::triangle::VERTICES;
    let centroid = [
        vertices.iter().map(|v| v.position[0]).sum::<f32>() / 3.0,
        vertices.iter().map(|v| v.position[1]).sum::<f32>() / 3.0,
    ];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let sample_near = |index: usize| -> (u32, u32) {
        let vertex = vertices[index].position;
        let x = vertex[0].mul_add(0.75, centroid[0] * 0.25);
        let y = vertex[1].mul_add(0.75, centroid[1] * 0.25);
        // NDC to pixels. +Y is up in the seam's convention, which the backend's
        // negative-height viewport preserves, so the Y term is inverted here and
        // nowhere else.
        (
            (((x + 1.0) * 0.5) * width as f32) as u32,
            (((1.0 - y) * 0.5) * height as f32) as u32,
        )
    };

    // Dominance rather than absolute values: the target is sRGB, so the exact
    // level depends on the transfer function, but "this corner is redder than it
    // is green or blue" is true under any encoding and is what actually
    // distinguishes a correct triangle from a mirrored or rotated one.
    let names = ["apex (red)", "bottom right (green)", "bottom left (blue)"];
    for (index, name) in names.iter().enumerate() {
        let (x, y) = sample_near(index);
        let pixel = image
            .pixel(x, y)
            .unwrap_or_else(|| panic!("{name} sample ({x}, {y}) is outside the frame"));
        let dominant = (0..3)
            .max_by_key(|channel| pixel[*channel])
            .expect("three channels");
        assert_eq!(
            dominant, index,
            "{name} at ({x}, {y}) is {pixel:?}; channel {dominant} dominates rather than \
             channel {index}. A Y flip, an X mirror or a reversed vertex order each produce \
             exactly this."
        );
        assert!(
            u32::from(pixel[index]) > 150,
            "{name} at ({x}, {y}) is {pixel:?}; the dominant channel must be strong, not \
             merely largest"
        );
    }

    // The centre is a blend of all three, which is the property that proves the
    // fragment stage really interpolated a per-vertex attribute rather than
    // outputting a constant.
    let centre = image.pixel(width / 2, height / 2).expect("inside");
    assert!(
        centre[0] > 20 && centre[1] > 20 && centre[2] > 20,
        "the centre must blend all three vertex colours, got {centre:?}"
    );
    assert_eq!(centre[3], 255, "alpha 1.0 must survive");

    // And the very corners of the frame are still the clear colour: the
    // triangle does not cover them, so a pipeline that drew a full-screen quad
    // would fail here.
    for corner in [
        (0, 0),
        (width - 1, 0),
        (0, height - 1),
        (width - 1, height - 1),
    ] {
        let pixel = image.pixel(corner.0, corner.1).expect("inside");
        assert!(
            pixel[0] < 60 && pixel[1] < 60 && pixel[2] < 80,
            "corner {corner:?} must still be the clear colour, got {pixel:?}"
        );
    }

    resources.destroy(headless.device.as_ref());
    headless.finish();
}

/// The golden-image gate: the rendered triangle against a checked-in reference.
///
/// `docs/plan/12-testing.md` schedules this for P1 and specifies the shape —
/// "per-pixel tolerance + SSIM-style metric (rasterizers differ slightly);
/// regenerate via `--bless` flag; diffs uploaded as CI artifacts on failure".
///
/// The tolerance is [`Tolerance::RASTERISER`], whose numbers were measured
/// between radv and lavapipe rather than guessed; `crcbl-golden`'s crate docs
/// carry the measurements and its unit tests pin that the same tolerance still
/// rejects a triangle that moved by a few pixels.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_triangle_matches_its_golden_image() {
    let headless = Headless::open_for_triangle();
    let resources = TriangleResources::new(&headless);
    let image = render_triangle(&headless, &resources);

    let reference =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/triangle.png");
    let golden = crcbl_golden::Golden::new(reference);
    let outcome = golden.check(&image).expect("the reference is readable");
    let comparison = match outcome.into_result() {
        Ok(comparison) => comparison,
        Err(message) => {
            // Destroyed before the panic so the device teardown does not report
            // leaked objects on top of the real failure.
            resources.destroy(headless.device.as_ref());
            headless.device.wait_idle().expect("idle");
            panic!("{message}");
        }
    };
    // Printed on success too: the numbers are how the tolerance stays honest
    // across drivers, and a run that quietly passes teaches nothing.
    eprintln!("vk e2e: golden triangle — {}", comparison.summary());

    resources.destroy(headless.device.as_ref());
    headless.finish();
}

/// The tier story for bind-group layouts, against whatever this machine is.
///
/// The seam requires a device without `DESCRIPTOR_INDEXING` to **reject** a
/// layout that sets any [`BindingFlags`](crcbl_hal::BindingFlags), rather than
/// ignoring it — "a bindless array quietly downgraded to a fixed one reads
/// garbage at index 4097". Which branch runs depends on the driver, and both are
/// asserted, which is exactly why this suite runs on radv and lavapipe.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_bindless_capable_layout_is_accepted_or_refused_according_to_the_tier() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();
    let indexing = device
        .caps()
        .features
        .contains(Features::DESCRIPTOR_INDEXING);

    // The Tier A shape: a runtime-sized texture array on the last binding.
    let entries = [
        crcbl_hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl_hal::ShaderStages::VERTEX,
            kind: crcbl_hal::BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: 1,
            flags: crcbl_hal::BindingFlags::empty(),
        },
        crcbl_hal::BindGroupLayoutEntry {
            binding: 1,
            visibility: crcbl_hal::ShaderStages::FRAGMENT,
            kind: crcbl_hal::BindingKind::SampledImage,
            // The seam's "as many as you can"; the backend clamps it to the
            // device's own `max_bindless_descriptors`.
            count: u32::MAX,
            flags: crcbl_hal::BindingFlags::VARIABLE_COUNT
                | crcbl_hal::BindingFlags::PARTIALLY_BOUND
                | crcbl_hal::BindingFlags::UPDATE_AFTER_BIND,
        },
    ];
    let result = device.create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
        label: Some("bindless probe"),
        entries: &entries,
    });

    match result {
        Ok(layout) => {
            assert!(
                indexing,
                "a device that reports no DESCRIPTOR_INDEXING must not accept a bindless layout"
            );
            assert!(
                device.caps().limits.max_bindless_descriptors > 0,
                "a Tier A device must report a bindless ceiling to clamp u32::MAX against"
            );
            device.destroy_bind_group_layout(layout);
        }
        Err(error) => {
            assert!(
                !indexing,
                "a Tier A device must accept the bindless shape: {error}"
            );
            assert!(
                matches!(error, crcbl_hal::HalError::Unsupported { .. }),
                "the refusal must be loud and typed, not an InvalidDescriptor: {error}"
            );
            eprintln!("vk e2e: Tier B device refused the bindless layout, as required: {error}");
        }
    }

    // `VARIABLE_COUNT` anywhere but last is a caller bug on *every* tier.
    let misplaced = [entries[1], entries[0]];
    let error = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: None,
            entries: &misplaced,
        })
        .expect_err("VARIABLE_COUNT is only legal on the last binding");
    eprintln!("vk e2e: misplaced VARIABLE_COUNT refused: {error}");

    headless.finish();
}

/// A pipeline naming an entry point the module does not have must be refused
/// **here**, with the available ones listed — not by the driver, which reports
/// it as an initialisation failure naming neither.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_missing_entry_point_is_named_before_the_driver_sees_it() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();

    let module = device
        .create_shader_module(&crcbl_hal::ShaderModuleDesc {
            label: Some("triangle.slang"),
            spirv: crcbl_shaders::TRIANGLE.spirv(),
            wgsl: crcbl_shaders::TRIANGLE.wgsl(),
        })
        .expect("the committed SPIR-V is accepted");
    let pipeline_layout = device
        .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
            label: None,
            bind_group_layouts: &[],
            push_constants: None,
        })
        .expect("an empty pipeline layout");
    let color_targets = [crcbl_hal::ColorTargetState::opaque(headless.format)];

    // `main` is what a GLSL habit reaches for, and Slang emits neither.
    let error = device
        .create_graphics_pipeline(&crcbl_hal::GraphicsPipelineDesc {
            label: None,
            layout: pipeline_layout,
            vertex: crcbl_hal::ShaderEntry {
                module,
                entry_point: "main",
            },
            fragment: None,
            primitive: crcbl_hal::PrimitiveState::default(),
            depth_stencil: None,
            multisample: crcbl_hal::MultisampleState::default(),
            color_targets: &color_targets,
        })
        .expect_err("there is no entry point called `main`");
    let text = error.to_string();
    assert!(
        text.contains("vertexMain"),
        "the list must be shown: {text}"
    );

    // And naming the right entry point at the wrong stage gets its own wording.
    let error = device
        .create_graphics_pipeline(&crcbl_hal::GraphicsPipelineDesc {
            label: None,
            layout: pipeline_layout,
            vertex: crcbl_hal::ShaderEntry {
                module,
                entry_point: "fragmentMain",
            },
            fragment: None,
            primitive: crcbl_hal::PrimitiveState::default(),
            depth_stencil: None,
            multisample: crcbl_hal::MultisampleState::default(),
            color_targets: &color_targets,
        })
        .expect_err("fragmentMain is not a vertex entry point");
    assert!(error.to_string().contains("but not at"), "{error}");

    // Bytes where words were wanted, which is the mistake the seam's docs single
    // out, must be caught here rather than by `vkCreateShaderModule`.
    let error = device
        .create_shader_module(&crcbl_hal::ShaderModuleDesc {
            label: None,
            spirv: &[0x0302_2307, 0, 0, 0, 0],
            wgsl: None,
        })
        .expect_err("a byte-swapped module is not SPIR-V");
    assert!(error.to_string().contains("byte-swapped"), "{error}");

    device.destroy_pipeline_layout(pipeline_layout);
    device.destroy_shader_module(module);
    headless.finish();
}

/// Samplers, which land with the rest of the pipeline surface at P1.2 and are
/// the one part of it milestone 2 does not otherwise exercise.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn samplers_honour_the_seams_defaults_and_its_limits() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();

    let sampler = device
        .create_sampler(&crcbl_hal::SamplerDesc::default())
        .expect("the default trilinear repeating sampler");
    device.destroy_sampler(sampler);

    // Reversed-Z reaches the comparison sampler too: a shadow test asking "is
    // the fragment closer?" is `Greater`, and the seam says so.
    let shadow = device
        .create_sampler(&crcbl_hal::SamplerDesc {
            label: Some("shadow pcf"),
            compare: Some(crcbl_hal::CompareOp::Greater),
            address_mode: [crcbl_hal::SamplerAddressMode::ClampToBorder; 3],
            ..crcbl_hal::SamplerDesc::default()
        })
        .expect("a comparison sampler");
    device.destroy_sampler(shadow);

    // Anisotropy past the device's ceiling is an error, not a clamp: silently
    // sampling differently from what was asked for is a golden-image difference
    // nobody can explain.
    let ceiling = device.caps().limits.max_sampler_anisotropy;
    let error = device
        .create_sampler(&crcbl_hal::SamplerDesc {
            anisotropy: ceiling + 1.0,
            ..crcbl_hal::SamplerDesc::default()
        })
        .expect_err("anisotropy past the limit must be refused");
    assert!(
        matches!(error, crcbl_hal::HalError::InvalidDescriptor(_)),
        "{error}"
    );

    headless.finish();
}

/// The **synchronisation** half of the validation gate: prove sync validation is
/// wired, not merely quiet.
///
/// `a_deliberate_violation_is_caught_by_the_layer` does this for ordinary
/// validation. Sync validation is a separate opt-in with a separate failure
/// mode, and `docs/plan/02-vulkan-backend.md` names sync bugs as this stage's
/// headline risk and this layer as the mitigation — so "sync validation is on"
/// has to be a test result rather than an environment variable somebody set.
///
/// It is worth its own test because it was **not** on. Until P1.2,
/// `CRCBL_VK_SYNC_VALIDATION=1` probed for `VK_EXT_validation_features` in the
/// loader's implicit extension list, where it does not appear — it is a *layer*
/// extension — so the flag bought a log line and no checking, here and in CI.
///
/// The hazard is a read-after-write inside one command buffer with no barrier
/// between the two copies. It is recorded and thrown away, never submitted, for
/// the reason the sibling test gives: a spec violation is undefined behaviour,
/// and lavapipe is under no obligation to survive one.
///
/// # Sync validation has two halves, and only one of them is asserted here
///
/// A hazard inside one command buffer is caught while it is being *recorded*.
/// A hazard that spans two command buffers — or two submissions — can only be
/// caught when the queue is submitted, which is a separate piece of the layer
/// (`syncval_submit_time_validation`) and, on some builds, not one that runs.
/// **Every cross-frame hazard is in the second category**, including the
/// write-after-write on the graph's depth transient that this branch fixes: it
/// was reported by the CI leg's layer and by nothing on the developer's, which
/// is how a harness that claims to stand in for CI stops doing so.
///
/// So the second half is *measured* and printed rather than asserted. Asserting
/// it would fail a machine whose layer simply does not implement it, which is
/// not a bug in this repository; leaving it unmeasured is worse, because then
/// "26 tests passed" reads as "I saw what CI sees" when it may not be. The
/// marker line this prints is what `tests/run-vk-e2e.sh` turns into a banner.
///
/// The gate for the *bug class* is deliberately not here at all — it is
/// `crcbl-render`'s `a_second_frame_barriers_against_what_the_first_one_left`,
/// which needs no layer, no ICD and no GPU and therefore cannot be switched off
/// by a distribution's packaging choices.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn synchronisation_validation_catches_a_missing_barrier() {
    let headless = Headless::open();
    let device = &headless.device;
    if !std::env::var("CRCBL_VK_SYNC_VALIDATION").is_ok_and(|value| value == "1") {
        eprintln!("vk e2e: CRCBL_VK_SYNC_VALIDATION is not set; skipping the sync-hazard probe");
        headless.finish();
        return;
    }

    let buffer = |label| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: 4096,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a buffer")
    };
    let (first, second, third) = (buffer("hazard a"), buffer("hazard b"), buffer("hazard c"));

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("deliberate hazard"),
        queue: headless.queue,
    });
    let copy = |src, dst| crcbl_hal::BufferCopy {
        src,
        src_offset: 0,
        dst,
        dst_offset: 0,
        size: 4096,
    };
    // Write `second`, then read it, with nothing ordering the two.
    encoder.copy_buffer_to_buffer(&copy(first, second));
    encoder.copy_buffer_to_buffer(&copy(second, third));
    let commands = encoder.finish().expect("the backend records it regardless");

    let report = headless.instance.validation_report();
    assert!(
        report.enabled,
        "this suite is meaningless without the layer"
    );
    assert!(
        report.messages.iter().any(|message| {
            message.id.contains("SYNC-HAZARD") || message.text.contains("SYNC-HAZARD")
        }),
        "synchronisation validation must have caught a read-after-write with no barrier. If \
         this fails, `CRCBL_VK_SYNC_VALIDATION=1` is buying nothing and the headline risk of \
         this stage is unmitigated.\n{}",
        report.summary()
    );

    // Deliberately not `headless.finish()`: the report is dirty on purpose.
    device.destroy_command_buffer(commands);
    for handle in [first, second, third] {
        device.destroy_buffer(handle);
    }
    device.wait_idle().expect("idle");
    device.destroy_swapchain(headless.swapchain);
    headless.instance.destroy_surface(headless.surface);

    // And the two halves the recording checks cannot see. The marker line is
    // grepped by `tests/run-vk-e2e.sh`; keep the spelling.
    eprintln!(
        "vk e2e: sync-validation reach: record-time=yes one-submission={} cross-submission={}",
        yes_no(queue_hazard_reported(HazardShape::OneSubmission)),
        yes_no(queue_hazard_reported(HazardShape::TwoSubmissions)),
    );
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// How far apart the two halves of a deliberate hazard are placed.
#[derive(Clone, Copy, Debug)]
enum HazardShape {
    /// Two command buffers inside one `vkQueueSubmit2`.
    OneSubmission,
    /// Two separate `vkQueueSubmit2` calls, which is where **every** hazard
    /// that spans a frame boundary lives.
    TwoSubmissions,
}

/// Whether this validation layer reports a write-after-write the *recording*
/// checks cannot see, at the given distance.
///
/// Both shapes are the same hazard — two copies into one buffer with nothing
/// ordering them — and a layer may model one and not the other. The distinction
/// is the whole point of measuring: a frame's barriers are ordered against the
/// *previous frame's submission*, so a layer that reports `OneSubmission` and
/// not `TwoSubmissions` is blind to every cross-frame bug while looking, from
/// a green test run, exactly like one that is not.
///
/// The payload is deliberately large. A layer retires a batch it can prove has
/// completed, and this backend queries the retire timeline after every submit,
/// so a four-kilobyte copy can finish before the next submission is validated
/// and turn a real answer into "no". Thirty-two megabytes will still be in
/// flight; a wrong answer here is then the layer's behaviour rather than the
/// GPU's speed.
///
/// Its own instance, because the report it produces is dirty by construction and
/// must not land on a caller's.
fn queue_hazard_reported(shape: HazardShape) -> bool {
    /// Big enough to still be running when the next submission is validated.
    const PAYLOAD: u64 = 32 << 20;

    let headless = Headless::open();
    let device = headless.device.as_ref();

    let buffer = |label| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size: PAYLOAD,
                usage: BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a buffer")
    };
    let (source, shared, other) = (buffer("reach a"), buffer("reach b"), buffer("reach c"));

    let record = |src, dst| {
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("reach probe"),
            queue: headless.queue,
        });
        encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
            src,
            src_offset: 0,
            dst,
            dst_offset: 0,
            size: PAYLOAD,
        });
        encoder.finish().expect("recorded")
    };
    let commands = [record(source, shared), record(other, shared)];

    match shape {
        HazardShape::OneSubmission => {
            device
                .submit(headless.queue, &SubmitInfo::new(&commands))
                .expect("submit");
        }
        HazardShape::TwoSubmissions => {
            for handle in commands {
                device
                    .submit(headless.queue, &SubmitInfo::new(&[handle]))
                    .expect("submit");
            }
        }
    }

    device.wait_idle().expect("idle");
    let report = headless.instance.validation_report();
    let seen = report
        .messages
        .iter()
        .any(|message| message.id.contains("SYNC-HAZARD") || message.text.contains("SYNC-HAZARD"));

    for handle in commands {
        device.destroy_command_buffer(handle);
    }
    for handle in [source, shared, other] {
        device.destroy_buffer(handle);
    }
    device.destroy_swapchain(headless.swapchain);
    headless.instance.destroy_surface(headless.surface);
    seen
}

// --- milestones 3, 4 and 5: the lit mesh, through the render graph -----------

/// The size the mesh suite renders at.
///
/// The same 256×192 as the triangle, and for the same reason: the golden's
/// structural metric works on 8×8 blocks, and a smaller frame gives it too few
/// to average over.
const MESH_EXTENT: (u32, u32) = (256, 192);

/// Where the camera is for every mesh golden.
///
/// Far enough back that the cube does not touch the frame edge under either
/// projection, and off-axis on two of three axes so **three faces are visible at
/// once** — which is what makes a directional light legible and an orientation
/// mistake a different picture rather than a plausible one.
fn mesh_camera(projection: crcbl_render::Projection) -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: glam::Vec3::new(1.6, 1.2, 2.2),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection,
    }
}

/// The animation time every mesh golden renders at.
///
/// A constant, not a clock: a golden image of a spinning cube is only evidence
/// if the cube is in the same place every run.
///
/// Chosen so **three faces are visible at once**, which is not automatic — at
/// `0.7` the cube's `+X` face is edge-on to this camera to within a fifth of a
/// degree, and a two-face frame cannot show a lighting gradient however correct
/// the shader is. Three faces also means no symmetry is left for a transposed
/// matrix or a mirrored axis to hide behind.
const MESH_SECONDS: f32 = 0.35;

impl Headless {
    /// Opens a ring at a pinned format for the mesh suite. See
    /// [`Headless::open_for_triangle`] on why the format is pinned rather than
    /// preferred.
    fn open_for_mesh() -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);
        // SAFETY: `Offscreen` names no platform object at all.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e mesh"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::TIER_A
                    | Features::TIMESTAMP_QUERY
                    | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");
        let format = Format::Rgba8UnormSrgb;
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e mesh ring"),
                surface,
                format,
                extent: MESH_EXTENT,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");
        Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
        }
    }

    /// Reads a whole image back into `out`, polling with a deadline.
    fn readback(&self, staging: crcbl_hal::BufferHandle, size: u64, out: &mut [u8]) {
        let device = self.device.as_ref();
        let readback = device
            .request_readback(&ReadbackDesc {
                label: Some("vk e2e pixels"),
                buffer: staging,
                offset: 0,
                size,
                after: None,
            })
            .expect("a readback request");
        // Poll with a deadline, never a fixed sleep — `docs/plan/12-testing.md`.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match device
                .poll_readback(readback, out)
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
        device.destroy_readback(readback);
    }
}

/// What one mesh frame produced.
struct MeshFrame {
    /// The tonemapped swapchain image.
    image: crcbl_golden::Image,
    /// The raw `Rgba16Float` scene target, as half-floats.
    hdr: Vec<u8>,
}

impl MeshFrame {
    /// The linear HDR value at a texel, decoded from `Rgba16Float`.
    fn hdr_pixel(&self, x: u32, y: u32) -> [f32; 4] {
        let index = ((y * MESH_EXTENT.0 + x) * 4) as usize * 2;
        let mut out = [0.0f32; 4];
        for (channel, value) in out.iter_mut().enumerate() {
            let bits = u16::from_le_bytes(
                self.hdr[index + channel * 2..index + channel * 2 + 2]
                    .try_into()
                    .expect("two bytes"),
            );
            *value = half_to_f32(bits);
        }
        out
    }

    /// The brightest linear channel anywhere in the HDR target.
    fn peak_hdr(&self) -> f32 {
        let mut peak = 0.0f32;
        for y in 0..MESH_EXTENT.1 {
            for x in 0..MESH_EXTENT.0 {
                // Alpha is a constant 1.0 and would mask the interesting number.
                for channel in self.hdr_pixel(x, y).iter().take(3) {
                    peak = peak.max(*channel);
                }
            }
        }
        peak
    }
}

/// Decodes an IEEE binary16 into an `f32`.
///
/// Written out rather than pulled in: this is the only place in the engine that
/// reads a `Rgba16Float` on the CPU, and a dependency for twelve lines of shifts
/// would be a `cargo deny` conversation about a test helper.
fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let value = match exponent {
        // Zero or subnormal.
        0 => {
            if mantissa == 0 {
                0
            } else {
                // Renormalise: shift the mantissa up until its leading bit
                // falls off, decrementing the exponent as it goes.
                let leading = mantissa.leading_zeros() - 21;
                let mantissa = (mantissa << (leading + 1)) & 0x3ff;
                ((127 - 15 - leading) << 23) | (mantissa << 13)
            }
        }
        // Infinity or NaN.
        31 => 0xff << 23 | (mantissa << 13),
        _ => ((exponent + 127 - 15) << 23) | (mantissa << 13),
    };
    f32::from_bits(sign | value)
}

/// Renders one frame of the forward pipeline **through the real render graph**
/// and reads back both the swapchain image and the HDR scene target.
///
/// Deliberately `crcbl_render::ForwardRenderer` and `crcbl_render::RenderGraph`
/// rather than a hand-built copy: a golden image is only evidence about the code
/// the sandbox runs if it *is* the code the sandbox runs.
fn render_mesh(
    headless: &Headless,
    renderer: &mut crcbl_render::ForwardRenderer,
    pool: &mut crcbl_render::TransientPool,
    camera: &crcbl_render::Camera,
) -> MeshFrame {
    use crcbl_render::{ForwardRenderer, RenderGraph};

    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, MESH_EXTENT);

    let color_bytes = u64::from(width) * u64::from(height) * 4;
    // `Rgba16Float`: four channels of two bytes.
    let hdr_bytes = u64::from(width) * u64::from(height) * 8;
    let staging = |label, size| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer")
    };
    let color_staging = staging("mesh readback", color_bytes);
    let hdr_staging = staging("mesh hdr readback", hdr_bytes);

    renderer
        .begin_frame(
            device,
            camera,
            &crcbl_render::DirectionalLight::default(),
            ForwardRenderer::spin(MESH_SECONDS),
            MESH_EXTENT,
        )
        .expect("the uniform buffer is writable");

    // Where the graph's realised HDR handle lands, so the copy below can name
    // it. `Cell` rather than a channel: the pass body runs synchronously inside
    // `execute`, on this thread.
    let hdr_handle: std::cell::Cell<Option<crcbl_hal::ImageHandle>> = std::cell::Cell::new(None);

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mesh frame"),
        queue: headless.queue,
    });

    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl_render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                // **Not `Present`**: this frame is read back rather than shown,
                // so the graph is asked to leave it as a copy source and the
                // copy below needs no barrier of its own. Saying so in the
                // import is the whole point — there is still not one
                // hand-written barrier anywhere in this file's mesh path.
                final_state: ResourceState::TransferSrc,
            },
        );
        let scene = renderer.add_passes(&mut graph, target, MESH_EXTENT);
        // One extra declaration, and the graph works out that the HDR target
        // has to move from `ShaderRead` (the tonemap sampled it) to
        // `TransferSrc` (this wants to copy it).
        let sink = &hdr_handle;
        graph
            .add_compute_pass("hdr probe")
            .use_image(scene, ResourceState::TransferSrc)
            .execute(move |ctx| sink.set(Some(ctx.image(scene))));
        // `&*pool`: the same pool the frame is about to be realised
        // against, so the barriers open where the last frame left off.
        graph.compile(&*pool).expect("a legal frame")
    };
    eprintln!("vk e2e: {}", compiled.dump());
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");

    let scene_image = hdr_handle.get().expect("the probe pass ran");
    let layers = ImageSubresourceLayers {
        aspect: ImageAspect::COLOR,
        mip: 0,
        base_layer: 0,
        layer_count: 1,
    };
    // Both copies are outside every pass and need no barrier: the graph left
    // both images in `TransferSrc` because both were declared that way.
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: color_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: layers,
        image_offset: crcbl_hal::Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });
    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: hdr_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: scene_image,
        image_subresource: layers,
        image_offset: crcbl_hal::Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
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
            },
        )
        .expect("present");

    let mut color = vec![0u8; color_bytes as usize];
    headless.readback(color_staging, color_bytes, &mut color);
    let mut hdr = vec![0u8; hdr_bytes as usize];
    headless.readback(hdr_staging, hdr_bytes, &mut hdr);

    device.destroy_command_buffer(commands);
    device.destroy_buffer(color_staging);
    device.destroy_buffer(hdr_staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    MeshFrame {
        image: crcbl_golden::Image::from_readback(width, height, &color, order)
            .expect("the readback is exactly one image"),
        hdr,
    }
}

/// Milestones 3 and 4: a depth-tested, lit, spinning cube drawn through the
/// render graph, against a checked-in reference.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_lit_mesh_through_the_graph_matches_its_golden_image() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    let frame = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(crcbl_render::Projection::default()),
    );

    // Something drew, and it is not the whole frame: the clear must still be
    // visible in the corners, or the "cube" is a full-screen quad.
    let corner = frame.image.pixel(1, 1).expect("inside");
    assert!(
        corner[0] < 40 && corner[1] < 40 && corner[2] < 50,
        "the corner must still be the clear colour, got {corner:?}"
    );
    let centre = frame.image.pixel(128, 96).expect("inside");
    assert!(
        u32::from(centre[0]) + u32::from(centre[1]) + u32::from(centre[2]) > 60,
        "the centre must be the cube, not the clear, got {centre:?}"
    );

    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mesh.png");
    let golden = crcbl_golden::Golden::new(reference);
    let outcome = golden
        .check(&frame.image)
        .expect("the reference is readable");
    let comparison = match outcome.into_result() {
        Ok(comparison) => comparison,
        Err(message) => {
            renderer.destroy(headless.device.as_ref());
            pool.destroy(headless.device.as_ref());
            headless.device.wait_idle().expect("idle");
            panic!("{message}");
        }
    };
    eprintln!("vk e2e: golden mesh — {}", comparison.summary());

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// Milestone 5: the orthographic camera is a **projection-matrix swap and
/// nothing else**.
///
/// The assertion is in two halves, and both matter. The golden proves the
/// orthographic frame is the one that was reviewed; comparing it against the
/// perspective frame proves the swap actually did something, so a
/// `Projection::Orthographic` that silently fell through to perspective could
/// not pass.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_orthographic_camera_is_a_projection_swap_and_matches_its_golden() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");

    let ortho = crcbl_render::Projection::Orthographic {
        half_height: 0.9,
        near: 0.1,
        far: 100.0,
    };
    // The *same* renderer, the same pipeline, the same geometry, the same
    // shader, the same graph. One field differs.
    let perspective_frame = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(crcbl_render::Projection::default()),
    );
    let frame = render_mesh(&headless, &mut renderer, &mut pool, &mesh_camera(ortho));

    let differing = (0..MESH_EXTENT.1)
        .flat_map(|y| (0..MESH_EXTENT.0).map(move |x| (x, y)))
        .filter(|(x, y)| frame.image.pixel(*x, *y) != perspective_frame.image.pixel(*x, *y))
        .count();
    assert!(
        differing > (MESH_EXTENT.0 * MESH_EXTENT.1) as usize / 100,
        "swapping the projection must change the picture; only {differing} pixels moved"
    );

    let reference =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mesh_ortho.png");
    let golden = crcbl_golden::Golden::new(reference);
    let outcome = golden
        .check(&frame.image)
        .expect("the reference is readable");
    let comparison = match outcome.into_result() {
        Ok(comparison) => comparison,
        Err(message) => {
            renderer.destroy(headless.device.as_ref());
            pool.destroy(headless.device.as_ref());
            headless.device.wait_idle().expect("idle");
            panic!("{message}");
        }
    };
    eprintln!("vk e2e: golden ortho mesh — {}", comparison.summary());

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// Milestone 4, measured rather than eyeballed: the directional light produces a
/// real gradient across the cube's faces.
///
/// A shader that returned the vertex colour unchanged would draw a perfectly
/// good cube and pass a golden image the day it was blessed. What it could not
/// do is make two faces of *different* colours differ in brightness by the same
/// factor the Lambert term predicts — which is what this checks, using the HDR
/// target so the sRGB transfer function is not in the way.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_directional_light_actually_shades_the_mesh() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    let frame = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(crcbl_render::Projection::default()),
    );

    // Collect the linear luminance of every pixel the cube covers, in the HDR
    // target — where the values are the shader's own output rather than an sRGB
    // encoding of it.
    let mut lit: Vec<f32> = Vec::new();
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let [r, g, b, _] = frame.hdr_pixel(x, y);
            let luminance = 0.2126f32.mul_add(r, 0.7152f32.mul_add(g, 0.0722 * b));
            // Anything above the clear colour's luminance is geometry.
            if luminance > 0.05 {
                lit.push(luminance);
            }
        }
    }
    assert!(
        lit.len() > 1000,
        "the cube must cover a meaningful part of the frame; got {} pixels",
        lit.len()
    );
    lit.sort_by(f32::total_cmp);
    let dimmest = lit[lit.len() / 20];
    let brightest = lit[lit.len() - lit.len() / 20 - 1];
    assert!(
        brightest > dimmest * 1.5,
        "a directional light must produce a gradient across the faces: the 95th \
         percentile is {brightest} and the 5th is {dimmest}, a ratio of {}",
        brightest / dimmest
    );
    // And nothing is pure black: the ambient term exists so an unlit face is
    // dark rather than invisible.
    assert!(dimmest > 0.0, "the ambient term must lift the unlit faces");

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **HDR from P1 is real**, not a format enum.
///
/// `docs/plan/ROADMAP.md`'s correction asks for an `Rgba16Float` scene target
/// and a trivial tonemap from the first lit mesh. This reads the scene target
/// back and asserts it carries a value above 1.0 — which an `Rgba8` attachment
/// could not have held — and that the tonemapped swapchain pixel underneath it
/// is at the top of its range, which is the tonemap doing the one thing it does.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_hdr_target_carries_values_an_eight_bit_target_could_not() {
    let headless = Headless::open_for_mesh();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the forward renderer builds");
    let frame = render_mesh(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(crcbl_render::Projection::default()),
    );

    let peak = frame.peak_hdr();
    eprintln!("vk e2e: peak linear value in the HDR target — {peak}");
    assert!(
        peak > 1.0,
        "the Blinn highlight must exceed 1.0 somewhere, or the RGBA16F target is \
         carrying nothing an Rgba8 one could not; peak was {peak}"
    );
    assert!(
        peak.is_finite() && peak < 100.0,
        "a peak of {peak} is a NaN or a runaway, not a specular highlight"
    );

    // Find the brightest texel and check the tonemap clamped it rather than
    // letting it wrap or go black.
    let mut hottest = (0u32, 0u32, 0.0f32);
    for y in 0..MESH_EXTENT.1 {
        for x in 0..MESH_EXTENT.0 {
            let value = frame
                .hdr_pixel(x, y)
                .iter()
                .take(3)
                .fold(0.0f32, |peak, channel| peak.max(*channel));
            if value > hottest.2 {
                hottest = (x, y, value);
            }
        }
    }
    let pixel = frame
        .image
        .pixel(hottest.0, hottest.1)
        .expect("inside the frame");
    let brightest_channel = pixel[..3].iter().copied().max().expect("three channels");
    assert_eq!(
        brightest_channel, 255,
        "the tonemap must clamp a linear {} to the top of the swapchain's range, \
         got {pixel:?} at ({}, {})",
        hottest.2, hottest.0, hottest.1
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// Per-pass GPU timers, against a real clock.
///
/// `docs/plan/02-vulkan-backend.md` §2.4 asks for "GPU timestamp per pass,
/// exposed as a frame-timing report". `crcbl-render`'s own tests cover the
/// report's shape; this is the half that needs a driver — that the numbers are
/// non-zero, ordered, and attached to the right pass names.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn per_pass_gpu_timers_report_real_numbers() {
    let headless = Headless::open_for_mesh();
    let device = headless.device.as_ref();
    if !device.caps().features.contains(Features::TIMESTAMP_QUERY) {
        eprintln!("vk e2e: no timestamp queries on this device; the report degrades to empty");
        headless.finish();
        return;
    }

    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    let mut timers =
        crcbl_render::PassTimers::new(device, 2, 8).expect("the device reports timestamps");
    let camera = mesh_camera(crcbl_render::Projection::default());

    // Enough frames for the timer ring to come round and resolve a slot.
    for _ in 0..6 {
        let acquired = device
            .acquire_next_frame(headless.swapchain)
            .expect("an image");
        renderer
            .begin_frame(
                device,
                &camera,
                &crcbl_render::DirectionalLight::default(),
                crcbl_render::ForwardRenderer::spin(MESH_SECONDS),
                MESH_EXTENT,
            )
            .expect("uniforms");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("timed frame"),
            queue: headless.queue,
        });
        let compiled = {
            let mut graph = crcbl_render::RenderGraph::new(headless.queue);
            let target = graph.import_image(
                "swapchain",
                crcbl_render::ForwardRenderer::present_target(
                    acquired.image,
                    acquired.view,
                    headless.format,
                    MESH_EXTENT,
                ),
            );
            let _ = renderer.add_passes(&mut graph, target, MESH_EXTENT);
            graph.compile(&pool).expect("a legal frame")
        };
        compiled
            .execute(device, &mut pool, encoder.as_mut(), Some(&mut timers))
            .expect("executed");
        let commands = encoder.finish().expect("recorded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device
            .present(
                headless.queue,
                &PresentInfo {
                    swapchain: headless.swapchain,
                    waits: acquired.present_semaphore.as_slice(),
                },
            )
            .expect("present");
        // The timers resolve a slot only when it comes back round, and this
        // suite submits without pipelining, so an idle here is what stands in
        // for the frame loop's timeline wait.
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
    }

    let timings = timers.latest();
    eprintln!("vk e2e: {}", timings.report());
    assert_eq!(
        timings
            .passes
            .iter()
            .map(|pass| pass.label.as_str())
            .collect::<Vec<_>>(),
        vec!["forward", "tonemap"],
        "the report must name the passes the graph ran, in order"
    );
    assert!(
        timings.total_nanos() > 0,
        "a real GPU took a measurable amount of time: {}",
        timings.report()
    );
    // A loose ceiling on purpose. The failure this guards against is a *unit*
    // mistake — raw ticks reported as nanoseconds, which on radv's 1.0 ns period
    // would be invisible and on another device would be out by orders of
    // magnitude — not slowness. A tight bound would instead be a load-dependent
    // flake, and lavapipe's "GPU" time is CPU time on a machine that may be
    // running thirty other things.
    assert!(
        timings.total_nanos() < 10_000_000_000,
        "a 256x192 frame reporting over ten seconds is a unit mistake, not a slow \
         machine: {}",
        timings.report()
    );

    timers.destroy(device);
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}

// --- reversed-Z, proved rather than asserted --------------------------------

/// Two overlapping quads, the **near one drawn first**, so the depth test is the
/// only thing deciding what is visible.
///
/// This is the fixture that makes reversed-Z a test result. `crcbl-render`'s
/// `camera` module proves the *maths* on the CPU — two surfaces a centimetre
/// apart at 300 m quantise to the same `f32` under a conventional projection and
/// to different ones under the engine's. This proves the *pipeline*: the same
/// geometry, the same shader, the same `CompareOp::Greater`, the same clear of
/// 0.0, and **only the projection matrix differs** between the two runs. One
/// produces a red square, the other a blue one.
///
/// Why the near quad is drawn first: with the far quad first, a broken depth
/// test would still leave the near one on top by draw order, and the test would
/// pass for the wrong reason.
struct DepthProbe {
    vertices: crcbl_hal::BufferHandle,
    indices: crcbl_hal::BufferHandle,
    uniforms: crcbl_hal::BufferHandle,
    layout: crcbl_hal::BindGroupLayoutHandle,
    group: crcbl_hal::BindGroupHandle,
    pipeline_layout: crcbl_hal::PipelineLayoutHandle,
    pipeline: crcbl_hal::GraphicsPipelineHandle,
}

/// Where the probe's camera sits, on the +Z axis looking at the origin.
const PROBE_EYE: f32 = 2.0;
/// The probe's near plane. The only number that controls depth precision.
const PROBE_NEAR: f32 = 0.1;
/// The probe's far plane — used **only** by the conventional control matrix; the
/// engine's own projection has none.
const PROBE_FAR: f32 = 100.0;

impl DepthProbe {
    /// The two quads, near-first, in `crcbl_shaders::mesh::MeshVertex` layout.
    fn geometry() -> (Vec<u8>, Vec<u8>) {
        // (z, half-extent, colour). The near quad is smaller, so a correct
        // frame is a red square inside a blue ring and a *wrong* one is a plain
        // blue rectangle — two visibly different pictures, not two shades.
        let quads = [
            (0.3f32, 0.25f32, [0.9f32, 0.05, 0.05]),
            (-0.3, 0.6, [0.05, 0.1, 0.9]),
        ];
        let mut vertices = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        for (quad, (z, half, color)) in quads.iter().enumerate() {
            let base = u32::try_from(quad * 4).expect("two quads");
            for (x, y) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                for value in [x * half, y * half, *z, 1.0] {
                    vertices.extend_from_slice(&value.to_le_bytes());
                }
                // Facing the camera, so both quads are lit identically and the
                // only difference between them is their albedo.
                for value in [0.0f32, 0.0, 1.0, 0.0] {
                    vertices.extend_from_slice(&value.to_le_bytes());
                }
                for value in [color[0], color[1], color[2], 1.0] {
                    vertices.extend_from_slice(&value.to_le_bytes());
                }
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let index_bytes = indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        (vertices, index_bytes)
    }

    fn new(headless: &Headless) -> Self {
        let device = headless.device.as_ref();
        let (vertex_bytes, index_bytes) = Self::geometry();

        let upload = |label, usage, bytes: &[u8], state| {
            let size = bytes.len() as u64;
            let staging = device
                .create_buffer(&BufferDesc {
                    label: Some("probe staging"),
                    size,
                    usage: BufferUsage::TRANSFER_SRC,
                    memory: MemoryLocation::HostUpload,
                })
                .expect("a staging buffer");
            device.write_buffer(staging, 0, bytes).expect("write");
            let target = device
                .create_buffer(&BufferDesc {
                    label: Some(label),
                    size,
                    usage: usage | BufferUsage::TRANSFER_DST,
                    memory: MemoryLocation::DeviceLocal,
                })
                .expect("a device-local buffer");
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("probe upload"),
                queue: headless.queue,
            });
            encoder.copy_buffer_to_buffer(&crcbl_hal::BufferCopy {
                src: staging,
                src_offset: 0,
                dst: target,
                dst_offset: 0,
                size,
            });
            encoder.pipeline_barrier(&Barriers {
                buffers: &[crcbl_hal::BufferBarrier::new(
                    target,
                    ResourceState::TransferDst,
                    state,
                )],
                ..Barriers::default()
            });
            let commands = encoder.finish().expect("recorded");
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);
            device.destroy_buffer(staging);
            target
        };

        let vertices = upload(
            "probe vertices",
            BufferUsage::STORAGE,
            &vertex_bytes,
            ResourceState::ShaderRead,
        );
        let indices = upload(
            "probe indices",
            BufferUsage::INDEX,
            &index_bytes,
            ResourceState::IndexBuffer,
        );
        let uniforms = device
            .create_buffer(&BufferDesc {
                label: Some("probe uniforms"),
                size: crcbl_shaders::mesh::FRAME_UNIFORMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a uniform buffer");

        let entries = [
            crcbl_hal::BindGroupLayoutEntry {
                binding: 0,
                visibility: crcbl_hal::ShaderStages::VERTEX
                    .union(crcbl_hal::ShaderStages::FRAGMENT),
                kind: crcbl_hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 1,
                visibility: crcbl_hal::ShaderStages::VERTEX,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
        ];
        let layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("probe"),
                entries: &entries,
            })
            .expect("a layout");
        let group_entries = [
            crcbl_hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(uniforms),
            },
            crcbl_hal::BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(vertices),
            },
        ];
        let group = device
            .create_bind_group(&crcbl_hal::BindGroupDesc {
                label: Some("probe"),
                layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");
        let set_layouts = [layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
                label: Some("probe"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        let module = device
            .create_shader_module(&crcbl_hal::ShaderModuleDesc {
                label: Some("mesh.slang"),
                spirv: crcbl_shaders::MESH.spirv(),
                wgsl: crcbl_shaders::MESH.wgsl(),
            })
            .expect("the committed SPIR-V is accepted");
        let color_targets = [crcbl_hal::ColorTargetState::opaque(headless.format)];
        let pipeline = device.create_graphics_pipeline(&crcbl_hal::GraphicsPipelineDesc {
            label: Some("depth probe"),
            layout: pipeline_layout,
            vertex: crcbl_hal::ShaderEntry {
                module,
                entry_point: "vertexMain",
            },
            fragment: Some(crcbl_hal::ShaderEntry {
                module,
                entry_point: "fragmentMain",
            }),
            primitive: crcbl_hal::PrimitiveState {
                // No culling: the point is the depth test, and a winding
                // mistake would otherwise delete a quad and look like one.
                cull_mode: crcbl_hal::CullMode::None,
                ..crcbl_hal::PrimitiveState::default()
            },
            // The seam's default, unchanged: `Greater` on `D32Float` with
            // writes on. **This is what the two projections are tested
            // against, and it is not adjusted between runs.**
            depth_stencil: Some(crcbl_hal::DepthStencilState::default()),
            multisample: crcbl_hal::MultisampleState::default(),
            color_targets: &color_targets,
        });
        device.destroy_shader_module(module);

        Self {
            vertices,
            indices,
            uniforms,
            layout,
            group,
            pipeline_layout,
            pipeline: pipeline.expect("a graphics pipeline"),
        }
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.group);
        device.destroy_bind_group_layout(self.layout);
        device.destroy_buffer(self.uniforms);
        device.destroy_buffer(self.indices);
        device.destroy_buffer(self.vertices);
    }
}

/// Renders the probe with `view_proj` and reads the frame back.
fn render_probe(
    headless: &Headless,
    probe: &DepthProbe,
    pool: &mut crcbl_render::TransientPool,
    view_proj: glam::Mat4,
) -> crcbl_golden::Image {
    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;

    let uniforms = crcbl_shaders::mesh::FrameUniforms {
        view_proj: view_proj.to_cols_array(),
        model: glam::Mat4::IDENTITY.to_cols_array(),
        camera_position: [0.0, 0.0, PROBE_EYE, 1.0],
        // Straight at the quads, so both are lit identically and the only
        // difference between them is their albedo.
        light_direction: [0.0, 0.0, 1.0, 0.0],
        light_color: [0.8, 0.8, 0.8, 0.0],
        ambient: [0.2, 0.2, 0.2, 0.0],
    };
    device
        .write_buffer(probe.uniforms, 0, &uniforms.to_bytes())
        .expect("write");

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("an image");
    let bytes = u64::from(width) * u64::from(height) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("probe readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("probe frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = crcbl_render::RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl_render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                final_state: ResourceState::TransferSrc,
            },
        );
        let depth = graph.create_image(
            "probe-depth",
            crcbl_render::TransientImageDesc::scene_depth(MESH_EXTENT),
        );
        graph
            .add_render_pass("probe")
            .clear_color(target, [0.0, 0.0, 0.0, 1.0])
            // The reversed-Z clear: `depth::CLEAR` = 0.0, so any geometry beats
            // the empty buffer under `Greater`.
            .clear_depth(depth)
            .execute(|ctx| {
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(probe.pipeline);
                encoder.bind_group(0, probe.group, &[], probe.pipeline_layout);
                encoder.bind_index_buffer(probe.indices, 0, crcbl_hal::IndexFormat::Uint32);
                encoder.draw_indexed(0..12, 0, 0..1);
            });
        graph.compile(&*pool).expect("a legal frame")
    };
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("executed");

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
        image_extent: Extent3d::d2(width, height),
    });
    let commands = encoder.finish().expect("recorded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
            },
        )
        .expect("present");

    let mut pixels = vec![0u8; bytes as usize];
    headless.readback(staging, bytes, &mut pixels);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &pixels, order).expect("one image")
}

/// **Reversed-Z, on the GPU, discriminated against the alternative.**
///
/// `docs/plan/02-vulkan-backend.md` locks reversed-Z, and it is the kind of
/// decision a comment can claim and nothing checks. This renders the *same*
/// geometry through the *same* pipeline with the *same* `Greater` compare op and
/// the *same* clear of 0.0, twice, changing one thing: the projection matrix.
///
/// * With the engine's reversed-Z projection, the near quad wins → **red**.
/// * With a conventional `0 at near, 1 at far` projection, the far quad has the
///   larger depth value, passes `Greater`, and overwrites it → **blue**.
///
/// So this test would fail under standard-Z, in the direction that says which
/// convention is in force — which is the point.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn reversed_z_puts_the_nearer_surface_in_front_and_standard_z_would_not() {
    let headless = Headless::open_for_mesh();
    let probe = DepthProbe::new(&headless);
    let mut pool = crcbl_render::TransientPool::new();

    #[allow(clippy::cast_precision_loss)]
    let aspect = MESH_EXTENT.0 as f32 / MESH_EXTENT.1 as f32;
    let view = glam::camera::rh::view::look_at_mat4(
        glam::Vec3::new(0.0, 0.0, PROBE_EYE),
        glam::Vec3::ZERO,
        glam::Vec3::Y,
    );
    let fov = core::f32::consts::FRAC_PI_4;

    // The engine's own projection, straight out of `crcbl-render`.
    let reversed = crcbl_render::Projection::Perspective {
        fov_y: fov,
        near: PROBE_NEAR,
    }
    .matrix(aspect)
        * view;
    // The control: conventional depth, 0 at the near plane and 1 at the far one.
    // `crcbl-render` deliberately has no constructor for this, which is why the
    // suite reaches for glam directly.
    let standard =
        glam::camera::rh::proj::directx::perspective(fov, aspect, PROBE_NEAR, PROBE_FAR) * view;

    let centre = (MESH_EXTENT.0 / 2, MESH_EXTENT.1 / 2);

    let reversed_frame = render_probe(&headless, &probe, &mut pool, reversed);
    let pixel = reversed_frame.pixel(centre.0, centre.1).expect("inside");
    assert!(
        pixel[0] > pixel[2] && pixel[0] > 100,
        "under reversed-Z the *near* quad must win the depth test, so the centre \
         must be red; got {pixel:?}. If it is blue, the projection matrix is not \
         reversed and every depth comparison in the engine is inverted."
    );

    // And the far quad really is there, around the edge of the near one — so
    // "red at the centre" is a depth test rather than the blue quad having
    // failed to draw at all.
    // Between the two quads' silhouettes. At this camera the near quad reaches
    // 34 pixels from the centre and the far one 60, so 48 is comfortably inside
    // the blue ring and comfortably outside the red square — worth deriving
    // rather than guessing, because a sample point that lands on neither reads
    // as a depth-test failure.
    let ring = (MESH_EXTENT.0 / 2, MESH_EXTENT.1 / 2 + 48);
    let pixel = reversed_frame.pixel(ring.0, ring.1).expect("inside");
    assert!(
        pixel[2] > pixel[0] && pixel[2] > 100,
        "the far quad is larger, so it must be visible around the near one; got \
         {pixel:?} at {ring:?}"
    );

    let standard_frame = render_probe(&headless, &probe, &mut pool, standard);
    let pixel = standard_frame.pixel(centre.0, centre.1).expect("inside");
    assert!(
        pixel[2] > pixel[0] && pixel[2] > 100,
        "the control is only meaningful if a conventional projection really does \
         invert the outcome under the engine's `Greater` test; it gave {pixel:?}, \
         which is not blue. Re-derive the quad depths rather than relaxing this."
    );

    eprintln!(
        "vk e2e: reversed-Z centre {:?}, conventional-Z centre {:?} — the same \
         pipeline, the same compare op, only the projection differs",
        reversed_frame.pixel(centre.0, centre.1).expect("inside"),
        standard_frame.pixel(centre.0, centre.1).expect("inside"),
    );

    probe.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// A resize storm, driven through the **render graph** rather than around it.
///
/// This is the path with the most new moving parts at P1.3 and the least
/// obvious failure mode. Every size change invalidates both scene transients,
/// so the pool must hand out new ones and retire the old; and the tonemap's bind
/// group names a *graph-owned* view, so it must be rebuilt when that view
/// changes and destroyed when it does — while a previous frame may still be
/// reading it.
///
/// Getting any of that wrong is a validation error rather than a wrong picture,
/// which is why the assertion is the report: `Headless::finish` fails on any
/// error or warning, and on the layer never having loaded.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_graph_and_its_pool_survive_a_resize_storm() {
    let headless = Headless::open_for_mesh();
    let device = headless.device.as_ref();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    let camera = mesh_camera(crcbl_render::Projection::default());

    // Sizes chosen to be genuinely different rather than a nudge — including one
    // that is not a multiple of anything, because a row-pitch assumption hides
    // behind round numbers.
    let sizes = [
        MESH_EXTENT,
        (64, 48),
        (300, 130),
        (17, 5),
        (256, 192),
        (64, 48),
        MESH_EXTENT,
    ];
    for extent in sizes {
        device
            .reconfigure_swapchain(
                headless.swapchain,
                &SwapchainDesc {
                    label: Some("vk e2e mesh ring"),
                    surface: headless.surface,
                    format: headless.format,
                    extent,
                    image_count: 2,
                    present_mode: PresentMode::Fifo,
                    composite_alpha: CompositeAlpha::Opaque,
                },
            )
            .expect("reconfigure keeps the handle valid");

        // Two frames per size, so the second one exercises the *reuse* path
        // rather than only the create path.
        for _ in 0..2 {
            let acquired = device
                .acquire_next_frame(headless.swapchain)
                .expect("an image");
            assert_eq!(acquired.extent, extent);
            renderer
                .begin_frame(
                    device,
                    &camera,
                    &crcbl_render::DirectionalLight::default(),
                    crcbl_render::ForwardRenderer::spin(MESH_SECONDS),
                    extent,
                )
                .expect("uniforms");

            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("resize frame"),
                queue: headless.queue,
            });
            let compiled = {
                let mut graph = crcbl_render::RenderGraph::new(headless.queue);
                let target = graph.import_image(
                    "swapchain",
                    crcbl_render::ForwardRenderer::present_target(
                        acquired.image,
                        acquired.view,
                        headless.format,
                        extent,
                    ),
                );
                let _ = renderer.add_passes(&mut graph, target, extent);
                graph.compile(&pool).expect("a legal frame")
            };
            // Every pass renders at the size that was just configured, which is
            // the graph deriving its render area from the attachments rather
            // than from anything remembered.
            for pass in compiled.passes() {
                assert_eq!(
                    (pass.render_area().width, pass.render_area().height),
                    extent,
                    "pass {:?} rendered at the wrong size",
                    pass.label()
                );
            }
            compiled
                .execute(device, &mut pool, encoder.as_mut(), None)
                .expect("executed");
            let commands = encoder.finish().expect("recorded");
            device
                .submit(headless.queue, &SubmitInfo::new(&[commands]))
                .expect("submit");
            device
                .present(
                    headless.queue,
                    &PresentInfo {
                        swapchain: headless.swapchain,
                        waits: acquired.present_semaphore.as_slice(),
                    },
                )
                .expect("present");
            // Nothing is pipelined here, so an idle stands in for the frame
            // loop's timeline wait before the pool retires anything.
            device.wait_idle().expect("idle");
            device.destroy_command_buffer(commands);
            pool.retire_unused(device);
        }

        // The pool must converge rather than accumulate one pair of targets per
        // size the window passed through. Two live transients, plus at most
        // `RETIRE_AFTER_FRAMES` generations of stale ones.
        let ceiling = 2 * (crcbl_render::transient::RETIRE_AFTER_FRAMES as usize + 1);
        assert!(
            pool.image_count() <= ceiling,
            "after resizing to {extent:?} the pool holds {} images, over the {ceiling} \
             a bounded retirement allows",
            pool.image_count()
        );
    }

    renderer.destroy(device);
    pool.destroy(device);
    // The report is the assertion: a bind group destroyed while in flight, a
    // transient freed too early, or a stale view sampled would all land here.
    headless.finish();
}

// --- regressions from the 2026-08 code review -------------------------------
//
// Three paths the review identified as both wrong and untested. Each one is a
// sequence a real frame loop performs and the offscreen suite never did.

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
            memory: MemoryLocation::DeviceLocal,
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
    encoder.fill_buffer(staging, 0, 256, 0);
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
    let mut bytes = vec![0u8; 256];
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
                },
            )
            .expect("present");
        headless.device.wait_idle().expect("idle");
    }

    headless.finish();
}

// --- slice 4: the sprite pass, and the first pixels it is shown to draw -------
//
// Everything above this line about sprites was a recorder assertion — instance
// bytes, batching, draw ranges, teardown. `docs/backlog.md` said so under
// "Coverage gaps": nothing in `crcbl-sprite` or `crcbl_render::sprite_pass` had
// ever been checked against a real image or a real renderer, and the first
// honest check of that is a golden image through the pass. This section is it.
//
// Four things are asserted, and the last two are the ones that could not have
// been faked from the CPU side:
//
// 1. A sprite is drawn at all, the right way up, in the right place, showing the
//    frame it named. The test sheet is deliberately **asymmetric** — a different
//    colour in each corner of each frame — because `sprite.slang`'s vertex stage
//    warns that a flipped V renders every sprite upside down while looking
//    entirely plausible on a symmetric image, and a symmetric test image is how
//    that ships.
// 2. Alpha blending composites onto what is already there rather than replacing
//    it, checked against the linear-light blend arithmetic rather than eyeballed.
// 3. `SampleMode::Pixel` and `SampleMode::Smooth` produce visibly different
//    pictures at a non-integer scale, in the place and by the amount predicted.
//    That difference is the only real evidence that sharp-bilinear happened.
// 4. `Pixel` at a whole scale is **exactly flat inside each texel** — which is
//    the whole difference between sharp-bilinear and plain linear, and is
//    asserted on sampled pixel values rather than only through a reference.

/// The size the sprite suite renders at.
///
/// The same 256×192 as the mesh and the triangle, for the same reason: the
/// golden's structural metric averages over 8×8 blocks.
const SPRITE_EXTENT: (u32, u32) = (256, 192);

/// The background every sprite frame composites onto, in **linear** light —
/// which is what a `vkClearColorValue` on an `Rgba8UnormSrgb` attachment means.
///
/// Deliberately not black and not grey: alpha blending onto black is the one
/// background where `src * a + dst * (1 - a)` and `src * a` agree, so a
/// premultiplication mistake would be invisible.
const SPRITE_CLEAR: [f32; 4] = [0.10, 0.20, 0.35, 1.0];

/// Half the visible world height, chosen so **one world unit is one device
/// pixel**: 96 up and down over 192 rows, and 96 × 256/192 = 128 left and right
/// over 256 columns.
///
/// That is what makes every placement assertion below arithmetic rather than a
/// number read off a picture.
const SPRITE_HALF_HEIGHT: f32 = 96.0;

/// The camera every sprite frame is drawn with: orthographic, looking down −Z at
/// the plane the sprites live on.
fn sprite_camera() -> crcbl_render::Camera {
    crcbl_render::Camera {
        eye: glam::Vec3::new(0.0, 0.0, 1.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection: crcbl_render::Projection::Orthographic {
            half_height: SPRITE_HALF_HEIGHT,
            near: 0.1,
            far: 10.0,
        },
    }
}

/// World → device pixels under [`sprite_camera`].
///
/// `x + 128` because world x runs −128..128 across 256 columns, and `96 − y`
/// because world Y is **up** and a framebuffer's rows run down.
/// [`assert_the_camera_maps_a_world_unit_to_a_pixel`] checks this against the
/// real matrix rather than leaving it as a claim.
fn world_to_pixel(world: [f32; 2]) -> [f32; 2] {
    [world[0] + 128.0, 96.0 - world[1]]
}

/// Confirms [`world_to_pixel`] agrees with the matrix the pass is actually
/// handed, including the Y flip `crcbl-vk`'s negative-height viewport applies.
///
/// Called from every placement test, because every one of them reads a pixel at
/// a coordinate this function produced: if the mapping is wrong the assertions
/// are checking the wrong pixels and would happily pass on a blank frame.
fn assert_the_camera_maps_a_world_unit_to_a_pixel() {
    let (width, height) = SPRITE_EXTENT;
    let aspect = width as f32 / height as f32;
    let view_projection = sprite_camera().view_projection(aspect);
    for world in [[0.0, 0.0], [-128.0, 96.0], [64.0, -32.0], [-100.0, 16.0]] {
        let clip = view_projection * glam::Vec4::new(world[0], world[1], 0.0, 1.0);
        let ndc = clip.truncate() / clip.w;
        // The seam's convention is +Y up in NDC and `crcbl-vk` submits a
        // negative-height viewport, so the framebuffer row is the flipped half.
        let pixel = [
            0.5f32.mul_add(ndc.x, 0.5) * width as f32,
            0.5f32.mul_add(-ndc.y, 0.5) * height as f32,
        ];
        let expected = world_to_pixel(world);
        assert!(
            (pixel[0] - expected[0]).abs() < 1e-3 && (pixel[1] - expected[1]).abs() < 1e-3,
            "world {world:?} projects to pixel {pixel:?}, not the {expected:?} every \
             assertion below is written against"
        );
    }
}

/// sRGB encode, the transfer function an `Rgba8UnormSrgb` attachment applies on
/// the way out.
fn srgb_encode(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055f32.mul_add(linear.powf(1.0 / 2.4), -0.055)
    }
}

/// sRGB decode, what the sampler and the blender apply on the way in.
fn srgb_decode(encoded: f32) -> f32 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// The 8-bit value a linear channel is stored as.
fn srgb_byte(linear: f32) -> u8 {
    (srgb_encode(linear) * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The background byte triple the clear lands on, which is what every "this
/// pixel was not drawn on" assertion compares against.
fn background_rgb() -> [u8; 3] {
    [
        srgb_byte(SPRITE_CLEAR[0]),
        srgb_byte(SPRITE_CLEAR[1]),
        srgb_byte(SPRITE_CLEAR[2]),
    ]
}

/// **The asymmetric test sheet**: 4×2 texels, two 2×2 frames side by side, and
/// no two texels alike.
///
/// ```text
///   frame A          frame B
///   red    green  |  cyan   magenta
///   blue   yellow |  white  black
/// ```
///
/// Every symmetry a mistake could hide behind is broken: a flipped V swaps red
/// with blue, a flipped U swaps red with green, a transposed pair swaps green
/// with blue, and picking the wrong frame swaps the whole palette. A symmetric
/// sheet — a checkerboard, a single square, anything with a mirror line — passes
/// every one of those.
fn asymmetric_sheet() -> Vec<u8> {
    let row = |texels: [[u8; 4]; 4]| texels.into_iter().flatten().collect::<Vec<u8>>();
    let mut pixels = row([
        [255, 0, 0, 255],   // A top-left: red
        [0, 255, 0, 255],   // A top-right: green
        [0, 255, 255, 255], // B top-left: cyan
        [255, 0, 255, 255], // B top-right: magenta
    ]);
    pixels.extend(row([
        [0, 0, 255, 255],     // A bottom-left: blue
        [255, 255, 0, 255],   // A bottom-right: yellow
        [255, 255, 255, 255], // B bottom-left: white
        [0, 0, 0, 255],       // B bottom-right: black
    ]));
    pixels
}

/// Frame A of [`asymmetric_sheet`], as normalised UVs in image order.
const FRAME_A: [f32; 4] = [0.0, 0.0, 0.5, 1.0];
/// Frame B of [`asymmetric_sheet`].
const FRAME_B: [f32; 4] = [0.5, 0.0, 1.0, 1.0];

/// The four corner colours of frame A, in `[top-left, top-right, bottom-left,
/// bottom-right]` order — the order the quadrant assertions read them in.
const FRAME_A_CORNERS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
/// The same for frame B.
const FRAME_B_CORNERS: [[u8; 3]; 4] = [[0, 255, 255], [255, 0, 255], [255, 255, 255], [0, 0, 0]];

/// A 2×2 sheet on its own, the same four colours as frame A.
///
/// The filtering tests use a sheet whose only frame *is* the whole image, so the
/// UV clamp at the frame's edge is the sheet's edge and there is no neighbouring
/// frame for a filter to bleed in from — which would be a second explanation for
/// any difference they measure.
fn quad_sheet() -> Vec<u8> {
    [
        [255u8, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A 2×2 sheet with a different alpha in each texel.
///
/// ```text
///   opaque red     half-alpha red
///   opaque green   fully transparent
/// ```
///
/// One sprite then covers all three cases at once: replace, blend, and leave
/// alone.
fn alpha_sheet() -> Vec<u8> {
    [
        [255u8, 0, 0, 255],
        [255, 0, 0, 128],
        [0, 255, 0, 255],
        [0, 0, 0, 0],
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The alpha the half-transparent texel is sampled with.
///
/// **Not sRGB-decoded.** Vulkan's sRGB formats apply the transfer function to
/// the colour channels only; alpha is linear, and a test that decoded it would
/// be asserting against the wrong number by 30%.
const HALF_ALPHA: f32 = 128.0 / 255.0;

impl Headless {
    /// Opens a ring at a pinned `Rgba8UnormSrgb` for the sprite suite.
    ///
    /// Pinned rather than preferred for the same reason
    /// [`Headless::open_for_mesh`] pins it: a golden image compared across two
    /// machines must not depend on which format each one's surface offered
    /// first. sRGB specifically, because `SpriteRenderer::register_sheet`
    /// uploads sheets as `Rgba8UnormSrgb` and the alpha blend is only in linear
    /// light if the target decodes too.
    fn open_for_sprites() -> Self {
        let instance = instance();
        let adapter = instance.adapters().remove(0);
        // SAFETY: `Offscreen` names no platform object at all.
        let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("offscreen always works");
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("vk e2e sprites"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::TIER_A | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(crcbl_hal::QueueKind::Graphics)
            .expect("a graphics queue always exists");
        let format = Format::Rgba8UnormSrgb;
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("vk e2e sprite ring"),
                surface,
                format,
                extent: SPRITE_EXTENT,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect("the ring is created");
        Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
        }
    }
}

/// Renders one sprite frame **through the real `SpriteRenderer` and the real
/// `RenderGraph`** and reads the swapchain image back.
///
/// A clear pass first and the sprite pass on top of it, because
/// `SpriteRenderer::add_pass` loads rather than clears — compositing onto
/// whatever is already there is the thing it is for, and a suite that cleared
/// inside the sprite pass would not be testing that.
fn render_sprites(
    headless: &Headless,
    renderer: &mut crcbl_render::SpriteRenderer,
    pool: &mut crcbl_render::TransientPool,
    sprites: &[crcbl_render::Sprite],
) -> crcbl_golden::Image {
    use crcbl_render::RenderGraph;

    let device = headless.device.as_ref();
    let (width, height) = SPRITE_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, SPRITE_EXTENT);

    let color_bytes = u64::from(width) * u64::from(height) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("sprite readback"),
            size: color_bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let aspect = width as f32 / height as f32;
    renderer
        .begin_frame(
            device,
            sprites,
            sprite_camera().view_projection(aspect),
            SPRITE_EXTENT,
        )
        .expect("the instance and constant buffers are writable");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("sprite frame"),
        queue: headless.queue,
    });

    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl_render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: SPRITE_EXTENT,
                initial: ResourceState::Undefined,
                // Read back rather than shown, so the graph is asked to leave it
                // as a copy source and the copy below needs no hand-written
                // barrier — the same trick the mesh path uses.
                final_state: ResourceState::TransferSrc,
            },
        );
        graph
            .add_render_pass("sprite background")
            .clear_color(target, SPRITE_CLEAR)
            .execute(|_| {});
        renderer.add_pass(&mut graph, target);
        graph.compile(&*pool).expect("a legal frame")
    };
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");

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
        image_extent: Extent3d::d2(width, height),
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
            },
        )
        .expect("present");

    let mut color = vec![0u8; color_bytes as usize];
    headless.readback(staging, color_bytes, &mut color);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &color, order)
        .expect("the readback is exactly one image")
}

/// Registers one sheet and returns its id.
fn register_sheet(
    renderer: &mut crcbl_render::SpriteRenderer,
    device: &dyn Device,
    label: &str,
    width: u32,
    height: u32,
    sample: crcbl_render::SampleMode,
    pixels: &[u8],
) -> crcbl_render::SheetId {
    renderer
        .register_sheet(
            device,
            &crcbl_render::SheetDesc {
                label,
                width,
                height,
                sample,
                pixels,
            },
        )
        .expect("the sheet uploads")
}

/// The RGB of a pixel, dropping alpha — every frame here is opaque, and carrying
/// the fourth channel through every assertion only makes them longer.
fn rgb(image: &crcbl_golden::Image, x: u32, y: u32) -> [u8; 3] {
    let pixel = image.pixel(x, y).unwrap_or_else(|| {
        panic!(
            "({x}, {y}) is outside a {}x{} frame",
            image.width(),
            image.height()
        )
    });
    [pixel[0], pixel[1], pixel[2]]
}

/// Whether two RGB triples agree to within `slack` on every channel.
fn close(actual: [u8; 3], expected: [u8; 3], slack: i32) -> bool {
    actual
        .iter()
        .zip(&expected)
        .all(|(a, e)| (i32::from(*a) - i32::from(*e)).abs() <= slack)
}

/// Asserts a pixel is the background — nothing was drawn here.
fn assert_background(image: &crcbl_golden::Image, x: u32, y: u32) {
    let actual = rgb(image, x, y);
    assert!(
        close(actual, background_rgb(), 2),
        "({x}, {y}) should still be the clear colour {:?}, got {actual:?}",
        background_rgb()
    );
}

/// Compares an image against a checked-in reference and **returns** the verdict
/// rather than asserting it.
///
/// Deferred on purpose: a test that panicked here would leave the renderer, the
/// pool and the device undestroyed, and the resulting `Drop` warning and dirty
/// validation report would be printed on top of the message that says what
/// actually went wrong. Every caller tears down first and unwraps last.
fn sprite_golden(name: &str, image: &crcbl_golden::Image) -> Result<String, String> {
    let reference =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.png"));
    crcbl_golden::Golden::new(reference)
        .check(image)
        .expect("the reference is readable")
        .into_result()
        .map(|comparison| format!("golden {name} — {}", comparison.summary()))
}

/// Reports every deferred golden verdict, failing on the first that did not
/// match.
fn report_goldens(verdicts: Vec<Result<String, String>>) {
    let mut failures = Vec::new();
    for verdict in verdicts {
        match verdict {
            Ok(summary) => eprintln!("vk e2e: {summary}"),
            Err(message) => failures.push(message),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// **The sprite pass draws.** Two sprites, two different frames of one
/// asymmetric sheet, at two places, at a whole scale.
///
/// This is the test the whole slice exists for: before it, nothing in the sprite
/// path had been shown to put a single pixel anywhere. Every assertion is on a
/// coordinate computed from the world rect through [`world_to_pixel`], so a
/// sprite in the wrong place, upside down, mirrored, or showing the wrong frame
/// fails on a named pixel rather than only on a reference image.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_sprite_is_drawn_the_right_way_up_in_the_right_place_from_the_right_frame() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let sheet = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "asymmetric",
        4,
        2,
        crcbl_render::SampleMode::Pixel,
        &asymmetric_sheet(),
    );

    // 64 world units is 64 pixels is 32 pixels per texel — a whole scale, so the
    // quadrants are square blocks of one colour and their centres are exact.
    let first = [-100.0f32, 16.0, 64.0, 64.0];
    let second = [20.0f32, -80.0, 64.0, 64.0];
    let image = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[
            crcbl_render::Sprite {
                sheet,
                rect: first,
                uv: FRAME_A,
                tint: [1.0; 4],
            },
            crcbl_render::Sprite {
                sheet,
                rect: second,
                uv: FRAME_B,
                tint: [1.0; 4],
            },
        ],
    );

    // Anti-vacuity, first: a blank frame passes a golden forever after it is
    // blessed, so the frame has to be a picture before the reference is worth
    // anything. Eight texel colours plus the clear is nine.
    let colors = image.distinct_colors(32);
    assert!(
        colors >= 9,
        "the frame must contain both sprites' eight colours and the clear; found {colors}"
    );

    // The quadrant centres, in `[top-left, top-right, bottom-left, bottom-right]`
    // order — which is the order the corner tables are written in and therefore
    // the order a flip would break.
    let quadrant_centres = |rect: [f32; 4]| {
        let low = world_to_pixel([rect[0], rect[1]]);
        let quarter = rect[2] / 4.0;
        let (left, right) = (low[0] + quarter, low[0] + 3.0 * quarter);
        // `low` is the world *minimum* corner, which is the **bottom** of the
        // quad, so it is the larger screen row.
        let (top, bottom) = (low[1] - 3.0 * quarter, low[1] - quarter);
        [
            [left as u32, top as u32],
            [right as u32, top as u32],
            [left as u32, bottom as u32],
            [right as u32, bottom as u32],
        ]
    };

    for (rect, corners, name) in [
        (first, FRAME_A_CORNERS, "frame A"),
        (second, FRAME_B_CORNERS, "frame B"),
    ] {
        for (centre, expected) in quadrant_centres(rect).into_iter().zip(corners) {
            let actual = rgb(&image, centre[0], centre[1]);
            assert!(
                close(actual, expected, 2),
                "{name} at pixel ({}, {}) should be {expected:?}, got {actual:?} — a \
                 swapped pair here is a flipped U or V, and a whole different palette \
                 is the wrong frame",
                centre[0],
                centre[1]
            );
        }
    }

    // And the sprites end where they are supposed to: two pixels outside each
    // edge is still the clear colour. Without this a full-screen quad of the
    // right colours would pass everything above.
    for rect in [first, second] {
        let low = world_to_pixel([rect[0], rect[1]]);
        let high = world_to_pixel([rect[0] + rect[2], rect[1] + rect[3]]);
        let middle_y = ((low[1] + high[1]) / 2.0) as u32;
        let middle_x = ((low[0] + high[0]) / 2.0) as u32;
        assert_background(&image, low[0] as u32 - 2, middle_y);
        assert_background(&image, high[0] as u32 + 2, middle_y);
        assert_background(&image, middle_x, high[1] as u32 - 2);
        assert_background(&image, middle_x, low[1] as u32 + 2);
    }
    // Including the corners of the frame, which no sprite is anywhere near.
    assert_background(&image, 2, 2);
    assert_background(&image, SPRITE_EXTENT.0 - 3, SPRITE_EXTENT.1 - 3);

    let verdict = sprite_golden("sprite", &image);
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(vec![verdict]);
}

/// **Alpha composites, it does not replace.** One sprite whose four texels are
/// opaque, half-transparent, opaque and fully transparent, over a background
/// that is not black.
///
/// Checked against the arithmetic rather than against a picture: the blend is
/// `src * a + dst * (1 - a)` in **linear** light, so the expected byte is the
/// sRGB encoding of that sum with the destination decoded first. A pass that
/// premultiplied in the shader as well as in the blender would land visibly
/// short of it, and a pass that ignored alpha entirely would land on the source.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn alpha_blending_composites_the_sprite_onto_what_is_already_there() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let sheet = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "alpha",
        2,
        2,
        crcbl_render::SampleMode::Pixel,
        &alpha_sheet(),
    );

    let rect = [-32.0f32, -32.0, 64.0, 64.0];
    let image = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[crcbl_render::Sprite {
            sheet,
            rect,
            uv: [0.0, 0.0, 1.0, 1.0],
            tint: [1.0; 4],
        }],
    );

    // Screen: x 96..160, y 64..128, so each texel is a 32-pixel square and the
    // quadrant centres are 16 in from each corner.
    let (x0, y0) = (96u32, 64u32);
    let opaque_red = rgb(&image, x0 + 16, y0 + 16);
    let blended = rgb(&image, x0 + 48, y0 + 16);
    let opaque_green = rgb(&image, x0 + 16, y0 + 48);
    let untouched = rgb(&image, x0 + 48, y0 + 48);
    let background = background_rgb();

    assert!(
        close(opaque_red, [255, 0, 0], 2),
        "an alpha of 1 must land on the source colour, got {opaque_red:?}"
    );
    assert!(
        close(opaque_green, [0, 255, 0], 2),
        "and so must the other opaque texel, got {opaque_green:?}"
    );
    assert!(
        close(untouched, background, 2),
        "an alpha of 0 must leave the destination exactly as it was: expected \
         {background:?}, got {untouched:?}"
    );

    // The half-transparent texel, computed rather than eyeballed. The
    // destination is the *stored* background byte decoded back to linear, not
    // `SPRITE_CLEAR` — the clear has already been through an 8-bit encode.
    let expected: [u8; 3] = std::array::from_fn(|channel| {
        let source = f32::from([255u8, 0, 0][channel]) / 255.0;
        let destination = srgb_decode(f32::from(background[channel]) / 255.0);
        srgb_byte(source.mul_add(HALF_ALPHA, destination * (1.0 - HALF_ALPHA)))
    });
    assert!(
        close(blended, expected, 3),
        "a half-transparent red over {background:?} blends to {expected:?} in linear \
         light; got {blended:?}"
    );
    // And it really is *between* the two, which is the property a premultiply
    // bug breaks in one direction and an ignored alpha breaks in the other.
    for channel in 0..3 {
        let (low, high) = (
            background[channel].min([255u8, 0, 0][channel]),
            background[channel].max([255u8, 0, 0][channel]),
        );
        assert!(
            blended[channel] > low && blended[channel] < high,
            "channel {channel} of the blend is {} — outside the open interval \
             ({low}, {high}) it has to be in",
            blended[channel]
        );
    }

    let colors = image.distinct_colors(8);
    assert!(
        colors >= 4,
        "three drawn texels and the clear is four colours; found {colors}"
    );

    let verdict = sprite_golden("sprite_alpha", &image);
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(vec![verdict]);
}

/// **`Pixel` and `Smooth` are visibly different pictures at a non-integer
/// scale — and different in the place and by the amount predicted.**
///
/// This is the only real evidence that sharp-bilinear happened. A 2×2 sheet
/// drawn 45 pixels wide is 22.5 pixels per texel, so:
///
/// * plain linear blends between the two texel *centres*, which are 22.5 pixels
///   apart — a gradient across the middle half of the sprite; while
/// * sharp-bilinear is flat inside each texel and crosses over in one fragment
///   at the boundary between them.
///
/// So a scanline through the top row of texels has two colours under `Pixel` and
/// a whole ramp of them under `Smooth`, and the assertion is on *that* rather
/// than on a count of differing pixels — which noise could reach.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn pixel_and_smooth_differ_where_and_by_how_much_sharp_bilinear_predicts() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    // The *same pixels* twice, so the only difference between the two frames is
    // the mode the sheet was registered with.
    let pixels = quad_sheet();
    let sharp = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad pixel",
        2,
        2,
        crcbl_render::SampleMode::Pixel,
        &pixels,
    );
    let smooth = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad smooth",
        2,
        2,
        crcbl_render::SampleMode::Smooth,
        &pixels,
    );

    // 45 units for 2 texels: 22.5 pixels per texel, and a fractional origin so
    // the quad's edges do not land on the pixel grid by accident either.
    let rect = [-22.3f32, -22.7, 45.0, 45.0];
    let sprite = |sheet| crcbl_render::Sprite {
        sheet,
        rect,
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: [1.0; 4],
    };
    let pixel_image = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(sharp)]);
    let smooth_image = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(smooth)]);

    // The sprite's screen box, from the world rect. Widened by two pixels
    // because `Pixel` snaps its corners to the grid and `Smooth` does not, so
    // the two outlines legitimately differ by up to one fragment.
    let low = world_to_pixel([rect[0], rect[1]]);
    let high = world_to_pixel([rect[0] + rect[2], rect[1] + rect[3]]);
    let box_left = low[0] as u32 - 2;
    let box_right = high[0] as u32 + 2;
    let box_top = high[1] as u32 - 2;
    let box_bottom = low[1] as u32 + 2;

    // Nothing outside that box may differ at all. A "the two are different"
    // test that counted differing pixels anywhere could be satisfied by driver
    // noise in the clear; this says the difference is the sprite.
    let mut outside = 0u32;
    let mut inside = 0u32;
    for y in 0..SPRITE_EXTENT.1 {
        for x in 0..SPRITE_EXTENT.0 {
            if pixel_image.pixel(x, y) == smooth_image.pixel(x, y) {
                continue;
            }
            if x >= box_left && x <= box_right && y >= box_top && y <= box_bottom {
                inside += 1;
            } else {
                outside += 1;
            }
        }
    }
    assert_eq!(
        outside, 0,
        "the two modes must differ only where the sprite is; {outside} pixels \
         elsewhere disagree"
    );
    assert!(
        inside > 500,
        "at 22.5 pixels per texel the two filters cannot agree over most of a \
         45x45 sprite; only {inside} pixels differ, which means the mode is not \
         reaching the shader"
    );

    // The scanline. Row 84 is inside the sprite's *top* row of texels under
    // either mode (that row spans screen y 73.7..96.2 unsnapped and 74..96.5
    // snapped), and the columns are two pixels in from each edge and two either
    // side of the texel boundary at x = 128.5.
    const ROW: u32 = 84;
    let left_run: Vec<[u8; 3]> = (108..127).map(|x| rgb(&pixel_image, x, ROW)).collect();
    let right_run: Vec<[u8; 3]> = (130..148).map(|x| rgb(&pixel_image, x, ROW)).collect();
    assert!(
        left_run.iter().all(|value| *value == left_run[0]),
        "Pixel must be flat inside the left texel; row {ROW} reads {left_run:?}"
    );
    assert!(
        right_run.iter().all(|value| *value == right_run[0]),
        "Pixel must be flat inside the right texel; row {ROW} reads {right_run:?}"
    );
    assert!(
        close(left_run[0], [255, 0, 0], 2) && close(right_run[0], [0, 255, 0], 2),
        "and flat at the texel's own colours, not at some average: {:?} then {:?}",
        left_run[0],
        right_run[0]
    );

    // The same span under `Smooth` is a ramp. The two texel centres are 22.5
    // pixels apart, so there are about that many distinct values between them.
    let smooth_run: Vec<[u8; 3]> = (108..148).map(|x| rgb(&smooth_image, x, ROW)).collect();
    let mut distinct: Vec<[u8; 3]> = Vec::new();
    for value in &smooth_run {
        if !distinct.contains(value) {
            distinct.push(*value);
        }
    }
    assert!(
        distinct.len() >= 10,
        "Smooth blends across the whole gap between texel centres, so row {ROW} \
         must be a ramp of many values; found {} — which is what it would be if \
         Pixel's bend were being applied to both",
        distinct.len()
    );
    // And the same span under Pixel is not: two flat colours and at most a
    // one-fragment crossover between them.
    let mut sharp_distinct: Vec<[u8; 3]> = Vec::new();
    for x in 108..148 {
        let value = rgb(&pixel_image, x, ROW);
        if !sharp_distinct.contains(&value) {
            sharp_distinct.push(value);
        }
    }
    assert!(
        sharp_distinct.len() <= 3,
        "Pixel's ramp is one fragment wide, so row {ROW} is two colours plus at \
         most one crossover; found {} ({sharp_distinct:?})",
        sharp_distinct.len()
    );

    let verdicts = vec![
        sprite_golden("sprite_pixel", &pixel_image),
        sprite_golden("sprite_smooth", &smooth_image),
    ];
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(verdicts);
}

/// **The assertion that pins the arithmetic: `Pixel` at a whole scale is exactly
/// flat inside every texel.**
///
/// Sharp-bilinear's ramp is one *fragment* wide, so at `n` device pixels per
/// texel the fragment centres nearest a boundary sit `0.5/n` texels either side
/// of it — exactly the ramp's half-width — and evaluate to a clean 0 and 1. The
/// picture is therefore bit-identical to nearest, with no intermediate pixel
/// anywhere. Plain linear at the same scale is a gradient across the middle half
/// of each texel pair, which is what the `Smooth` control at the bottom shows.
///
/// Read straight off sampled pixel values, not through a reference: a golden
/// blessed from a broken build would agree with itself forever.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn pixel_mode_is_exactly_flat_inside_each_texel_at_a_whole_scale() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let pixels = quad_sheet();
    let sharp = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad pixel",
        2,
        2,
        crcbl_render::SampleMode::Pixel,
        &pixels,
    );
    let smooth = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad smooth",
        2,
        2,
        crcbl_render::SampleMode::Smooth,
        &pixels,
    );

    // 64 pixels for 2 texels: 32 device pixels per texel, on the grid.
    let rect = [-32.0f32, -32.0, 64.0, 64.0];
    let sprite = |sheet| crcbl_render::Sprite {
        sheet,
        rect,
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: [1.0; 4],
    };
    let pixel_image = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(sharp)]);
    let smooth_image = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(smooth)]);

    // Screen x 96..160, y 64..128. Each quadrant is inset by one pixel, so the
    // assertion is about a texel's *interior* and cannot be defeated or
    // satisfied by whatever happens on the boundary itself.
    let quadrants = [
        (97u32, 65u32, [255u8, 0, 0]),
        (129, 65, [0, 255, 0]),
        (97, 97, [0, 0, 255]),
        (129, 97, [255, 255, 0]),
    ];
    let mut flat_under_smooth = 0;
    for (left, top, expected) in quadrants {
        let first = rgb(&pixel_image, left, top);
        assert!(
            close(first, expected, 2),
            "the texel at ({left}, {top}) should be {expected:?}, got {first:?}"
        );
        let mut uniform_smooth = true;
        for y in top..top + 31 {
            for x in left..left + 31 {
                let value = rgb(&pixel_image, x, y);
                assert_eq!(
                    value, first,
                    "Pixel must be exactly flat inside a texel: ({x}, {y}) reads \
                     {value:?} where ({left}, {top}) reads {first:?}. A gradient here \
                     is plain linear sampling, which is the thing sharp-bilinear \
                     replaces."
                );
                uniform_smooth &= rgb(&smooth_image, x, y) == rgb(&smooth_image, left, top);
            }
        }
        flat_under_smooth += u32::from(uniform_smooth);
    }

    // **The control.** If plain linear also came out flat, the assertion above
    // would be measuring nothing — a 2×2 sheet where both filters agree, or a
    // mode that never reached the shader.
    assert_eq!(
        flat_under_smooth, 0,
        "every one of the four texels must be a gradient under Smooth, or the \
         flatness asserted above is not evidence of anything"
    );

    // No intermediate value anywhere in the sprite: at a whole scale the ramp
    // has no fragment to land in, so the picture is exactly four colours.
    let mut inside: Vec<[u8; 3]> = Vec::new();
    for y in 64..128 {
        for x in 96..160 {
            let value = rgb(&pixel_image, x, y);
            if !inside.contains(&value) {
                inside.push(value);
            }
        }
    }
    assert_eq!(
        inside.len(),
        4,
        "a whole scale leaves the ramp with no fragment to land in, so the sprite \
         is exactly its four texels; found {inside:?}"
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// **The snap: a `Pixel` sprite's picture is piecewise-constant in position, and
/// a `Smooth` one is not.**
///
/// This is the half of `SampleMode::Pixel` the filtering tests cannot see.
/// Snapping the quad's corners to the device-pixel grid does not change how many
/// fragments an axis-aligned rectangle covers — `ceil(a - 0.5)` is `round(a)` —
/// so a coverage assertion would pass with the snap deleted. What it changes is
/// where the art's own texel grid lands: with the snap, the sheet's texels start
/// on whole device pixels and stay there while the sprite drifts by fractions of
/// one; without it they slide, and the boundary between two art pixels crosses
/// the fragment grid at a different place every frame. That is the crawl.
///
/// So: move the sprite by a fifth of a pixel, within one rounding bucket. Under
/// `Pixel` the frame must come back **byte for byte identical**. Under `Smooth`
/// — the control, without which this would pass on a renderer that drew nothing
/// — it must not, because a fifth of a pixel genuinely reached the GPU.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_sub_pixel_move_leaves_a_pixel_sprite_alone_and_moves_a_smooth_one() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let pixels = quad_sheet();
    let sharp = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad pixel",
        2,
        2,
        crcbl_render::SampleMode::Pixel,
        &pixels,
    );
    let smooth = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "quad smooth",
        2,
        2,
        crcbl_render::SampleMode::Smooth,
        &pixels,
    );

    // Screen x 105.6 and 105.8, which round to the same 106; the width is whole
    // so the far edge rounds together with it. A fractional height too, so the
    // test is not accidentally about one axis.
    let here = [-22.4f32, -22.7, 45.0, 45.0];
    let nudged = [-22.2f32, -22.7, 45.0, 45.0];
    let sprite = |sheet, rect| crcbl_render::Sprite {
        sheet,
        rect,
        uv: [0.0, 0.0, 1.0, 1.0],
        tint: [1.0; 4],
    };

    let sharp_here = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(sharp, here)]);
    let sharp_there = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[sprite(sharp, nudged)],
    );
    let smooth_here = render_sprites(&headless, &mut renderer, &mut pool, &[sprite(smooth, here)]);
    let smooth_there = render_sprites(
        &headless,
        &mut renderer,
        &mut pool,
        &[sprite(smooth, nudged)],
    );

    let differing = |a: &crcbl_golden::Image, b: &crcbl_golden::Image| {
        (0..SPRITE_EXTENT.1)
            .flat_map(|y| (0..SPRITE_EXTENT.0).map(move |x| (x, y)))
            .filter(|(x, y)| a.pixel(*x, *y) != b.pixel(*x, *y))
            .count()
    };

    let moved = differing(&smooth_here, &smooth_there);
    assert!(
        moved > 100,
        "the control: a fifth of a pixel must reach the GPU and move a Smooth \
         sprite's gradient. Only {moved} pixels changed, so this test is not \
         measuring anything"
    );

    let crawled = differing(&sharp_here, &sharp_there);
    assert_eq!(
        crawled, 0,
        "a Pixel sprite nudged within one rounding bucket must render \
         identically; {crawled} pixels moved, which is the texel grid sliding \
         against the fragment grid — the crawl the snap exists to remove"
    );

    // And it is a picture, not two blank frames agreeing.
    let colors = sharp_here.distinct_colors(16);
    assert!(
        colors >= 5,
        "four texels and a background is five colours; found {colors}"
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

// --- slice 6: nine-slice geometry, drawn --------------------------------------
//
// `crcbl_render::nine_slice` is exact arithmetic over rectangles and its unit
// tests assert the quads to the float. What they cannot show is the thing the
// feature exists for: that those quads, handed to the real pass on a real
// driver, come back as one picture — corners the size they were drawn at, edges
// stretched on one axis, and **no seam anywhere between the bands**.
//
// A seam is the failure mode a rect assertion is worst at. Two quads can share
// an edge exactly in world space and still leave a visible line if their UVs
// disagree by a texel, or if the sampler bleeds across a band boundary. Both are
// pixel facts, and this is where they are checked.

/// The nine-slice test sheet: 48×48 texels, a 3×3 grid of 16-texel blocks, and
/// **no two blocks alike**.
///
/// ```text
///   red     green   blue
///   yellow  white   cyan
///   magenta grey    black
/// ```
///
/// Every symmetry a mistake could hide behind is broken. A transposed axis swaps
/// green with yellow, a flipped V swaps red with magenta, a corner drawn with
/// the centre's UVs comes out white, and a band sampled from its neighbour comes
/// out the neighbour's colour rather than a plausible blend of the two.
const NINE_SLICE_COLORS: [[u8; 3]; 9] = [
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [255, 255, 0],
    [255, 255, 255],
    [0, 255, 255],
    [255, 0, 255],
    [128, 128, 128],
    [0, 0, 0],
];

/// The sheet's size in texels, and the inset on all four sides.
const NINE_SLICE_SIZE: u32 = 48;
const NINE_SLICE_INSET: u32 = 16;

fn nine_slice_sheet() -> Vec<u8> {
    let mut pixels = Vec::with_capacity((NINE_SLICE_SIZE * NINE_SLICE_SIZE * 4) as usize);
    for y in 0..NINE_SLICE_SIZE {
        for x in 0..NINE_SLICE_SIZE {
            let cell = (y / NINE_SLICE_INSET) * 3 + (x / NINE_SLICE_INSET);
            let [r, g, b] = NINE_SLICE_COLORS[cell as usize];
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    pixels
}

/// **A nine-slice stretched well beyond its source, drawn.** A 48×48 frame
/// expanded to 192×160 world units — four times as wide and over three times as
/// tall — with 16-texel insets.
///
/// The scales are whole on purpose, all of them: the corners are 1×, the top and
/// bottom edges 10× across, the left and right edges 8× down, and the centre
/// 10× by 8×. Under [`SampleMode::Pixel`] that makes every band an exactly flat
/// block, so the assertions below can be equality on colours rather than "close
/// to a gradient", and **the whole target must be exactly the sheet's nine
/// colours and nothing else**. A one-texel seam between two bands would show as
/// the clear colour or as a blend, and either fails that count.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_nine_slice_stretched_beyond_its_source_keeps_its_corners_and_shows_no_seam() {
    assert_the_camera_maps_a_world_unit_to_a_pixel();

    // The geometry first, from the same call the frame is drawn from: nine
    // quads, because every band of this slice is non-empty at this target.
    let source = crcbl_render::NineSliceSource {
        nine: crcbl_render::NineSlice::new(
            NINE_SLICE_INSET,
            NINE_SLICE_INSET,
            NINE_SLICE_INSET,
            NINE_SLICE_INSET,
        ),
        frame: crcbl_render::Rect::new(0, 0, NINE_SLICE_SIZE, NINE_SLICE_SIZE),
        sheet_width: NINE_SLICE_SIZE,
        sheet_height: NINE_SLICE_SIZE,
    };
    // World 192 × 160 at 1 unit per pixel: screen x 32..224, y 16..176.
    let target = [-96.0f32, -80.0, 192.0, 160.0];
    let quads = source.expand(target);
    assert_eq!(quads.len(), 9, "every band of this slice has extent");

    let headless = Headless::open_for_sprites();
    let mut pool = crcbl_render::TransientPool::new();
    let mut renderer = crcbl_render::SpriteRenderer::new(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
    )
    .expect("the sprite renderer builds");
    let sheet = register_sheet(
        &mut renderer,
        headless.device.as_ref(),
        "nine slice",
        NINE_SLICE_SIZE,
        NINE_SLICE_SIZE,
        crcbl_render::SampleMode::Pixel,
        &nine_slice_sheet(),
    );

    let sprites: Vec<crcbl_render::Sprite> = quads.sprites(sheet, [1.0; 4]).collect();
    assert_eq!(sprites.len(), 9);
    let image = render_sprites(&headless, &mut renderer, &mut pool, &sprites);

    // The band boundaries in screen pixels. `low`/`high` come from the world
    // rect through `world_to_pixel`, so a mistake in the expansion shows up as a
    // wrong colour rather than as a wrong coordinate.
    let low = world_to_pixel([target[0], target[1]]);
    let high = world_to_pixel([target[0] + target[2], target[1] + target[3]]);
    let (left, right) = (low[0] as u32, high[0] as u32); // 32, 224
    let (top, bottom) = (high[1] as u32, low[1] as u32); // 16, 176
    let inset = NINE_SLICE_INSET;
    assert_eq!((left, right, top, bottom), (32, 224, 16, 176));

    // --- every band is the colour it was cut from -------------------------
    let columns = [left + inset / 2, (left + right) / 2, right - inset / 2];
    let rows = [top + inset / 2, (top + bottom) / 2, bottom - inset / 2];
    for (row_index, y) in rows.into_iter().enumerate() {
        for (column_index, x) in columns.into_iter().enumerate() {
            let expected = NINE_SLICE_COLORS[row_index * 3 + column_index];
            let actual = rgb(&image, x, y);
            assert!(
                close(actual, expected, 2),
                "band ({row_index}, {column_index}) at pixel ({x}, {y}) should be \
                 {expected:?}, got {actual:?} — a swapped pair here is a flipped \
                 axis, and white is a corner drawn with the centre's UVs"
            );
        }
    }

    // --- the corners really are their natural size ------------------------
    //
    // The whole claim of a nine-slice: at 4× the source width the caps did not
    // grow. So the colour changes at exactly `inset` pixels in from each edge,
    // and not a pixel earlier or later.
    let middle_row = (top + bottom) / 2;
    let middle_column = (left + right) / 2;
    for (x, y, expected, what) in [
        (
            left + inset - 1,
            rows[0],
            NINE_SLICE_COLORS[0],
            "left cap ends",
        ),
        (
            left + inset,
            rows[0],
            NINE_SLICE_COLORS[1],
            "top edge starts",
        ),
        (
            right - inset - 1,
            rows[0],
            NINE_SLICE_COLORS[1],
            "top edge ends",
        ),
        (
            right - inset,
            rows[0],
            NINE_SLICE_COLORS[2],
            "right cap starts",
        ),
        (
            columns[0],
            top + inset - 1,
            NINE_SLICE_COLORS[0],
            "top cap ends",
        ),
        (
            columns[0],
            top + inset,
            NINE_SLICE_COLORS[3],
            "left edge starts",
        ),
        (
            columns[0],
            bottom - inset - 1,
            NINE_SLICE_COLORS[3],
            "left edge ends",
        ),
        (
            columns[0],
            bottom - inset,
            NINE_SLICE_COLORS[6],
            "bottom cap starts",
        ),
        // And the middle of the stretched bands, so "the caps are 16" is not
        // satisfied by a sprite that is only the caps.
        (
            middle_column,
            rows[0],
            NINE_SLICE_COLORS[1],
            "top edge middle",
        ),
        (
            columns[0],
            middle_row,
            NINE_SLICE_COLORS[3],
            "left edge middle",
        ),
    ] {
        let actual = rgb(&image, x, y);
        assert!(
            close(actual, expected, 2),
            "{what}: ({x}, {y}) should be {expected:?}, got {actual:?} — a corner \
             that stretched with the target moves this boundary"
        );
    }

    // --- no seam, anywhere ------------------------------------------------
    //
    // Every scale here is whole, so under `Pixel` the target is exactly the
    // sheet's nine flat colours. A one-texel gap between two bands shows the
    // clear colour through; a UV that overran shows a tenth colour; a filter
    // bleeding across a band boundary shows a blend. All three fail this.
    // Which of the nine each pixel matched, rather than a set of raw byte
    // triples: two neighbouring drivers can round the grey block to 127 and 128
    // and both are the same band, so counting distinct *values* would fail for a
    // reason that has nothing to do with a seam.
    let mut seen = [false; 9];
    for y in top..bottom {
        for x in left..right {
            let value = rgb(&image, x, y);
            let band = NINE_SLICE_COLORS
                .iter()
                .position(|expected| close(value, *expected, 2));
            let Some(band) = band else {
                panic!(
                    "({x}, {y}) is {value:?}, which is none of the sheet's nine \
                     colours — a seam between two bands, a UV that ran past one, \
                     or a filter blending across a band boundary"
                );
            };
            seen[band] = true;
        }
    }
    assert!(
        seen.iter().all(|hit| *hit),
        "the stretched slice must show all nine of its bands; missing {:?}",
        seen.iter()
            .enumerate()
            .filter(|(_, hit)| !**hit)
            .map(|(band, _)| NINE_SLICE_COLORS[band])
            .collect::<Vec<_>>()
    );

    // --- and it ends where it was told to ---------------------------------
    for (x, y) in [
        (left - 2, middle_row),
        (right + 1, middle_row),
        (middle_column, top - 2),
        (middle_column, bottom + 1),
    ] {
        assert_background(&image, x, y);
    }
    assert_background(&image, 2, 2);
    assert_background(&image, SPRITE_EXTENT.0 - 3, SPRITE_EXTENT.1 - 3);

    let verdict = sprite_golden("sprite_nine_slice", &image);
    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    report_goldens(vec![verdict]);
}
