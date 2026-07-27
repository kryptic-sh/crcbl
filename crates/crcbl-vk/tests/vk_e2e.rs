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
    ImageAspect, ImageSubresourceLayers, ImageSubresourceRange, ImageViewDesc, ImageViewType,
    Instance, LoadOp, MemoryLocation, PresentInfo, PresentMode, QueryKind, QuerySetDesc,
    ReadbackDesc, ReadbackState, Rect2d, RenderPassDesc, ResourceState, SemaphoreDesc,
    SemaphoreKind, SemaphoreSignal, SemaphoreWait, StoreOp, SubmitInfo, SurfaceError,
    SwapchainDesc,
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
    assert!(
        matches!(
            error,
            crcbl_hal::HalError::ForeignObject { .. } | crcbl_hal::HalError::InvalidHandle { .. }
        ),
        "detected, never undefined: {error}"
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
    // Fails: push constants need a pipeline layout, which lands at P1.2.
    encoder.push_constants(crcbl_hal::ShaderStages::VERTEX, 0, &[0u8; 4]);
    // The pass is deliberately left open too, so `finish` has both a scope and
    // a label to close on the failure path.
    let error = encoder
        .finish()
        .expect_err("a failed recording must not produce a command buffer");
    assert!(
        error.to_string().contains("pipeline layout"),
        "the first failure is the one reported, not the unclosed pass: {error}"
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
