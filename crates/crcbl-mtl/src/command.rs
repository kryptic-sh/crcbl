//! [`MetalCommandEncoder`]: real recording into an `MTLCommandBuffer` — render
//! passes, blit copies, and the encoder boundary a barrier becomes.
//!
//! # A render pass, not a blit clear
//!
//! `docs/plan/09-backends-metal-dx12.md`'s bring-up ladder starts at "clear",
//! and the cheap way to satisfy it would be a blit that fills a texture.
//! [`begin_render_pass`](CommandEncoder::begin_render_pass) instead builds a
//! real `MTLRenderPassDescriptor` and opens a real `MTLRenderCommandEncoder`,
//! so the clear happens because the pass *loads that way* — which is what
//! exercises the attachment table, the load and store actions and the clear
//! value, all of which the triangle rung is then built on top of. A pass opened
//! and immediately closed is a clear, and that is this slice's deliverable.
//!
//! # One `MTLCommandBuffer` per encoder, taken at construction
//!
//! Metal has no record-then-submit split: an `MTLCommandBuffer` comes from the
//! queue already recording, and `commit` *is* the submission. So the encoder
//! takes its command buffer in
//! [`Device::create_command_encoder`](crcbl_hal::Device::create_command_encoder)
//! and [`finish`](CommandEncoder::finish) hands it to the device's table rather
//! than building anything. That has one visible consequence the seam does not
//! otherwise imply: **the command buffer exists from the moment the encoder
//! does**, so a queue with its uncommitted-buffer limit reached blocks in
//! `create_command_encoder` rather than at submit.
//!
//! # Only one Metal encoder may be open at a time
//!
//! Metal raises if a second encoder is created while one is still encoding, and
//! an Objective-C exception crossing into Rust aborts the process. So this type
//! owns exactly one [`Open`] encoder and closes it before opening another:
//! copies open a blit encoder and keep it, a render pass closes the blit
//! encoder first, and [`finish`](CommandEncoder::finish) closes whatever is
//! left. A copy recorded *inside* a render pass is refused with
//! [`HalError::InvalidDescriptor`] rather than being allowed to raise — the
//! seam already forbids it ("copies, barriers and query writes are legal only
//! outside any pass"), so this is a caller bug being caught, not a restriction
//! this backend invents.
//!
//! # A barrier is an encoder boundary
//!
//! See [`pipeline_barrier`](CommandEncoder::pipeline_barrier). Metal tracks
//! hazards between encoders for tracked resources, which is every resource this
//! backend allocates, so the seam's barrier does not translate into a *call* —
//! it translates into the encoder split that makes Metal's tracking apply.
//!
//! # Failures are recorded and reported at `finish`
//!
//! Every recording method returns `()`, so a bad handle or an
//! impossible-to-express region has nowhere to go until
//! [`finish`](CommandEncoder::finish). The first failure is kept and every
//! later command dropped, exactly as `crcbl-vk` does, because a command buffer
//! that submits with commands silently missing is far worse than one that
//! refuses to be built.

use core::ops::Range;
use std::sync::Arc;

use crcbl_hal::{
    Barriers, BindGroupHandle, BufferCopy, BufferHandle, BufferImageCopy, CommandBufferHandle,
    CommandEncoder, CommandEncoderDesc, ComputePassDesc, ComputePipelineHandle, DrawIndirect,
    DrawIndirectCount, GraphicsPipelineHandle, HalError, ImageCopy, ImageSubresourceLayers,
    ImageType, IndexFormat, PipelineLayoutHandle, QuerySetHandle, Rect2d, RenderPassDesc,
    ShaderStages, Viewport,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSRange, NSString, NSUInteger};
use objc2_metal::{
    MTLBlitCommandEncoder, MTLBuffer, MTLCommandBuffer, MTLCommandEncoder as _, MTLCommandQueue,
    MTLOrigin, MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLScissorRect, MTLTexture,
    MTLViewport,
};

use crate::conv;
use crate::device::{CommandBufferEntry, DeviceInner, ResolvedImage, to_ns};

/// The one Metal encoder that may be open on the command buffer.
enum Open {
    /// Nothing is encoding; the command buffer will accept a new encoder.
    None,
    /// A blit encoder, kept open across consecutive copies.
    Blit(Retained<ProtocolObject<dyn MTLBlitCommandEncoder>>),
    /// A render pass.
    Render(Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>),
}

impl Open {
    /// The open encoder as the base protocol, for the calls both share.
    fn as_encoder(&self) -> Option<&ProtocolObject<dyn objc2_metal::MTLCommandEncoder>> {
        match self {
            Self::None => None,
            Self::Blit(encoder) => Some(ProtocolObject::from_ref(&**encoder)),
            Self::Render(encoder) => Some(ProtocolObject::from_ref(&**encoder)),
        }
    }
}

/// Where a debug group was pushed, so it is popped in the same place.
struct Label {
    /// `true` if the group went on a Metal encoder rather than on the command
    /// buffer. An encoder's groups die with `endEncoding`, so one whose encoder
    /// has since closed is dropped rather than popped off its successor — which
    /// would fold every later command into the wrong region of the capture
    /// tree, the same failure `crcbl-vk`'s `render_pass_label` exists to avoid.
    on_encoder: bool,
    /// Which encoder it went on. Compared against
    /// [`MetalCommandEncoder::epoch`].
    epoch: u64,
}

/// Records into one `MTLCommandBuffer`.
pub(crate) struct MetalCommandEncoder {
    device: Arc<DeviceInner>,
    /// `None` only when `MTLCommandQueue::commandBuffer` returned nil, which is
    /// recorded in `failed` at the same moment.
    raw: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    open: Open,
    /// Bumped every time an encoder is opened, so a debug group pushed on one
    /// encoder is never popped on the next.
    epoch: u64,
    /// The first failure, reported by `finish`. Every later command is dropped.
    failed: Option<HalError>,
    /// Whether the seam thinks a render or compute pass is open.
    ///
    /// `in_compute_pass` has no Metal object behind it: a compute pass with no
    /// pipeline can dispatch nothing, so this slice tracks the scope and
    /// creates no `MTLComputeCommandEncoder`. See `begin_compute_pass`.
    in_render_pass: bool,
    in_compute_pass: bool,
    /// Open debug groups, innermost last.
    labels: Vec<Label>,
    /// Whether the open render pass pushed a label of its own.
    render_pass_label: bool,
}

impl core::fmt::Debug for MetalCommandEncoder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MetalCommandEncoder")
            .field("failed", &self.failed.is_some())
            .field("in_render_pass", &self.in_render_pass)
            .finish_non_exhaustive()
    }
}

// SAFETY: `MTLCommandBuffer` and the encoder protocols are the objects
// `objc2-metal` deliberately leaves unmarked — Apple documents a command buffer
// and its encoders as usable from one thread at a time and *not* safe to encode
// into concurrently — and the seam requires this type to be `Send + Sync`
// through `HalThreadSafe`. Both halves are discharged by the trait's own shape
// rather than by a promise:
//
// * **Every recording method takes `&mut self`.** Two threads cannot hold `&mut`
//   to one encoder, so "one thread at a time" is enforced by the borrow checker,
//   which is exactly the discharge `crcbl_hal::command` describes for Vulkan's
//   identical external-synchronisation rule.
// * **`&self` can do nothing to the Metal objects.** The only method taking
//   `&self` is `Debug::fmt`, which reads two Rust-side flags and touches no
//   Objective-C object at all, so sharing a reference across threads exposes no
//   Metal call.
// * Retain and release are atomic in the Objective-C runtime, so moving the
//   `Retained` pointers between threads and dropping them on another is sound
//   on its own.
unsafe impl Send for MetalCommandEncoder {}
// SAFETY: as above.
unsafe impl Sync for MetalCommandEncoder {}

impl MetalCommandEncoder {
    pub(crate) fn new(device: Arc<DeviceInner>, desc: &CommandEncoderDesc<'_>) -> Self {
        let mut encoder = Self {
            device,
            raw: None,
            open: Open::None,
            epoch: 0,
            failed: None,
            in_render_pass: false,
            in_compute_pass: false,
            labels: Vec::new(),
            render_pass_label: false,
        };
        // The queue is checked before the command buffer is taken, so a handle
        // belonging to another device is reported as the crossing it is rather
        // than being answered with a working encoder on the wrong device.
        if let Err(error) = encoder.device.check_queue(desc.queue) {
            encoder.failed = Some(error);
            return encoder;
        }
        let Some(raw) = encoder.device.queue.commandBuffer() else {
            encoder.failed = Some(HalError::DeviceLost(
                "MTLCommandQueue::commandBuffer returned nil".to_string(),
            ));
            return encoder;
        };
        if let Some(label) = desc.label {
            raw.setLabel(Some(&NSString::from_str(label)));
        }
        encoder.raw = Some(raw);
        encoder
    }

    /// Whether *new* commands may be recorded.
    fn ok(&self) -> bool {
        self.failed.is_none() && self.raw.is_some()
    }

    /// Records the first failure and drops every later command.
    fn fail(&mut self, error: HalError) {
        if self.failed.is_none() {
            log::error!("crcbl-mtl: command recording failed: {error}");
            self.failed = Some(error);
        }
    }

    /// Closes whatever encoder is open, leaving the command buffer ready to
    /// accept another.
    ///
    /// Called before every encoder change *and* by `finish`, because
    /// `MTLCommandBuffer::commit` with an encoder still encoding raises.
    fn close_open(&mut self) {
        if let Some(encoder) = self.open.as_encoder() {
            encoder.endEncoding();
        }
        self.open = Open::None;
    }

    /// The blit encoder, opening one if the command buffer has none.
    ///
    /// `None` means the caller has already been told why — either recording had
    /// failed, or a render pass is open and the seam forbids a copy there.
    fn blit(&mut self) -> Option<Retained<ProtocolObject<dyn MTLBlitCommandEncoder>>> {
        if !self.ok() {
            return None;
        }
        if self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "a copy inside a render pass: the seam places copies between passes, and Metal \
                 raises rather than accepting a second encoder"
                    .to_string(),
            ));
            return None;
        }
        if let Open::Blit(encoder) = &self.open {
            return Some(encoder.clone());
        }
        self.close_open();
        let raw = self.raw.as_ref()?;
        let Some(encoder) = raw.blitCommandEncoder() else {
            self.fail(HalError::DeviceLost(
                "MTLCommandBuffer::blitCommandEncoder returned nil".to_string(),
            ));
            return None;
        };
        self.epoch += 1;
        self.open = Open::Blit(encoder.clone());
        Some(encoder)
    }

    /// Resolves a buffer handle, recording the failure if it does not.
    fn buffer(&mut self, handle: BufferHandle) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        let resolved = self.device.buffer_raw(handle);
        match resolved {
            Ok(raw) => Some(raw),
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    /// Resolves an image handle to its texture, seam format and dimensionality.
    fn image(&mut self, handle: crcbl_hal::ImageHandle) -> Option<ResolvedImage> {
        match self.device.image_raw(handle) {
            Ok(resolved) => Some(resolved),
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    /// Checks a copy's subresource against the texture it names, and returns
    /// the slice range to loop over.
    ///
    /// Metal's buffer↔texture and texture↔texture copies each move **one
    /// slice**, where the seam's [`ImageSubresourceLayers`] may name several, so
    /// every such copy is a loop and this is what bounds it. Metal does not
    /// bounds-check the slice or level, so an out-of-range one raises; refusing
    /// here keeps it an error the caller can catch.
    fn slices(
        &mut self,
        texture: &ProtocolObject<dyn MTLTexture>,
        subresource: ImageSubresourceLayers,
        what: &str,
    ) -> Option<Range<NSUInteger>> {
        let levels = texture.mipmapLevelCount();
        let slices = texture.arrayLength();
        let base = to_ns(u64::from(subresource.base_layer));
        let count = to_ns(u64::from(subresource.layer_count));
        if to_ns(u64::from(subresource.mip)) >= levels {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} names mip {} of an image with {levels}",
                subresource.mip
            )));
            return None;
        }
        if count == 0 || base.checked_add(count).is_none_or(|end| end > slices) {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} names layers {base}..{} of an image with {slices}",
                base.saturating_add(count)
            )));
            return None;
        }
        Some(base..base + count)
    }
}

impl CommandEncoder for MetalCommandEncoder {
    // --- debug ---

    /// Opens a named region, on whichever object is currently encoding.
    ///
    /// Metal has two debug-group stacks rather than one:
    /// `MTLCommandBuffer::pushDebugGroup:` for the space between encoders and
    /// `MTLCommandEncoder::pushDebugGroup:` for the space inside one. A group
    /// pushed on an encoder ends when that encoder does, whatever this trait's
    /// caller intended, so [`Label`] records which stack each push went on and
    /// `end_debug_label` pops it there — never on the encoder that happens to
    /// be open by then.
    fn begin_debug_label(&mut self, label: &str) {
        if !self.ok() {
            return;
        }
        let name = NSString::from_str(label);
        let on_encoder = if let Some(encoder) = self.open.as_encoder() {
            encoder.pushDebugGroup(&name);
            true
        } else {
            let Some(raw) = self.raw.as_ref() else {
                return;
            };
            raw.pushDebugGroup(&name);
            false
        };
        self.labels.push(Label {
            on_encoder,
            epoch: self.epoch,
        });
    }

    fn end_debug_label(&mut self) {
        // Popped whether or not recording has failed: `finish` drains this
        // stack, and a push that is never matched leaves the capture tool's
        // tree open around every later command.
        let Some(label) = self.labels.pop() else {
            return;
        };
        if !label.on_encoder {
            if let Some(raw) = self.raw.as_ref() {
                raw.popDebugGroup();
            }
            return;
        }
        // Only if the same encoder is still open; otherwise `endEncoding`
        // already closed the group.
        if label.epoch == self.epoch
            && let Some(encoder) = self.open.as_encoder()
        {
            encoder.popDebugGroup();
        }
    }

    /// Inserts a zero-width marker.
    ///
    /// Metal puts `insertDebugSignpost:` on the *encoder* and gives the command
    /// buffer no equivalent, so a marker recorded between encoders has nowhere
    /// to go and is dropped — which is what the seam permits for debug
    /// instrumentation ("accepted and dropped … degrades, it does not break").
    fn insert_debug_marker(&mut self, label: &str) {
        if !self.ok() {
            return;
        }
        if let Some(encoder) = self.open.as_encoder() {
            encoder.insertDebugSignpost(&NSString::from_str(label));
        }
    }

    // --- sync ---

    /// Ends the open encoder, which is what a barrier *is* on Metal.
    ///
    /// # The decision, and what would make it wrong
    ///
    /// Metal has no `vkCmdPipelineBarrier`. What it has is **automatic hazard
    /// tracking between encoders**: for a resource whose
    /// `MTLResource::hazardTrackingMode` is
    /// [`Tracked`](objc2_metal::MTLHazardTrackingMode::Tracked), the driver
    /// inserts the dependency itself when one encoder writes what a later
    /// encoder reads. Every resource this backend allocates is tracked —
    /// `newBufferWithLength:options:` and `newTextureWithDescriptor:` default to
    /// it, and `conv::resource_options` never sets
    /// `MTLResourceHazardTrackingModeUntracked`, which
    /// `every_resource_is_hazard_tracked` asserts on real objects rather than on
    /// paper.
    ///
    /// So the translation of [`Barriers`] is not a call. It is the **encoder
    /// boundary** that makes the tracking apply, and this closes the open blit
    /// encoder to create one. Leaving the encoder open instead would put two
    /// copies with a hazard between them inside a single blit encoder, which is
    /// precisely the case the inter-encoder tracking does not cover.
    ///
    /// The states themselves are read and dropped, and that is correct rather
    /// than lazy: Metal has no image layouts, so [`ResourceState`] has no
    /// per-resource expansion to make, and the seam's own module docs say a
    /// backend expands a state into "a Vulkan `sync2` triple, a DX12 state, **a
    /// Metal fence, or nothing at all**".
    ///
    /// **Three things would break this**, and all three are additions rather
    /// than accidents:
    ///
    /// 1. **Sub-allocating from an `MTLHeap`.** Heap resources are
    ///    `MTLHazardTrackingModeUntracked` by default, and Metal inserts
    ///    nothing for them. That is the residency work
    ///    `docs/plan/09-backends-metal-dx12.md` puts in the bindless slice, and
    ///    it is the slice that must turn this into `MTLFence`
    ///    `updateFence:`/`waitForFence:` pairs.
    /// 2. **`MTLParallelRenderCommandEncoder`.** Its sub-encoders are not
    ///    ordered against one another, so "between encoders" stops meaning
    ///    "before and after".
    /// 3. **A barrier inside a pass.** Metal's within-encoder ordering needs
    ///    `memoryBarrierWithScope:` instead, and this method cannot reach for it
    ///    because it does not know it is inside one. The seam forbids the case
    ///    outright, [`crcbl_hal::null`] fails the render graph's own suite on
    ///    it, and this backend leaves the render encoder alone rather than
    ///    guessing.
    ///
    /// [`ResourceState`]: crcbl_hal::ResourceState
    fn pipeline_barrier(&mut self, _barriers: &Barriers<'_>) {
        if !self.ok() || self.in_render_pass {
            return;
        }
        self.close_open();
    }

    // --- copies ---

    fn copy_buffer_to_buffer(&mut self, copy: &BufferCopy) {
        let (Some(src), Some(dst)) = (self.buffer(copy.src), self.buffer(copy.dst)) else {
            return;
        };
        if copy.size == 0 {
            return;
        }
        // Metal does not bounds-check either end, so an overrun raises instead
        // of returning an error. Both are checked against the allocation's own
        // length, read off the object.
        for (buffer, offset, what) in [
            (&src, copy.src_offset, "source"),
            (&dst, copy.dst_offset, "destination"),
        ] {
            let end = offset.checked_add(copy.size);
            if end.is_none_or(|end| end > buffer.length() as u64) {
                self.fail(HalError::InvalidDescriptor(format!(
                    "copy_buffer_to_buffer {what} range {offset}..{} exceeds the buffer's {} bytes",
                    offset.saturating_add(copy.size),
                    buffer.length()
                )));
                return;
            }
        }
        let Some(encoder) = self.blit() else {
            return;
        };
        // SAFETY: `objc2` marks this unsafe because Metal bounds-checks neither
        // offset nor size. Both ranges were just checked against the two
        // buffers' own `length()` immediately above, and both objects are kept
        // alive by the `Retained` held across the call.
        unsafe {
            encoder.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                &src,
                to_ns(copy.src_offset),
                &dst,
                to_ns(copy.dst_offset),
                to_ns(copy.size),
            );
        }
    }

    fn copy_buffer_to_image(&mut self, copy: &BufferImageCopy) {
        let Some(plan) = self.plan_buffer_image_copy(copy, "copy_buffer_to_image") else {
            return;
        };
        let Some(encoder) = self.blit() else {
            return;
        };
        for (index, slice) in plan.slices.clone().enumerate() {
            let offset = plan.buffer_offset + index as u64 * plan.footprint.bytes_per_image;
            // SAFETY: `objc2` marks this unsafe because Metal bounds-checks
            // neither the buffer range nor the region. `plan_buffer_image_copy`
            // checked the region against the texture's own extent at this mip
            // level, the slice against its `arrayLength`, and the whole buffer
            // span against the allocation's `length()`; both objects are kept
            // alive by the `Retained`s held across the call.
            unsafe {
                encoder
                    .copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                        &plan.buffer,
                        to_ns(offset),
                        to_ns(plan.footprint.bytes_per_row),
                        to_ns(plan.slice_stride),
                        plan.size,
                        &plan.texture,
                        slice,
                        to_ns(u64::from(copy.image_subresource.mip)),
                        plan.origin,
                    );
            }
        }
    }

    fn copy_image_to_buffer(&mut self, copy: &BufferImageCopy) {
        let Some(plan) = self.plan_buffer_image_copy(copy, "copy_image_to_buffer") else {
            return;
        };
        let Some(encoder) = self.blit() else {
            return;
        };
        for (index, slice) in plan.slices.clone().enumerate() {
            let offset = plan.buffer_offset + index as u64 * plan.footprint.bytes_per_image;
            // SAFETY: as `copy_buffer_to_image`, with the two ends swapped —
            // the same `plan_buffer_image_copy` checked the same region, slice
            // range and buffer span.
            unsafe {
                encoder
                    .copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                        &plan.texture,
                        slice,
                        to_ns(u64::from(copy.image_subresource.mip)),
                        plan.origin,
                        plan.size,
                        &plan.buffer,
                        to_ns(offset),
                        to_ns(plan.footprint.bytes_per_row),
                        to_ns(plan.slice_stride),
                    );
            }
        }
    }

    fn copy_image_to_image(&mut self, copy: &ImageCopy) {
        let (Some(src), Some(dst)) = (self.image(copy.src), self.image(copy.dst)) else {
            return;
        };
        let (src, dst, src_type) = (src.raw, dst.raw, src.image_type);
        let (Some(src_origin), Some(dst_origin)) =
            (conv::origin(copy.src_offset), conv::origin(copy.dst_offset))
        else {
            self.fail(HalError::InvalidDescriptor(format!(
                "copy_image_to_image has a negative texel offset: {:?} → {:?}",
                copy.src_offset, copy.dst_offset
            )));
            return;
        };
        let size = conv::copy_size(copy.extent, matches!(src_type, ImageType::D3));
        let (Some(src_slices), Some(dst_slices)) = (
            self.slices(&src, copy.src_subresource, "copy_image_to_image source"),
            self.slices(
                &dst,
                copy.dst_subresource,
                "copy_image_to_image destination",
            ),
        ) else {
            return;
        };
        if src_slices.len() != dst_slices.len() {
            self.fail(HalError::InvalidDescriptor(format!(
                "copy_image_to_image moves {} source layers into {} destination layers",
                src_slices.len(),
                dst_slices.len()
            )));
            return;
        }
        if !self.region_fits(&src, copy.src_subresource.mip, src_origin, size, "source")
            || !self.region_fits(
                &dst,
                copy.dst_subresource.mip,
                dst_origin,
                size,
                "destination",
            )
        {
            return;
        }
        let Some(encoder) = self.blit() else {
            return;
        };
        for (src_slice, dst_slice) in src_slices.zip(dst_slices) {
            // SAFETY: `objc2` marks this unsafe because Metal bounds-checks
            // neither the region nor the slice. `region_fits` checked the region
            // against each texture's own extent at the named mip level and
            // `slices` checked both slice ranges against their `arrayLength`;
            // both textures are kept alive by the `Retained`s held here.
            unsafe {
                encoder
                    .copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                        &src,
                        src_slice,
                        to_ns(u64::from(copy.src_subresource.mip)),
                        src_origin,
                        size,
                        &dst,
                        dst_slice,
                        to_ns(u64::from(copy.dst_subresource.mip)),
                        dst_origin,
                    );
            }
        }
    }

    /// Fills a buffer range with a repeating 32-bit value.
    ///
    /// **Metal's `fillBuffer:range:value:` takes a byte, not a word.** So a
    /// `u32` whose four bytes are not identical has no Metal encoding at all,
    /// and this refuses it by name rather than filling with the low byte and
    /// leaving a caller to find out from a corrupt indirect count. Every value
    /// the seam's own reason for existing needs — "the idiomatic way to zero an
    /// indirect count buffer" — is a repeated byte, and `0` most of all.
    fn fill_buffer(&mut self, buffer: BufferHandle, offset: u64, size: u64, value: u32) {
        let bytes = value.to_ne_bytes();
        if bytes.iter().any(|byte| *byte != bytes[0]) {
            self.fail(HalError::InvalidDescriptor(format!(
                "fill_buffer cannot write {value:#010x} on Metal: \
                 MTLBlitCommandEncoder::fillBuffer:range:value: repeats a single byte, so only a \
                 value whose four bytes are equal has an encoding"
            )));
            return;
        }
        let Some(raw) = self.buffer(buffer) else {
            return;
        };
        if size == 0 {
            return;
        }
        if offset
            .checked_add(size)
            .is_none_or(|end| end > raw.length() as u64)
        {
            self.fail(HalError::InvalidDescriptor(format!(
                "fill_buffer range {offset}..{} exceeds the buffer's {} bytes",
                offset.saturating_add(size),
                raw.length()
            )));
            return;
        }
        let Some(encoder) = self.blit() else {
            return;
        };
        encoder.fillBuffer_range_value(&raw, NSRange::new(to_ns(offset), to_ns(size)), bytes[0]);
    }

    // --- render scope ---

    /// Opens a render pass, which on Metal is a descriptor plus an encoder.
    ///
    /// # `render_area` becomes a scissor, and that is a real difference
    ///
    /// Metal's `MTLRenderPassDescriptor` has **no render-area rectangle**.
    /// `renderTargetWidth`/`renderTargetHeight` are an origin-anchored *size
    /// limit* on the whole pass rather than Vulkan's `renderArea`, and they
    /// cannot express an offset at all. So the seam's
    /// [`RenderPassDesc::render_area`] is applied as the render encoder's
    /// scissor rectangle, which bounds draws exactly as Vulkan's render area
    /// does.
    ///
    /// What that leaves different, stated rather than hidden: **a
    /// [`LoadOp::Clear`](crcbl_hal::LoadOp::Clear) on Metal clears the whole
    /// attachment**, where Vulkan clears only the render area. The seam
    /// documents `render_area` as "usually the full attachment size" and the
    /// render graph always passes exactly that, so nothing above the seam
    /// depends on the difference today; a caller that wanted a partial clear
    /// would need one this backend cannot give it.
    fn begin_render_pass(&mut self, desc: &RenderPassDesc<'_>) {
        if !self.ok() {
            return;
        }
        if self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "begin_render_pass inside a render pass; passes do not nest".to_string(),
            ));
            return;
        }
        if desc.color_attachments.is_empty() && desc.depth_stencil_attachment.is_none() {
            // Metal raises on a descriptor with no attachment at all, and the
            // pass would render nowhere in any case.
            self.fail(HalError::InvalidDescriptor(
                "a render pass with no colour and no depth/stencil attachment".to_string(),
            ));
            return;
        }

        let descriptor = MTLRenderPassDescriptor::new();
        // The attachment extent every scissor is clamped against, taken from
        // the first attachment. Metal derives the render target size from the
        // attachments the same way.
        let mut target: Option<(NSUInteger, NSUInteger)> = None;
        for (index, attachment) in desc.color_attachments.iter().enumerate() {
            let view = match self.device.view_raw(attachment.view) {
                Ok((view, _)) => view,
                Err(error) => {
                    self.fail(error);
                    return;
                }
            };
            target.get_or_insert((view.width(), view.height()));
            // `MTLRenderPassDescriptor`'s colour array has a fixed number of
            // slots and subscripting past them raises, so the index is bounded
            // first. This backend leaves `max_color_attachments` at the seam's
            // floor, which is below Metal's array length — a ceiling lower than
            // the truth is always safe, and `crcbl_mtl::adapter` says why every
            // unqueried limit stays at the floor.
            let ceiling = self.device.caps.limits.max_color_attachments as usize;
            if index >= ceiling {
                self.fail(HalError::InvalidDescriptor(format!(
                    "{} colour attachments exceed this device's limit of {ceiling}",
                    desc.color_attachments.len()
                )));
                return;
            }
            // SAFETY: `objc2` marks the subscript unsafe because Metal does not
            // bounds-check the attachment index. It was just checked against
            // `ceiling`, which is at or below the array's own length.
            let slot = unsafe {
                descriptor
                    .colorAttachments()
                    .objectAtIndexedSubscript(index)
            };
            slot.setTexture(Some(&view));
            slot.setLoadAction(conv::load_action(attachment.load));
            slot.setClearColor(conv::clear_color(attachment.clear.color));
            match attachment.resolve {
                Some(handle) => {
                    let resolve = match self.device.view_raw(handle) {
                        Ok((resolve, _)) => resolve,
                        Err(error) => {
                            self.fail(error);
                            return;
                        }
                    };
                    slot.setResolveTexture(Some(&resolve));
                    slot.setStoreAction(conv::resolve_store_action(attachment.store));
                }
                None => slot.setStoreAction(conv::store_action(attachment.store)),
            }
        }

        if let Some(attachment) = desc.depth_stencil_attachment {
            let (view, format) = match self.device.view_raw(attachment.view) {
                Ok(resolved) => resolved,
                Err(error) => {
                    self.fail(error);
                    return;
                }
            };
            target.get_or_insert((view.width(), view.height()));
            let depth = descriptor.depthAttachment();
            depth.setTexture(Some(&view));
            depth.setLoadAction(conv::load_action(attachment.depth_load));
            depth.setStoreAction(conv::store_action(attachment.depth_store));
            // Reversed-Z: the seam's default is `depth::CLEAR`, which is 0.0,
            // and this backend widens it and passes it on. Clearing to 1.0 with
            // the engine's `Greater` depth test renders nothing at all.
            depth.setClearDepth(f64::from(attachment.clear.depth));
            // Keyed off the **format**, not the ops: a `D32Float` attachment has
            // no stencil plane to load or store however the caller filled
            // `stencil_load`/`stencil_store`, and giving Metal a stencil
            // attachment whose texture has no stencil raises.
            if format.has_stencil() {
                let stencil = descriptor.stencilAttachment();
                stencil.setTexture(Some(&view));
                stencil.setLoadAction(conv::load_action(attachment.stencil_load));
                stencil.setStoreAction(conv::store_action(attachment.stencil_store));
                stencil.setClearStencil(attachment.clear.stencil);
            }
            // `DepthStencilAttachment::read_only` selects an image *layout* on
            // Vulkan and DX12 and has no Metal counterpart — Metal has no
            // layouts, and whether the pass writes depth is already carried by
            // the store action and by the pipeline's depth-write flag. So it is
            // read and deliberately not acted on here.
        }

        self.close_open();
        let Some(raw) = self.raw.as_ref() else {
            return;
        };
        let Some(encoder) = raw.renderCommandEncoderWithDescriptor(&descriptor) else {
            self.fail(HalError::DeviceLost(
                "MTLCommandBuffer::renderCommandEncoderWithDescriptor: returned nil".to_string(),
            ));
            return;
        };
        self.epoch += 1;
        if let Some(label) = desc.label {
            encoder.setLabel(Some(&NSString::from_str(label)));
        }
        if let Some((width, height)) = target {
            let area = desc.render_area;
            // Clamped to the attachment, because `setScissorRect:` raises on a
            // rectangle that leaves the render target.
            let x = to_ns(u64::try_from(area.x).unwrap_or(0)).min(width);
            let y = to_ns(u64::try_from(area.y).unwrap_or(0)).min(height);
            let scissor = MTLScissorRect {
                x,
                y,
                width: to_ns(u64::from(area.width)).min(width - x),
                height: to_ns(u64::from(area.height)).min(height - y),
            };
            // Set only for a genuine sub-rectangle. Metal's default scissor is
            // already the whole render target, so the common case — the seam's
            // own "usually the full attachment size" — makes no call at all,
            // and a degenerate rectangle (which Metal rejects) is never sent.
            let whole = scissor.x == 0
                && scissor.y == 0
                && scissor.width >= width
                && scissor.height >= height;
            if !whole && scissor.width > 0 && scissor.height > 0 {
                encoder.setScissorRect(scissor);
            }
        }
        self.open = Open::Render(encoder);
        self.in_render_pass = true;
        if let Some(label) = desc.label {
            self.begin_debug_label(label);
            self.render_pass_label = true;
        }
    }

    fn end_render_pass(&mut self) {
        if !self.in_render_pass {
            return;
        }
        if core::mem::take(&mut self.render_pass_label) {
            self.end_debug_label();
        }
        // Not gated on `ok`: an already-failed encoder still has to close its
        // render encoder, because `commit` with one still encoding raises.
        self.close_open();
        self.in_render_pass = false;
    }

    fn set_viewport(&mut self, viewport: &Viewport) {
        let Open::Render(encoder) = &self.open else {
            return;
        };
        // No Y flip and no depth inversion. Metal's clip space already matches
        // the seam's convention — unlike Vulkan, whose inverted Y is why
        // `crcbl-vk` passes a negative height — and reversed-Z comes from the
        // projection matrix, so touching `znear`/`zfar` here would apply it
        // twice.
        encoder.setViewport(MTLViewport {
            originX: f64::from(viewport.x),
            originY: f64::from(viewport.y),
            width: f64::from(viewport.width),
            height: f64::from(viewport.height),
            znear: f64::from(viewport.depth_min),
            zfar: f64::from(viewport.depth_max),
        });
    }

    fn set_scissor(&mut self, rect: &Rect2d) {
        let Open::Render(encoder) = &self.open else {
            return;
        };
        encoder.setScissorRect(MTLScissorRect {
            x: to_ns(rect.x.max(0) as u64),
            y: to_ns(rect.y.max(0) as u64),
            width: to_ns(u64::from(rect.width)),
            height: to_ns(u64::from(rect.height)),
        });
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        let Open::Render(encoder) = &self.open else {
            return;
        };
        encoder.setStencilReferenceValue(reference);
    }

    fn bind_graphics_pipeline(&mut self, _pipeline: GraphicsPipelineHandle) {
        self.fail(pipeline_slice(
            "graphics pipelines (the Metal pipeline slice)",
        ));
    }

    fn bind_index_buffer(&mut self, _buffer: BufferHandle, _offset: u64, _format: IndexFormat) {
        // Nothing to bind it for: Metal takes the index buffer as an argument
        // of the draw call itself, so there is no state to set until there is a
        // draw. The pipeline slice is where both arrive together.
        self.fail(pipeline_slice("indexed draws (the Metal pipeline slice)"));
    }

    fn bind_group(
        &mut self,
        _slot: u32,
        _group: BindGroupHandle,
        _dynamic_offsets: &[u32],
        _layout: PipelineLayoutHandle,
    ) {
        self.fail(binding_slice());
    }

    fn push_constants(
        &mut self,
        _stages: ShaderStages,
        _offset: u32,
        _data: &[u8],
        _layout: PipelineLayoutHandle,
    ) {
        self.fail(binding_slice());
    }

    // --- draws ---

    fn draw(&mut self, _vertices: Range<u32>, _instances: Range<u32>) {
        self.fail(pipeline_slice("draws (the Metal pipeline slice)"));
    }

    fn draw_indexed(&mut self, _indices: Range<u32>, _base_vertex: i32, _instances: Range<u32>) {
        self.fail(pipeline_slice("draws (the Metal pipeline slice)"));
    }

    fn draw_indirect(&mut self, _draw: &DrawIndirect) {
        self.fail(pipeline_slice("draws (the Metal pipeline slice)"));
    }

    fn draw_indexed_indirect(&mut self, _draw: &DrawIndirect) {
        self.fail(pipeline_slice("draws (the Metal pipeline slice)"));
    }

    fn draw_indirect_count(&mut self, _draw: &DrawIndirectCount) {
        self.fail(indirect_count());
    }

    fn draw_indexed_indirect_count(&mut self, _draw: &DrawIndirectCount) {
        self.fail(indirect_count());
    }

    // --- compute scope ---

    /// Opens a compute scope, and creates no Metal encoder for it.
    ///
    /// An `MTLComputeCommandEncoder` exists to hold a pipeline state and
    /// dispatches, and this slice has neither — `create_compute_pipeline`
    /// refuses, so nothing can be bound and nothing can be dispatched. Creating
    /// an encoder that could only ever be opened and closed again would be a
    /// driver object allocated to do nothing, so the scope is tracked here and
    /// [`dispatch`](CommandEncoder::dispatch) is the refusal.
    fn begin_compute_pass(&mut self, _desc: &ComputePassDesc<'_>) {
        if !self.ok() {
            return;
        }
        if self.in_compute_pass || self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "begin_compute_pass inside a pass; passes do not nest".to_string(),
            ));
            return;
        }
        self.in_compute_pass = true;
    }

    fn end_compute_pass(&mut self) {
        self.in_compute_pass = false;
    }

    fn bind_compute_pipeline(&mut self, _pipeline: ComputePipelineHandle) {
        self.fail(pipeline_slice(
            "compute pipelines (the Metal pipeline slice)",
        ));
    }

    fn dispatch(&mut self, _x: u32, _y: u32, _z: u32) {
        self.fail(pipeline_slice(
            "compute dispatches (the Metal pipeline slice)",
        ));
    }

    fn dispatch_indirect(&mut self, _args: BufferHandle, _offset: u64) {
        self.fail(pipeline_slice(
            "compute dispatches (the Metal pipeline slice)",
        ));
    }

    // --- queries ---

    /// A no-op with nothing to reset.
    ///
    /// The seam documents this as "required on Vulkan before every write; a
    /// no-op on backends that reset implicitly", and no query set can exist on
    /// this backend — `create_query_set` refuses — so there is no handle a
    /// caller could pass that names anything. Failing here would turn a call the
    /// seam asks every caller to make unconditionally into an error.
    fn reset_query_set(&mut self, _set: QuerySetHandle, _range: Range<u32>) {}

    fn write_timestamp(&mut self, _set: QuerySetHandle, _index: u32) {
        // Accepted and dropped, which is exactly what the seam prescribes for a
        // device without `Features::TIMESTAMP_QUERY` — and this device reports
        // it absent. See `Device::create_query_set` for why.
    }

    fn resolve_query_set(
        &mut self,
        set: QuerySetHandle,
        _range: Range<u32>,
        _dst: BufferHandle,
        _dst_offset: u64,
    ) {
        // Unlike the two above, this one *writes* somewhere: silently leaving
        // `dst` untouched would hand a caller a buffer of stale bytes it
        // believes are timings.
        self.fail(HalError::invalid_handle("query set", set));
    }

    // --- finish ---

    fn finish(mut self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
        if self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "finish with a render pass still open".to_string(),
            ));
            self.end_render_pass();
        }
        if self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(
                "finish with a compute pass still open".to_string(),
            ));
            self.in_compute_pass = false;
        }
        while !self.labels.is_empty() {
            self.end_debug_label();
        }
        // Always, and before anything else can fail: `commit` with an encoder
        // still encoding raises, and a raise aborts the process.
        self.close_open();
        if let Some(error) = self.failed.take() {
            return Err(error);
        }
        let Some(raw) = self.raw.take() else {
            // Unreachable: `raw` is `None` only when construction recorded a
            // failure, which the `take` above would have returned.
            return Err(HalError::DeviceLost(
                "this encoder never had a command buffer".to_string(),
            ));
        };
        let device = Arc::clone(&self.device);
        let handle = device.state().command_buffers.insert(CommandBufferEntry {
            owner: device.id,
            raw,
            committed: false,
        });
        Ok(device.stamp(handle))
    }
}

impl MetalCommandEncoder {
    /// Everything a buffer↔image copy needs, once every bound has been checked.
    fn plan_buffer_image_copy(&mut self, copy: &BufferImageCopy, what: &str) -> Option<CopyPlan> {
        let buffer = self.buffer(copy.buffer)?;
        let image = self.image(copy.image)?;
        let (texture, format, image_type) = (image.raw, image.format, image.image_type);
        let Some(footprint) = conv::copy_footprint(format, copy.image_subresource.aspect, copy)
        else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} names aspect {:?} of a {format:?} image, which is not one plane",
                copy.image_subresource.aspect
            )));
            return None;
        };
        let Some(origin) = conv::origin(copy.image_offset) else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} has a negative texel offset: {:?}",
                copy.image_offset
            )));
            return None;
        };
        let is_3d = matches!(image_type, ImageType::D3);
        let size = conv::copy_size(copy.image_extent, is_3d);
        let slices = self.slices(&texture, copy.image_subresource, what)?;
        if !self.region_fits(&texture, copy.image_subresource.mip, origin, size, what) {
            return None;
        }
        // Metal wants the *source*/*destination* image stride only where there
        // is more than one image to stride between — a 3D copy, or a slice loop
        // whose regions really are `bytes_per_image` apart. For a single 2D
        // slice the parameter is unused and Metal's own header asks for 0.
        let slice_stride = if is_3d || slices.len() > 1 {
            footprint.bytes_per_image
        } else {
            0
        };
        // How many 2D images the buffer side spans: one per array slice, or one
        // per depth step for a volume. The product is right for either, because
        // whichever is not in play is 1.
        let images = (slices.len() as u64) * (size.depth as u64);
        let span = footprint
            .bytes_per_image
            .checked_mul(images)
            .and_then(|span| copy.buffer_offset.checked_add(span));
        if span.is_none_or(|end| end > buffer.length() as u64) {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} reads or writes {images} images of {} bytes from offset {} of a {}-byte \
                 buffer",
                footprint.bytes_per_image,
                copy.buffer_offset,
                buffer.length()
            )));
            return None;
        }
        Some(CopyPlan {
            buffer,
            buffer_offset: copy.buffer_offset,
            texture,
            footprint,
            slice_stride,
            origin,
            size,
            slices,
        })
    }

    /// Whether a copy region lies inside a texture's extent at `mip`.
    ///
    /// Metal bounds-checks none of this and raises on an overrun, so it is
    /// checked against the object's own dimensions — halved per mip level the
    /// way the chain is defined, with a floor of one texel.
    fn region_fits(
        &mut self,
        texture: &ProtocolObject<dyn MTLTexture>,
        mip: u32,
        origin: MTLOrigin,
        size: objc2_metal::MTLSize,
        what: &str,
    ) -> bool {
        let shift = u32::min(mip, NSUInteger::BITS - 1);
        let level = |extent: NSUInteger| (extent >> shift).max(1);
        let (width, height, depth) = (
            level(texture.width()),
            level(texture.height()),
            level(texture.depth()),
        );
        let fits = origin.x.saturating_add(size.width) <= width
            && origin.y.saturating_add(size.height) <= height
            && origin.z.saturating_add(size.depth) <= depth;
        if !fits {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} region at {origin:?} of {size:?} does not fit mip {mip}, which is \
                 {width}x{height}x{depth}"
            )));
        }
        fits
    }
}

/// A buffer↔image copy with every bound already checked.
struct CopyPlan {
    buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
    buffer_offset: u64,
    texture: Retained<ProtocolObject<dyn MTLTexture>>,
    footprint: conv::CopyFootprint,
    /// What Metal is told the image stride is; `0` for a single 2D slice, which
    /// is what its header asks for.
    slice_stride: u64,
    origin: MTLOrigin,
    size: objc2_metal::MTLSize,
    slices: Range<NSUInteger>,
}

/// The refusal for anything that needs a pipeline state object.
///
/// `what` is already the whole phrase, so a reader of the call site sees the
/// message a caller will get rather than a key into a table somewhere else.
fn pipeline_slice(what: &'static str) -> HalError {
    crate::MetalInstance::not_yet(what)
}

/// The refusal for anything that needs a bind group or a pipeline layout.
fn binding_slice() -> HalError {
    crate::MetalInstance::not_yet("bind groups and push constants (the Metal binding slice)")
}

/// The refusal for the two calls Metal has no direct answer for at all.
///
/// Metal's `drawPrimitives:indirectBuffer:indirectBufferOffset:` emits exactly
/// one draw and reads no count buffer, which is why this backend reports
/// neither [`Features::DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT)
/// nor [`Features::MULTI_DRAW_INDIRECT`](crcbl_hal::Features::MULTI_DRAW_INDIRECT)
/// and why its derived tier is B. Indirect command buffers are the closest fit
/// and the slice that builds them is the one that moves the tier.
fn indirect_count() -> HalError {
    crate::MetalInstance::not_yet("indirect-count draws (the Metal indirect slice)")
}

impl Drop for MetalCommandEncoder {
    fn drop(&mut self) {
        // Only reached when `finish` was never called or failed. The command
        // buffer is released without ever being committed, which Metal permits
        // — but an encoder still encoding on it is not, so close it first.
        self.close_open();
    }
}
