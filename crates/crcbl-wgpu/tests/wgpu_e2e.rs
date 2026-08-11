//! End-to-end suite against a **real GPU through wgpu**.
//!
//! ```text
//! crates/crcbl-wgpu/tests/run-wgpu-e2e.sh [extra nextest args…]
//! ```
//!
//! Feature-gated *and* `#[ignore]`d, exactly like `crcbl-vk`'s suite: a plain
//! `cargo nextest run --workspace --all-features` on a machine with no adapter
//! must stay green, and the harness script is the only thing that turns these on
//! — and it fails when the suite reports zero tests run, because
//! `docs/plan/12-testing.md` calls a silently-skipped e2e job a known trap.
//!
//! # What this covers, and what the script covers
//!
//! Everything here is **offscreen**, through
//! [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen): the image
//! ring, the acquire/present cycle over it, and the `map_async` readback that
//! turns a rendered frame into bytes. That is the half a windowless CI runner
//! can exercise, and it is the half the cross-backend image comparison needs.
//!
//! The **windowed** acquire/present cycle cannot be a test here — it needs a
//! window system — so the harness script drives `apps/sandbox` and
//! `apps/breakout` under Xvfb for a fixed frame budget instead. That is not
//! decoration: presenting is what returns a swapchain image to the presentation
//! engine, and a backend that dropped the acquired texture instead ran for
//! exactly `image_count` frames and then blocked until it timed out. A frame
//! budget larger than the ring is what catches it.

#![cfg(feature = "wgpu-e2e")]

use std::time::{Duration, Instant};

use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    Barriers, BufferCopy, BufferDesc, BufferHandle, BufferImageCopy, BufferUsage, ClearValue,
    ColorAttachment, ColorTargetState, CommandEncoder, CommandEncoderDesc, CompositeAlpha,
    ComputePassDesc, ComputePipelineDesc, Device, DeviceDesc, DrawIndirect, Extent3d, Features,
    Format, GraphicsPipelineDesc, HalError, ImageAspect, ImageDesc, ImageSubresourceLayers,
    ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewType, IndexFormat,
    Instance, LoadOp, MemoryLocation, MultisampleState, Offset3d, PipelineLayoutDesc, PresentInfo,
    PresentMode, PrimitiveState, PushConstantRange, QueueKind, ReadbackDesc, ReadbackState, Rect2d,
    RenderPassDesc, ResourceState, SampleType, SemaphoreDesc, SemaphoreKind, SemaphoreSignal,
    SemaphoreWait, ShaderEntry, ShaderModuleDesc, ShaderStages, StoreOp, SubmitInfo, SurfaceError,
    SwapchainDesc, Viewport,
};
use crcbl_wgpu::WgpuInstance;

/// The size every offscreen test renders at.
///
/// 64 pixels wide is deliberate: `64 * 4` is exactly wgpu's 256-byte row-pitch
/// requirement for a buffer↔image copy, so the copy is legal without padding
/// while the height still makes a row-stride mistake visible.
const EXTENT: (u32, u32) = (64, 48);

/// A distinctive clear colour. Every channel differs and none is 0 or 1, so a
/// channel swap or an sRGB round-trip bug shows up in the bytes.
const CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// [`CLEAR`] with its colour channels reversed, for the one test that has to
/// tell two frames apart in the same buffer.
///
/// Reversed rather than merely different so the check needs no knowledge of the
/// sRGB transfer function: whatever the encoding does to the values, `CLEAR`
/// lands with red < green < blue and this one with red > green > blue.
const CLEAR_REVERSED: [f32; 4] = [0.75, 0.5, 0.25, 1.0];

/// How long a readback may take before the test calls it a failure.
const READBACK_DEADLINE: Duration = Duration::from_secs(20);

/// How long a satisfiable timeline wait may take before the test calls it a
/// failure, in the nanoseconds [`Device::wait_semaphores`] takes.
///
/// Separate from [`READBACK_DEADLINE`] because it is passed *to* the seam
/// rather than enforced around it: this is the value the wait itself is told,
/// so a backend that never signals fails the test instead of hanging the run.
const SEMAPHORE_DEADLINE_NS: u64 = 20_000_000_000;

/// Opens an instance, or explains why the suite cannot run.
///
/// A missing adapter is a hard failure, not a skip: this suite is only ever
/// started by the harness, which has already established that a GPU is present.
fn instance() -> WgpuInstance {
    let instance = crcbl_wgpu::create_native().unwrap_or_else(|| {
        panic!(
            "the harness starts this suite only when an adapter is available, so finding none \
             here is a real failure"
        )
    });
    for adapter in instance.adapters() {
        eprintln!(
            "wgpu e2e: adapter {:?} ({:?}) driver {:?} geometry {:?} binding {:?} lighting {:?}",
            adapter.name,
            adapter.device_type,
            adapter.driver,
            adapter.caps.geometry_path(),
            adapter.caps.binding_model(),
            adapter.caps.lighting_path()
        );
    }
    instance
}

/// An offscreen surface, a device, and a swapchain-shaped image ring.
struct Headless {
    instance: WgpuInstance,
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
        let (instance, device, surface, queue, format) = Self::open_device();
        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("wgpu e2e ring"),
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

    /// Everything up to but not including the swapchain, so a test can try to
    /// create a bad one.
    fn open_device() -> (
        WgpuInstance,
        Box<dyn Device>,
        crcbl_hal::SurfaceHandle,
        crcbl_hal::QueueHandle,
        Format,
    ) {
        Self::open_device_with(Features::GPU_DRIVEN | Features::DEBUG_MARKERS)
    }

    /// [`Self::open_device`], with the caller's choice of optional features — a
    /// test that needs an optional feature requests it here and checks
    /// `Device::caps` afterwards to learn whether the adapter granted it.
    fn open_device_with(
        optional: Features,
    ) -> (
        WgpuInstance,
        Box<dyn Device>,
        crcbl_hal::SurfaceHandle,
        crcbl_hal::QueueHandle,
        Format,
    ) {
        let instance = instance();
        let adapter = instance.adapters().remove(0);

        let target = SurfaceTarget::Offscreen;
        // SAFETY: `Offscreen` names no platform object at all, so there is
        // nothing to outlive the surface. The teardown below destroys the
        // swapchain before the surface regardless, which is the general rule.
        let surface = unsafe { instance.create_surface(&target) }
            .expect("an offscreen surface needs no window system");

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
                label: Some("wgpu e2e"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: optional,
                compatible_surface: Some(surface),
            })
            .expect("a device opens");
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("a graphics queue always exists");
        (instance, device, surface, queue, format)
    }

    /// Tears down in the order `crcbl-hal`'s obligation 2 requires.
    fn finish(self) {
        self.device.wait_idle().expect("idle");
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        drop(self.device);
        drop(self.instance);
    }

    /// Clears the acquired frame through a real render pass, submits, and
    /// presents. Returns the command buffer so the caller can retire it.
    fn clear_frame(&self, acquired: &crcbl_hal::AcquiredFrame) -> crcbl_hal::CommandBufferHandle {
        let mut encoder = self.device.create_command_encoder(&CommandEncoderDesc {
            label: Some("wgpu e2e frame"),
            queue: self.queue,
        });
        encoder.pipeline_barrier(&Barriers {
            images: &[crcbl_hal::ImageBarrier::new(
                acquired.image,
                ImageSubresourceRange::all(self.format),
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
        let commands = encoder.finish().expect("recording succeeded");
        self.device
            .submit(self.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        self.device
            .present(
                self.queue,
                &PresentInfo {
                    swapchain: self.swapchain,
                    waits: acquired.present_semaphore.as_slice(),
                    present_id: None,
                },
            )
            .expect("present");
        commands
    }
}

/// Polls a readback to completion, or fails on the deadline.
fn drain(device: &dyn Device, readback: crcbl_hal::ReadbackHandle, out: &mut [u8]) {
    let deadline = Instant::now() + READBACK_DEADLINE;
    loop {
        match device
            .poll_readback(readback, out)
            .expect("the readback did not fail")
        {
            ReadbackState::Ready => return,
            ReadbackState::Pending => assert!(
                Instant::now() < deadline,
                "the readback never completed within {READBACK_DEADLINE:?}"
            ),
        }
        std::thread::yield_now();
    }
}

/// The bytes a host-readable buffer holds, through one readback request.
///
/// Requesting, draining and destroying live together because wgpu maps a buffer
/// **once**: a caller that left the previous request alive is refused rather
/// than handed the new bytes, which is what
/// [`a_readback_of_the_wrong_buffer_or_range_is_refused_instead_of_panicking`]
/// asserts. Every test below reads the same staging buffer more than once.
fn read_bytes(device: &dyn Device, buffer: BufferHandle, size: u64) -> Vec<u8> {
    let readback = device
        .request_readback(&ReadbackDesc {
            label: Some("wgpu e2e readback"),
            buffer,
            offset: 0,
            size,
            after: None,
        })
        .expect("a readback request");
    let mut bytes = vec![0u8; size as usize];
    drain(device, readback, &mut bytes);
    device.destroy_readback(readback);
    bytes
}

/// The slice's deliverable, end to end: a frame rendered offscreen through wgpu
/// reaches a host buffer with the colour it was given.
///
/// A `clear_buffer` would put bytes somewhere while exercising none of the
/// attachment, load-op or ring machinery, so this goes through
/// `begin_render_pass` with [`LoadOp::Clear`] — and then through
/// `copy_image_to_buffer` and `map_async`, which is the pair the cross-backend
/// image comparison needs and which this backend refused outright until now.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_render_pass_clear_reaches_host_memory_with_the_colour_it_was_given() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert!(
        acquired.acquire_semaphore.is_none() && acquired.present_semaphore.is_none(),
        "an offscreen ring has an implicit acquire, like WebGPU's"
    );
    assert_eq!(
        acquired.extent, EXTENT,
        "an offscreen ring has no window system to clamp against"
    );
    assert_eq!(acquired.index, 0, "the first acquire hands out image zero");

    let pixels = u64::from(EXTENT.0 * EXTENT.1 * 4);
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e readback"),
            size: pixels,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("wgpu e2e frame"),
        queue: headless.queue,
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
        image_offset: Offset3d::default(),
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
            label: Some("wgpu e2e pixels"),
            buffer: staging,
            offset: 0,
            size: pixels,
            after: None,
        })
        .expect("a readback request");

    let mut bytes = vec![0u8; pixels as usize];
    drain(device, readback, &mut bytes);

    // The ring's format is sRGB, so the clear's linear values are encoded on
    // write. Rather than reimplement the transfer function, assert the two
    // properties that catch the bugs this test is for: every pixel is identical
    // (so the whole attachment really was cleared), and the channels are ordered
    // and distinct (so no channel swap or all-zero "nothing happened" slipped
    // through).
    let first: [u8; 4] = bytes[0..4].try_into().expect("four bytes");
    assert!(
        bytes.chunks_exact(4).all(|pixel| pixel == first),
        "the whole render area must be cleared uniformly; got {first:?} then {:?}",
        &bytes[4..8]
    );
    assert_ne!(first, [0, 0, 0, 0], "an all-zero result means nothing ran");
    assert_eq!(first[3], 255, "alpha 1.0 must survive");
    let (r, g, b) = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => (first[2], first[1], first[0]),
        _ => (first[0], first[1], first[2]),
    };
    assert!(
        r < g && g < b,
        "the clear was {CLEAR:?}, so red < green < blue must survive into memory; got r={r} g={g} \
         b={b} in {:?}",
        headless.format
    );

    // Polling again after `Ready` is legal and yields the same bytes — the
    // seam says so, and this backend has to keep the mapping alive to honour
    // it.
    let mut again = vec![0u8; pixels as usize];
    assert_eq!(
        device
            .poll_readback(readback, &mut again)
            .expect("a second poll is legal"),
        ReadbackState::Ready
    );
    assert_eq!(again, bytes, "a second poll must yield the same bytes");

    device.destroy_readback(readback);
    device.destroy_buffer(staging);
    device.destroy_command_buffer(commands);
    headless.finish();
}

/// The hazard `crcbl-vk` had, asked of this backend: a ring image written again
/// while the previous trip's copy is still reading it.
///
/// `crates/crcbl-vk/tests/vk_e2e.rs` has
/// `reusing_an_offscreen_vulkan_ring_image_is_ordered_against_the_frame_that_had_it`,
/// which records the same two trips and asserts the **validation layer** saw no
/// write-after-read. That test exists because the Vulkan backend genuinely had
/// the bug: an offscreen ring hands its image back with no acquire semaphore,
/// so nothing ordered the next trip's `ResourceState::Undefined` transition —
/// which discards the image's contents — against the copy that read it.
///
/// **This backend cannot spell that transition.** `WgpuCommandEncoder::
/// pipeline_barrier` is a no-op, so the `Undefined` above never reaches wgpu;
/// wgpu-core tracks each texture's usage itself and emits the transition, at
/// `command/transfer.rs`'s `transition_textures(&src_barrier)` before a
/// texture→buffer copy and at `device/queue.rs`'s
/// `insert_barriers_from_device_tracker` in front of each submitted command
/// buffer, which is what carries a texture's state from one submission to the
/// next. So the ordering this test wants should be inserted for us.
///
/// What is asserted is the **observable**, not the absence of a layer message:
/// the staging buffer must still hold trip one's colour after trip two has
/// cleared the same image to a different one. Unlike the `crcbl-vk` test there
/// is no synchronisation-validation confirmation available here — wgpu-hal does
/// not request that validation feature — so the honest report is in
/// `docs/backlog.md`: this is a functional check plus the two call sites above,
/// not a layer verdict.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn reusing_an_offscreen_wgpu_ring_image_is_ordered_against_the_frame_that_had_it() {
    // One image, so the second acquire is genuinely the first image again
    // rather than the other half of a ring.
    let headless = Headless::open_with(EXTENT, 1);
    let device = headless.device.as_ref();

    let pixels = u64::from(EXTENT.0 * EXTENT.1 * 4);
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("ring reuse readback"),
            size: pixels,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("ring reuse"),
        queue: headless.queue,
    });
    let clear_into = |encoder: &mut Box<dyn crcbl_hal::CommandEncoder>,
                      frame: &crcbl_hal::AcquiredFrame,
                      colour: [f32; 4]| {
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("ring reuse clear"),
            color_attachments: &[ColorAttachment {
                view: frame.view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(colour),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(frame.extent.0, frame.extent.1),
        });
        encoder.end_render_pass();
    };

    // Trip one: draw into the image, then read it back out. The copy is the
    // read the next trip must not run over.
    let first = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    clear_into(&mut encoder, &first, CLEAR);
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
        image_offset: Offset3d::default(),
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

    // Trip two: the write. Same image, a colour trip one's copy must not see.
    clear_into(&mut encoder, &second, CLEAR_REVERSED);
    let commands = encoder.finish().expect("recording succeeded");
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

    let readback = device
        .request_readback(&ReadbackDesc {
            label: Some("ring reuse pixels"),
            buffer: staging,
            offset: 0,
            size: pixels,
            after: None,
        })
        .expect("a readback request");
    let mut bytes = vec![0u8; pixels as usize];
    drain(device, readback, &mut bytes);

    let first_pixel: [u8; 4] = bytes[0..4].try_into().expect("four bytes");
    assert!(
        bytes.chunks_exact(4).all(|pixel| pixel == first_pixel),
        "half a frame in the buffer is the hazard itself: the copy read the \
         image while the next trip was clearing it. Got {first_pixel:?} then {:?}",
        &bytes[4..8]
    );
    assert_ne!(
        first_pixel,
        [0, 0, 0, 0],
        "an all-zero result means the copy never ran, so this test observed nothing"
    );
    let (r, g, b) = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => {
            (first_pixel[2], first_pixel[1], first_pixel[0])
        }
        _ => (first_pixel[0], first_pixel[1], first_pixel[2]),
    };
    assert!(
        r < g && g < b,
        "the buffer must hold trip one's {CLEAR:?} (red < green < blue). \
         Red > blue is trip two's {CLEAR_REVERSED:?}, which would mean the \
         second clear overwrote the image before the copy read it. Got r={r} \
         g={g} b={b} in {:?}",
        headless.format
    );

    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);
    headless.finish();
}

/// The frame budget the windowed runs prove, proved here on the ring: more
/// frames than the ring has images, all of them presented.
///
/// A backend that never handed an image back would stop at `image_count`. The
/// index sequence is also the check that present *advances* the ring rather
/// than handing out the same image forever — a difference no clear colour can
/// see.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn the_ring_keeps_presenting_past_its_own_image_count() {
    let headless = Headless::open_with(EXTENT, 2);
    let device = headless.device.as_ref();

    let mut indices = Vec::new();
    let mut retired = Vec::new();
    for _ in 0..8 {
        let acquired = device
            .acquire_next_frame(headless.swapchain)
            .expect("the ring always has an image");
        indices.push(acquired.index);
        retired.push(headless.clear_frame(&acquired));
    }
    device.wait_idle().expect("idle");
    for commands in retired {
        device.destroy_command_buffer(commands);
    }

    assert_eq!(
        indices,
        vec![0, 1, 0, 1, 0, 1, 0, 1],
        "present advances the ring cursor, and the ring wraps"
    );
    headless.finish();
}

/// A second acquire with a frame still outstanding is refused by name.
///
/// On the windowed path this is what would silently *discard* the previous
/// swapchain image — wgpu's `SurfaceTexture::drop` discards rather than
/// presents — and a discarded image never comes back, so the failure would
/// surface four frames later as a timeout with no cause attached to it.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn acquiring_twice_without_a_present_is_refused_rather_than_dropping_a_frame() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the first acquire works");
    let error = device
        .acquire_next_frame(headless.swapchain)
        .expect_err("the second must not");
    assert!(
        matches!(error, SurfaceError::Hal(HalError::InvalidDescriptor(ref m)) if m.contains("present it first")),
        "{error}"
    );

    // And presenting the first one puts it right.
    let commands = headless.clear_frame(&acquired);
    device
        .acquire_next_frame(headless.swapchain)
        .expect("after a present the ring hands out the next image");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    headless.finish();
}

/// Present without an acquire is a caller bug the backend names, not a no-op.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn presenting_a_wgpu_swapchain_without_an_acquire_is_refused() {
    let headless = Headless::open();
    let error = headless
        .device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: &[],
                present_id: None,
            },
        )
        .expect_err("nothing was acquired");
    assert!(
        matches!(error, SurfaceError::Hal(HalError::InvalidDescriptor(ref m)) if m.contains("without a matching acquire")),
        "{error}"
    );
    headless.finish();
}

/// A zero extent is the caller's problem — obligation 4 — and the message says
/// so rather than producing a ring of images nothing can render into.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_zero_extent_offscreen_ring_is_refused_with_the_rule_named() {
    let (instance, device, surface, _queue, format) = Headless::open_device();
    for extent in [(0, 48), (64, 0), (0, 0)] {
        let error = device
            .create_swapchain(&SwapchainDesc {
                label: Some("zero"),
                surface,
                format,
                extent,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            })
            .expect_err("a zero-extent ring has no images");
        let SurfaceError::Hal(HalError::InvalidDescriptor(message)) = error else {
            panic!("{extent:?} gave the wrong variant");
        };
        assert!(message.contains("do not create one yet"), "{message}");
    }
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}

/// The readback's descriptor checks, which exist because wgpu **panics** on a
/// misaligned or unmappable range rather than returning — and a panic through a
/// trait object is not a diagnosis.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_readback_of_the_wrong_buffer_or_range_is_refused_instead_of_panicking() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    let device_local = device
        .create_buffer(&BufferDesc {
            label: Some("device local"),
            size: 256,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a buffer");
    let host = device
        .create_buffer(&BufferDesc {
            label: Some("host readback"),
            size: 256,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a buffer");

    let request = |buffer, offset, size| {
        device.request_readback(&ReadbackDesc {
            label: Some("bad"),
            buffer,
            offset,
            size,
            after: None,
        })
    };

    for (what, result) in [
        (
            "a device-local buffer has no MAP_READ",
            request(device_local, 0, 256),
        ),
        ("past the end of the buffer", request(host, 0, 512)),
        ("a misaligned offset", request(host, 4, 128)),
        ("a zero size", request(host, 0, 0)),
    ] {
        assert!(
            matches!(result, Err(HalError::InvalidDescriptor(_))),
            "{what}: {result:?}"
        );
    }

    // And two live readbacks of one buffer, which wgpu panics on rather than
    // refusing — a buffer can be mapped once.
    let first = request(host, 0, 256).expect("the first readback is fine");
    let second = request(host, 0, 256);
    assert!(
        matches!(second, Err(HalError::InvalidDescriptor(ref m)) if m.contains("maps a buffer once")),
        "{second:?}"
    );
    // Destroying a readback that never resolved must release the buffer, not
    // leave it mapped for the life of the process — wgpu's `unmap` aborts a
    // pending map, and this is the check that it is actually called. Polling
    // the replacement to `Ready` is what makes the check real: if the abandoned
    // map were still holding the buffer, wgpu would refuse the second one
    // immediately and this would be an error rather than bytes.
    device.destroy_readback(first);
    let third = request(host, 0, 256).expect("a second request is a fresh readback");
    let mut bytes = vec![0u8; 256];
    drain(device, third, &mut bytes);
    device.destroy_readback(third);

    device.destroy_buffer(device_local);
    device.destroy_buffer(host);
    headless.finish();
}

/// A reconfigure reissues the ring, and the old handles stop resolving — the
/// seam's rule for holding an image across a resize, which an offscreen ring has
/// to obey too because `crcbl screenshot` and the headless shell both resize.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn reconfiguring_an_offscreen_ring_reissues_its_images() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    let before = device
        .acquire_next_frame(headless.swapchain)
        .expect("an image");
    let commands = headless.clear_frame(&before);
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    let bigger = (128, 96);
    device
        .reconfigure_swapchain(
            headless.swapchain,
            &SwapchainDesc {
                label: Some("wgpu e2e ring"),
                surface: headless.surface,
                format: headless.format,
                extent: bigger,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: CompositeAlpha::Opaque,
            },
        )
        .expect("a ring reconfigures");

    let after = device
        .acquire_next_frame(headless.swapchain)
        .expect("an image");
    assert_eq!(after.extent, bigger, "the ring reports its new size");
    assert_ne!(
        after.image, before.image,
        "a reconfigure reissues every handle, so a caller holding one gets InvalidHandle"
    );
    let commands = headless.clear_frame(&after);
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    headless.finish();
}

/// **A shader that does not compile is an error, not a handle.**
///
/// WebGPU reports a creation failure on the device's error channel rather than
/// to the call, so this backend used to return a module handle for source naga
/// had rejected, build a pipeline on it, and submit command buffers the
/// implementation discarded — the shape P5.13 found in a browser, where it
/// presented as a black canvas over a game that reported itself as playing.
///
/// `wgpu-core` raises the error during the call, so the guard in
/// `WgpuDevice::checked` catches it here and the seam's caller sees the `Err` it
/// was already checking for. The asynchronous half of the same fix —
/// `Device::take_error` — is what covers the browser, and the headless-browser
/// gate is where that one is exercised.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_shader_module_that_will_not_compile_is_refused_instead_of_handed_back() {
    let (instance, device, surface, _queue, _format) = Headless::open_device();

    // Valid WGSL first, so the failure below is attributable to the source and
    // not to a backend that refuses every module.
    let good = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("valid"),
            wgsl: Some(
                "@vertex fn vs() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }",
            ),
            ..ShaderModuleDesc::default()
        })
        .expect("well-formed WGSL compiles");
    assert!(
        device.take_error().is_none(),
        "a module that compiled must leave nothing on the error channel"
    );
    device.destroy_shader_module(good);

    let error = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("not WGSL at all"),
            wgsl: Some("this is not a shader"),
            ..ShaderModuleDesc::default()
        })
        .expect_err("source naga cannot parse is not a shader module");
    let HalError::Backend(message) = &error else {
        panic!("wrong variant: {error:?}");
    };
    assert!(
        message.contains("create_shader_module"),
        "the message must name the call that failed: {message}"
    );

    // Taken by the guard, so the frame loop's drain does not report it twice.
    assert!(
        device.take_error().is_none(),
        "an error reported as an Err must not also be reported out of band"
    );

    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}

/// `PushConstantRange { offset: u32::MAX, size: 1 }` used to panic in debug
/// ("attempt to add with overflow") and wrap to `0` in release — and `0` then
/// *passed* the limit check it was supposed to fail, silently creating a
/// zero-size immediate block.
///
/// The overflow is only reachable when the device actually enabled wgpu's
/// `IMMEDIATES` feature: the unsupported check fires first otherwise. This
/// backend never enables it — `instance.rs` deliberately never reports
/// [`Features::PUSH_CONSTANTS`] (the seam's name for `IMMEDIATES`), so even an
/// optional request maps to nothing and the overflow path is unreachable
/// through the seam. Requesting it as optional and checking `Device::caps` is
/// the honest probe: a device that granted it exercises the real path, and one
/// that did not skips with the reason printed.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_push_constant_range_that_overflows_is_refused_by_wgpu_not_wrapped() {
    let (instance, device, surface, _queue, _format) =
        Headless::open_device_with(Features::PUSH_CONSTANTS);
    if !device.caps().features.contains(Features::PUSH_CONSTANTS) {
        println!(
            "wgpu e2e: IMMEDIATES is not enabled on this adapter, so the push-constant \
             overflow path is unreachable here; skipping"
        );
        instance.destroy_surface(surface);
        drop(device);
        drop(instance);
        return;
    }

    let overflow = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("overflow"),
            bind_group_layouts: &[],
            push_constants: Some(PushConstantRange {
                stages: ShaderStages::ALL,
                offset: u32::MAX,
                size: 1,
            }),
        })
        .expect_err("the range ends past every possible budget");
    assert!(
        matches!(overflow, HalError::InvalidDescriptor(_)),
        "{overflow:?}"
    );

    // And a range inside the budget still creates a layout — the check must
    // reject the overflow, not push constants wholesale.
    let small = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("in budget"),
            bind_group_layouts: &[],
            push_constants: Some(PushConstantRange {
                stages: ShaderStages::ALL,
                offset: 0,
                size: 4,
            }),
        })
        .expect("a range inside the budget still creates");
    device.destroy_pipeline_layout(small);

    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}

/// A 4x-MSAA pass with [`ColorAttachment::resolve`] set must land its resolved
/// image in the resolve view.
///
/// The seam documents `resolve` as the MSAA resolve destination, the null
/// backend records it, and `crcbl-vk` renders it. This backend used to build
/// every `wgpu::RenderPassColorAttachment` with `resolve_target: None`, so the
/// pass rendered into the MSAA target and nothing was ever resolved — silent
/// wrong output, no error, no log. The resolve view is read back here, so a
/// backend that drops the field yields the texture's untouched contents
/// instead of the clear colour.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn an_msaa_pass_resolves_into_its_resolve_target() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    // RGBA so the channel order below needs no BGRA swap — this test owns its
    // format, unlike the ring tests that read back the adapter's preferred one.
    let format = Format::Rgba8UnormSrgb;
    let pixels = u64::from(EXTENT.0 * EXTENT.1 * 4);

    let msaa = device
        .create_image(&ImageDesc {
            label: Some("wgpu e2e msaa target"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(EXTENT.0, EXTENT.1),
            format,
            mip_levels: 1,
            samples: 4,
            usage: ImageUsage::COLOR_ATTACHMENT,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a 4x target");
    let resolve = device
        .create_image(&ImageDesc {
            label: Some("wgpu e2e resolve target"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(EXTENT.0, EXTENT.1),
            format,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a 1x resolve target");
    let msaa_view = device
        .create_image_view(&ImageViewDesc {
            label: Some("wgpu e2e msaa view"),
            image: msaa,
            view_type: ImageViewType::D2,
            format,
            range: ImageSubresourceRange::all(format),
        })
        .expect("a view of the msaa target");
    let resolve_view = device
        .create_image_view(&ImageViewDesc {
            label: Some("wgpu e2e resolve view"),
            image: resolve,
            view_type: ImageViewType::D2,
            format,
            range: ImageSubresourceRange::all(format),
        })
        .expect("a view of the resolve target");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e resolve readback"),
            size: pixels,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("wgpu e2e resolve pass"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            msaa,
            ImageSubresourceRange::all(format),
            ResourceState::Undefined,
            ResourceState::ColorAttachment,
        )],
        ..Barriers::default()
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("resolve clear"),
        color_attachments: &[ColorAttachment {
            view: msaa_view,
            resolve: Some(resolve_view),
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(EXTENT.0, EXTENT.1),
    });
    encoder.end_render_pass();
    encoder.pipeline_barrier(&Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            resolve,
            ImageSubresourceRange::all(format),
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
        image: resolve,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d::default(),
        image_extent: Extent3d::d2(EXTENT.0, EXTENT.1),
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let readback = device
        .request_readback(&ReadbackDesc {
            label: Some("wgpu e2e resolve pixels"),
            buffer: staging,
            offset: 0,
            size: pixels,
            after: None,
        })
        .expect("a readback request");
    let mut bytes = vec![0u8; pixels as usize];
    drain(device, readback, &mut bytes);

    // Every sample of the 4x target held the clear colour, so the resolved
    // image must be exactly it: uniform, non-zero, alpha 1.0, and — through
    // the sRGB encoding, exactly like the ring tests — red < green < blue.
    let first: [u8; 4] = bytes[0..4].try_into().expect("four bytes");
    assert!(
        bytes.chunks_exact(4).all(|pixel| pixel == first),
        "the resolved image must be uniformly the clear colour; got {first:?} then {:?}",
        &bytes[4..8]
    );
    assert_ne!(
        first,
        [0, 0, 0, 0],
        "an all-zero result means the resolve never wrote the target"
    );
    assert_eq!(first[3], 255, "alpha 1.0 must survive the resolve");
    assert!(
        first[0] < first[1] && first[1] < first[2],
        "the clear was {CLEAR:?}, so red < green < blue must survive the resolve; got {first:?}"
    );

    device.destroy_readback(readback);
    device.destroy_buffer(staging);
    device.destroy_command_buffer(commands);
    device.destroy_image_view(resolve_view);
    device.destroy_image_view(msaa_view);
    device.destroy_image(resolve);
    device.destroy_image(msaa);
    headless.finish();
}

/// A stale [`ColorAttachment::resolve`] handle is the same class of bug as a
/// stale attachment handle, so the pass must refuse it at `finish` — not
/// silently drop the resolve and keep rendering, which is how a resolve that
/// never happens goes unnoticed.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_stale_resolve_handle_is_refused_rather_than_dropped() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    let format = Format::Rgba8UnormSrgb;
    let msaa = device
        .create_image(&ImageDesc {
            label: Some("wgpu e2e msaa target"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(EXTENT.0, EXTENT.1),
            format,
            mip_levels: 1,
            samples: 4,
            usage: ImageUsage::COLOR_ATTACHMENT,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a 4x target");
    let msaa_view = device
        .create_image_view(&ImageViewDesc {
            label: Some("wgpu e2e msaa view"),
            image: msaa,
            view_type: ImageViewType::D2,
            format,
            range: ImageSubresourceRange::all(format),
        })
        .expect("a view of the msaa target");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("wgpu e2e stale resolve"),
        queue: headless.queue,
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("stale resolve"),
        color_attachments: &[ColorAttachment {
            view: msaa_view,
            // A generation no pool ever issued: valid handle value, matches no
            // live slot.
            resolve: Some(
                crcbl_hal::ImageViewHandle::from_bits(0xFFFF_FFFF_0000_0000)
                    .expect("the generation half is non-zero"),
            ),
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(EXTENT.0, EXTENT.1),
    });
    encoder.end_render_pass();
    let error = encoder
        .finish()
        .expect_err("a resolve handle no pool issued is not a pass");
    assert!(
        matches!(
            error,
            HalError::InvalidHandle { kind, .. } if kind == "image view"
        ),
        "{error}"
    );

    device.destroy_image_view(msaa_view);
    device.destroy_image(msaa);
    headless.finish();
}

/// A padded [`DrawIndirect`] stride is wgpu's silent-garbage case: wgpu reads
/// tightly packed 16-byte argument structs, while `crcbl-vk` honours the
/// caller's stride — so a padded stride here must fail at `finish` like every
/// other recording error, never record a draw that reads padding bytes as
/// argument fields.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_padded_indirect_draw_stride_is_refused_at_finish() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    // A real indirect draw needs a pass to record into and an argument buffer;
    // no pipeline is needed because the stride check fires before wgpu ever
    // sees the draw.
    let format = Format::Rgba8UnormSrgb;
    let target = device
        .create_image(&ImageDesc {
            label: Some("wgpu e2e indirect target"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(EXTENT.0, EXTENT.1),
            format,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a render target");
    let view = device
        .create_image_view(&ImageViewDesc {
            label: Some("wgpu e2e indirect view"),
            image: target,
            view_type: ImageViewType::D2,
            format,
            range: ImageSubresourceRange::all(format),
        })
        .expect("a view of the target");
    let args = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e indirect args"),
            size: 64,
            usage: BufferUsage::INDIRECT,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("an argument buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("wgpu e2e padded stride"),
        queue: headless.queue,
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("padded stride"),
        color_attachments: &[ColorAttachment {
            view,
            resolve: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear: ClearValue::color(CLEAR),
        }],
        depth_stencil_attachment: None,
        render_area: Rect2d::from_size(EXTENT.0, EXTENT.1),
    });
    // 32 bytes between arguments is twice the tightly packed 16 wgpu reads.
    encoder.draw_indirect(&DrawIndirect {
        args,
        offset: 0,
        draw_count: 1,
        stride: 32,
    });
    encoder.end_render_pass();
    let error = encoder
        .finish()
        .expect_err("a padded stride would draw from padding bytes, so it must fail at finish");
    assert!(
        matches!(error, HalError::Unsupported { .. }),
        "the padded stride is a wgpu limitation, so the refusal must be Unsupported; got {error}"
    );

    device.destroy_buffer(args);
    device.destroy_image_view(view);
    device.destroy_image(target);
    headless.finish();
}

/// A `compatible_surface` naming a surface this instance has already destroyed
/// is not a surface at all, so `create_device` must refuse it up front — the
/// null backend's `InvalidHandle`, not a device that quietly presents nowhere.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_destroyed_compatible_surface_is_refused_when_creating_a_device() {
    let instance = instance();
    let adapter = instance.adapters().remove(0);

    let target = SurfaceTarget::Offscreen;
    // SAFETY: `Offscreen` names no platform object at all, so there is nothing
    // to outlive the surface — exactly as `Headless::open_device` documents.
    let surface = unsafe { instance.create_surface(&target) }.expect("an offscreen surface");
    instance.destroy_surface(surface);

    let error = instance
        .create_device(&DeviceDesc {
            label: Some("wgpu e2e stale surface"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: Some(surface),
        })
        .expect_err("a surface this instance destroyed is not a compatible surface");
    assert!(
        matches!(error, HalError::InvalidHandle { kind, .. } if kind == "surface"),
        "{error}"
    );
}

/// `write_buffer` is only valid for [`MemoryLocation::HostUpload`] memory; a
/// `HostReadback` buffer is mappable, so the old mappability guard let it
/// through and uploaded into a buffer the seam reserves for the readback ring.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn write_buffer_refuses_a_host_readback_buffer() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    let readback = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e readback-only"),
            size: 64,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a HostReadback buffer");

    let error = device
        .write_buffer(readback, 0, &[0u8; 4])
        .expect_err("write_buffer needs HostUpload memory, and this buffer is HostReadback");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error}");

    device.destroy_buffer(readback);
    headless.finish();
}

// ---------------------------------------------------------------------------
// The compute path
// ---------------------------------------------------------------------------
//
// `begin_compute_pass`, `bind_compute_pipeline`, `dispatch`,
// `dispatch_indirect` and `Device::create_compute_pipeline`, reaching a driver.
// `crates/crcbl-vk/tests/vk_e2e/compute.rs` is the original and `crcbl-mtl` and
// `crcbl-dx12` carry twins; this backend implemented all five and executed none
// of them, so a `dispatch` that recorded nothing at all submitted cleanly and
// left a buffer full of `PROBE_SENTINEL`. Only reading the destination back
// tells that apart from a dispatch that did the right thing.

/// Workgroups the probe's buffers are sized for.
///
/// Eight, so `dispatch_indirect` can ask for two and leave six workgroups'
/// worth of untouched sentinel behind it — which is what tells "the argument
/// buffer was read" apart from "everything was dispatched anyway".
const PROBE_GROUPS: u32 = 8;

/// Elements the probe transforms.
const PROBE_ELEMENTS: u32 = PROBE_GROUPS * crcbl_shaders::compute_probe::WORKGROUP_SIZE;

/// What the destination buffer holds before every dispatch.
///
/// Deliberately not zero, and deliberately not a square: a destination that was
/// never written must not be confusable with one the shader wrote, and zero is
/// both its own square and what fresh device memory tends to be. The same value
/// `crcbl-vk`'s twin uses.
const PROBE_SENTINEL: u32 = 0xDEAD_BEEF;

/// Bytes one probe buffer occupies.
const fn probe_bytes() -> u64 {
    PROBE_ELEMENTS as u64 * 4
}

/// The probe's input, one distinct value per index.
///
/// Distinct matters: with a constant input, a shader that indexed `source`
/// wrongly would still produce the right number in every slot. `index + 1`
/// avoids zero, whose square is itself.
fn probe_source() -> Vec<u32> {
    (0..PROBE_ELEMENTS).map(|index| index + 1).collect()
}

/// What the destination must hold for `elements` dispatched elements, and the
/// sentinel beyond them.
///
/// Written out here rather than derived from the shader: squaring is a closed
/// form the test states for itself, which is the whole reason the probe squares.
fn probe_expected(elements: u32) -> Vec<u32> {
    probe_source()
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            if (index as u32) < elements {
                value * value
            } else {
                PROBE_SENTINEL
            }
        })
        .collect()
}

/// Everything one compute dispatch needs, built through the seam.
struct ComputeProbe {
    params: BufferHandle,
    source: BufferHandle,
    destination: BufferHandle,
    /// A host-visible buffer holding [`PROBE_SENTINEL`] in every slot, copied
    /// over the destination before each dispatch.
    ///
    /// The twins reset the destination with `fill_buffer`, which this backend
    /// **refuses** for a non-zero value: wgpu's only fill is `clear_buffer`, a
    /// zero fill. Resetting to zero instead would have made "the shader wrote
    /// nothing" and "the shader wrote zero" the same reading, so the sentinel
    /// arrives by copy rather than by fill.
    sentinel: BufferHandle,
    /// Host-readable copy target, so the result can be asserted rather than
    /// assumed.
    staging: BufferHandle,
    bind_group_layout: crcbl_hal::BindGroupLayoutHandle,
    bind_group: crcbl_hal::BindGroupHandle,
    pipeline_layout: crcbl_hal::PipelineLayoutHandle,
    pipeline: crcbl_hal::ComputePipelineHandle,
}

impl ComputeProbe {
    /// Builds the pipeline and stages the input in.
    fn new(headless: &Headless) -> Self {
        let device = headless.device.as_ref();
        let params = crcbl_shaders::compute_probe::Params {
            count: PROBE_ELEMENTS,
        }
        .to_bytes();
        let source_bytes: Vec<u8> = probe_source()
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();

        let upload = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe upload"),
                size: (params.len() + source_bytes.len()) as u64,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        device.write_buffer(upload, 0, &params).expect("write");
        device
            .write_buffer(upload, params.len() as u64, &source_bytes)
            .expect("write");

        let sentinel = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe sentinel"),
                size: probe_bytes(),
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a sentinel buffer");
        let sentinel_bytes: Vec<u8> = std::iter::repeat_n(PROBE_SENTINEL, PROBE_ELEMENTS as usize)
            .flat_map(u32::to_le_bytes)
            .collect();
        device
            .write_buffer(sentinel, 0, &sentinel_bytes)
            .expect("write");

        let params_buffer = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe params"),
                size: crcbl_shaders::compute_probe::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a uniform buffer");
        let source = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe source"),
                size: probe_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a source buffer");
        let destination = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe destination"),
                size: probe_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a destination buffer");
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("compute probe readback"),
                size: probe_bytes(),
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("compute probe upload"),
            queue: headless.queue,
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: upload,
            src_offset: 0,
            dst: params_buffer,
            dst_offset: 0,
            size: params.len() as u64,
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: upload,
            src_offset: params.len() as u64,
            dst: source,
            dst_offset: 0,
            size: probe_bytes(),
        });
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                crcbl_hal::BufferBarrier {
                    buffer: params_buffer,
                    from: ResourceState::TransferDst,
                    to: ResourceState::ShaderRead,
                    queue_transfer: None,
                },
                crcbl_hal::BufferBarrier {
                    buffer: source,
                    from: ResourceState::TransferDst,
                    to: ResourceState::ShaderRead,
                    queue_transfer: None,
                },
            ],
            ..Barriers::default()
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
        device.destroy_buffer(upload);

        let layout_entries = [
            crcbl_hal::BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                kind: crcbl_hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::COMPUTE,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::COMPUTE,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: false,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
        ];
        let bind_group_layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("compute probe"),
                entries: &layout_entries,
            })
            .expect("the probe's layout");

        let group_entries = [
            crcbl_hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(params_buffer),
            },
            crcbl_hal::BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(source),
            },
            crcbl_hal::BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(destination),
            },
        ];
        let bind_group = device
            .create_bind_group(&crcbl_hal::BindGroupDesc {
                label: Some("compute probe"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDesc {
                label: Some("compute probe"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        let module = device
            .create_shader_module(&ShaderModuleDesc {
                label: Some("compute_probe.slang"),
                wgsl: crcbl_shaders::COMPUTE_PROBE.wgsl(),
                ..ShaderModuleDesc::default()
            })
            .expect("the committed WGSL is accepted");
        // The manifest's name rather than a literal: it is read out of the
        // artifact by the compile script, so a Slang release that renamed it
        // would fail here rather than in a driver.
        let entry_point = crcbl_shaders::COMPUTE_PROBE
            .entry_point(crcbl_shaders::Stage::Compute)
            .expect("the probe has exactly one compute entry point");
        let pipeline = device
            .create_compute_pipeline(&ComputePipelineDesc {
                label: Some("compute probe"),
                layout: pipeline_layout,
                compute: ShaderEntry {
                    module,
                    entry_point,
                },
                // The shader's own number, not a literal: `crcbl-shaders`
                // checks this constant against the `[numthreads(…)]` in
                // `compute_probe.slang`.
                workgroup_size: [crcbl_shaders::compute_probe::WORKGROUP_SIZE, 1, 1],
            })
            .expect("a compute pipeline");
        device.destroy_shader_module(module);

        Self {
            params: params_buffer,
            source,
            destination,
            sentinel,
            staging,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            pipeline,
        }
    }

    /// Resets the destination to the sentinel, runs `record` inside a compute
    /// pass, and reads the destination back.
    ///
    /// `record` is the *only* thing that varies between the dispatch and the
    /// indirect-dispatch tests, so both go through the same barriers and the
    /// same readback and a difference in the result is a difference in the
    /// dispatch.
    fn run(&self, headless: &Headless, record: impl FnOnce(&mut dyn CommandEncoder)) -> Vec<u32> {
        let device = headless.device.as_ref();
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("compute probe dispatch"),
            queue: headless.queue,
        });
        let buffer_barrier = |buffer, from, to| crcbl_hal::BufferBarrier {
            buffer,
            from,
            to,
            queue_transfer: None,
        };
        // `TransferSrc` as the source state is vacuous on the first run and is
        // the real prior use on every later one.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[buffer_barrier(
                self.destination,
                ResourceState::TransferSrc,
                ResourceState::TransferDst,
            )],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: self.sentinel,
            src_offset: 0,
            dst: self.destination,
            dst_offset: 0,
            size: probe_bytes(),
        });
        // `ShaderReadWrite`, not `ShaderWrite`: a barrier names the access the
        // *descriptor* permits rather than the one the source performs, and the
        // destination is bound as a read-write storage buffer.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[buffer_barrier(
                self.destination,
                ResourceState::TransferDst,
                ResourceState::ShaderReadWrite,
            )],
            ..Barriers::default()
        });

        encoder.begin_compute_pass(&ComputePassDesc {
            label: Some("compute probe"),
        });
        encoder.bind_compute_pipeline(self.pipeline);
        // Inside the pass, because the open scope is the only signal the seam
        // gives the backend about which bind point a group is for.
        encoder.bind_group(0, self.bind_group, &[], self.pipeline_layout);
        record(encoder.as_mut());
        encoder.end_compute_pass();

        encoder.pipeline_barrier(&Barriers {
            buffers: &[buffer_barrier(
                self.destination,
                ResourceState::ShaderReadWrite,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: self.destination,
            src_offset: 0,
            dst: self.staging,
            dst_offset: 0,
            size: probe_bytes(),
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);

        read_bytes(device, self.staging, probe_bytes())
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect()
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.staging);
        device.destroy_buffer(self.sentinel);
        device.destroy_buffer(self.destination);
        device.destroy_buffer(self.source);
        device.destroy_buffer(self.params);
    }
}

/// Compares a probe result against what the CPU says it should be, and says
/// which element disagreed first.
///
/// The element count is asserted before the values: a readback that came back
/// short would otherwise satisfy a `zip` over nothing at all.
fn assert_probe(actual: &[u32], expected: &[u32], what: &str) {
    assert_eq!(
        actual.len(),
        PROBE_ELEMENTS as usize,
        "{what}: the readback is not the whole destination buffer"
    );
    assert_eq!(expected.len(), actual.len(), "{what}: expectation length");
    if let Some((index, (got, want))) = actual
        .iter()
        .zip(expected)
        .enumerate()
        .find(|(_, (got, want))| got != want)
    {
        panic!(
            "{what}: element {index} is {got} ({got:#x}), expected {want} ({want:#x}). \
             {} of {} elements were expected to be written.",
            expected.iter().filter(|v| **v != PROBE_SENTINEL).count(),
            expected.len()
        );
    }
}

/// A dispatch that really ran, and really wrote the values it was asked for.
///
/// The distinction this test exists for: `dispatch` returns nothing, so a
/// backend that recorded no `dispatch_workgroups` at all would submit cleanly
/// and leave a buffer full of [`PROBE_SENTINEL`]. Only reading the destination
/// back tells the two apart.
///
/// The empty pass at the end is what makes the assertion above about the
/// *dispatch* rather than about the sentinel copy that precedes it — the same
/// second arm `crcbl-dx12`'s twin carries.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_compute_dispatch_writes_the_values_it_was_asked_for() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    // Not a skip. `Features::COMPUTE` exists for a fallback that has no compute
    // at all; every adapter wgpu opens a device on has it, so an absence here
    // is a capability-reporting bug rather than a machine to tiptoe around.
    assert!(
        device.caps().features.contains(Features::COMPUTE),
        "a wgpu device always has compute; adapter caps report {:?}",
        device.caps().features
    );

    let probe = ComputeProbe::new(&headless);
    let values = probe.run(&headless, |encoder| {
        encoder.dispatch(PROBE_GROUPS, 1, 1);
    });

    assert_probe(&values, &probe_expected(PROBE_ELEMENTS), "a full dispatch");
    assert!(
        !values.contains(&PROBE_SENTINEL),
        "a full dispatch must leave no element unwritten"
    );

    let empty = probe.run(&headless, |_| {});
    assert!(
        empty.iter().all(|value| *value == PROBE_SENTINEL),
        "a compute pass with no dispatch in it must write nothing, or the assertion \
         above was about the sentinel copy rather than about the dispatch; got {:?}…",
        &empty[..4]
    );

    probe.destroy(device);
    headless.finish();
}

/// `dispatch_indirect` reads its workgroup count out of GPU memory, at the
/// offset it was given.
///
/// The argument buffer carries a **decoy** at offset zero that would dispatch
/// every workgroup. So three different failures are distinguishable here rather
/// than confusable: a backend that ignored the offset dispatches eight groups
/// and overwrites the tail; a backend that ignored the argument buffer entirely
/// writes nothing; a correct one writes exactly the front of the buffer and
/// leaves the sentinel behind it.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_indirect_dispatch_reads_its_workgroup_count_from_the_buffer() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    let probe = ComputeProbe::new(&headless);

    /// Workgroups the real arguments ask for. Fewer than [`PROBE_GROUPS`], so
    /// the difference is visible in the readback.
    const DISPATCHED_GROUPS: u32 = 2;
    /// Where the real arguments live. Non-zero, and the decoy sits at zero.
    const ARGS_OFFSET: u64 = 16;

    // Three `u32`s, `x`, `y`, `z` — WebGPU's dispatch-indirect parameters, the
    // same triple Vulkan and D3D12 spell. `crcbl-hal` does not describe the
    // argument layout because it is the backend's native one.
    let mut args_bytes = vec![0u8; ARGS_OFFSET as usize + 12];
    for (slot, value) in [PROBE_GROUPS, 1, 1].iter().enumerate() {
        args_bytes[slot * 4..slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (slot, value) in [DISPATCHED_GROUPS, 1, 1].iter().enumerate() {
        let at = ARGS_OFFSET as usize + slot * 4;
        args_bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("dispatch args upload"),
            size: args_bytes.len() as u64,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a staging buffer");
    device.write_buffer(upload, 0, &args_bytes).expect("write");
    let args = device
        .create_buffer(&BufferDesc {
            label: Some("dispatch args"),
            size: args_bytes.len() as u64,
            usage: BufferUsage::INDIRECT | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("an indirect buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("dispatch args upload"),
        queue: headless.queue,
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: upload,
        src_offset: 0,
        dst: args,
        dst_offset: 0,
        size: args_bytes.len() as u64,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[crcbl_hal::BufferBarrier {
            buffer: args,
            from: ResourceState::TransferDst,
            to: ResourceState::IndirectArgument,
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
    device.destroy_buffer(upload);

    let values = probe.run(&headless, |encoder| {
        encoder.dispatch_indirect(args, ARGS_OFFSET);
    });

    let dispatched = DISPATCHED_GROUPS * crcbl_shaders::compute_probe::WORKGROUP_SIZE;
    assert!(
        dispatched > 0 && dispatched < PROBE_ELEMENTS,
        "the indirect dispatch must cover part of the buffer, not none and not all"
    );
    assert_probe(&values, &probe_expected(dispatched), "an indirect dispatch");
    // Said again in its own words, because the two halves fail for different
    // reasons: the front proves work happened, the tail proves the *count* came
    // from the buffer at the offset that was named.
    assert!(
        values[..dispatched as usize]
            .iter()
            .all(|value| *value != PROBE_SENTINEL),
        "the dispatched workgroups wrote nothing"
    );
    assert!(
        values[dispatched as usize..]
            .iter()
            .all(|value| *value == PROBE_SENTINEL),
        "the workgroups past the indirect count ran anyway — the argument buffer \
         or its offset was not honoured"
    );

    device.destroy_buffer(args);
    probe.destroy(device);
    headless.finish();
}

// ---------------------------------------------------------------------------
// The indexed draw
// ---------------------------------------------------------------------------

/// The indexed draw's target, square so the three colour probes below sit where
/// `crcbl-dx12`'s twin puts them and the two runs can be compared by eye.
const SQUARE: u32 = 64;

/// Bytes one [`SQUARE`]-sided `Rgba8Unorm` image occupies.
const SQUARE_BYTES: u64 = SQUARE as u64 * SQUARE as u64 * 4;

/// The indexed draw's clear colour, and the texel it must become.
///
/// White, which the triangle **cannot** produce: `triangle.slang` interpolates
/// three saturated primaries barycentrically, so every rasterised texel sums to
/// roughly 255 across RGB while this one sums to 765. That makes "cleared" and
/// "drawn" unmistakable without knowing the transfer function — and, being
/// non-zero, it is also not what an image nothing touched would hold.
const INDEXED_CLEAR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// [`INDEXED_CLEAR`] in `Rgba8Unorm`, which encodes 1.0 exactly.
const INDEXED_CLEAR_TEXEL: [u8; 4] = [255, 255, 255, 255];

/// The index buffer the draw below is bound: three degenerate indices, then the
/// real triangle.
///
/// The decoy prefix is the whole point. Drawing `0..3` names vertex zero three
/// times, which is a degenerate triangle that rasterises nothing; drawing
/// `3..6` names the real corners. A backend that ignored the index buffer and
/// drew vertices `0,1,2` directly would rasterise the triangle for *both*
/// ranges, which is exactly what the second half of the test refuses.
const INDICES: [u32; 6] = [0, 0, 0, 0, 1, 2];

/// Where the real triangle's indices start in [`INDICES`].
const FIRST_INDEX: u32 = 3;

/// One texel of a [`SQUARE`]-sided `Rgba8Unorm` readback.
fn texel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = ((y * SQUARE + x) * 4) as usize;
    pixels[at..at + 4].try_into().expect("four bytes")
}

/// The triangle really rasterised, in the orientation WebGPU puts it.
///
/// `set_viewport` passes the height through unflipped on this backend, so NDC
/// `+y` is up and `triangle.slang`'s red apex lands at the **top**. The three
/// probes sit one near each corner of the triangle, and each must be dominated
/// by that corner's primary — which a channel swap, a mirror or a vertex-order
/// mistake each break differently.
fn assert_triangle_drawn(pixels: &[u8], what: &str) {
    assert_eq!(
        pixels.len(),
        SQUARE_BYTES as usize,
        "{what}: the readback is not the whole image"
    );
    for (x, y) in [(0, 0), (SQUARE - 1, SQUARE - 1), (SQUARE - 1, 0)] {
        assert_eq!(
            texel(pixels, x, y),
            INDEXED_CLEAR_TEXEL,
            "{what}: ({x},{y}) is outside the triangle and must still be the clear colour"
        );
    }
    let centre = texel(pixels, SQUARE / 2, SQUARE / 2);
    assert_ne!(
        centre, INDEXED_CLEAR_TEXEL,
        "{what}: the centre is still the clear colour, so nothing was rasterised"
    );
    assert_eq!(centre[3], 255, "{what}: alpha 1.0 must survive");

    for (x, y, channel, name) in [
        (SQUARE / 2, 12, 0, "red"),
        (16, 48, 2, "blue"),
        (48, 48, 1, "green"),
    ] {
        let got = texel(pixels, x, y);
        let others: Vec<u8> = (0..3).filter(|c| *c != channel).map(|c| got[c]).collect();
        assert!(
            others.iter().all(|other| got[channel] > *other),
            "{what}: ({x},{y}) sits nearest the {name} corner, so {name} must dominate; got {got:?}"
        );
        // Barycentric weights sum to one and each corner is a saturated
        // primary, so a texel inside the triangle sums to full scale. A pass
        // that blended against the white clear, or wrote a constant, would not.
        let sum = u32::from(got[0]) + u32::from(got[1]) + u32::from(got[2]);
        assert!(
            (250..=260).contains(&sum),
            "{what}: ({x},{y}) must be a barycentric blend summing to full scale; got {got:?} \
             summing to {sum}"
        );
    }
}

/// Nothing rasterised at all: every texel is still the clear colour.
fn assert_nothing_drawn(pixels: &[u8], what: &str) {
    assert_eq!(
        pixels.len(),
        SQUARE_BYTES as usize,
        "{what}: the readback is not the whole image"
    );
    if let Some((index, found)) = pixels
        .chunks_exact(4)
        .enumerate()
        .find(|(_, chunk)| *chunk != INDEXED_CLEAR_TEXEL)
    {
        let (x, y) = (index as u32 % SQUARE, index as u32 / SQUARE);
        panic!(
            "{what}: ({x},{y}) is {found:?} rather than the clear colour \
             {INDEXED_CLEAR_TEXEL:?} — something rasterised"
        );
    }
}

/// `draw_indexed` pulls its vertices through the index buffer that was bound,
/// over the range it was given.
///
/// This backend recorded `draw_indexed` and nothing ever executed it. The two
/// halves are what make the claim precise rather than merely "a triangle
/// appeared": the real range draws the triangle, and the decoy range — three
/// copies of vertex zero, a degenerate triangle — must draw **nothing**. A
/// backend that dropped the index buffer and drew `0,1,2` for both ranges
/// passes the first half and fails the second.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_indexed_draw_reads_the_index_buffer_it_was_bound() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    // This test owns its format rather than reading the adapter's preferred
    // one: `Rgba8Unorm` needs no channel swap and no sRGB round trip, so the
    // barycentric sum above is a number rather than an approximation.
    let format = Format::Rgba8Unorm;
    let target = device
        .create_image(&ImageDesc {
            label: Some("wgpu e2e indexed target"),
            image_type: ImageType::D2,
            extent: Extent3d::d2(SQUARE, SQUARE),
            format,
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a render target");
    let view = device
        .create_image_view(&ImageViewDesc {
            label: Some("wgpu e2e indexed view"),
            image: target,
            view_type: ImageViewType::D2,
            format,
            range: ImageSubresourceRange::all(format),
        })
        .expect("a view of the target");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e indexed readback"),
            size: SQUARE_BYTES,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    // The geometry is pulled from a storage buffer — there is no
    // `bind_vertex_buffer` in this seam — so only the *indices* decide which
    // vertex the shader reads.
    let vertex_bytes = crcbl_shaders::triangle::vertex_bytes();
    let index_bytes: Vec<u8> = INDICES
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect();
    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e indexed upload"),
            size: (vertex_bytes.len() + index_bytes.len()) as u64,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a staging buffer");
    device
        .write_buffer(upload, 0, &vertex_bytes)
        .expect("write");
    device
        .write_buffer(upload, vertex_bytes.len() as u64, &index_bytes)
        .expect("write");

    let vertices = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e triangle vertices"),
            size: vertex_bytes.len() as u64,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a vertex storage buffer");
    let indices = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e triangle indices"),
            size: index_bytes.len() as u64,
            usage: BufferUsage::INDEX | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("an index buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("wgpu e2e indexed upload"),
        queue: headless.queue,
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: upload,
        src_offset: 0,
        dst: vertices,
        dst_offset: 0,
        size: vertex_bytes.len() as u64,
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: upload,
        src_offset: vertex_bytes.len() as u64,
        dst: indices,
        dst_offset: 0,
        size: index_bytes.len() as u64,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[
            crcbl_hal::BufferBarrier {
                buffer: vertices,
                from: ResourceState::TransferDst,
                to: ResourceState::ShaderRead,
                queue_transfer: None,
            },
            crcbl_hal::BufferBarrier {
                buffer: indices,
                from: ResourceState::TransferDst,
                to: ResourceState::IndexBuffer,
                queue_transfer: None,
            },
        ],
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    device.destroy_buffer(upload);

    let layout_entries = [crcbl_hal::BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::VERTEX,
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
        .expect("the triangle's layout");
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
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("triangle"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })
        .expect("a pipeline layout");

    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("triangle.slang"),
            wgsl: crcbl_shaders::TRIANGLE.wgsl(),
            ..ShaderModuleDesc::default()
        })
        .expect("the committed WGSL is accepted");
    let color_targets = [ColorTargetState::opaque(format)];
    let pipeline = device
        .create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("triangle"),
            layout: pipeline_layout,
            vertex: ShaderEntry {
                module,
                entry_point: crcbl_shaders::TRIANGLE
                    .entry_point(crcbl_shaders::Stage::Vertex)
                    .expect("the triangle has exactly one vertex entry point"),
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: crcbl_shaders::TRIANGLE
                    .entry_point(crcbl_shaders::Stage::Fragment)
                    .expect("the triangle has exactly one fragment entry point"),
            }),
            // The default cull mode is `None`, so the winding — which this
            // backend does not flip, unlike `crcbl-vk`'s negative-height
            // viewport — cannot decide whether the triangle appears. This test
            // is about the index buffer, not about culling.
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            color_targets: &color_targets,
        })
        .expect("a graphics pipeline");
    device.destroy_shader_module(module);

    let render = |range: std::ops::Range<u32>| -> Vec<u8> {
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("wgpu e2e indexed draw"),
            queue: headless.queue,
        });
        encoder.pipeline_barrier(&Barriers {
            images: &[crcbl_hal::ImageBarrier::new(
                target,
                ImageSubresourceRange::all(format),
                ResourceState::Undefined,
                ResourceState::ColorAttachment,
            )],
            ..Barriers::default()
        });
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("clear + indexed triangle"),
            color_attachments: &[ColorAttachment {
                view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(INDEXED_CLEAR),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(SQUARE, SQUARE),
        });
        encoder.set_viewport(&Viewport::from_size(SQUARE, SQUARE));
        encoder.set_scissor(&Rect2d::from_size(SQUARE, SQUARE));
        encoder.bind_graphics_pipeline(pipeline);
        encoder.bind_group(0, bind_group, &[], pipeline_layout);
        encoder.bind_index_buffer(indices, 0, IndexFormat::Uint32);
        encoder.draw_indexed(range, 0, 0..1);
        encoder.end_render_pass();
        encoder.pipeline_barrier(&Barriers {
            images: &[crcbl_hal::ImageBarrier::new(
                target,
                ImageSubresourceRange::all(format),
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
            image: target,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: Offset3d::default(),
            image_extent: Extent3d::d2(SQUARE, SQUARE),
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
        read_bytes(device, staging, SQUARE_BYTES)
    };

    let drawn = render(FIRST_INDEX..FIRST_INDEX + 3);
    assert_triangle_drawn(&drawn, "an indexed draw of indices 3..6");

    // The decoy's own three indices are all vertex zero, so drawing them is a
    // degenerate triangle and nothing rasterises. That is what makes the
    // assertion above about the *range* rather than about the draw.
    let decoy = render(0..FIRST_INDEX);
    assert_nothing_drawn(&decoy, "an indexed draw of the decoy's three zeros");

    device.destroy_graphics_pipeline(pipeline);
    device.destroy_pipeline_layout(pipeline_layout);
    device.destroy_bind_group(bind_group);
    device.destroy_bind_group_layout(bind_group_layout);
    device.destroy_buffer(indices);
    device.destroy_buffer(vertices);
    device.destroy_buffer(staging);
    device.destroy_image_view(view);
    device.destroy_image(target);
    headless.finish();
}

// ---------------------------------------------------------------------------
// The timeline semaphore
// ---------------------------------------------------------------------------

/// A submission's timeline signal reaches the CPU, at the value it was given.
///
/// `crcbl-mtl` carries the twin of this claim. This backend has no timeline
/// object to hand to a driver — wgpu offers none — so `create_semaphore` keeps
/// a counter beside the `SubmissionIndex` each signal was attached to, and
/// `semaphore_value` advances it by polling. That is a real mechanism with a
/// real failure mode: a signal recorded against no submission, or a poll that
/// never resolved, leaves the counter at its initial value forever and every
/// waiter blocks. Nothing executed it until now.
///
/// The value **9** against an initial **5** is deliberate on both ends: a
/// backend that merely incremented a counter, or one that reported "signalled"
/// as a boolean, agrees with a 0→1 test and disagrees with this one.
///
/// # Where this backend differs from Metal
///
/// A wait on a value *nothing submitted will ever signal* is
/// [`HalError::Unsupported`] here, where Metal returns `Ok(false)` on the
/// timeout. That is not a shortcut: a `MTLSharedEvent` can be signalled later
/// by anything holding it, so waiting is meaningful; wgpu can only wait on a
/// `SubmissionIndex` that already exists, so the same call would block forever
/// with nothing to point at. The refusal is asserted below rather than glossed.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_timeline_semaphore_signals_from_a_submission_and_the_cpu_sees_it() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    assert!(
        device
            .caps()
            .features
            .contains(Features::TIMELINE_SEMAPHORE),
        "this backend reports TIMELINE_SEMAPHORE unconditionally; caps say {:?}",
        device.caps().features
    );

    let semaphore = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("wgpu e2e timeline"),
            kind: SemaphoreKind::Timeline { initial_value: 5 },
        })
        .expect("this device reports TIMELINE_SEMAPHORE");
    assert_eq!(
        device.semaphore_value(semaphore).expect("a timeline value"),
        5,
        "initial_value must be the counter's starting point, not zero"
    );

    // Nothing has signalled 9 yet, and on this backend nothing can signal it
    // later either — see the header. The refusal is loud rather than a hang.
    let error = device
        .wait_semaphores(
            &[SemaphoreWait {
                semaphore,
                value: 9,
            }],
            1_000_000,
        )
        .expect_err("nothing submitted will ever reach 9, so there is nothing to wait on");
    assert!(matches!(error, HalError::Unsupported { .. }), "{error}");

    // Real work, not a bare signal: the point is that the value arrives when
    // the submission *completes*, so there has to be something to complete.
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("wgpu e2e timeline frame"),
        queue: headless.queue,
    });
    encoder.begin_render_pass(&RenderPassDesc {
        label: Some("timeline clear"),
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
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(
            headless.queue,
            &SubmitInfo {
                command_buffers: &[commands],
                waits: &[],
                signals: &[SemaphoreSignal {
                    semaphore,
                    value: 9,
                }],
            },
        )
        .expect("a submission may signal a timeline");

    assert!(
        device
            .wait_semaphores(
                &[SemaphoreWait {
                    semaphore,
                    value: 9
                }],
                SEMAPHORE_DEADLINE_NS
            )
            .expect("the wait resolves"),
        "the submission's signal never reached the semaphore"
    );
    assert_eq!(
        device.semaphore_value(semaphore).expect("a timeline value"),
        9,
        "the CPU must read back the value the submission signalled, not the initial one"
    );

    // Backwards is refused. Without this the counter would never reach the
    // value a later waiter is blocked on, and the process would hang with
    // nothing to point at.
    let encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("wgpu e2e timeline backwards"),
        queue: headless.queue,
    });
    let backwards = encoder.finish().expect("an empty command buffer records");
    let error = device
        .submit(
            headless.queue,
            &SubmitInfo {
                command_buffers: &[backwards],
                waits: &[],
                signals: &[SemaphoreSignal {
                    semaphore,
                    value: 9,
                }],
            },
        )
        .expect_err("9 has already been signalled, so signalling it again is not monotonic");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error}");

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
    device.destroy_command_buffer(backwards);
    device.destroy_command_buffer(commands);
    device.destroy_semaphore(semaphore);
    headless.finish();
}

/// **Obligation 3.** A handle from device A used on device B is a caller bug
/// that must be *detected*, and detected as `ForeignObject` rather than as a
/// stale handle — the two send a reader to different bugs.
///
/// Device B is given a buffer of its own first, and that is the whole design of
/// this test: without the device tag in the handle, A's first handle and B's
/// first handle are bit-identical, so B would resolve A's handle to B's own
/// buffer, find the owner matching, and write into the wrong object with no
/// error anywhere. Two devices from *one* instance, because that is the
/// arrangement in which their pools genuinely allocate in step.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_handle_from_another_wgpu_device_is_foreign_not_merely_unresolvable() {
    let instance = instance();
    let adapter = instance.adapters().remove(0);
    let open = |label| {
        instance
            .create_device(&DeviceDesc {
                label: Some(label),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::empty(),
                compatible_surface: None,
            })
            .expect("a featureless headless device opens on any adapter")
    };
    let a = open("wgpu e2e obligation 3 A");
    let b = open("wgpu e2e obligation 3 B");

    let describe = |label| BufferDesc {
        label: Some(label),
        size: 64,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    };
    let on_a = a.create_buffer(&describe("on A")).expect("a buffer on A");
    let on_b = b
        .create_buffer(&describe("on B"))
        .expect("a buffer on B, occupying the slot A's handle would land in");
    assert_eq!(
        on_a.generation(),
        on_b.generation(),
        "both pools are fresh, so only the tag can tell these apart"
    );

    let error = b
        .write_buffer(on_a, 0, &[0xFF; 4])
        .expect_err("A's buffer is not B's to write");
    assert!(
        matches!(error, HalError::ForeignObject { kind: "buffer", .. }),
        "a live handle from another device is foreign, not merely unresolvable: {error}"
    );

    // B's own still resolves, so the check is not simply refusing everything.
    b.write_buffer(on_b, 0, &[0xFF; 4])
        .expect("B's own buffer resolves");

    // A destroy with a foreign handle must not take the local object that
    // shares its bits.
    b.destroy_buffer(on_a);
    b.write_buffer(on_b, 0, &[0xEE; 4])
        .expect("B's buffer survived a foreign destroy");

    // The queue is synthesised rather than pooled, and obligation 3 covers it
    // too: without the tag, every device accepted every other device's.
    let queue_of_a = a.queue(QueueKind::Graphics).expect("a graphics queue");
    let error = b
        .submit(queue_of_a, &SubmitInfo::new(&[]))
        .expect_err("A's queue is not B's to submit to");
    assert!(
        matches!(error, HalError::ForeignObject { kind: "queue", .. }),
        "{error}"
    );

    a.destroy_buffer(on_a);
    b.destroy_buffer(on_b);
}

/// **Obligation 3, across two instances.** Handle bits are only unique within
/// the backend that issued them, so two instances genuinely hand out identical
/// ones — a surface crossing them must be detected, and detected as
/// `ForeignObject`.
///
/// Surfaces are scoped to the *instance* rather than the device, which is why
/// this is a separate test from the buffer above and not a second assertion in
/// it.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_surface_from_one_instance_is_foreign_to_another() {
    let owner = instance();
    let other = instance();

    // SAFETY: `Offscreen` names no platform object at all, so there is nothing
    // that has to outlive the surface.
    let surface = unsafe { owner.create_surface(&SurfaceTarget::Offscreen) }
        .expect("an offscreen surface needs no window system");
    // The other instance makes one of its own first, so its pool is occupied at
    // the slot the foreign handle names — without the tag it would resolve.
    let native = unsafe { other.create_surface(&SurfaceTarget::Offscreen) }.expect("offscreen");
    assert_eq!(
        surface.generation(),
        native.generation(),
        "both pools are fresh, so only the tag can tell these apart"
    );
    let adapter = other.adapters().remove(0);

    let error = other
        .surface_caps(surface, adapter.id)
        .expect_err("this surface belongs to the other instance");
    assert!(
        matches!(
            error,
            HalError::ForeignObject {
                kind: "surface",
                ..
            }
        ),
        "a live handle from another instance is foreign, not unresolvable: {error}"
    );

    let error = other
        .create_device(&DeviceDesc {
            label: Some("foreign surface"),
            adapter: adapter.id,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: Some(surface),
        })
        .expect_err("and the same on the device-creation path");
    assert!(
        matches!(
            error,
            HalError::ForeignObject {
                kind: "surface",
                ..
            }
        ),
        "{error}"
    );

    // The other instance must not be able to free it either, which is what
    // stops a stray `destroy_surface` from turning a bit collision into a
    // double free.
    other.destroy_surface(surface);
    owner
        .surface_caps(surface, owner.adapters().remove(0).id)
        .expect("the owning instance still has its surface");

    other.destroy_surface(native);
    owner.destroy_surface(surface);
}

/// A recycled slot must not resurrect the handle that used to name it.
///
/// The tagging is what could break this: the generation half is the only thing
/// separating the two handles, so a stamp that disturbed it would make the dead
/// handle name the live buffer.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_destroyed_wgpu_handle_does_not_alias_the_buffer_that_replaces_it() {
    let (instance, device, surface, _queue, _format) = Headless::open_device();
    let describe = BufferDesc {
        label: Some("wgpu e2e recycled slot"),
        size: 256,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    };
    let first = device.create_buffer(&describe).expect("first buffer");
    device.destroy_buffer(first);
    let second = device.create_buffer(&describe).expect("second buffer");

    assert_eq!(
        first.index(),
        second.index(),
        "the free list should have handed back the same slot; if not, this test is not exercising \
         recycling at all"
    );
    assert_ne!(
        first, second,
        "the pool reissued the identical handle, so the generation never moved"
    );
    device
        .write_buffer(second, 0, &[1u8; 4])
        .expect("the live handle resolves");
    let error = device
        .write_buffer(first, 0, &[1u8; 4])
        .expect_err("the dead handle must not name its replacement");
    assert!(
        matches!(error, HalError::InvalidHandle { kind: "buffer", .. }),
        "a stale handle of this device's own is stale, not foreign: {error}"
    );

    device.destroy_buffer(second);
    device.wait_idle().expect("idle");
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}

// ---------------------------------------------------------------------------
// Array bindings, which is what `BindGroupEntry::array_index` is for
// ---------------------------------------------------------------------------

/// The scalar binding every array test puts beside its array — a storage buffer
/// below, a sampler in the layout the refusals fail against.
///
/// Both spellings in one group deliberately: wgpu takes a single object for a
/// binding the layout declared without a count and a slice for one it declared
/// with, so a group holding both is the shape that catches a backend which
/// applies one rule to everything.
const SCALAR_BINDING: u32 = 0;

/// The binding the array tests fill.
const ARRAY_BINDING: u32 = 1;

/// How many elements the array binding declares when a test fills it whole.
const ARRAY_COUNT: u32 = 2;

/// The red channel each array element is cleared to, as a byte.
///
/// Distinct, and neither 0 nor 255: an element the shader never saw then reads
/// as a mismatch rather than as whatever fresh device memory happened to hold,
/// and a backend that bound one texture into both slots reports the same number
/// twice.
const ELEMENT_REDS: [u32; 2] = [60, 200];

/// `count` sampled 1x1 images and a view of each.
///
/// Distinct images rather than one image viewed twice: two entries resolving to
/// the same wgpu object would let a backend that dropped one of them still look
/// correct.
fn sampled_views(
    device: &dyn Device,
    count: u32,
) -> (Vec<crcbl_hal::ImageHandle>, Vec<crcbl_hal::ImageViewHandle>) {
    let format = Format::Rgba8Unorm;
    let mut images = Vec::new();
    let mut views = Vec::new();
    for index in 0..count {
        let image = device
            .create_image(&ImageDesc {
                label: Some("wgpu e2e array element"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(1, 1),
                format,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::SAMPLED | ImageUsage::COLOR_ATTACHMENT,
                memory: MemoryLocation::DeviceLocal,
            })
            .unwrap_or_else(|error| panic!("array element {index}: {error}"));
        let view = device
            .create_image_view(&ImageViewDesc {
                label: Some("wgpu e2e array element view"),
                image,
                view_type: ImageViewType::D2,
                format,
                range: ImageSubresourceRange::all(format),
            })
            .unwrap_or_else(|error| panic!("array element {index} view: {error}"));
        images.push(image);
        views.push(view);
    }
    (images, views)
}

/// The layout the array tests bind against: a scalar sampler, and an image
/// array `count` elements long.
///
/// `PARTIALLY_BOUND` only once the binding is genuinely an array: every
/// [`BindingFlags`](crcbl_hal::BindingFlags) needs `DESCRIPTOR_INDEXING`, so
/// declaring one on the single-element layout would make the refusals that need
/// no array at all unbuildable on an adapter without the feature — which is the
/// half those tests run unconditionally.
fn array_layout(device: &dyn Device, count: u32) -> crcbl_hal::BindGroupLayoutHandle {
    let flags = if count > 1 {
        crcbl_hal::BindingFlags::PARTIALLY_BOUND
    } else {
        crcbl_hal::BindingFlags::empty()
    };
    let entries = [
        crcbl_hal::BindGroupLayoutEntry {
            binding: SCALAR_BINDING,
            visibility: ShaderStages::FRAGMENT,
            kind: crcbl_hal::BindingKind::Sampler { comparison: false },
            count: 1,
            flags: crcbl_hal::BindingFlags::empty(),
        },
        crcbl_hal::BindGroupLayoutEntry {
            binding: ARRAY_BINDING,
            visibility: ShaderStages::FRAGMENT,
            kind: crcbl_hal::BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            count,
            flags,
        },
    ];
    device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: Some("wgpu e2e array"),
            entries: &entries,
        })
        .expect("a layout with an image array")
}

/// One entry filling one array element.
fn view_entry(array_index: u32, view: crcbl_hal::ImageViewHandle) -> crcbl_hal::BindGroupEntry {
    crcbl_hal::BindGroupEntry {
        binding: ARRAY_BINDING,
        array_index,
        resource: crcbl_hal::BindingResource::ImageView(view),
    }
}

/// Fails a bind group and hands back what the seam said, so each refusal below
/// is one line and its assertions are about the message rather than about the
/// boilerplate.
fn refused(
    device: &dyn Device,
    layout: crcbl_hal::BindGroupLayoutHandle,
    entries: &[crcbl_hal::BindGroupEntry],
    variable_count: Option<u32>,
    what: &str,
) -> String {
    let error = device
        .create_bind_group(&crcbl_hal::BindGroupDesc {
            label: Some("wgpu e2e refused"),
            layout,
            entries,
            variable_count,
        })
        .err()
        .unwrap_or_else(|| panic!("{what} must be refused, not built"));
    assert!(
        matches!(error, HalError::InvalidDescriptor(_)),
        "{what}: a descriptor this backend cannot express is InvalidDescriptor, got {error:?}"
    );
    error.to_string()
}

/// A compute shader reads back the element the bind group put in each slot.
///
/// `BindGroupEntry::array_index` had no reader in this backend at all: every
/// entry became a `wgpu::BindGroupEntry` keyed on `binding` alone, so two
/// entries naming elements 0 and 1 of one binding arrived as two entries with
/// the same binding number and wgpu rejected the whole group. The layout half
/// already mapped the seam's `count` onto wgpu's `Some(NonZero)`, so the layout
/// was expressible while the group was not — which is what made this look
/// supported from a distance.
///
/// Creating the group is not enough of an observable on its own: a backend that
/// bound element 0 twice, or dropped element 1, or ordered the slice by the
/// order the caller happened to write its entries in, builds a group wgpu
/// accepts and paints the wrong picture. So the two textures are cleared to
/// different values and a shader reads both slots, which is the only thing that
/// tells those apart. The entries are listed element 1 first for the same
/// reason: wgpu's array is dense and positional, so the backend has to sort by
/// `array_index` itself.
///
/// The WGSL is written here rather than compiled from `crcbl-shaders`. Nothing
/// in the engine's Slang declares a `binding_array` yet, and a test-local module
/// needs no toolchain and no committed artifact — the shader-module suite above
/// already feeds this backend inline WGSL.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_shader_reads_the_array_element_the_bind_group_put_in_each_slot() {
    let (instance, device, surface, queue, _format) = Headless::open_device();
    if !device
        .caps()
        .features
        .contains(Features::DESCRIPTOR_INDEXING)
    {
        println!(
            "wgpu e2e: this adapter has no descriptor indexing, so an array bind-group layout \
             cannot be created here at all; skipping"
        );
        instance.destroy_surface(surface);
        drop(device);
        drop(instance);
        return;
    }

    let format = Format::Rgba8Unorm;
    let (images, views) = sampled_views(device.as_ref(), ARRAY_COUNT);
    let seen_bytes = ELEMENT_REDS.len() as u64 * 4;
    let seen = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e array readback target"),
            size: seen_bytes,
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a storage buffer");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("wgpu e2e array staging"),
            size: seen_bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let layout_entries = [
        crcbl_hal::BindGroupLayoutEntry {
            binding: SCALAR_BINDING,
            visibility: ShaderStages::COMPUTE,
            kind: crcbl_hal::BindingKind::StorageBuffer {
                read_only: false,
                dynamic: false,
            },
            count: 1,
            flags: crcbl_hal::BindingFlags::empty(),
        },
        crcbl_hal::BindGroupLayoutEntry {
            binding: ARRAY_BINDING,
            visibility: ShaderStages::COMPUTE,
            kind: crcbl_hal::BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            count: ARRAY_COUNT,
            flags: crcbl_hal::BindingFlags::PARTIALLY_BOUND,
        },
    ];
    let layout = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: Some("wgpu e2e array read"),
            entries: &layout_entries,
        })
        .expect("a layout with an image array");

    let scalar_entry = crcbl_hal::BindGroupEntry {
        binding: SCALAR_BINDING,
        array_index: 0,
        resource: crcbl_hal::BindingResource::whole_buffer(seen),
    };
    let entries = [
        view_entry(1, views[1]),
        scalar_entry,
        view_entry(0, views[0]),
    ];
    let group = device
        .create_bind_group(&crcbl_hal::BindGroupDesc {
            label: Some("wgpu e2e array read"),
            layout,
            entries: &entries,
            variable_count: None,
        })
        .expect("both elements of the array reach one wgpu binding");

    let set_layouts = [layout];
    let pipeline_layout = device
        .create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("wgpu e2e array read"),
            bind_group_layouts: &set_layouts,
            push_constants: None,
        })
        .expect("a pipeline layout over the array");
    let module = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("wgpu e2e array read"),
            wgsl: Some(
                "enable wgpu_binding_array;\n\
                 @group(0) @binding(0) var<storage, read_write> seen : array<u32, 2>;\n\
                 @group(0) @binding(1) var slots : binding_array<texture_2d<f32>, 2>;\n\
                 @compute @workgroup_size(1)\n\
                 fn main() {\n\
                 seen[0] = u32(round(textureLoad(slots[0], vec2<i32>(0, 0), 0).r * 255.0));\n\
                 seen[1] = u32(round(textureLoad(slots[1], vec2<i32>(0, 0), 0).r * 255.0));\n\
                 }\n",
            ),
            ..ShaderModuleDesc::default()
        })
        .expect("naga accepts a binding array behind its own enable directive");
    let pipeline = device
        .create_compute_pipeline(&ComputePipelineDesc {
            label: Some("wgpu e2e array read"),
            layout: pipeline_layout,
            compute: ShaderEntry {
                module,
                entry_point: "main",
            },
            workgroup_size: [1, 1, 1],
        })
        .expect("a compute pipeline over the array");
    device.destroy_shader_module(module);

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("wgpu e2e array read"),
        queue,
    });
    for (index, red) in ELEMENT_REDS.iter().enumerate() {
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("wgpu e2e array element clear"),
            color_attachments: &[ColorAttachment {
                view: views[index],
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                // Rgba8Unorm, so the store is a plain quantisation and the
                // shader's `round(r * 255.0)` recovers the byte exactly. An
                // sRGB format would put a transfer function in between.
                clear: ClearValue::color([*red as f32 / 255.0, 0.0, 0.0, 1.0]),
            }],
            depth_stencil_attachment: None,
            render_area: Rect2d::from_size(1, 1),
        });
        encoder.end_render_pass();
    }
    encoder.pipeline_barrier(&Barriers {
        images: &images
            .iter()
            .map(|image| {
                crcbl_hal::ImageBarrier::new(
                    *image,
                    ImageSubresourceRange::all(format),
                    ResourceState::ColorAttachment,
                    ResourceState::ShaderRead,
                )
            })
            .collect::<Vec<_>>(),
        ..Barriers::default()
    });
    encoder.begin_compute_pass(&ComputePassDesc {
        label: Some("wgpu e2e array read"),
    });
    encoder.bind_compute_pipeline(pipeline);
    encoder.bind_group(0, group, &[], pipeline_layout);
    encoder.dispatch(1, 1, 1);
    encoder.end_compute_pass();
    encoder.pipeline_barrier(&Barriers {
        buffers: &[crcbl_hal::BufferBarrier {
            buffer: seen,
            from: ResourceState::ShaderReadWrite,
            to: ResourceState::TransferSrc,
            queue_transfer: None,
        }],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: seen,
        src_offset: 0,
        dst: staging,
        dst_offset: 0,
        size: seen_bytes,
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);
    assert!(
        device.take_error().is_none(),
        "the array bind group reached a dispatch without wgpu objecting"
    );

    let read: Vec<u32> = read_bytes(device.as_ref(), staging, seen_bytes)
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        .collect();
    assert_eq!(
        read,
        ELEMENT_REDS.to_vec(),
        "the shader read {read:?} out of the array; slot i must hold the texture the entry with \
         array_index i named, in that order and no other"
    );

    // A trailing shortfall — elements 0..n with n below the declared count — is
    // the one partial fill wgpu accepts, so it must still build. Not dispatched:
    // reading an element nothing wrote is exactly what PARTIALLY_BOUND does not
    // promise.
    let short = device
        .create_bind_group(&crcbl_hal::BindGroupDesc {
            label: Some("wgpu e2e array, trailing shortfall"),
            layout,
            entries: &[scalar_entry, view_entry(0, views[0])],
            variable_count: None,
        })
        .expect("a partial fill from element zero upwards is legal");
    device.destroy_bind_group(short);

    device.destroy_compute_pipeline(pipeline);
    device.destroy_bind_group(group);
    device.destroy_pipeline_layout(pipeline_layout);
    device.destroy_bind_group_layout(layout);
    device.destroy_buffer(staging);
    device.destroy_buffer(seen);
    for view in views {
        device.destroy_image_view(view);
    }
    for image in images {
        device.destroy_image(image);
    }
    device.wait_idle().expect("idle");
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}

/// Every array fill wgpu has no spelling for is refused here, by name.
///
/// wgpu's binding arrays are **dense**: element *i* of the slice is array
/// element *i*, and nothing carries an index alongside it. The seam allows a
/// sparse fill, so a group naming elements 0 and 2 is a legal descriptor with no
/// wgpu counterpart — and packing it into a two-element slice would bind element
/// 2's texture into element 1, a wrong picture with no error anywhere. The same
/// goes for an element written twice, an index past the declared length, and one
/// binding holding two kinds of resource, whose array spellings are typed.
///
/// The scalar half runs on every adapter. Only the array half needs descriptor
/// indexing, because without it no array layout can be created to fail against.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_array_binding_wgpu_cannot_spell_is_refused_instead_of_packed() {
    let (instance, device, surface, _queue, _format) = Headless::open_device();
    let (images, views) = sampled_views(device.as_ref(), 3);
    let sampler = device
        .create_sampler(&crcbl_hal::SamplerDesc {
            label: Some("wgpu e2e array sampler"),
            ..crcbl_hal::SamplerDesc::default()
        })
        .expect("a filtering sampler");

    let scalar = array_layout(device.as_ref(), 1);
    let twice = refused(
        device.as_ref(),
        scalar,
        &[view_entry(0, views[0]), view_entry(0, views[1])],
        None,
        "one slot written twice",
    );
    for expected in ["binding 1", "twice"] {
        assert!(
            twice.contains(expected),
            "{expected:?} missing from {twice}"
        );
    }
    let past_end = refused(
        device.as_ref(),
        scalar,
        &[view_entry(1, views[1])],
        None,
        "an index past a scalar binding's one slot",
    );
    for expected in ["index 1", "binding 1", "holds 1"] {
        assert!(
            past_end.contains(expected),
            "{expected:?} missing from {past_end}"
        );
    }
    let undeclared = refused(
        device.as_ref(),
        scalar,
        &[crcbl_hal::BindGroupEntry {
            binding: 9,
            array_index: 0,
            resource: crcbl_hal::BindingResource::ImageView(views[0]),
        }],
        None,
        "an entry naming a binding the layout never declared",
    );
    for expected in ["binding 9", "does not declare"] {
        assert!(
            undeclared.contains(expected),
            "{expected:?} missing from {undeclared}"
        );
    }
    device.destroy_bind_group_layout(scalar);

    if device
        .caps()
        .features
        .contains(Features::DESCRIPTOR_INDEXING)
    {
        // Three elements so the hole at 1 is a hole rather than an index past
        // the end, which would be a different refusal.
        let sparse = array_layout(device.as_ref(), 3);
        let hole = refused(
            device.as_ref(),
            sparse,
            &[view_entry(0, views[0]), view_entry(2, views[2])],
            None,
            "an array with a hole in it",
        );
        for expected in ["binding 1", "index 1", "sparse"] {
            assert!(hole.contains(expected), "{expected:?} missing from {hole}");
        }
        device.destroy_bind_group_layout(sparse);

        let mixed_layout = array_layout(device.as_ref(), ARRAY_COUNT);
        let mixed = refused(
            device.as_ref(),
            mixed_layout,
            &[
                view_entry(0, views[0]),
                crcbl_hal::BindGroupEntry {
                    binding: ARRAY_BINDING,
                    array_index: 1,
                    resource: crcbl_hal::BindingResource::Sampler(sampler),
                },
            ],
            None,
            "one binding holding both an image view and a sampler",
        );
        for expected in ["binding 1", "kind"] {
            assert!(
                mixed.contains(expected),
                "{expected:?} missing from {mixed}"
            );
        }
        device.destroy_bind_group_layout(mixed_layout);
    } else {
        println!(
            "wgpu e2e: this adapter has no descriptor indexing, so the array-shaped refusals have \
             no layout to fail against; the scalar ones above still ran"
        );
    }

    assert!(
        device.take_error().is_none(),
        "a descriptor this backend refuses must never have reached wgpu"
    );

    device.destroy_sampler(sampler);
    for view in views {
        device.destroy_image_view(view);
    }
    for image in images {
        device.destroy_image(image);
    }
    device.wait_idle().expect("idle");
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}

/// A [`BindingFlags`](crcbl_hal::BindingFlags) this device cannot honour is
/// refused, and so is a `VARIABLE_COUNT` entry in the wrong place.
///
/// `BindGroupLayoutEntry::flags` had no reader in this backend at all: the
/// layout was built from `visibility`, `kind` and `count`, and every flag was
/// dropped without a word. Both obligations the seam states were therefore
/// silently unmet — a device without `DESCRIPTOR_INDEXING` handed back a fixed
/// array wearing a bindless declaration ("a bindless array quietly downgraded to
/// a fixed one reads garbage at index 4097"), and a `VARIABLE_COUNT` entry that
/// was neither last nor highest-numbered was accepted, which is what makes every
/// "the variable binding is `entries.last()`" reading in a backend wrong.
///
/// The feature half runs on **every** adapter, because it runs against a device
/// that deliberately did not ask for descriptor indexing rather than against an
/// adapter that happens to lack it. Only the ordering half needs the feature: a
/// `VARIABLE_COUNT` layout cannot be created at all without it, so there would
/// be nothing for the ordering rule to fail against.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_bind_group_layout_flag_this_device_cannot_honour_is_refused_not_dropped() {
    // Deliberately not `open_device`, which asks for `GPU_DRIVEN` — that
    // contains `DESCRIPTOR_INDEXING`, so on an adapter that has it this half
    // would never be reached.
    let (instance, device, surface, _queue, _format) =
        Headless::open_device_with(Features::DEBUG_MARKERS);
    assert!(
        !device
            .caps()
            .features
            .contains(Features::DESCRIPTOR_INDEXING),
        "a device that never asked for descriptor indexing must not report it"
    );

    // `count` 1 on both bindings, so the flag is the *only* thing this device
    // cannot express: an array of two would need wgpu's TEXTURE_BINDING_ARRAY
    // as well, and a refusal would no longer say which of the two caused it.
    let flagged = |flags| {
        [
            crcbl_hal::BindGroupLayoutEntry {
                binding: SCALAR_BINDING,
                visibility: ShaderStages::FRAGMENT,
                kind: crcbl_hal::BindingKind::Sampler { comparison: false },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: ARRAY_BINDING,
                visibility: ShaderStages::FRAGMENT,
                kind: crcbl_hal::BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: 1,
                flags,
            },
        ]
    };
    let entries = flagged(crcbl_hal::BindingFlags::PARTIALLY_BOUND);
    let error = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: Some("wgpu e2e flagged layout"),
            entries: &entries,
        })
        .expect_err("a descriptor-indexing flag this device cannot honour must be refused");
    assert!(
        matches!(error, HalError::Unsupported { .. }),
        "a feature the device does not have is Unsupported, got {error:?}"
    );
    assert!(
        error.to_string().contains("DESCRIPTOR_INDEXING"),
        "the feature that is missing must be named: {error}"
    );

    // The same layout with the flag cleared, so the refusal above is about the
    // flag and not about anything else in the descriptor.
    let entries = flagged(crcbl_hal::BindingFlags::empty());
    let plain = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: Some("wgpu e2e unflagged layout"),
            entries: &entries,
        })
        .expect("the same layout without the flag is an ordinary one");
    device.destroy_bind_group_layout(plain);
    assert!(
        device.take_error().is_none(),
        "a layout this backend refuses must never have reached wgpu"
    );
    device.wait_idle().expect("idle");
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);

    // The ordering rule needs a device that *does* honour the flags, or the
    // refusal above fires first and proves nothing about the ordering.
    let (instance, device, surface, _queue, _format) = Headless::open_device();
    if device
        .caps()
        .features
        .contains(Features::DESCRIPTOR_INDEXING)
    {
        let scalar = crcbl_hal::BindGroupLayoutEntry {
            binding: SCALAR_BINDING,
            visibility: ShaderStages::FRAGMENT,
            kind: crcbl_hal::BindingKind::Sampler { comparison: false },
            count: 1,
            flags: crcbl_hal::BindingFlags::empty(),
        };
        let variable = |binding| crcbl_hal::BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            kind: crcbl_hal::BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            count: ARRAY_COUNT,
            flags: crcbl_hal::BindingFlags::VARIABLE_COUNT
                | crcbl_hal::BindingFlags::PARTIALLY_BOUND,
        };
        let refuse = |entries: &[crcbl_hal::BindGroupLayoutEntry], what: &str| -> String {
            let error = device
                .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                    label: Some("wgpu e2e variable count ordering"),
                    entries,
                })
                .err()
                .unwrap_or_else(|| panic!("{what} must be refused, not built"));
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: a layout the seam forbids is InvalidDescriptor, got {error:?}"
            );
            error.to_string()
        };

        // The highest-numbered binding, but not the last element of the slice —
        // the half a backend checking only Vulkan's own rule would accept.
        let unsorted = refuse(
            &[variable(9), scalar],
            "a VARIABLE_COUNT entry that is not the last element of the slice",
        );
        for expected in ["binding 9", "VARIABLE_COUNT", "last entry"] {
            assert!(
                unsorted.contains(expected),
                "{expected:?} missing from {unsorted}"
            );
        }

        // The last element of the slice, but outranked by a binding number
        // above it — the half Vulkan itself refuses.
        let outranked = refuse(
            &[
                crcbl_hal::BindGroupLayoutEntry {
                    binding: 9,
                    ..scalar
                },
                variable(ARRAY_BINDING),
            ],
            "a VARIABLE_COUNT entry that is not the highest binding number",
        );
        for expected in ["binding 1", "VARIABLE_COUNT", "highest-numbered"] {
            assert!(
                outranked.contains(expected),
                "{expected:?} missing from {outranked}"
            );
        }

        // Both halves satisfied, so it builds — without which the two refusals
        // above would pass just as well against a backend that refused every
        // VARIABLE_COUNT layout there is.
        let legal = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("wgpu e2e variable count"),
                entries: &[scalar, variable(ARRAY_BINDING)],
            })
            .expect("last in the slice and highest-numbered is the legal placement");
        device.destroy_bind_group_layout(legal);
    } else {
        println!(
            "wgpu e2e: this adapter has no descriptor indexing, so no VARIABLE_COUNT layout can \
             be created here for the ordering rule to be checked against; the feature refusal \
             above still ran"
        );
    }
    assert!(
        device.take_error().is_none(),
        "a layout this backend refuses must never have reached wgpu"
    );
    device.wait_idle().expect("idle");
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}

/// A [`BindGroupDesc::variable_count`](crcbl_hal::BindGroupDesc::variable_count)
/// the entries do not bear out is refused, by name.
///
/// The field was dropped here without a word. wgpu sizes a binding array from
/// the entry list itself and this backend's `update_bind_group` is
/// `Unsupported`, so an explicit `n` chooses no allocation — but an `n` that
/// disagrees with the entries is a caller bug either way, and a backend that
/// ignores the field reports one group for two different descriptors.
///
/// The first half runs on every adapter: a layout that declares no
/// `VARIABLE_COUNT` binding has nothing for the field to describe, and saying so
/// needs no descriptor indexing. Only the counting half needs a `VARIABLE_COUNT`
/// layout to fail against.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_variable_count_the_entries_contradict_is_refused_not_dropped() {
    let (instance, device, surface, _queue, _format) = Headless::open_device();
    let (images, views) = sampled_views(device.as_ref(), 1);
    let sampler = device
        .create_sampler(&crcbl_hal::SamplerDesc {
            label: Some("wgpu e2e variable count sampler"),
            ..crcbl_hal::SamplerDesc::default()
        })
        .expect("a filtering sampler");
    let sampler_entry = crcbl_hal::BindGroupEntry {
        binding: SCALAR_BINDING,
        array_index: 0,
        resource: crcbl_hal::BindingResource::Sampler(sampler),
    };
    let filled = [sampler_entry, view_entry(0, views[0])];

    let scalar = array_layout(device.as_ref(), 1);
    let no_array = refused(
        device.as_ref(),
        scalar,
        &filled,
        Some(2),
        "a variable count on a layout that declares no VARIABLE_COUNT binding",
    );
    for expected in ["variable_count", "no VARIABLE_COUNT binding"] {
        assert!(
            no_array.contains(expected),
            "{expected:?} missing from {no_array}"
        );
    }
    device.destroy_bind_group_layout(scalar);

    if device
        .caps()
        .features
        .contains(Features::DESCRIPTOR_INDEXING)
    {
        let entries = [
            crcbl_hal::BindGroupLayoutEntry {
                binding: SCALAR_BINDING,
                visibility: ShaderStages::FRAGMENT,
                kind: crcbl_hal::BindingKind::Sampler { comparison: false },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: ARRAY_BINDING,
                visibility: ShaderStages::FRAGMENT,
                kind: crcbl_hal::BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                count: ARRAY_COUNT,
                flags: crcbl_hal::BindingFlags::VARIABLE_COUNT
                    | crcbl_hal::BindingFlags::PARTIALLY_BOUND,
            },
        ];
        let layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("wgpu e2e variable count"),
                entries: &entries,
            })
            .expect("a VARIABLE_COUNT array on the last and highest binding");

        // One element supplied and one declared used: the count the entries
        // bear out, so the group builds. Without this the refusals below would
        // pass against a backend that refused the field outright.
        let group = device
            .create_bind_group(&crcbl_hal::BindGroupDesc {
                label: Some("wgpu e2e variable count agrees"),
                layout,
                entries: &filled,
                variable_count: Some(1),
            })
            .expect("a variable count the entries bear out");
        device.destroy_bind_group(group);

        // Inside the layout's ceiling and still a slot this group never fills.
        // Vulkan would allocate it and let `update_bind_group` write it later;
        // wgpu has neither the slot nor the update.
        let unfilled = refused(
            device.as_ref(),
            layout,
            &filled,
            Some(ARRAY_COUNT),
            "a variable count above what the entries fill",
        );
        for expected in ["variable_count is 2", "fills 1", "binding 1"] {
            assert!(
                unfilled.contains(expected),
                "{expected:?} missing from {unfilled}"
            );
        }

        // Past the layout's own ceiling, which the message names.
        let over = refused(
            device.as_ref(),
            layout,
            &filled,
            Some(ARRAY_COUNT + 1),
            "a variable count above the layout's declared count",
        );
        for expected in ["variable_count is 3", "binding 1's 2 elements"] {
            assert!(over.contains(expected), "{expected:?} missing from {over}");
        }

        // A group that names the variable binding nowhere fills none of it,
        // which is a disagreement like any other rather than a case with no
        // answer.
        let absent = refused(
            device.as_ref(),
            layout,
            &[sampler_entry],
            Some(1),
            "a variable count on a group that never names the variable binding",
        );
        for expected in ["variable_count is 1", "fills 0", "binding 1"] {
            assert!(
                absent.contains(expected),
                "{expected:?} missing from {absent}"
            );
        }
        device.destroy_bind_group_layout(layout);
    } else {
        println!(
            "wgpu e2e: this adapter has no descriptor indexing, so no VARIABLE_COUNT layout can \
             be created here for a variable count to be checked against; the scalar refusal above \
             still ran"
        );
    }

    assert!(
        device.take_error().is_none(),
        "a descriptor this backend refuses must never have reached wgpu"
    );
    device.destroy_sampler(sampler);
    for view in views {
        device.destroy_image_view(view);
    }
    for image in images {
        device.destroy_image(image);
    }
    device.wait_idle().expect("idle");
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}

/// A binding declaring no descriptors at all is refused, and the message names
/// it.
///
/// wgpu spells "no count" and "a count of one" the same way — `count: None` on
/// its layout entry — so a seam `count` of 0 arrived as an ordinary scalar
/// binding, and `binding::resolve` then clamped its capacity up to 1 and let one
/// element be written to a binding the layout had reserved nothing for. Every
/// other backend refuses it in these words; this one built it. Needs no optional
/// feature, so it runs on every adapter.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_binding_declaring_no_descriptors_is_refused_by_number() {
    let (instance, device, surface, _queue, _format) = Headless::open_device();

    let sized = |count| {
        [crcbl_hal::BindGroupLayoutEntry {
            binding: ARRAY_BINDING,
            visibility: ShaderStages::FRAGMENT,
            kind: crcbl_hal::BindingKind::Sampler { comparison: false },
            count,
            flags: crcbl_hal::BindingFlags::empty(),
        }]
    };
    let entries = sized(0);
    let error = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: Some("wgpu e2e empty binding"),
            entries: &entries,
        })
        .expect_err("a binding holding no descriptors must be refused, not built");
    assert!(
        matches!(error, HalError::InvalidDescriptor(_)),
        "a descriptor the seam forbids is InvalidDescriptor, got {error:?}"
    );
    let error = error.to_string();
    for expected in ["binding 1", "count 0"] {
        assert!(
            error.contains(expected),
            "{expected:?} missing from {error}"
        );
    }

    // The same binding with one descriptor, so the refusal is about the zero
    // and not about anything else in the entry.
    let entries = sized(1);
    let one = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: Some("wgpu e2e one descriptor"),
            entries: &entries,
        })
        .expect("one descriptor is the ordinary case");
    device.destroy_bind_group_layout(one);

    assert!(
        device.take_error().is_none(),
        "a layout this backend refuses must never have reached wgpu"
    );
    device.wait_idle().expect("idle");
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}

/// A layout **wgpu** rejects comes back as an `Err` from the call that made it,
/// not as a `take_error` a frame later.
///
/// `create_bind_group_layout` was the one creation call on this device left
/// unguarded. wgpu-core reports a rejected layout to the device's error handler
/// and still hands back an object, so the seam's caller got `Ok` and a poisoned
/// layout, and the first symptom was a validation failure in whichever pipeline
/// or bind group later named it — one call removed from the descriptor that
/// caused it. `Self::checked` is what attributes it, and the proof that it did
/// is that `take_error` afterwards has nothing left to report.
///
/// The unconditional half asks a device that never requested descriptor indexing
/// for an array binding, which needs wgpu's `TEXTURE_BINDING_ARRAY`: a refusal
/// this backend does not make itself, which is the whole point — it has to come
/// from wgpu to test the wrapper.
#[test]
#[ignore = "needs a real GPU; run tests/run-wgpu-e2e.sh"]
fn a_wgpu_bind_group_layout_wgpu_rejects_arrives_as_an_error_from_the_call() {
    let (instance, device, surface, _queue, _format) =
        Headless::open_device_with(Features::DEBUG_MARKERS);
    assert!(
        !device
            .caps()
            .features
            .contains(Features::DESCRIPTOR_INDEXING),
        "a device that never asked for descriptor indexing must not report it"
    );

    let entries = [crcbl_hal::BindGroupLayoutEntry {
        binding: ARRAY_BINDING,
        visibility: ShaderStages::FRAGMENT,
        kind: crcbl_hal::BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        },
        count: ARRAY_COUNT,
        flags: crcbl_hal::BindingFlags::empty(),
    }];
    let error = device
        .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
            label: Some("wgpu e2e array without the feature"),
            entries: &entries,
        })
        .expect_err("wgpu rejects an array binding without TEXTURE_BINDING_ARRAY");
    assert!(
        matches!(error, HalError::Backend(_)),
        "a refusal from wgpu itself is Backend, got {error:?}"
    );
    assert!(
        error.to_string().contains("create_bind_group_layout"),
        "the call that was refused must be named: {error}"
    );
    assert!(
        device.take_error().is_none(),
        "an error attributed to the call must not also be reported out of band"
    );
    device.wait_idle().expect("idle");
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);

    // The same proof on a *bindless* device, where the seam's own checks pass
    // and only wgpu can say no. A storage-buffer array needs wgpu's
    // `BUFFER_BINDING_ARRAY`, which `wgpu_features_for` never requests —
    // `DESCRIPTOR_INDEXING` buys the three texture-array features and nothing
    // else — so this is a layout the backend builds and wgpu rejects.
    //
    // Alongside it, the sentinel that used to be the refusal here: `u32::MAX`
    // is the seam's "as many as you can", and this backend now resolves it
    // against `max_bindless_descriptors` the way `crcbl-vk` does, so the
    // portable bindless declaration must *build* rather than come back as
    // "Too many bindings of type BindingArrayElements … count was 4294967295".
    let (instance, device, surface, _queue, _format) = Headless::open_device();
    if device
        .caps()
        .features
        .contains(Features::DESCRIPTOR_INDEXING)
    {
        let entries = [crcbl_hal::BindGroupLayoutEntry {
            binding: ARRAY_BINDING,
            visibility: ShaderStages::FRAGMENT,
            kind: crcbl_hal::BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            count: ARRAY_COUNT,
            flags: crcbl_hal::BindingFlags::empty(),
        }];
        let error = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("wgpu e2e buffer array without the feature"),
                entries: &entries,
            })
            .expect_err("wgpu rejects a buffer array without BUFFER_BINDING_ARRAY");
        assert!(
            matches!(error, HalError::Backend(_)),
            "a refusal from wgpu itself is Backend, got {error:?}"
        );
        assert!(
            error.to_string().contains("create_bind_group_layout"),
            "the call that was refused must be named: {error}"
        );
        assert!(
            device.take_error().is_none(),
            "an error attributed to the call must not also be reported out of band"
        );

        let ceiling = device.caps().limits.max_bindless_descriptors;
        assert!(
            ceiling > 0 && ceiling < u32::MAX,
            "a bindless device must report a ceiling for the sentinel to resolve against, got \
             {ceiling}"
        );
        let entries = [crcbl_hal::BindGroupLayoutEntry {
            binding: ARRAY_BINDING,
            visibility: ShaderStages::FRAGMENT,
            kind: crcbl_hal::BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            count: u32::MAX,
            flags: crcbl_hal::BindingFlags::VARIABLE_COUNT
                | crcbl_hal::BindingFlags::PARTIALLY_BOUND,
        }];
        let layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("wgpu e2e unbounded array"),
                entries: &entries,
            })
            .expect("the sentinel resolves to the device's own ceiling");
        assert!(
            device.take_error().is_none(),
            "a layout wgpu accepted must not have reported anything out of band"
        );
        device.destroy_bind_group_layout(layout);
        println!("wgpu e2e: the count sentinel resolved to {ceiling} and the layout built");
    } else {
        println!(
            "wgpu e2e: this adapter has no descriptor indexing, so neither the buffer array nor \
             the count sentinel has a bindless layout to appear in; the refusal above still ran"
        );
    }
    device.wait_idle().expect("idle");
    instance.destroy_surface(surface);
    drop(device);
    drop(instance);
}
