//! Offscreen render → readback → golden image — the `crcbl screenshot` path.
//!
//! Opens a GPU backend, creates an offscreen surface and swapchain, renders
//! one frame through [`ForwardRenderer`], and reads the pixels back.
//!
//! What comes back is the swapchain image's bytes *as they are in memory*, in
//! [`OffscreenSetup::format`]'s channel order — which on an ordinary desktop
//! surface is BGRA, not RGBA. Turning them into a `crcbl_golden::Image` is
//! the caller's job and needs `Image::from_readback` with the matching
//! `ChannelOrder`, not `Image::from_rgba8`; this module deliberately does not
//! guess, because the format is the thing it knows and the image type is not
//! its dependency to reach for.
//!
//! This module is the render half of the CLI subcommand; the CLI module
//! owns the argument parsing and I/O.
//!
//! # Native only
//!
//! The whole path blocks: [`crate::backend::open`], a blocking device creation,
//! and a `std::thread::sleep` poll loop waiting for the readback copy to land.
//! The browser's main thread may not block, so this module is
//! `#[cfg(not(target_arch = "wasm32"))]` in `lib.rs` and a wasm build that
//! reaches for it fails to *compile* rather than hanging on the first
//! screenshot — the rule [`crate::backend`] states and
//! [`Instance::create_device`] established.
//!
//! Nothing here is wanted in a browser at P5: it is a command-line tool's back
//! end, not something a game calls per frame. If it ever is, the polled shapes
//! it would have to be rewritten onto already exist —
//! [`Instance::request_device`] and
//! [`Device::poll_readback`].
//!
//! # Open gap: a screenshot does not say which adapter drew it
//!
//! This module picks `adapters().first()` and never reports which one that was,
//! and `crcbl screenshot` installs no logger, so the backends' own adapter lines
//! go nowhere. `run-vk-e2e.sh` and `run-wgpu-e2e.sh` both read that line out of
//! their suites precisely so a pinned ICD the loader ignored is caught; the
//! cross-backend harness (`tests/run-cross-backend-e2e.sh`) cannot, and pins by
//! manifest path alone. Recorded here rather than worked around: closing it
//! means either a `--json` field naming the adapter or a logger in the CLI, and
//! both are `crcbl-cli`'s call to make.

use std::time::Duration;

use crate::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Device, DeviceDesc,
    Extent3d, Features, Format, ImageAspect, ImageBarrier, ImageSubresourceLayers,
    ImageSubresourceRange, Instance, MemoryLocation, Offset3d, PresentInfo, PresentMode,
    QueueHandle, QueueKind, ReadbackDesc, ReadbackState, ResourceState, SubmitInfo, SurfaceError,
    SurfaceHandle, SurfaceTarget, SwapchainDesc, SwapchainHandle,
};
use crate::render::{
    Camera, DirectionalLight, ForwardRenderer, GraphError, Projection, RenderGraph, TransientPool,
};

// ---------------------------------------------------------------------------
// OffscreenSetup
// ---------------------------------------------------------------------------

/// The largest edge, in pixels, an offscreen frame may have.
///
/// 16384 is `maxImageDimension2D` on every implementation the engine targets,
/// so anything past it would be refused by swapchain creation anyway — but
/// only *after* this module had already tried to allocate a host-visible
/// staging buffer for it. `16384x16384` RGBA8 is one gibibyte, which is the
/// most this path will ever ask an allocator for.
///
/// The CLI checks `--size` against this at parse time so an absurd request is
/// a bad *invocation* (exit 2) rather than a failed command; [`OffscreenSetup::open`]
/// checks it again because a library may not have gone through the CLI.
pub const MAX_DIMENSION: u32 = 16_384;

/// Row-pitch alignment, in bytes, for the readback staging buffer.
///
/// **Not a performance hint — a portability requirement.** wgpu refuses a
/// multi-row buffer↔image copy whose row pitch is not a multiple of
/// `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` (256), so a tightly packed readback —
/// which is legal on Vulkan and is what this module used to record — fails on
/// `crcbl-wgpu` for every width that is not a multiple of 64:
///
/// ```text
/// $ CRCBL_GPU=wgpu crcbl screenshot --size 32x32 --output /tmp/x.png
/// crcbl: render/readback failed: HAL: invalid descriptor: a buffer↔image copy
///   of 32 texel(s) per row is 128 bytes, which wgpu requires to be a multiple
///   of 256
/// ```
///
/// Found by the cross-backend harness at P5.12, which compares this path's
/// output between the two backends at more than one frame size — `256x192`, the
/// only size anything had ever asked for, happens to be a multiple of 64 and hid
/// it. Vulkan imposes no such rule and pads harmlessly, so the padded pitch is
/// unconditional rather than a backend-specific branch: nothing above
/// `crcbl-hal` may key off which backend is behind the seam.
///
/// The padding never reaches the caller — [`OffscreenSetup::draw_and_readback`]
/// compacts the rows before returning.
pub const READBACK_ROW_ALIGNMENT: u32 = 256;

/// How long [`OffscreenSetup::draw_and_readback`] waits for the copy to land.
///
/// Generous because an offscreen ring on a software rasteriser can take
/// hundreds of milliseconds for a single frame. Public because the two error
/// paths that mention it are public, and a deadline a caller can be hit by is
/// one it should be able to read.
pub const READBACK_DEADLINE: Duration = Duration::from_secs(10);

/// Holds everything needed to render one frame offscreen: a GPU instance,
/// device, offscreen swapchain ring, and the forward renderer.
///
/// The caller drives one frame via [`Self::draw_and_readback`], then tears
/// down with [`Self::finish`].
#[allow(missing_debug_implementations)]
pub struct OffscreenSetup {
    instance: Box<dyn Instance>,
    device: Box<dyn Device>,
    surface: SurfaceHandle,
    swapchain: SwapchainHandle,
    queue: QueueHandle,
    format: Format,
    /// Where the camera is and how it projects.
    camera: Camera,
    light: DirectionalLight,
    renderer: ForwardRenderer,
    pool: TransientPool,
}

/// Reasons an offscreen render might fail before a pixel is written.
#[derive(Debug, thiserror::Error)]
pub enum OffscreenError {
    /// No GPU backend could be opened.
    #[error("GPU backend: {0}")]
    Backend(#[from] crate::backend::GpuError),

    /// No adapter, no queue, no format, or a surface-cap query failed.
    #[error("device not usable: {0}")]
    Unusable(&'static str),

    /// A HAL call failed.
    #[error("HAL: {0}")]
    Hal(#[from] crate::hal::HalError),

    /// A surface operation failed.
    #[error("surface: {0}")]
    Surface(#[from] crate::hal::SurfaceError),

    /// A graph compile or execute failed.
    #[error("graph: {0}")]
    Graph(#[from] GraphError),

    /// The swapchain went out of date before the first frame completed.
    #[error("offscreen swapchain is out of date")]
    OutOfDate,

    /// The requested frame is larger than [`MAX_DIMENSION`] on an edge, or its
    /// byte count does not fit this machine's address space.
    #[error("{width}x{height} is larger than the {MAX_DIMENSION}x{MAX_DIMENSION} offscreen limit")]
    TooLarge {
        /// Requested width, in pixels.
        width: u32,
        /// Requested height, in pixels.
        height: u32,
    },

    /// The readback did not land within [`READBACK_DEADLINE`].
    ///
    /// A `Result` rather than a panic because the CLI's contract is exit 1 with
    /// a `--json`-shaped message, and a `panic!` here aborted with exit 101
    /// past `report::emit` entirely.
    #[error("readback did not complete within {0:?}")]
    ReadbackTimeout(Duration),
}

impl OffscreenSetup {
    /// Opens the auto-selected GPU backend, creates an offscreen surface,
    /// adapter, device, swapchain, and forward renderer for a frame of
    /// `(width, height)` pixels.
    ///
    /// Returns `Err` if the frame is not between `1x1` and
    /// [`MAX_DIMENSION`]`x`[`MAX_DIMENSION`], if no GPU is available (lavapipe,
    /// swiftshader, or a real card), if the device is unusable, or if any HAL
    /// call fails.
    pub fn open(width: u32, height: u32) -> Result<Self, OffscreenError> {
        // Checked before the backend is opened, so an absurd `--size` costs a
        // comparison rather than a device.
        if width == 0 || height == 0 {
            return Err(OffscreenError::Unusable("a frame must be at least 1x1"));
        }
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(OffscreenError::TooLarge { width, height });
        }

        let extent = (width, height);
        let instance = crate::backend::open()?;

        let target = SurfaceTarget::Offscreen;
        // SAFETY: `Offscreen` names no platform object, so nothing can dangle.
        let surface = unsafe {
            instance
                .create_surface(&target)
                .map_err(OffscreenError::Hal)?
        };

        let adapters = instance.adapters();
        let adapter = adapters
            .first()
            .ok_or(OffscreenError::Unusable("no adapter"))?;

        let caps = instance
            .surface_caps(surface, adapter.id)
            .map_err(OffscreenError::Hal)?;

        let format = caps
            .preferred_format()
            .ok_or(OffscreenError::Unusable("no surface format"))?;

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("crcbl screenshot"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::TIER_A | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .map_err(OffscreenError::Hal)?;

        let queue = device
            .queue(QueueKind::Graphics)
            .ok_or(OffscreenError::Unusable("no graphics queue"))?;

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("screenshot ring"),
                surface,
                format,
                extent,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: crate::hal::CompositeAlpha::Opaque,
            })
            .map_err(OffscreenError::Surface)?;

        let renderer =
            ForwardRenderer::new(device.as_ref(), queue, format).map_err(OffscreenError::Hal)?;

        let camera = Camera::default().with_projection(Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        });

        Ok(Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
            camera,
            light: DirectionalLight::default(),
            renderer,
            pool: TransientPool::new(),
        })
    }

    /// The surface format the readback bytes are in.
    ///
    /// The swapchain's preferred format is `Bgra8UnormSrgb` on most surfaces,
    /// and [`Self::draw_and_readback`] copies the swapchain image *raw*. A
    /// caller turning those bytes into an image therefore has to know whether
    /// to swizzle red and blue, and this is how it knows. Feeding them to an
    /// RGBA constructor unconditionally produces a channel-swapped PNG on
    /// every ordinary desktop surface.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// Records, submits, and reads back one frame.
    ///
    /// Returns the swapchain image's bytes as `((width, height), Vec<u8>)`,
    /// four bytes per pixel, row-major, top row first, in [`Self::format`]'s
    /// channel order — **not** necessarily RGBA.
    ///
    /// The pose is fixed at `t = 0`: a screenshot is a golden-image input, and
    /// a deterministic frame is the only kind worth comparing against a
    /// reference. (There was an `advance`/`elapsed` pair here to move the
    /// clock; nothing ever called it, so every screenshot rendered `t = 0`
    /// anyway and the state only made the frame look configurable.)
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if recording, submission, or readback fail,
    /// [`OffscreenError::OutOfDate`] if the swapchain is stale,
    /// [`OffscreenError::TooLarge`] if the acquired extent's byte count does
    /// not fit in a `usize`, or [`OffscreenError::ReadbackTimeout`] if the copy
    /// has not landed after [`READBACK_DEADLINE`].
    pub fn draw_and_readback(&mut self) -> Result<((u32, u32), Vec<u8>), OffscreenError> {
        let device = self.device.as_ref();
        let acquired = device
            .acquire_next_frame(self.swapchain)
            .map_err(|error| match error {
                SurfaceError::OutOfDate => OffscreenError::OutOfDate,
                other => OffscreenError::Surface(other),
            })?;

        let extent = acquired.extent;
        // The extent comes back from the swapchain rather than from `open`, so
        // it is checked here too: `u32 * u32 * 4` overflows a `u32` and the
        // product has to survive narrowing to a `usize` before it can size a
        // staging buffer or a `Vec`.
        let too_large = || OffscreenError::TooLarge {
            width: extent.0,
            height: extent.1,
        };
        // The *staged* row pitch, padded to `READBACK_ROW_ALIGNMENT`; the
        // tightly packed pitch is what the caller gets back.
        let packed_pitch = u64::from(extent.0).checked_mul(4).ok_or_else(too_large)?;
        let staged_pitch = packed_pitch
            .checked_next_multiple_of(u64::from(READBACK_ROW_ALIGNMENT))
            .ok_or_else(too_large)?;
        let byte_count = staged_pitch
            .checked_mul(u64::from(extent.1))
            .ok_or_else(too_large)?;
        let packed_bytes = packed_pitch
            .checked_mul(u64::from(extent.1))
            .ok_or_else(too_large)?;
        let staged_capacity = usize::try_from(byte_count).map_err(|_| too_large())?;
        let host_capacity = usize::try_from(packed_bytes).map_err(|_| too_large())?;
        // `buffer_row_length` is in *texels*, and the padded pitch is a multiple
        // of 4 for every 4-byte format, so this division is exact.
        let staged_row_texels = u32::try_from(staged_pitch / 4).map_err(|_| too_large())?;

        // ---- render the frame through the graph ----

        self.renderer.begin_frame(
            device,
            &self.camera,
            &self.light,
            ForwardRenderer::spin(0.0),
            extent,
        )?;

        let compiled = {
            let mut graph = RenderGraph::new(self.queue);
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, self.format, extent),
            );
            let _hdr = self.renderer.add_passes(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("screenshot frame"),
            queue: self.queue,
        });
        compiled.execute(device, &mut self.pool, encoder.as_mut(), None)?;

        // ---- readback: barrier to TransferSrc, copy, barrier back, submit ----

        let staging = device.create_buffer(&BufferDesc {
            label: Some("screenshot readback"),
            size: byte_count,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })?;

        let range = ImageSubresourceRange::all(self.format);
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
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
            buffer_row_length: staged_row_texels,
            buffer_image_height: 0,
            image: acquired.image,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: Offset3d::default(),
            image_extent: Extent3d::d2(extent.0, extent.1),
        });

        let commands = encoder.finish()?;
        device.submit(self.queue, &SubmitInfo::new(&[commands]))?;
        device.present(
            self.queue,
            &PresentInfo {
                swapchain: self.swapchain,
                waits: acquired.present_semaphore.as_slice(),
            },
        )?;

        let readback = device.request_readback(&ReadbackDesc {
            label: Some("screenshot readback"),
            buffer: staging,
            offset: 0,
            size: byte_count,
            after: None,
        })?;

        let mut staged = vec![0u8; staged_capacity];

        let deadline = std::time::Instant::now() + READBACK_DEADLINE;
        loop {
            let state = device.poll_readback(readback, &mut staged)?;
            if let ReadbackState::Ready = state {
                break;
            }
            if std::time::Instant::now() > deadline {
                // The command buffer, staging buffer and readback are left
                // alone deliberately: the GPU may still be reading them, and
                // destroying them now would be worse than leaking them until
                // `finish` waits the device idle and drops it.
                return Err(OffscreenError::ReadbackTimeout(READBACK_DEADLINE));
            }
            std::thread::sleep(Duration::from_micros(100));
        }

        device.destroy_command_buffer(commands);
        device.destroy_buffer(staging);
        device.destroy_readback(readback);

        // Drop the row padding. Done here rather than left to the caller because
        // the pitch is this module's decision, and a caller that forgot it would
        // get a sheared image — the one failure a structural comparison sees and
        // a per-pixel one does not describe usefully.
        let pixels = compact_rows(&staged, staged_pitch, packed_pitch, host_capacity);

        Ok((extent, pixels))
    }

    /// Tears down in correct order: wait idle, destroy swapchain → surface →
    /// device.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if the device could not be brought to idle — a
    /// device lost during the frame surfaces here and nowhere else, and a
    /// caller about to save the pixels as a golden image needs to be told
    /// before it trusts them. The teardown still runs either way; the failure
    /// is reported after it.
    pub fn finish(self) -> Result<(), OffscreenError> {
        let idle = self.device.wait_idle();
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        drop(self.device);
        drop(self.instance);
        idle.map_err(OffscreenError::Hal)
    }
}

/// Copies `packed_pitch` bytes out of every `staged_pitch`-byte row.
///
/// A free function so the arithmetic is testable with no GPU: this is the step
/// that turns a padded readback back into an image, and getting it wrong shears
/// the frame by a few pixels per row — which looks like a rendering bug and is
/// not one.
///
/// A short final row is copied as far as it goes rather than dropped: a backend
/// that wrote less than it promised should produce a visibly truncated image,
/// not a panic in the middle of a screenshot.
fn compact_rows(staged: &[u8], staged_pitch: u64, packed_pitch: u64, packed_len: usize) -> Vec<u8> {
    if staged_pitch == packed_pitch {
        let end = packed_len.min(staged.len());
        return staged[..end].to_vec();
    }
    // Both pitches sized a `Vec` above, so both fit a `usize`.
    let staged_pitch = staged_pitch as usize;
    let packed_pitch = packed_pitch as usize;
    let mut packed = Vec::with_capacity(packed_len);
    let mut offset = 0usize;
    while offset < staged.len() && packed.len() < packed_len {
        let row_end = (offset + packed_pitch).min(staged.len());
        packed.extend_from_slice(&staged[offset..row_end]);
        offset += staged_pitch;
    }
    packed.truncate(packed_len);
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The padding is only correct if it is dropped again, and this is the
    /// arithmetic that drops it. A GPU is not needed to check it, so it is
    /// checked in the plain suite that runs everywhere rather than only in the
    /// e2e run that needs two backends.
    #[test]
    fn a_padded_readback_compacts_back_to_a_tight_image() {
        // Three rows of two RGBA pixels each, staged at a 16-byte pitch.
        let staged = vec![
            1, 2, 3, 4, 5, 6, 7, 8, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, //
            9, 10, 11, 12, 13, 14, 15, 16, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, //
            17, 18, 19, 20, 21, 22, 23, 24, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
        ];
        let packed = compact_rows(&staged, 16, 8, 24);
        assert_eq!(
            packed,
            (1u8..=24).collect::<Vec<u8>>(),
            "the padding bytes must not survive"
        );
    }

    /// The unpadded case is the one every existing caller was already on, and
    /// it must stay a straight copy.
    #[test]
    fn an_unpadded_readback_is_copied_verbatim() {
        let staged: Vec<u8> = (0u8..32).collect();
        assert_eq!(compact_rows(&staged, 8, 8, 32), staged);
        // A staging buffer larger than the image is truncated, never read past.
        assert_eq!(compact_rows(&staged, 8, 8, 16), staged[..16].to_vec());
    }

    /// A backend that returned a short buffer must produce a short image, not
    /// an out-of-range slice.
    #[test]
    fn a_truncated_readback_does_not_panic() {
        let staged: Vec<u8> = (0u8..20).collect();
        let packed = compact_rows(&staged, 16, 8, 24);
        assert_eq!(packed, vec![0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19]);
    }

    /// The pitch rule the wgpu backend enforces, stated as arithmetic: every
    /// width has to land on a multiple of 256 bytes.
    #[test]
    fn the_staged_pitch_is_always_a_legal_wgpu_row_pitch() {
        for width in [1u32, 3, 32, 63, 64, 97, 256, 1920, 4096] {
            let packed = u64::from(width) * 4;
            let staged = packed
                .checked_next_multiple_of(u64::from(READBACK_ROW_ALIGNMENT))
                .expect("no overflow at these widths");
            assert!(staged >= packed, "{width}: padding may not shrink a row");
            assert_eq!(
                staged % u64::from(READBACK_ROW_ALIGNMENT),
                0,
                "{width}: wgpu refuses this pitch",
            );
            assert_eq!(staged % 4, 0, "{width}: texel count must be exact");
        }
    }

    /// `--size 4000000000x4000000000` used to reach an unchecked
    /// `width * height * 4`, and `--size 100000x100000` a 40 GB allocation.
    /// Both are now refused, and refused *before* a GPU is opened, which is
    /// what makes this testable without one.
    #[test]
    fn an_absurd_frame_is_refused_before_a_backend_is_opened() {
        for (width, height) in [
            (u32::MAX, u32::MAX),
            (4_000_000_000, 4_000_000_000),
            (100_000, 100_000),
            (MAX_DIMENSION + 1, 1),
            (1, MAX_DIMENSION + 1),
        ] {
            let error = OffscreenSetup::open(width, height)
                .err()
                .unwrap_or_else(|| panic!("{width}x{height} is not a frame"));
            assert!(
                matches!(error, OffscreenError::TooLarge { .. }),
                "{width}x{height}: {error}"
            );
        }
    }

    /// A zero edge would make a swapchain nothing can present, and the error
    /// says so rather than being a validation-layer message later.
    #[test]
    fn a_zero_sized_frame_is_refused() {
        for (width, height) in [(0, 16), (16, 0), (0, 0)] {
            assert!(
                matches!(
                    OffscreenSetup::open(width, height),
                    Err(OffscreenError::Unusable(_))
                ),
                "{width}x{height} should be unusable"
            );
        }
    }

    /// The null backend exercises the same code paths but renders nothing.
    /// The test exists to prove the module compiles and the HAL seam is
    /// wired correctly; pixel content is tested by the Vulkan e2e suite.
    #[test]
    fn offscreen_setup_opens_with_null_backend() {
        // Force the null backend to avoid needing a real GPU.
        let instance = crate::backend::open_backend(crate::backend::GpuBackend::Null)
            .expect("null backend is always compiled in");

        let surface = unsafe {
            instance
                .create_surface(&SurfaceTarget::Offscreen)
                .expect("null accepts every surface")
        };
        let adapters = instance.adapters();
        let adapter = adapters
            .first()
            .expect("null backend always has an adapter");

        let caps = instance
            .surface_caps(surface, adapter.id)
            .expect("null reports surface caps");
        let format = caps.preferred_format().expect("null has a format");

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("screenshot test"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::TIER_A,
                compatible_surface: Some(surface),
            })
            .expect("null device");

        let queue = device
            .queue(QueueKind::Graphics)
            .expect("null has a graphics queue");

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("test ring"),
                surface,
                format,
                extent: (16, 16),
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: crate::hal::CompositeAlpha::Opaque,
            })
            .expect("null creates a swapchain");

        let mut renderer = ForwardRenderer::new(device.as_ref(), queue, format)
            .expect("null builds the forward renderer");

        // Acquire, record, submit — no readback on the null backend.
        let acquired = device
            .acquire_next_frame(swapchain)
            .expect("null ring has an image");

        let mut graph = RenderGraph::new(queue);
        let target = graph.import_image(
            "swapchain",
            ForwardRenderer::present_target(acquired.image, acquired.view, format, (16, 16)),
        );
        let pool = TransientPool::new();
        let _hdr = renderer.add_passes(&mut graph, target, (16, 16));

        let compiled = graph.compile(&pool).expect("graph compiles");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("test frame"),
            queue,
        });
        compiled
            .execute(
                device.as_ref(),
                &mut TransientPool::new(),
                encoder.as_mut(),
                None,
            )
            .expect("graph executes");
        let commands = encoder.finish().expect("encoding succeeds");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");

        device.destroy_command_buffer(commands);
        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
        drop(device);
        drop(instance);
    }
}
