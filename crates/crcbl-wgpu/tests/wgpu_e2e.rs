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
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, ClearValue, ColorAttachment,
    CommandEncoderDesc, CompositeAlpha, Device, DeviceDesc, DrawIndirect, Extent3d, Features,
    Format, HalError, ImageAspect, ImageDesc, ImageSubresourceLayers, ImageSubresourceRange,
    ImageType, ImageUsage, ImageViewDesc, ImageViewType, Instance, LoadOp, MemoryLocation,
    Offset3d, PipelineLayoutDesc, PresentInfo, PresentMode, PushConstantRange, QueueKind,
    ReadbackDesc, ReadbackState, Rect2d, RenderPassDesc, ResourceState, ShaderModuleDesc,
    ShaderStages, StoreOp, SubmitInfo, SurfaceError, SwapchainDesc,
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
