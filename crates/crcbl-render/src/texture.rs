//! One texture upload, for every format and every caller.
//!
//! ```text
//! pixels ──pad rows to the device's copy alignment──▶ staging buffer
//!            │
//!            └──barrier ▸ copy_buffer_to_image ▸ barrier──▶ sampled image + view
//! ```
//!
//! # One layer or several, one level or a chain
//!
//! [`upload_texture`] uploads a single-layer `D2` image, which is what a sprite
//! sheet and a glyph atlas are. [`upload_texture_layers`] uploads several layers
//! of the same size into one `D2Array` image, which is
//! `docs/plan/03-gpu-driven-rendering.md` §3.2's
//! [`ArrayPages`](crcbl_hal::BindingModel::ArrayPages) page: one image, one
//! descriptor, and a layer index in the material row selecting between them.
//! [`upload_texture_mip_layers`] is the same page with every layer's mip chain
//! behind it — `docs/plan/43-render-standards.md`'s filtering rung, whose chain
//! [`crate::mip`] builds on the host.
//!
//! All three go through the same body, and it records **one copy per level of
//! every layer** rather than one covering all of them. That is not caution: a
//! copy region's extent is a 2D extent on every backend here — `crcbl-dx12`
//! refuses a `depth_or_layers` other than 1 by name, because
//! `CopyTextureRegion` addresses one subresource — so the layer travels in
//! [`ImageSubresourceLayers::base_layer`], the level in
//! [`ImageSubresourceLayers::mip`], and the buffer offset says where that
//! subresource's rows start.
//!
//! This is a **startup** path, not a frame path: it records its own barriers
//! and blocks on [`Device::wait_idle`], which is only legal because no graph
//! exists yet. See this crate's docs on the one rule — the two staging uploads
//! ([`crate::forward`]'s cube and this) are its named exceptions.
//!
//! # Why it is not in `ui_pass`
//!
//! It was, and it was `R8_UNORM` only, with the glyph atlas's labels baked in.
//! The interesting part is that the old code was *right by coincidence*: it
//! computed a row pitch from `width`, called it `row_texels`, and handed it to
//! [`BufferImageCopy::buffer_row_length`] — which really is in texels, and
//! really did equal the byte count, because `R8Unorm` is one byte per texel.
//! For `Rgba8Unorm` the two differ by four, so the pitch here is computed in
//! **bytes** and converted back to texels exactly once, at the copy.

use crcbl_hal::{
    BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Device, Extent3d, Format,
    HalError, ImageAspect, ImageDesc, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange,
    ImageType, ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType, MemoryLocation, Offset3d,
    QueueHandle, ResourceState, SubmitInfo,
};

use crate::mip::level_extent;

/// A texture that has been uploaded: the image, and the view callers bind.
///
/// The two travel together because they die together, and every caller that
/// kept them as two loose handles has had to remember that on both its failure
/// path and its teardown path. [`UploadedTexture::destroy`] is the one place
/// the order lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadedTexture {
    /// The device-local image the copy filled.
    pub image: ImageHandle,
    /// A full-subresource colour view of [`UploadedTexture::image`]: `D2` from
    /// [`upload_texture`], `D2Array` from [`upload_texture_layers`] and
    /// [`upload_texture_mip_layers`].
    pub view: ImageViewHandle,
}

impl UploadedTexture {
    /// Releases the view and then the image. The device must be idle.
    pub fn destroy(&self, device: &dyn Device) {
        device.destroy_image_view(self.view);
        device.destroy_image(self.image);
    }
}

/// Uploads a 2D colour texture via a staging-buffer copy, and returns the image
/// and a view onto it.
///
/// `label` names the image and the view, and is the stem of the staging
/// buffer's and the encoder's names, so a capture shows which texture an
/// upload belonged to.
///
/// `pixels` must be exactly `width * height * texel_size` bytes, tightly
/// packed; the row padding the device's copy alignment wants is added here.
/// Padding rather than packing is what makes one upload path work on both
/// backends: Vulkan takes either, WebGPU requires a 256-byte row pitch and says
/// so through
/// [`optimal_buffer_copy_offset_alignment`](crcbl_hal::Limits::optimal_buffer_copy_offset_alignment).
///
/// Every object this creates is released on every path out, including the
/// failing ones: a `?` that dropped the staging buffer on the floor would leak
/// one per failed startup, and the recorder's leak assertions would only notice
/// once something actually failed.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] if `format` has no single colour plane, if
/// either extent is zero, or if `pixels` is not exactly the size the extent and
/// the format imply — the last of which used to be a `copy_from_slice` panic
/// naming neither number. [`HalError`] from any seam call otherwise; an extent
/// past the device's `max_image_2d` is the device's to refuse, not this
/// function's.
pub fn upload_texture(
    device: &dyn Device,
    queue: QueueHandle,
    label: &str,
    format: Format,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<UploadedTexture, HalError> {
    upload(
        device,
        queue,
        label,
        format,
        (width, height),
        &[&[pixels]],
        ImageViewType::D2,
    )
}

/// Uploads several equally-sized layers into one `D2Array` colour image, and
/// returns it with an array view onto every layer.
///
/// §3.2's [`ArrayPages`](crcbl_hal::BindingModel::ArrayPages) page: the index a
/// material row carries is a layer of the image this returns. One image rather
/// than one descriptor per texture is what makes the lookup need no
/// [`Features::DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING),
/// and therefore run on every backend the engine has — see
/// [`crate::forward`], which builds the page this crate's only caller binds.
///
/// Every slice in `layers` must be exactly one tightly packed
/// `width * height * texel_size` image, and they become layers 0, 1, … in the
/// order given. [`upload_texture`]'s account of the label, the row padding and
/// the release-on-every-path rule applies here unchanged. The image has one
/// mip level; [`upload_texture_mip_layers`] is the form that carries a chain.
///
/// # Errors
///
/// [`upload_texture`]'s errors, plus [`HalError::InvalidDescriptor`] when
/// `layers` is empty — a zero-layer image is not a page, and it reaches the
/// device as a zero extent whose complaint names a different thing. A layer
/// count past the device's `max_image_array_layers` is the device's to refuse.
pub fn upload_texture_layers(
    device: &dyn Device,
    queue: QueueHandle,
    label: &str,
    format: Format,
    width: u32,
    height: u32,
    layers: &[&[u8]],
) -> Result<UploadedTexture, HalError> {
    let chains: Vec<[&[u8]; 1]> = layers.iter().map(|pixels| [*pixels]).collect();
    let layers: Vec<&[&[u8]]> = chains.iter().map(|chain| &chain[..]).collect();
    upload(
        device,
        queue,
        label,
        format,
        (width, height),
        &layers,
        ImageViewType::D2Array,
    )
}

/// [`upload_texture_layers`] with a mip chain behind every layer: one
/// `D2Array` image whose level count is the chain's length, and an array view
/// onto every level of every layer.
///
/// `layers[layer][level]` is level `level` of layer `layer`, tightly packed at
/// that level's extent — `width` and `height` each halved per level and never
/// below one texel, which is [`crate::mip::level_extent`] and every backend's
/// own definition. Every layer carries the same number of levels, at least one
/// and at most the full chain
/// [`Extent3d::full_mip_levels`](crcbl_hal::Extent3d::full_mip_levels) counts;
/// a shorter chain is legal and leaves the sampler's `lod_max` to the caller.
///
/// # Errors
///
/// [`upload_texture_layers`]'s errors, plus [`HalError::InvalidDescriptor`]
/// when a layer has no levels, when two layers disagree on how many, when the
/// count exceeds the full chain, or when a level is not exactly its own
/// extent's worth of texels — each naming the layer and the level.
pub fn upload_texture_mip_layers(
    device: &dyn Device,
    queue: QueueHandle,
    label: &str,
    format: Format,
    width: u32,
    height: u32,
    layers: &[&[&[u8]]],
) -> Result<UploadedTexture, HalError> {
    upload(
        device,
        queue,
        label,
        format,
        (width, height),
        layers,
        ImageViewType::D2Array,
    )
}

/// The body the public uploads share: validate, stage every level of every
/// layer into one buffer, then hand over to [`upload_image`].
///
/// `extent` is `(width, height)` as one argument rather than two, which is what
/// keeps this inside the argument count the public wrappers already sit at.
fn upload(
    device: &dyn Device,
    queue: QueueHandle,
    label: &str,
    format: Format,
    extent: (u32, u32),
    layers: &[&[&[u8]]],
    view_type: ImageViewType,
) -> Result<UploadedTexture, HalError> {
    let (width, height) = extent;
    // `texel_size`, not `block_size`: it is the number a `BufferImageCopy` is
    // sized against, and it is `None` for exactly the formats this path cannot
    // describe — a combined depth/stencil format, whose two planes need two
    // copies and neither of which is a `COLOR` aspect.
    let texel = format.texel_size(ImageAspect::COLOR).ok_or_else(|| {
        HalError::InvalidDescriptor(format!(
            "{label}: {format:?} has no single colour plane, so it cannot be uploaded as one \
             colour-aspect copy"
        ))
    })?;
    // A compressed format's `block_size` covers a 4×4 block, not one texel, so
    // the per-texel sizing below would be wrong by a factor of four: a BC1 row
    // is `ceil(width / 4) × 8` bytes, not `width × 8`. No caller uploads one
    // today; refuse rather than corrupt.
    if format.is_compressed() {
        return Err(HalError::InvalidDescriptor(format!(
            "{label}: {format:?} is block-compressed and this path sizes per texel; \
             it cannot be uploaded as a colour-aspect copy"
        )));
    }
    if width == 0 || height == 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "{label}: texture extent {width}x{height} must be non-zero in both dimensions"
        )));
    }
    let layer_count = u32::try_from(layers.len()).unwrap_or(u32::MAX);
    if layer_count == 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "{label}: a texture needs at least one layer of pixels"
        )));
    }
    let mip_levels = u32::try_from(layers[0].len()).unwrap_or(u32::MAX);
    let full_chain = Extent3d::d2(width, height).full_mip_levels(ImageType::D2);
    if mip_levels == 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "{label}: layer 0 has no mip levels; a layer needs at least its own texels"
        )));
    }
    if mip_levels > full_chain {
        return Err(HalError::InvalidDescriptor(format!(
            "{label}: {mip_levels} mip levels for a {width}x{height} image, whose full chain \
             is {full_chain}"
        )));
    }

    let alignment = device
        .caps()
        .limits
        .optimal_buffer_copy_offset_alignment
        .max(1);

    // Every level of every layer is validated and staged in the order the
    // copies below read them: layer-major, level-minor. Each level's rows are a
    // multiple of the alignment, so stacking them puts every subresource's own
    // offset on it too — which is what lets each copy name one buffer offset.
    let mut padded = Vec::new();
    let mut regions = Vec::with_capacity(layers.len() * mip_levels as usize);
    for (layer, chain) in layers.iter().enumerate() {
        if chain.len() as u32 != mip_levels {
            return Err(HalError::InvalidDescriptor(format!(
                "{label}: layer {layer} carries {} mip levels and layer 0 carries {mip_levels}; \
                 every layer of one image has the same chain",
                chain.len()
            )));
        }
        for (level, pixels) in chain.iter().enumerate() {
            let level_width = level_extent(width, level as u32);
            let level_height = level_extent(height, level as u32);
            let row_bytes = u64::from(level_width) * u64::from(texel);
            let expected = row_bytes * u64::from(level_height);
            if pixels.len() as u64 != expected {
                return Err(HalError::InvalidDescriptor(format!(
                    "{label}: layer {layer} level {level} of a {width}x{height} {format:?} image \
                     is {level_width}x{level_height}, {expected} bytes ({texel} per texel), got {}",
                    pixels.len()
                )));
            }
            let row_pitch = padded_row_pitch(row_bytes, texel, alignment);
            let staged =
                stage_rows(row_bytes, level_height, row_pitch, pixels).ok_or_else(|| {
                    HalError::InvalidDescriptor(format!(
                        "{label}: a staging image of {} bytes and more does not fit in this \
                         host's address space",
                        padded.len()
                    ))
                })?;
            regions.push(StagedRegion {
                buffer_offset: padded.len() as u64,
                // The copy is in texels; the pitch above is in bytes.
                // `padded_row_pitch` guarantees the division is exact.
                row_texels: u32::try_from(row_pitch / u64::from(texel)).unwrap_or(u32::MAX),
                extent: Extent3d::d2(level_width, level_height),
                layer: layer as u32,
                mip: level as u32,
            });
            padded.extend_from_slice(&staged);
        }
    }

    let staging_label = format!("{label} staging");
    let staging = device.create_buffer(&BufferDesc {
        label: Some(&staging_label),
        size: padded.len() as u64,
        usage: BufferUsage::TRANSFER_SRC,
        memory: MemoryLocation::HostUpload,
    })?;
    let outcome = upload_image(
        device,
        queue,
        UploadArgs {
            label,
            format,
            width,
            height,
            layer_count,
            mip_levels,
            view_type,
            staging,
            regions: &regions,
        },
        &padded,
    );
    device.destroy_buffer(staging);
    outcome
}

/// The row pitch in **bytes**: at least `row_bytes`, a multiple of `alignment`,
/// and a whole number of texels.
///
/// The last of those is why this is not one `next_multiple_of` call. The copy
/// expresses its pitch in texels, so a byte pitch that is not divisible by the
/// texel size cannot be expressed at all. Every real device's alignment is a
/// power of two at least as large as any uncompressed texel, so in practice the
/// lcm *is* the alignment — this costs nothing and removes the assumption.
fn padded_row_pitch(row_bytes: u64, texel: u32, alignment: u64) -> u64 {
    let texel = u64::from(texel);
    let step = texel / gcd(texel, alignment) * alignment;
    row_bytes.next_multiple_of(step)
}

/// Greatest common divisor, for [`padded_row_pitch`]'s lcm.
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Copies `height` tightly-packed rows of `row_bytes` into a buffer whose rows
/// are `row_pitch` bytes apart, zero-filling the padding.
///
/// `None` only when the padded image does not fit in a `usize`, which on a
/// 64-bit host means the caller asked for more than the address space.
fn stage_rows(row_bytes: u64, height: u32, row_pitch: u64, pixels: &[u8]) -> Option<Vec<u8>> {
    let size = usize::try_from(row_pitch * u64::from(height)).ok()?;
    let row_bytes = usize::try_from(row_bytes).ok()?;
    let row_pitch = usize::try_from(row_pitch).ok()?;
    let mut padded = vec![0u8; size];
    for row in 0..height as usize {
        let src = row * row_bytes;
        let dst = row * row_pitch;
        padded[dst..dst + row_bytes].copy_from_slice(&pixels[src..src + row_bytes]);
    }
    Some(padded)
}

/// One subresource's place in the staging buffer: what its
/// [`BufferImageCopy`] names besides the buffer and the image.
struct StagedRegion {
    /// Where this subresource's first row starts, on the copy alignment.
    buffer_offset: u64,
    /// The padded row pitch, in texels — the unit
    /// [`BufferImageCopy::buffer_row_length`] documents.
    row_texels: u32,
    /// This level's own extent, halved from the image's per level.
    extent: Extent3d,
    layer: u32,
    mip: u32,
}

/// Everything [`upload_image`] needs that is not the device, the queue or the
/// bytes — one struct rather than seven positional arguments, which is what
/// `clippy::too_many_arguments` is objecting to and also what makes the two
/// `u32`s impossible to swap at the call site.
struct UploadArgs<'a> {
    label: &'a str,
    format: Format,
    width: u32,
    height: u32,
    /// Array layers in the image.
    layer_count: u32,
    /// Mip levels in the image, and in every layer's chain.
    mip_levels: u32,
    /// `D2` for one layer, `D2Array` for a page. Not derived from
    /// `layer_count`: a one-layer array view is a legitimate thing to want, and
    /// the shader's declaration is what decides which it is.
    view_type: ImageViewType,
    staging: crcbl_hal::BufferHandle,
    /// One copy per level of every layer, in staging order.
    regions: &'a [StagedRegion],
}

/// The half of [`upload_texture`] that owns the image and the view, so the
/// caller can destroy the staging buffer on every path out of it.
fn upload_image(
    device: &dyn Device,
    queue: QueueHandle,
    args: UploadArgs<'_>,
    pixels: &[u8],
) -> Result<UploadedTexture, HalError> {
    device.write_buffer(args.staging, 0, pixels)?;

    let image = device.create_image(&ImageDesc {
        label: Some(args.label),
        image_type: ImageType::D2,
        format: args.format,
        extent: Extent3d {
            width: args.width,
            height: args.height,
            // A `D2` image's `depth_or_layers` is its array length, which the
            // seam says on the field itself.
            depth_or_layers: args.layer_count,
        },
        mip_levels: args.mip_levels,
        samples: 1,
        // `TRANSFER_SRC` as well as the copy's `TRANSFER_DST`, so a test or a
        // capture can copy a level back out — `tests/forward_e2e/page.rs` reads
        // the page's chain that way. Free on every backend: a sampled image
        // that is also a copy source costs no layout, no memory and no
        // decompression it would not already pay.
        usage: ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
    })?;

    let view = match device.create_image_view(&ImageViewDesc {
        label: Some(args.label),
        image,
        view_type: args.view_type,
        format: args.format,
        range: color_range(args.mip_levels, args.layer_count),
    }) {
        Ok(view) => view,
        Err(error) => {
            device.destroy_image(image);
            return Err(error);
        }
    };

    let uploaded = UploadedTexture { image, view };
    match record_upload(device, queue, &args, image) {
        Ok(()) => Ok(uploaded),
        Err(error) => {
            uploaded.destroy(device);
            Err(error)
        }
    }
}

/// The whole of a colour image `mips` levels deep and `layers` layers deep.
const fn color_range(mips: u32, layers: u32) -> ImageSubresourceRange {
    ImageSubresourceRange {
        aspect: ImageAspect::COLOR,
        base_mip: 0,
        mip_count: mips,
        base_layer: 0,
        layer_count: layers,
    }
}

/// Records, submits and drains the staging copy, leaving the image in
/// [`ResourceState::ShaderRead`].
fn record_upload(
    device: &dyn Device,
    queue: QueueHandle,
    args: &UploadArgs<'_>,
    image: ImageHandle,
) -> Result<(), HalError> {
    let encoder_label = format!("{} upload", args.label);
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some(&encoder_label),
        queue,
    });

    let range = color_range(args.mip_levels, args.layer_count);
    // Undefined → TransferDst: the image has never held anything, so its old
    // contents are explicitly discarded rather than transitioned. The range
    // covers every level of every layer, so one barrier serves all the copies
    // below.
    encoder.pipeline_barrier(&crcbl_hal::Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            image,
            range,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Default::default()
    });

    // One copy per subresource. A region's `image_extent` is a 2D extent
    // whatever the image is — `crcbl-dx12` refuses anything else by name,
    // because `CopyTextureRegion` addresses one subresource — so the layer and
    // the level are named by `base_layer` and `mip`, and the buffer offset says
    // where that subresource's rows start.
    for region in args.regions {
        encoder.copy_buffer_to_image(&BufferImageCopy {
            buffer: args.staging,
            buffer_offset: region.buffer_offset,
            buffer_row_length: region.row_texels,
            buffer_image_height: region.extent.height,
            image,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: region.mip,
                base_layer: region.layer,
                layer_count: 1,
            },
            image_offset: Offset3d { x: 0, y: 0, z: 0 },
            image_extent: region.extent,
        });
    }

    encoder.pipeline_barrier(&crcbl_hal::Barriers {
        images: &[crcbl_hal::ImageBarrier::new(
            image,
            range,
            ResourceState::TransferDst,
            ResourceState::ShaderRead,
        )],
        ..Default::default()
    });

    let commands = encoder.finish()?;
    let submitted = device
        .submit(queue, &SubmitInfo::new(&[commands]))
        .and_then(|()| device.wait_idle());
    device.destroy_command_buffer(commands);
    submitted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{Command, Event, NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Features, Instance, QueueKind};

    /// Tier B, because its `optimal_buffer_copy_offset_alignment` is 256 —
    /// WebGPU's — and Tier A's is 4, which an `Rgba8Unorm` row satisfies for
    /// free. Padding is only exercised on the tier that asks for it.
    fn open_tier_b(recorder: &Recorder) -> (Box<dyn Device>, QueueHandle) {
        let instance = NullInstance::portable().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::COMPUTE,
                optional_features: Features::empty(),
                compatible_surface: None,
            })
            .expect("the tier B null adapter opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (device, queue)
    }

    fn open_tier_a(recorder: &Recorder) -> (Box<dyn Device>, QueueHandle) {
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::GPU_DRIVEN,
                optional_features: Features::empty(),
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (device, queue)
    }

    fn the_copies(recorder: &Recorder) -> Vec<BufferImageCopy> {
        recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::CopyBufferToImage(copy) => Some(copy),
                _ => None,
            })
            .collect()
    }

    fn the_copy(recorder: &Recorder) -> BufferImageCopy {
        let copies = the_copies(recorder);
        assert_eq!(
            copies.len(),
            1,
            "a single-layer upload records exactly one buffer-to-image copy"
        );
        copies[0]
    }

    fn bytes_written(recorder: &Recorder) -> usize {
        recorder
            .events()
            .into_iter()
            .find_map(|event| match event {
                Event::BufferWritten { len, .. } => Some(len),
                _ => None,
            })
            .expect("an upload writes its staging buffer once")
    }

    /// A 100-texel RGBA row: 400 bytes, which Tier B's 256-byte alignment pads
    /// to 512. Every number here is different from the one the old
    /// texels-as-bytes arithmetic would produce — that pitched 100 up to 256 —
    /// so this is what fails if a pitch is ever computed from `width` rather
    /// than `width * texel_size` again.
    const WIDTH: u32 = 100;
    const HEIGHT: u32 = 3;
    const ROW_BYTES: usize = 400;
    const PITCH: usize = 512;

    fn ramp(len: usize) -> Vec<u8> {
        // Coprime with 256, so no two bytes 256 apart are equal and a row
        // written at the wrong offset cannot compare equal by accident.
        (0..len).map(|index| (index % 251) as u8).collect()
    }

    /// The row pitch is in **bytes**, and each source row lands at its padded
    /// byte offset with zeroes after it.
    #[test]
    fn each_source_row_lands_at_its_padded_byte_offset() {
        let texel = Format::Rgba8Unorm
            .texel_size(ImageAspect::COLOR)
            .expect("rgba8 is a colour format");
        assert_eq!(texel, 4);

        let row_bytes = u64::from(WIDTH) * u64::from(texel);
        assert_eq!(row_bytes as usize, ROW_BYTES);
        let pitch = padded_row_pitch(row_bytes, texel, 256);
        assert_eq!(
            pitch as usize, PITCH,
            "400 bytes padded up to the 256-byte alignment is 512, not 256"
        );

        let pixels = ramp(ROW_BYTES * HEIGHT as usize);
        let padded = stage_rows(row_bytes, HEIGHT, pitch, &pixels).expect("fits");
        assert_eq!(padded.len(), PITCH * HEIGHT as usize);
        for row in 0..HEIGHT as usize {
            let start = row * PITCH;
            assert_eq!(
                &padded[start..start + ROW_BYTES],
                &pixels[row * ROW_BYTES..(row + 1) * ROW_BYTES],
                "row {row} must sit at byte {start}"
            );
            assert!(
                padded[start + ROW_BYTES..start + PITCH]
                    .iter()
                    .all(|&byte| byte == 0),
                "row {row}'s padding must be zero, not the next row"
            );
        }
    }

    /// And the copy is told that pitch in *texels*, which is the unit
    /// [`BufferImageCopy::buffer_row_length`] documents.
    #[test]
    fn an_rgba_upload_records_its_pitch_in_texels() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_b(&recorder);
        assert_eq!(
            device.caps().limits.optimal_buffer_copy_offset_alignment,
            256,
            "the padding this test exercises only exists on the tier that asks for it"
        );
        let pixels = ramp(ROW_BYTES * HEIGHT as usize);

        let texture = upload_texture(
            device.as_ref(),
            queue,
            "sprite atlas",
            Format::Rgba8Unorm,
            WIDTH,
            HEIGHT,
            &pixels,
        )
        .expect("the null backend accepts this");

        assert_eq!(
            bytes_written(&recorder),
            PITCH * HEIGHT as usize,
            "the staging write is the padded image, not the packed one"
        );
        let copy = the_copy(&recorder);
        assert_eq!(
            copy.buffer_row_length, 128,
            "512 bytes of pitch is 128 rgba texels, not 512"
        );
        assert_eq!(copy.buffer_image_height, HEIGHT);
        assert_eq!(copy.image_extent, Extent3d::d2(WIDTH, HEIGHT));
        assert_eq!(copy.image_offset, Offset3d { x: 0, y: 0, z: 0 });

        texture.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A device whose alignment an `Rgba8Unorm` row already satisfies pads
    /// nothing, and the copy's pitch is then simply the width.
    #[test]
    fn an_aligned_row_is_not_padded() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_a(&recorder);
        assert_eq!(device.caps().limits.optimal_buffer_copy_offset_alignment, 4);
        let pixels = vec![0u8; 5 * 3 * 4];
        let texture = upload_texture(
            device.as_ref(),
            queue,
            "tight",
            Format::Rgba8Unorm,
            5,
            3,
            &pixels,
        )
        .expect("accepted");
        assert_eq!(bytes_written(&recorder), 5 * 3 * 4);
        assert_eq!(the_copy(&recorder).buffer_row_length, 5);
        texture.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// The pitch satisfies the alignment *and* stays a whole number of texels,
    /// including when the two do not divide each other — a byte pitch the copy
    /// cannot express in texels is not a pitch.
    #[test]
    fn a_row_pitch_is_aligned_and_a_whole_number_of_texels() {
        // The ordinary cases: alignment already a multiple of the texel.
        assert_eq!(padded_row_pitch(20, 4, 256), 256);
        assert_eq!(padded_row_pitch(20, 4, 4), 20);
        assert_eq!(padded_row_pitch(5, 1, 4), 8);
        assert_eq!(padded_row_pitch(1024, 4, 256), 1024);
        // The awkward one: 6-byte alignment, 4-byte texel. 4 is not enough (it
        // is not a multiple of 6) and 6 is not enough (it is not a whole texel);
        // 12 is the first that is both.
        let pitch = padded_row_pitch(4, 4, 6);
        assert_eq!(pitch, 12);
        assert_eq!(pitch % 6, 0, "must satisfy the alignment");
        assert_eq!(pitch % 4, 0, "must be a whole number of texels");
    }

    /// **A page is one image whose layers each get their own copy**, and each
    /// copy reads the layer's own slice of the staging buffer.
    ///
    /// The offsets are what this is for. Every layer's rows are padded, so a
    /// second layer read at `width * height * texel` — the *unpadded* size —
    /// would take the tail of the first layer plus part of the second and put
    /// a plausible smear in the page. Tier B is used because its 256-byte
    /// alignment is what makes the padded and unpadded strides differ.
    #[test]
    fn a_page_copies_each_layer_from_its_own_padded_offset() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_b(&recorder);
        let layers: Vec<Vec<u8>> = (0..3).map(|_| ramp(ROW_BYTES * HEIGHT as usize)).collect();
        let refs: Vec<&[u8]> = layers.iter().map(Vec::as_slice).collect();

        let page = upload_texture_layers(
            device.as_ref(),
            queue,
            "material page",
            Format::Rgba8Unorm,
            WIDTH,
            HEIGHT,
            &refs,
        )
        .expect("the null backend accepts this");

        let layer_bytes = PITCH * HEIGHT as usize;
        assert_eq!(
            bytes_written(&recorder),
            layer_bytes * layers.len(),
            "the staging write is every layer, padded"
        );
        let copies = the_copies(&recorder);
        assert_eq!(
            copies.len(),
            layers.len(),
            "one copy per layer, because a region's extent is 2D on every backend"
        );
        for (layer, copy) in copies.iter().enumerate() {
            assert_eq!(
                copy.buffer_offset,
                (layer * layer_bytes) as u64,
                "layer {layer} must read from its own slice of the staging buffer"
            );
            assert_eq!(
                copy.image_subresource.base_layer, layer as u32,
                "layer {layer} must be written to layer {layer} of the image"
            );
            assert_eq!(copy.image_subresource.layer_count, 1);
            assert_eq!(copy.buffer_row_length, 128);
            assert_eq!(
                copy.image_extent,
                Extent3d::d2(WIDTH, HEIGHT),
                "a copy region's extent stays 2D"
            );
        }

        page.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A page with no layers is refused here, by name.
    ///
    /// Without the guard it reaches the device as an image whose
    /// `depth_or_layers` is zero, whose complaint is about an *extent* — a
    /// different object and a message that does not say what the caller did.
    #[test]
    fn a_page_with_no_layers_is_rejected() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_a(&recorder);
        let error = upload_texture_layers(
            device.as_ref(),
            queue,
            "empty page",
            Format::Rgba8Unorm,
            4,
            4,
            &[],
        )
        .expect_err("a page needs a layer");
        assert!(
            error.to_string().contains("at least one layer"),
            "got: {error}"
        );
        assert_eq!(recorder.total_live_objects(), 0);
    }

    /// A short or long slice is an error naming both numbers, not a
    /// `copy_from_slice` panic naming neither — and nothing is created before
    /// it is noticed.
    #[test]
    fn a_wrong_sized_pixel_slice_is_rejected() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_a(&recorder);
        let before = recorder.total_live_objects();

        let short = vec![0u8; 5 * 3 * 4 - 1];
        let error = upload_texture(
            device.as_ref(),
            queue,
            "short",
            Format::Rgba8Unorm,
            5,
            3,
            &short,
        )
        .expect_err("a 59-byte slice is not a 5x3 rgba image");
        let message = error.to_string();
        assert!(
            message.contains("60") && message.contains("59"),
            "the error must name both sizes, got: {message}"
        );

        let long = vec![0u8; 5 * 3 * 4 + 1];
        upload_texture(
            device.as_ref(),
            queue,
            "long",
            Format::Rgba8Unorm,
            5,
            3,
            &long,
        )
        .expect_err("a 61-byte slice is not one either");

        assert_eq!(
            recorder.total_live_objects(),
            before,
            "a rejected upload creates nothing"
        );
    }

    /// A zero extent is refused here rather than reaching the device as a
    /// zero-sized staging buffer, which is a different error about a different
    /// object.
    #[test]
    fn a_zero_extent_is_rejected() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_a(&recorder);
        for (width, height) in [(0u32, 4u32), (4, 0)] {
            let error = upload_texture(
                device.as_ref(),
                queue,
                "empty",
                Format::Rgba8Unorm,
                width,
                height,
                &[],
            )
            .expect_err("a zero extent is not a texture");
            // "texture extent", not merely "non-zero": without the guard above
            // this reaches the device as a zero-sized *buffer*, whose own error
            // also says "non-zero" and would have made this test pass while
            // naming the wrong object.
            assert!(
                error.to_string().contains("texture extent"),
                "got: {error}, which is not the extent complaint"
            );
        }
        assert_eq!(recorder.total_live_objects(), 0);
    }

    /// A combined depth/stencil format has two planes and no colour aspect, so
    /// there is no single `texel_size` to pitch against. Refused by name rather
    /// than silently pitched against `block_size`, which would be 8.
    #[test]
    fn a_depth_stencil_format_is_rejected() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_a(&recorder);
        let error = upload_texture(
            device.as_ref(),
            queue,
            "depth",
            Format::D32FloatS8Uint,
            4,
            4,
            &[0u8; 128],
        )
        .expect_err("a depth/stencil upload needs one copy per plane");
        assert!(error.to_string().contains("colour plane"), "got: {error}");
        assert_eq!(recorder.total_live_objects(), 0);
    }

    /// A compressed format's `block_size` covers a 4×4 block, not one texel, so
    /// the per-texel sizing here would be wrong by a factor of four — and there
    /// is no caller that needs one. Refused by name rather than silently
    /// pitched against the block size, before any device call.
    #[test]
    fn a_compressed_format_is_rejected() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_a(&recorder);
        // 8x8 BC1 is 2x2 blocks of 8 bytes: the *correct* compressed size, so a
        // regression that accepted compressed uploads would pass the size check
        // and fail only here.
        let error = upload_texture(
            device.as_ref(),
            queue,
            "compressed",
            Format::Bc1RgbaUnorm,
            8,
            8,
            &[0u8; 2 * 2 * 8],
        )
        .expect_err("a BC1 upload is sized per block, not per texel");
        assert!(
            error.to_string().contains("block-compressed"),
            "got: {error}"
        );
        assert_eq!(recorder.total_live_objects(), 0);
    }

    /// A failure after the staging buffer exists releases it. `create_image`
    /// is what fails: Tier A's `max_image_2d` is 16384, and a 16385x1 R8 image
    /// is one byte per texel, so the staging allocation this must clean up is
    /// 16 KiB rather than something that would fail for its own reasons first.
    #[test]
    fn a_failed_upload_leaks_nothing() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_a(&recorder);
        let limit = device.caps().limits.max_image_2d;
        assert_eq!(limit, 16384);
        let before = recorder.total_live_objects();

        let width = limit + 1;
        let pixels = vec![0u8; width as usize];
        let error = upload_texture(
            device.as_ref(),
            queue,
            "too wide",
            Format::R8Unorm,
            width,
            1,
            &pixels,
        )
        .expect_err("the device refuses an image past max_image_2d");
        assert!(
            error.to_string().contains("max_image_2d"),
            "the failure must be the image, not something earlier: {error}"
        );

        assert_eq!(
            recorder.total_live_objects(),
            before,
            "the staging buffer created before the failure must be destroyed"
        );
        assert!(
            recorder
                .events()
                .iter()
                .any(|event| matches!(event, Event::BufferWritten { .. })),
            "the test is only meaningful if the staging buffer really was created \
             and written before the failure"
        );
    }

    /// **A chain is one copy per level of every layer**, each into its own mip
    /// and each from its own padded offset — level-minor, so a layer's whole
    /// chain sits together in the staging buffer.
    ///
    /// Tier B again, for the padded-versus-unpadded reason above, and a width
    /// that halves onto rows of three different pitches: 100 texels pad to 512
    /// bytes, 50 to 256, 25 to 256. A level offset computed from level 0's
    /// pitch would land the second level's rows inside the first's tail.
    #[test]
    fn a_chain_copies_each_level_into_its_own_mip_from_its_own_offset() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_b(&recorder);
        // Three of the seven levels a 100-wide image has: a partial chain is
        // legal, and each level is its own extent's worth of texels.
        let level_extents = [(WIDTH, HEIGHT), (50, 1), (25, 1)];
        let chains: Vec<Vec<Vec<u8>>> = (0..2)
            .map(|_| {
                level_extents
                    .iter()
                    .map(|&(width, height)| ramp((width * height * 4) as usize))
                    .collect()
            })
            .collect();
        let levels: Vec<Vec<&[u8]>> = chains
            .iter()
            .map(|chain| chain.iter().map(Vec::as_slice).collect())
            .collect();
        let layers: Vec<&[&[u8]]> = levels.iter().map(Vec::as_slice).collect();

        let page = upload_texture_mip_layers(
            device.as_ref(),
            queue,
            "mipped page",
            Format::Rgba8Unorm,
            WIDTH,
            HEIGHT,
            &layers,
        )
        .expect("the null backend accepts a partial chain");

        let level_bytes = [PITCH * HEIGHT as usize, 256, 256];
        let layer_bytes: usize = level_bytes.iter().sum();
        assert_eq!(
            bytes_written(&recorder),
            layer_bytes * chains.len(),
            "the staging write is every level of every layer, each padded to its own pitch"
        );
        let copies = the_copies(&recorder);
        assert_eq!(copies.len(), chains.len() * level_extents.len());
        let mut expected_offset = 0u64;
        for (index, copy) in copies.iter().enumerate() {
            let layer = index / level_extents.len();
            let level = index % level_extents.len();
            let (width, height) = level_extents[level];
            assert_eq!(
                (
                    copy.image_subresource.base_layer,
                    copy.image_subresource.mip
                ),
                (layer as u32, level as u32),
                "copy {index} is layer {layer}'s level {level}"
            );
            assert_eq!(copy.image_subresource.layer_count, 1);
            assert_eq!(
                copy.buffer_offset, expected_offset,
                "layer {layer} level {level} reads from its own slice of the staging buffer"
            );
            assert_eq!(
                copy.image_extent,
                Extent3d::d2(width, height),
                "level {level} is copied at its own extent"
            );
            assert_eq!(copy.buffer_image_height, height);
            assert_eq!(
                copy.buffer_row_length as usize * 4,
                level_bytes[level] / height as usize,
                "level {level}'s pitch is its own row padded, in texels"
            );
            expected_offset += level_bytes[level] as u64;
        }

        page.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// The single-level form is the chain form with one level, so the image it
    /// creates has one mip and the copies name it — a `mip_levels` that
    /// followed the chain length would otherwise be off by one somewhere.
    #[test]
    fn a_single_level_page_is_a_chain_of_one() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_a(&recorder);
        let pixels = vec![0u8; 4 * 4 * 4];
        let page = upload_texture_layers(
            device.as_ref(),
            queue,
            "flat page",
            Format::Rgba8Unorm,
            4,
            4,
            &[&pixels],
        )
        .expect("accepted");
        let copy = the_copy(&recorder);
        assert_eq!(copy.image_subresource.mip, 0);
        page.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A chain that is ragged, too long, empty, or has a level of the wrong
    /// size is refused by name before anything is created.
    #[test]
    fn a_malformed_chain_is_rejected_naming_the_layer_and_the_level() {
        let recorder = Recorder::new();
        let (device, queue) = open_tier_a(&recorder);
        let before = recorder.total_live_objects();
        let level0 = vec![0u8; 4 * 4 * 4];
        let level1 = vec![0u8; 2 * 2 * 4];
        let level2 = vec![0u8; 4];
        let upload = |layers: &[&[&[u8]]]| {
            upload_texture_mip_layers(
                device.as_ref(),
                queue,
                "chain",
                Format::Rgba8Unorm,
                4,
                4,
                layers,
            )
            .expect_err("a malformed chain is not a page")
            .to_string()
        };

        let ragged = upload(&[&[&level0, &level1], &[&level0]]);
        assert!(
            ragged.contains("layer 1 carries 1 mip levels and layer 0 carries 2"),
            "got: {ragged}"
        );
        let long = upload(&[&[&level0, &level1, &level2, &level2]]);
        assert!(
            long.contains("4 mip levels") && long.contains("full chain is 3"),
            "got: {long}"
        );
        let empty = upload(&[&[]]);
        assert!(empty.contains("no mip levels"), "got: {empty}");
        let wrong = upload(&[&[&level0, &level0]]);
        assert!(
            wrong.contains("layer 0 level 1") && wrong.contains("is 2x2, 16 bytes"),
            "got: {wrong}"
        );

        assert_eq!(
            recorder.total_live_objects(),
            before,
            "a rejected chain creates nothing"
        );
    }
}
