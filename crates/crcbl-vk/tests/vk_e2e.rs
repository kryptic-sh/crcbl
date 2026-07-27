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
    encoder.bind_group(0, resources.bind_group, &[]);
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
}
