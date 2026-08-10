//! [`MetalCommandEncoder`]: real recording into an `MTLCommandBuffer` — render
//! passes, compute passes, blit copies, and the encoder boundary a barrier
//! becomes.
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
//! copies open a blit encoder and keep it, a render or compute pass closes the
//! blit encoder first, and [`finish`](CommandEncoder::finish) closes whatever is
//! left. A copy recorded *inside* a pass is refused with
//! [`HalError::InvalidDescriptor`] rather than being allowed to raise — the
//! seam already forbids it ("copies, barriers and query writes are legal only
//! outside any pass"), so this is a caller bug being caught, not a restriction
//! this backend invents.
//!
//! **A pass owns its encoder for exactly its own length.** Both
//! [`begin_render_pass`](CommandEncoder::begin_render_pass) and
//! [`begin_compute_pass`](CommandEncoder::begin_compute_pass) create one, and
//! their `end_*` closes it — which is what makes pipeline state, argument
//! tables and the seam's scope rules line up with Metal's, since every one of
//! those dies with `endEncoding`.
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
    MTLBlitCommandEncoder, MTLBuffer, MTLCommandBuffer, MTLCommandEncoder as _,
    MTLComputeCommandEncoder, MTLOrigin, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLScissorRect, MTLTexture, MTLViewport,
};

use crate::conv;
use crate::device::{CommandBufferEntry, DeviceInner, ResolvedImage, to_ns};

/// What a command buffer and its encoders are called when the seam gave no
/// label, and the name the copy encoder always carries.
///
/// **This is not cosmetic.** A GPU fault names the encoder that caused it by
/// its label — see `crcbl_mtl::fault` — and an encoder that was never labelled
/// reports an empty string, which turns a report naming the culprit back into a
/// list of blanks. [`RenderPassDesc::label`] is optional and the copy encoder is
/// this backend's own object that no seam call ever names, so these three are
/// the floor beneath every label a caller does supply.
const UNLABELLED_COMMAND_BUFFER: &str = "crcbl command buffer";
/// See [`UNLABELLED_COMMAND_BUFFER`].
const UNLABELLED_RENDER_PASS: &str = "crcbl render pass";
/// See [`UNLABELLED_COMMAND_BUFFER`].
const UNLABELLED_COMPUTE_PASS: &str = "crcbl compute pass";
/// See [`UNLABELLED_COMMAND_BUFFER`].
const COPY_ENCODER: &str = "crcbl copies";

/// The one Metal encoder that may be open on the command buffer.
enum Open {
    /// Nothing is encoding; the command buffer will accept a new encoder.
    None,
    /// A blit encoder, kept open across consecutive copies.
    Blit(Retained<ProtocolObject<dyn MTLBlitCommandEncoder>>),
    /// A render pass.
    Render(Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>),
    /// A compute pass.
    Compute(Retained<ProtocolObject<dyn MTLComputeCommandEncoder>>),
}

impl Open {
    /// The open encoder as the base protocol, for the calls they all share.
    fn as_encoder(&self) -> Option<&ProtocolObject<dyn objc2_metal::MTLCommandEncoder>> {
        match self {
            Self::None => None,
            Self::Blit(encoder) => Some(ProtocolObject::from_ref(&**encoder)),
            Self::Render(encoder) => Some(ProtocolObject::from_ref(&**encoder)),
            Self::Compute(encoder) => Some(ProtocolObject::from_ref(&**encoder)),
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
    /// Both track a scope that [`Open`] also has an encoder for; they are
    /// separate because the seam's rules are about the *pass* — a copy inside
    /// one is refused whether or not an encoder happens to be open, and a pass
    /// left open at `finish` is an error rather than something to close
    /// quietly.
    in_render_pass: bool,
    in_compute_pass: bool,
    /// Open debug groups, innermost last.
    labels: Vec<Label>,
    /// Whether the open render pass pushed a label of its own.
    render_pass_label: bool,
    /// Whether the open compute pass pushed a label of its own.
    compute_pass_label: bool,
    /// The topology of the bound graphics pipeline, which Metal takes at the
    /// draw call rather than in the pipeline object.
    ///
    /// `None` means nothing is bound. It is cleared by [`Self::close_open`]
    /// along with the encoder, because a render encoder's pipeline state does
    /// not survive `endEncoding` — keeping it would let a draw in the *next*
    /// pass proceed with no pipeline set and let Metal raise.
    bound_primitive: Option<objc2_metal::MTLPrimitiveType>,
    /// The threadgroup size of the bound compute pipeline, which Metal takes at
    /// the dispatch call rather than in the pipeline object.
    ///
    /// The compute half of [`Self::bound_primitive`], and cleared by
    /// [`Self::close_open`] for the same reason: an `MTLComputeCommandEncoder`
    /// loses its pipeline state at `endEncoding`, so a dispatch in the next
    /// pass must not inherit the previous pass's number.
    bound_threads: Option<objc2_metal::MTLSize>,
    /// The index buffer an indexed draw will read, which Metal takes at the
    /// draw call rather than as encoder state.
    ///
    /// Unlike [`Self::bound_primitive`] this **survives** [`Self::close_open`],
    /// and `crcbl_mtl::draw` argues why: there is no Metal state to lose, so
    /// keeping it is what makes the binding behave the way Vulkan's does.
    index: Option<crate::draw::IndexBinding>,
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
            compute_pass_label: false,
            bound_primitive: None,
            bound_threads: None,
            index: None,
        };
        // The queue is checked before the command buffer is taken, so a handle
        // belonging to another device is reported as the crossing it is rather
        // than being answered with a working encoder on the wrong device.
        if let Err(error) = encoder.device.check_queue(desc.queue) {
            encoder.failed = Some(error);
            return encoder;
        }
        // Through `crcbl_mtl::fault`, which is what asks Metal for per-encoder
        // status on a fault. Every command buffer this backend creates goes
        // through it, because the one that skipped it is the one that faults.
        let Some(raw) = crate::fault::command_buffer(
            &encoder.device.queue,
            desc.label.unwrap_or(UNLABELLED_COMMAND_BUFFER),
        ) else {
            encoder.failed = Some(HalError::DeviceLost(
                "MTLCommandQueue::commandBufferWithDescriptor: returned nil".to_string(),
            ));
            return encoder;
        };
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
        // Pipeline state belongs to the encoder that is going away; see
        // `bound_primitive` and `bound_threads`.
        self.bound_primitive = None;
        self.bound_threads = None;
    }

    /// The blit encoder, opening one if the command buffer has none.
    ///
    /// `None` means the caller has already been told why — either recording had
    /// failed, or a pass is open and the seam forbids a copy there.
    fn blit(&mut self) -> Option<Retained<ProtocolObject<dyn MTLBlitCommandEncoder>>> {
        if !self.ok() {
            return None;
        }
        if self.in_render_pass || self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(
                "a copy inside a pass: the seam places copies between passes, and opening a blit \
                 encoder here would end the pass's own encoder — taking its pipeline state and \
                 argument tables with it"
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
        encoder.setLabel(Some(&NSString::from_str(COPY_ENCODER)));
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
    /// 1. **Sub-allocating from an `MTLHeap`, or reaching resources through an
    ///    argument buffer.** Both are `MTLHazardTrackingModeUntracked` or
    ///    invisible to the encoder, and Metal inserts nothing for them — which
    ///    is what `useResource:usage:` and `MTLFence` exist for. The binding
    ///    slice deliberately introduced neither: `crcbl_mtl::binding` binds
    ///    argument-table slots directly, which Metal both makes resident and
    ///    hazard-tracks, so this argument survives it intact. The slice that
    ///    adds argument buffers is the one that must turn this into
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
        if !self.ok() || self.in_render_pass || self.in_compute_pass {
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
        encoder.setLabel(Some(&NSString::from_str(
            desc.label.unwrap_or(UNLABELLED_RENDER_PASS),
        )));
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

    /// Sets the pipeline state, **and the rasteriser state Metal keeps on the
    /// encoder rather than in the pipeline object**.
    ///
    /// Cull mode, winding, fill mode, depth clip, depth bias, the depth/stencil
    /// test and the stencil reference are all encoder calls in Metal and all
    /// fields of [`GraphicsPipelineDesc`](crcbl_hal::GraphicsPipelineDesc)
    /// here, so binding replays every one of them from
    /// `crcbl_mtl::pipeline`'s `RasterState`. Leaving any of them out would
    /// make a pipeline draw with whatever the *previous* pipeline set, which is
    /// the class of bug that shows up as one pass rendering correctly and the
    /// next one inheriting its culling.
    ///
    /// Outside a render pass this is a caller error rather than a no-op: there
    /// is no Metal object to hold the state, so silently dropping it would mean
    /// the following draw used none of it.
    fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        if !self.ok() {
            return;
        }
        let encoder = match &self.open {
            Open::Render(encoder) => encoder.clone(),
            _ => {
                self.fail(HalError::InvalidDescriptor(
                    "bind_graphics_pipeline outside a render pass: Metal keeps pipeline state on \
                     the render encoder, so there is nowhere to record it"
                        .to_string(),
                ));
                return;
            }
        };
        let bound = match self.device.graphics_pipeline_raw(pipeline) {
            Ok(bound) => bound,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        encoder.setRenderPipelineState(&bound.raw);
        encoder.setCullMode(bound.raster.cull);
        encoder.setFrontFacingWinding(bound.raster.winding);
        encoder.setTriangleFillMode(bound.raster.fill);
        encoder.setDepthClipMode(bound.raster.clip);
        let [constant, slope_scale, clamp] = bound.raster.bias;
        encoder.setDepthBias_slopeScale_clamp(constant, slope_scale, clamp);
        // Always an object, never nil: a pipeline with no depth/stencil state
        // carries the device's always-pass one instead, because nil hangs
        // Apple's paravirtual GPU. `crcbl_mtl::pipeline`'s
        // `default_depth_stencil_state` has the bisect and the no-op argument.
        encoder.setDepthStencilState(Some(&bound.depth_stencil));
        if let Some(reference) = bound.raster.stencil_reference {
            encoder.setStencilReferenceValue(reference);
        }
        self.bound_primitive = Some(bound.raster.primitive);
    }

    /// Records which buffer a later indexed draw reads, because Metal takes it
    /// at the draw rather than as encoder state.
    ///
    /// `drawIndexedPrimitives:` takes the index buffer, its offset and its type
    /// as **arguments of the draw itself**, so there is no encoder call to make
    /// here and the backend carries the binding across to
    /// [`draw_indexed`](CommandEncoder::draw_indexed) itself. Two consequences,
    /// both argued in `crcbl_mtl::draw`: this is legal outside a render pass,
    /// and the binding survives a pass boundary the way Vulkan's does.
    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64, format: IndexFormat) {
        if !self.ok() {
            return;
        }
        let Some(raw) = self.buffer(buffer) else {
            return;
        };
        let capacity = match crate::draw::plan_index_binding(offset, format, raw.length() as u64) {
            Ok(capacity) => capacity,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        self.index = Some(crate::draw::IndexBinding {
            raw,
            offset,
            format,
            capacity,
        });
    }

    /// Sets one bind group's resources on the open encoder, whichever kind of
    /// pass it belongs to.
    ///
    /// The indices come from `crcbl_mtl::binding`'s flattening of
    /// `(set, binding)` onto Metal's three per-stage argument tables; the
    /// pipeline layout is what says where this slot's tables start, which is
    /// exactly why the seam passes one. **The open pass is what decides the
    /// bind point** — the seam has no compute-vs-graphics parameter here, and
    /// the scope is the only signal a backend gets.
    ///
    /// Outside a pass this is a caller error rather than a no-op, for the
    /// reason [`bind_graphics_pipeline`](CommandEncoder::bind_graphics_pipeline)
    /// gives: Metal keeps argument tables on the encoder, so there is nowhere
    /// to record the binding and the following draw or dispatch would use none
    /// of it.
    fn bind_group(
        &mut self,
        slot: u32,
        group: BindGroupHandle,
        dynamic_offsets: &[u32],
        layout: PipelineLayoutHandle,
    ) {
        if !self.ok() {
            return;
        }
        enum Target {
            Render(Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>),
            Compute(Retained<ProtocolObject<dyn MTLComputeCommandEncoder>>),
        }
        let target = match &self.open {
            Open::Render(encoder) => Target::Render(encoder.clone()),
            Open::Compute(encoder) => Target::Compute(encoder.clone()),
            _ => {
                self.fail(HalError::InvalidDescriptor(
                    "bind_group outside a render or compute pass: Metal keeps its argument \
                     tables on the encoder, so there is nowhere to record it"
                        .to_string(),
                ));
                return;
            }
        };
        let limits = self.device.caps.limits;
        match self
            .device
            .bind_group_raw(slot, group, dynamic_offsets, layout, &limits)
        {
            Ok(bindings) => match target {
                Target::Render(encoder) => crate::binding::apply(&bindings, &encoder),
                Target::Compute(encoder) => crate::binding::apply_compute(&bindings, &encoder),
            },
            Err(error) => self.fail(error),
        }
    }

    /// Refused: this device reports no
    /// [`Features::PUSH_CONSTANTS`](crcbl_hal::Features::PUSH_CONSTANTS).
    ///
    /// `create_pipeline_layout` is where the refusal that matters happens — the
    /// seam requires a backend without the feature to fail there rather than
    /// dropping writes here — so a caller can only reach this by passing a
    /// layout it did not get from this backend. Failing anyway is what stops a
    /// draw reading constants nobody wrote.
    fn push_constants(
        &mut self,
        _stages: ShaderStages,
        _offset: u32,
        _data: &[u8],
        _layout: PipelineLayoutHandle,
    ) {
        self.fail(HalError::InvalidDescriptor(
            "push_constants on a device that reports no Features::PUSH_CONSTANTS; \
             create_pipeline_layout refuses the range that would have declared them, and says why"
                .to_string(),
        ));
    }

    // --- draws ---

    /// `drawPrimitives:vertexStart:vertexCount:instanceCount:baseInstance:`.
    ///
    /// The five-argument form always, not the three-argument one: the seam
    /// hands over an instance *range*, and the short form has no base instance
    /// to put its start in. Passing `0..1` through the long form costs nothing
    /// and keeps one code path.
    ///
    /// # The topology comes from the pipeline, and there must be one
    ///
    /// Metal takes the primitive type as a **draw** argument while the seam
    /// declares it on the pipeline, so this reads the value
    /// `bind_graphics_pipeline` recorded. With nothing bound there is no
    /// topology and no pipeline state, and Metal answers that by raising —
    /// which aborts the process — so it is refused here while it is still an
    /// error a caller can catch.
    ///
    /// An empty vertex or instance range draws nothing and is not an error: the
    /// seam's ranges are half-open and `0..0` is a legitimate "no work this
    /// frame" a culling pass produces.
    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        let Some((encoder, primitive)) = self.draw_target("draw") else {
            return;
        };
        if vertices.is_empty() || instances.is_empty() {
            return;
        }
        // SAFETY: `objc2` marks this unsafe because Metal bounds-checks neither
        // count. Neither is a bound into an object here — a draw's vertex count
        // indexes whatever the vertex shader chooses to read, not a buffer this
        // call names — and both ranges were just checked to be non-empty, which
        // is the one value (`0`) Metal's own validation layer objects to. The
        // encoder is kept alive by the `Retained` held across the call.
        unsafe {
            encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                primitive,
                to_ns(u64::from(vertices.start)),
                to_ns(u64::from(vertices.end - vertices.start)),
                to_ns(u64::from(instances.end - instances.start)),
                to_ns(u64::from(instances.start)),
            );
        }
    }

    /// `drawIndexedPrimitives:…:baseVertex:baseInstance:`, with the index
    /// buffer [`bind_index_buffer`](CommandEncoder::bind_index_buffer)
    /// recorded.
    ///
    /// The eight-argument form always, for the reason [`draw`](CommandEncoder::draw)
    /// takes the five-argument one: the seam hands over a base vertex and an
    /// instance *range*, and the shorter forms have nowhere to put either.
    ///
    /// An indexed draw with no index buffer bound is refused rather than left
    /// to Metal, which has no nil to pass — `indexBuffer` is a non-optional
    /// argument of the selector, so there is no call to make at all.
    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        let Some((encoder, primitive)) = self.draw_target("draw_indexed") else {
            return;
        };
        if indices.is_empty() || instances.is_empty() {
            return;
        }
        let Some(index) = self.index.as_ref() else {
            self.fail(HalError::InvalidDescriptor(
                "draw_indexed with no index buffer bound: Metal takes the buffer as an argument \
                 of the draw and has no nil to pass for it"
                    .to_string(),
            ));
            return;
        };
        let count = indices.end - indices.start;
        let (raw, index_type) = (index.raw.clone(), index.index_type());
        let offset = match index.draw_offset(indices.start, count) {
            Ok(offset) => offset,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        // SAFETY: `objc2` marks this unsafe because Metal bounds-checks neither
        // the index count nor the buffer offset. `draw_offset` just checked the
        // whole index range against the bound region's own length, which
        // `bind_index_buffer` computed from the allocation's, and the counts
        // were checked non-empty above. The buffer and the encoder are kept
        // alive by the `Retained`s held across the call.
        unsafe {
            encoder.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount_baseVertex_baseInstance(
                primitive,
                to_ns(u64::from(count)),
                index_type,
                &raw,
                to_ns(offset),
                to_ns(u64::from(instances.end - instances.start)),
                base_vertex as isize,
                to_ns(u64::from(instances.start)),
            );
        }
    }

    /// One `drawPrimitives:indirectBuffer:indirectBufferOffset:` per argument
    /// structure. See `crcbl_mtl::draw` for why a loop *is* multi-draw-indirect
    /// here rather than an approximation of it.
    fn draw_indirect(&mut self, draw: &DrawIndirect) {
        self.indirect(draw, false);
    }

    fn draw_indexed_indirect(&mut self, draw: &DrawIndirect) {
        self.indirect(draw, true);
    }

    fn draw_indirect_count(&mut self, _draw: &DrawIndirectCount) {
        self.fail(indirect_count());
    }

    fn draw_indexed_indirect_count(&mut self, _draw: &DrawIndirectCount) {
        self.fail(indirect_count());
    }

    /// Refused for the same reason `create_mesh_pipeline` is: there is no mesh
    /// pipeline for this to have been recorded against.
    fn draw_mesh_tasks(&mut self, _x: u32, _y: u32, _z: u32) {
        self.fail(crate::MetalInstance::not_yet(
            "drawMeshThreadgroups: (the Metal mesh slice)",
        ));
    }

    // --- compute scope ---

    /// Opens a compute pass, **and the `MTLComputeCommandEncoder` that is its
    /// Metal object**.
    ///
    /// The encoder's lifetime is the pass's, which is the one shape that keeps
    /// the seam's scope and Metal's agreeing: everything a compute pass does —
    /// the pipeline state, the argument tables a `bind_group` writes, the
    /// dispatches — lives on that encoder and dies with `endEncoding`. Opening
    /// it at the first dispatch instead would leave `bind_group` and
    /// `bind_compute_pipeline` with nowhere to record, which is precisely the
    /// state this backend was in before compute could dispatch at all.
    fn begin_compute_pass(&mut self, desc: &ComputePassDesc<'_>) {
        if !self.ok() {
            return;
        }
        if self.in_compute_pass || self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "begin_compute_pass inside a pass; passes do not nest".to_string(),
            ));
            return;
        }
        // Metal raises on a second encoder, so the copy encoder a preceding
        // upload left open goes first — which is also this backend's barrier.
        self.close_open();
        let Some(raw) = self.raw.as_ref() else {
            return;
        };
        let Some(encoder) = raw.computeCommandEncoder() else {
            self.fail(HalError::DeviceLost(
                "MTLCommandBuffer::computeCommandEncoder returned nil".to_string(),
            ));
            return;
        };
        self.epoch += 1;
        encoder.setLabel(Some(&NSString::from_str(
            desc.label.unwrap_or(UNLABELLED_COMPUTE_PASS),
        )));
        self.open = Open::Compute(encoder);
        self.in_compute_pass = true;
        if let Some(label) = desc.label {
            self.begin_debug_label(label);
            self.compute_pass_label = true;
        }
    }

    fn end_compute_pass(&mut self) {
        if !self.in_compute_pass {
            return;
        }
        if core::mem::take(&mut self.compute_pass_label) {
            self.end_debug_label();
        }
        // Not gated on `ok`, as `end_render_pass`: an already-failed encoder
        // still has to close its compute encoder, because `commit` with one
        // still encoding raises.
        self.close_open();
        self.in_compute_pass = false;
    }

    /// Sets the pipeline state, and records the threadgroup size the dispatch
    /// after it will need.
    ///
    /// Metal takes threads-per-threadgroup at
    /// `dispatchThreadgroups:threadsPerThreadgroup:` while every other backend
    /// reads it out of the module, which is why
    /// [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size)
    /// exists — see `crcbl_mtl::pipeline`, where the size is checked and stored
    /// on the pipeline entry. This is where it comes back off.
    fn bind_compute_pipeline(&mut self, pipeline: ComputePipelineHandle) {
        if !self.ok() {
            return;
        }
        let Open::Compute(encoder) = &self.open else {
            self.fail(HalError::InvalidDescriptor(
                "bind_compute_pipeline outside a compute pass: Metal keeps pipeline state on the \
                 compute encoder, so there is nowhere to record it"
                    .to_string(),
            ));
            return;
        };
        let encoder = encoder.clone();
        let (raw, threads) = match self.device.compute_pipeline_raw(pipeline) {
            Ok(bound) => bound,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        encoder.setComputePipelineState(&raw);
        self.bound_threads = Some(threads);
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        let Some((encoder, threads)) = self.dispatch_target("dispatch") else {
            return;
        };
        // A dispatch of nothing, which `vkCmdDispatch` accepts as a no-op — and
        // which a caller computing `count.div_ceil(WORKGROUP_SIZE)` writes
        // whenever `count` is zero. Metal's header documents every threadgroup
        // count as at least one, and its validation layer raises on a zero,
        // which aborts the process; the no-op is performed here instead.
        if x == 0 || y == 0 || z == 0 {
            return;
        }
        encoder.dispatchThreadgroups_threadsPerThreadgroup(
            objc2_metal::MTLSize {
                width: to_ns(u64::from(x)),
                height: to_ns(u64::from(y)),
                depth: to_ns(u64::from(z)),
            },
            threads,
        );
    }

    /// `dispatchThreadgroupsWithIndirectBuffer:indirectBufferOffset:threadsPerThreadgroup:`.
    ///
    /// The argument structure is `MTLDispatchThreadgroups­IndirectArguments` —
    /// three `uint32_t`s, field for field what Vulkan calls
    /// `VkDispatchIndirectCommand`, which is what lets one compute pass write
    /// arguments both backends read. The **threadgroup size is still a CPU
    /// value**: only the counts come from the buffer.
    fn dispatch_indirect(&mut self, args: BufferHandle, offset: u64) {
        let Some((encoder, threads)) = self.dispatch_target("dispatch_indirect") else {
            return;
        };
        let Some(raw) = self.buffer(args) else {
            return;
        };
        // Metal reads three `uint32_t`s and bounds-checks neither the offset
        // nor the span, so a short buffer is a GPU fault rather than an error.
        // Refused here while it is still one a caller can catch.
        let length = raw.length() as u64;
        if offset
            .checked_add(DISPATCH_ARGUMENTS)
            .is_none_or(|end| end > length)
        {
            self.fail(HalError::InvalidDescriptor(format!(
                "dispatch_indirect reads {DISPATCH_ARGUMENTS} bytes at offset {offset} of a \
                 {length}-byte buffer"
            )));
            return;
        }
        // SAFETY: `objc2` marks this unsafe because Metal bounds-checks neither
        // the offset nor the argument structure it reads. The span was checked
        // against this buffer's own length just above, the buffer is kept alive
        // by the `Retained` held here, and the encoder is the open one.
        unsafe {
            encoder
                .dispatchThreadgroupsWithIndirectBuffer_indirectBufferOffset_threadsPerThreadgroup(
                    &raw,
                    to_ns(offset),
                    threads,
                );
        }
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
            self.end_compute_pass();
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
    /// The open render encoder and the topology a draw needs, or the failure
    /// that says which is missing.
    ///
    /// Metal takes the primitive type as a **draw** argument while the seam
    /// declares it on the pipeline, so every draw reads the value
    /// `bind_graphics_pipeline` recorded. With nothing bound there is no
    /// topology and no pipeline state, and Metal answers that by raising —
    /// which aborts the process — so it is refused here while it is still an
    /// error a caller can catch.
    fn draw_target(
        &mut self,
        what: &str,
    ) -> Option<(
        Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>,
        objc2_metal::MTLPrimitiveType,
    )> {
        if !self.ok() {
            return None;
        }
        let Open::Render(encoder) = &self.open else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} outside a render pass; the seam places every draw inside one"
            )));
            return None;
        };
        let encoder = encoder.clone();
        let Some(primitive) = self.bound_primitive else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} with no graphics pipeline bound: Metal raises rather than reporting it, \
                 and a raise aborts the process"
            )));
            return None;
        };
        Some((encoder, primitive))
    }

    /// The open compute encoder and the threadgroup size a dispatch needs, or
    /// the failure that says which is missing.
    ///
    /// The compute twin of [`Self::draw_target`], and refused for the same
    /// reason: Metal answers a dispatch with no pipeline bound by raising,
    /// which aborts the process, so it is caught here while it is still an
    /// error a caller can handle.
    fn dispatch_target(
        &mut self,
        what: &str,
    ) -> Option<(
        Retained<ProtocolObject<dyn MTLComputeCommandEncoder>>,
        objc2_metal::MTLSize,
    )> {
        if !self.ok() {
            return None;
        }
        let Open::Compute(encoder) = &self.open else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} outside a compute pass; the seam places every dispatch inside one"
            )));
            return None;
        };
        let encoder = encoder.clone();
        let Some(threads) = self.bound_threads else {
            self.fail(HalError::InvalidDescriptor(format!(
                "{what} with no compute pipeline bound: Metal takes the threadgroup size at the \
                 dispatch and raises rather than reporting it, and a raise aborts the process"
            )));
            return None;
        };
        Some((encoder, threads))
    }

    /// The shared body of the two indirect draws.
    ///
    /// One Metal call per argument structure; `crcbl_mtl::draw`'s
    /// [`plan_indirect`](crate::draw::plan_indirect) is what bounds the span
    /// they read first.
    fn indirect(&mut self, draw: &DrawIndirect, indexed: bool) {
        let Some((encoder, primitive)) = self.draw_target(if indexed {
            "draw_indexed_indirect"
        } else {
            "draw_indirect"
        }) else {
            return;
        };
        let Some(args) = self.buffer(draw.args) else {
            return;
        };
        let plan = match crate::draw::plan_indirect(draw, indexed, args.length() as u64) {
            Ok(Some(plan)) => plan,
            // A draw of nothing, which the seam permits and which reads no
            // argument structure at all.
            Ok(None) => return,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        // Resolved before the loop so an indexed draw with nothing bound is one
        // failure rather than `draw_count` of them.
        let index = if indexed {
            let Some(index) = self.index.as_ref() else {
                self.fail(HalError::InvalidDescriptor(
                    "draw_indexed_indirect with no index buffer bound: Metal takes the buffer as \
                     an argument of the draw and has no nil to pass for it"
                        .to_string(),
                ));
                return;
            };
            Some((index.raw.clone(), index.index_type(), index.offset))
        } else {
            None
        };
        for structure in 0..plan.count {
            let offset = to_ns(plan.offset(structure));
            match &index {
                // SAFETY: `objc2` marks these unsafe because Metal
                // bounds-checks neither the indirect offset nor the index
                // buffer's. `plan_indirect` checked every structure this loop
                // reads against the argument buffer's own length, and the index
                // buffer's offset was checked against its allocation by
                // `bind_index_buffer`. Both buffers and the encoder are kept
                // alive by the `Retained`s held across the loop.
                //
                // What Metal cannot check, and neither can this, is the
                // *contents* of the argument structures — an index count larger
                // than the bound index buffer is a GPU-side fault either way,
                // which is the whole nature of an indirect draw.
                Some((raw, index_type, index_offset)) => unsafe {
                    encoder.drawIndexedPrimitives_indexType_indexBuffer_indexBufferOffset_indirectBuffer_indirectBufferOffset(
                        primitive,
                        *index_type,
                        raw,
                        to_ns(*index_offset),
                        &args,
                        offset,
                    );
                },
                None => unsafe {
                    encoder.drawPrimitives_indirectBuffer_indirectBufferOffset(
                        primitive, &args, offset,
                    );
                },
            }
        }
    }

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

/// Bytes an indirect dispatch reads: `MTLDispatchThreadgroupsIndirectArguments`
/// is three `uint32_t`s, fixed by Metal's own header.
const DISPATCH_ARGUMENTS: u64 = 12;

/// The refusal for the one indirect shape Metal cannot express at all.
///
/// `drawPrimitives:indirectBuffer:indirectBufferOffset:` emits one draw and
/// reads no count, and the multi-draw loop `crcbl_mtl::draw` builds on it works
/// only because [`DrawIndirect::draw_count`] is a **CPU** value.
/// [`DrawIndirectCount`] takes its count from GPU memory, and Metal's only
/// count-from-memory execution —
/// `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:` over an
/// `MTLIndirectCommandBuffer` — needs the commands to already exist, written by
/// a compute kernel that would have to run before the render encoder this call
/// happens inside was opened. So
/// [`DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT) stays off
/// and this backend's derived tier stays B; see the crate docs.
fn indirect_count() -> HalError {
    crate::MetalInstance::not_yet(
        "indirect-count draws: Metal's only GPU-side count is an MTLIndirectCommandBuffer whose \
         commands a compute pass must encode before the render pass begins (the Metal ICB slice)",
    )
}

impl Drop for MetalCommandEncoder {
    fn drop(&mut self) {
        // Only reached when `finish` was never called or failed. The command
        // buffer is released without ever being committed, which Metal permits
        // — but an encoder still encoding on it is not, so close it first.
        self.close_open();
    }
}
