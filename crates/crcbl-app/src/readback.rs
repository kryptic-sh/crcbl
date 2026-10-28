//! GPU→CPU read-back of a presented image: the staging layout, the copy and
//! the compaction that drops its row padding.
//!
//! `Presenter::submit_present_and_read_back` is the whole of it for a
//! program: draw a frame as for the window, hand the command buffer to it
//! instead of to `submit_and_present`, and get the presented image back as an
//! [`Rgba8Frame`]. On a presenter from `Presenter::open_offscreen` that is an
//! image test of a program's own draw path with no display at all. Neither
//! exists on `wasm32`, whose readbacks are a promise.

use std::time::Duration;

use crcbl_hal::{
    Barriers, BufferDesc, BufferHandle, BufferImageCopy, BufferUsage, CommandEncoder, Device,
    Extent3d, Format, HalError, ImageAspect, ImageBarrier, ImageHandle, ImageSubresourceLayers,
    ImageSubresourceRange, MemoryLocation, Offset3d, ResourceState,
};

use crate::present::{FrameOutcome, PresentError};

/// How long a blocking read-back waits for its copy to land.
///
/// A hang detector, not a performance claim: a software rasteriser on a busy
/// CI runner can take seconds over one frame, and a copy that has not landed
/// by then is one that never will.
pub const READBACK_DEADLINE: Duration = Duration::from_secs(30);

/// A presented frame's pixels, four bytes each in R, G, B, A order, row-major
/// and top row first.
///
/// The bytes are the image's own, reordered and nothing else: an sRGB
/// swapchain format reads back sRGB-encoded, as it was shown, and the alpha is
/// whatever the frame's passes left — premultiplied where they blended
/// premultiplied colour over a transparent clear, as `crcbl-frame`'s UI pass
/// does for a window a compositor blends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba8Frame {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// The swapchain format the bytes were read from, which says whether they
    /// are sRGB-encoded.
    pub format: Format,
    /// `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

/// Why a frame could not be read back.
#[derive(Debug)]
#[non_exhaustive]
pub enum ReadbackError {
    /// Submitting or presenting the frame failed.
    Present(PresentError),
    /// Recording the copy or reading it failed.
    Hal(HalError),
    /// The swapchain's format is not four 8-bit channels.
    UnsupportedFormat(Format),
    /// The frame's byte count does not fit in host memory.
    TooLarge {
        /// Pixels across.
        width: u32,
        /// Pixels down.
        height: u32,
    },
    /// The copy had not landed after [`READBACK_DEADLINE`].
    Timeout(Duration),
    /// The frame was not presented, so there is no image to read: the
    /// swapchain went out of date, and has been rebuilt.
    NotPresented(FrameOutcome),
}

impl std::fmt::Display for ReadbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Present(error) => write!(f, "{error}"),
            Self::Hal(error) => write!(f, "{error}"),
            Self::UnsupportedFormat(format) => {
                write!(f, "a {format:?} swapchain cannot be read back as RGBA8")
            }
            Self::TooLarge { width, height } => {
                write!(f, "a {width}x{height} frame does not fit in host memory")
            }
            Self::Timeout(after) => write!(f, "the read-back had not landed after {after:?}"),
            Self::NotPresented(outcome) => {
                write!(
                    f,
                    "the frame was not presented ({outcome:?}), so nothing was read"
                )
            }
        }
    }
}

impl std::error::Error for ReadbackError {}

impl From<PresentError> for ReadbackError {
    fn from(error: PresentError) -> Self {
        Self::Present(error)
    }
}

impl From<HalError> for ReadbackError {
    fn from(error: HalError) -> Self {
        Self::Hal(error)
    }
}

/// Reorders `pixels`, read from an image in `format`, to R, G, B, A in place.
///
/// # Errors
///
/// [`ReadbackError::UnsupportedFormat`] for a format that is not four 8-bit
/// channels.
pub fn to_rgba8(format: Format, pixels: &mut [u8]) -> Result<(), ReadbackError> {
    match format {
        Format::Rgba8Unorm | Format::Rgba8UnormSrgb => Ok(()),
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            Ok(())
        }
        other => Err(ReadbackError::UnsupportedFormat(other)),
    }
}

/// Requests a read of `staging`, which a submitted copy is filling, and waits
/// up to [`READBACK_DEADLINE`] for it, returning the image with its row
/// padding dropped.
///
/// Blocking, so not on `wasm32`, whose readbacks are a promise.
///
/// # Errors
///
/// [`ReadbackError::Hal`] if the read failed, or [`ReadbackError::Timeout`].
/// On a timeout the read is left alone, since the GPU may still be writing it,
/// and goes with the device.
#[cfg(not(target_arch = "wasm32"))]
pub fn wait_for_readback(
    device: &dyn Device,
    staging: BufferHandle,
    layout: &ReadbackLayout,
    label: &str,
) -> Result<Vec<u8>, ReadbackError> {
    let readback = device.request_readback(&crcbl_hal::ReadbackDesc {
        label: Some(label),
        buffer: staging,
        offset: 0,
        size: layout.byte_count,
        after: None,
    })?;
    let mut staged = vec![0u8; layout.staged_capacity];
    let started = std::time::Instant::now();
    while !matches!(
        device.poll_readback(readback, &mut staged)?,
        crcbl_hal::ReadbackState::Ready
    ) {
        if started.elapsed() > READBACK_DEADLINE {
            return Err(ReadbackError::Timeout(READBACK_DEADLINE));
        }
        std::thread::yield_now();
    }
    device.destroy_readback(readback);
    Ok(compact_rows(
        &staged,
        layout.staged_pitch,
        layout.packed_pitch,
        layout.host_capacity,
    ))
}

/// Row-pitch alignment, in bytes, for the readback staging buffer.
///
/// **Not a performance hint — a portability requirement, and it outlived the
/// backend that exposed it.** WebGPU specifies `bytesPerRow` in *bytes* and
/// requires a multiple of 256 for any multi-row buffer↔image copy, so a tightly
/// packed readback — which is legal on Vulkan and is what this module used to
/// record — is invalid for every width that is not a multiple of 64.
/// `crcbl-webgpu` enforces it at the seam (see `command.rs`'s copy encoders and
/// `writer.rs`), so removing this padding breaks the browser backend.
///
/// It was found on `crcbl-wgpu`, which is deleted; the transcript is kept
/// because it is the clearest statement of the failure and no surviving native
/// backend can reproduce it — `webgpu` refuses to open on native, so there is no
/// command here that shows it any more:
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
/// The padding never reaches the caller — [`compact_rows`] drops it before
/// the pixels are handed over.
pub const READBACK_ROW_ALIGNMENT: u32 = 256;

/// The staging geometry of a readback of one four-bytes-per-texel image.
///
/// Two callers, which is why it is a type rather than six locals: the
/// `crcbl` crate's `OffscreenSetup::begin_readback`, and its
/// `GpuContext::submit_and_present` for `--screenshot`. Both have to pad to [`READBACK_ROW_ALIGNMENT`] and both
/// have to drop the padding again, and a second copy of that arithmetic is a
/// second chance to shear the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackLayout {
    /// The padded row pitch the copy writes, in bytes.
    pub staged_pitch: u64,
    /// The tight row pitch the caller gets back, in bytes.
    pub packed_pitch: u64,
    /// The whole staging buffer, in bytes.
    pub byte_count: u64,
    /// `byte_count` as a host length.
    pub staged_capacity: usize,
    /// The compacted image's length, in bytes.
    pub host_capacity: usize,
    /// `staged_pitch` in *texels*, which is the unit `BufferImageCopy` names it
    /// in.
    pub staged_row_texels: u32,
}

impl ReadbackLayout {
    /// The layout for an `extent`-sized frame, or `None` if it does not fit.
    ///
    /// `None` rather than an error because the two callers report the failure
    /// in their own vocabularies — `OffscreenError::TooLarge` here and a
    /// screenshot failure in the engine — and neither of them wants the
    /// other's type.
    ///
    /// Every step is checked: `u32 * u32 * 4` overflows a `u32`, and the
    /// product has to survive narrowing to a `usize` before it can size a
    /// staging buffer or a `Vec`.
    pub fn for_extent(extent: (u32, u32)) -> Option<Self> {
        let packed_pitch = u64::from(extent.0).checked_mul(4)?;
        let staged_pitch =
            packed_pitch.checked_next_multiple_of(u64::from(READBACK_ROW_ALIGNMENT))?;
        let byte_count = staged_pitch.checked_mul(u64::from(extent.1))?;
        let packed_bytes = packed_pitch.checked_mul(u64::from(extent.1))?;
        Some(Self {
            staged_pitch,
            packed_pitch,
            byte_count,
            staged_capacity: usize::try_from(byte_count).ok()?,
            host_capacity: usize::try_from(packed_bytes).ok()?,
            // `buffer_row_length` is in texels, and the padded pitch is a
            // multiple of 4 for every 4-byte format, so this division is exact.
            staged_row_texels: u32::try_from(staged_pitch / 4).ok()?,
        })
    }
}

/// Records the barrier → copy → barrier that reads a **presented** swapchain
/// image into a fresh host-visible buffer, and returns that buffer.
///
/// The caller owns the buffer from here: submit `encoder`, request a readback
/// over it, and destroy it once the copy has landed.
///
/// # Both ends of the barrier pair are `ResourceState::Present`
///
/// Not `ColorAttachment`. A render graph does not hand the image back in the
/// state its last pass left it in: `ForwardRenderer::present_target` declares
/// `final_state: Present`, and `CompiledGraph::execute` emits a trailing
/// barrier to reach it. So `Present` is what the image is in when the copy
/// starts, and declaring anything else is a lie the API checks — lavapipe's
/// validation layer reported this one as
/// `VUID-VkImageMemoryBarrier2-oldLayout-01197`, "cannot transition … from
/// VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL when the previous known layout is
/// VK_IMAGE_LAYOUT_PRESENT_SRC_KHR".
///
/// And the second barrier is why there is a pair at all. `present` takes the
/// image back into the ring, and the next trip round declares `Undefined` —
/// legal from any layout on Vulkan, but on D3D12 `Undefined` and `Present` are
/// both `COMMON` and the declared before-state is validated, so an image left
/// in `COPY_SOURCE` makes that next declaration false. `crcbl-dx12`'s own
/// offscreen-ring suite ends every frame with this same transition for that
/// reason.
///
/// # Errors
///
/// [`HalError`] if the staging buffer could not be
/// allocated.
pub fn record_image_readback(
    device: &dyn Device,
    encoder: &mut dyn CommandEncoder,
    image: ImageHandle,
    format: Format,
    extent: (u32, u32),
    layout: &ReadbackLayout,
    label: &str,
) -> Result<BufferHandle, HalError> {
    let staging = device.create_buffer(&BufferDesc {
        label: Some(label),
        size: layout.byte_count,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    })?;

    let range = ImageSubresourceRange::all(format);
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            image,
            range,
            ResourceState::Present,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });

    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: layout.staged_row_texels,
        buffer_image_height: 0,
        image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d::default(),
        image_extent: Extent3d::d2(extent.0, extent.1),
    });

    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            image,
            range,
            ResourceState::TransferSrc,
            ResourceState::Present,
        )],
        ..Barriers::default()
    });

    Ok(staging)
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
pub fn compact_rows(
    staged: &[u8],
    staged_pitch: u64,
    packed_pitch: u64,
    packed_len: usize,
) -> Vec<u8> {
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

    /// **BGRA is reordered to RGBA and RGBA is left alone**, and a format that
    /// is not four 8-bit channels is refused rather than misread.
    #[test]
    fn to_rgba8_reorders_only_bgra() {
        let mut bgra = vec![1, 2, 3, 4, 5, 6, 7, 8];
        to_rgba8(Format::Bgra8UnormSrgb, &mut bgra).expect("BGRA");
        assert_eq!(bgra, [3, 2, 1, 4, 7, 6, 5, 8]);
        let mut rgba = vec![1, 2, 3, 4];
        to_rgba8(Format::Rgba8Unorm, &mut rgba).expect("RGBA");
        assert_eq!(rgba, [1, 2, 3, 4]);
        assert!(matches!(
            to_rgba8(Format::Rgba16Float, &mut []),
            Err(ReadbackError::UnsupportedFormat(Format::Rgba16Float))
        ));
    }

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
}
