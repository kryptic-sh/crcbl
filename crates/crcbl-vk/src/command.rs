//! [`VkCommandEncoder`]: command recording, `sync2` barriers, and the
//! dynamic-rendering pass that makes milestone 1 a real clear.
//!
//! # A render pass, not `vkCmdClearColorImage`
//!
//! `docs/plan/02-vulkan-backend.md`'s milestone ladder starts at "clear colour
//! through the graph". A `vkCmdClearColorImage` would put the same pixels on the
//! screen and prove almost nothing: it exercises no attachment, no load op, no
//! layout transition into `COLOR_ATTACHMENT_OPTIMAL`, and none of the
//! dynamic-rendering path every later milestone is built on. So
//! [`begin_render_pass`](crcbl_hal::CommandEncoder::begin_render_pass) really
//! does open a `vkCmdBeginRendering` scope with
//! [`LoadOp::Clear`](crcbl_hal::LoadOp), and the clear happens because the pass
//! loads that way.
//!
//! # One pool per encoder
//!
//! Simple and correct: a `VkCommandPool` is externally synchronised, and
//! [`CommandEncoder`] is `Send` but not `Sync`, so a pool that belongs to
//! exactly one encoder cannot be raced on by construction. It is also
//! wasteful — a pool per frame is a driver allocation per frame — and
//! `docs/plan/02-vulkan-backend.md` §2.2 asks for per-frame pools recycled by
//! the frame loop instead. That belongs with the render graph at P1.3, which is
//! the thing that will own the frame ring; doing it here first would mean
//! guessing the ring's shape.
//!
//! # The encoder remembers the pipeline layout, because the seam does not
//!
//! `vkCmdBindDescriptorSets` and `vkCmdPushConstants` both take a
//! The seam's [`bind_group`](crcbl_hal::CommandEncoder::bind_group) and
//! [`push_constants`](crcbl_hal::CommandEncoder::push_constants) now carry an
//! explicit [`PipelineLayoutHandle`] — the implicit layout inference from the
//! last bound pipeline is gone as of 2026-07-31.

use core::ops::Range;
use std::sync::Arc;

use ash::vk;
use ash::vk::Handle as _;

use crcbl_hal::{
    Barriers, BindGroupHandle, BufferCopy, BufferHandle, BufferImageCopy, CommandBufferHandle,
    CommandEncoder, CommandEncoderDesc, ComputePassDesc, ComputePipelineHandle, DrawIndirect,
    DrawIndirectCount, GraphicsPipelineHandle, HalError, ImageCopy, IndexFormat,
    PipelineLayoutHandle, QuerySetHandle, Rect2d, RenderPassDesc, ResourceState, ShaderStages,
    Viewport,
};

use crcbl_core::Handle;

use crate::conv;
use crate::device::{CommandBufferEntry, DeviceInner};

/// Records into one command buffer, from one pool it owns.
pub(crate) struct VkCommandEncoder {
    device: Arc<DeviceInner>,
    pool: vk::CommandPool,
    raw: vk::CommandBuffer,
    /// Set when something went wrong during recording. `finish` reports it
    /// rather than handing back a command buffer that is missing commands —
    /// a half-recorded frame that submits is far worse than one that does not.
    failed: Option<HalError>,
    /// Whether a render or compute scope is open, so `finish` can close it and
    /// complain rather than tripping a validation error at submit.
    in_render_pass: bool,
    in_compute_pass: bool,
    /// Debug label nesting, so an unbalanced label does not corrupt the
    /// capture tool's tree.
    label_depth: u32,
    /// Whether the open render pass pushed a label of its own.
    ///
    /// `begin_render_pass` pushes only when `desc.label.is_some()`, so popping
    /// on any non-zero `label_depth` closed the *caller's* enclosing
    /// `begin_debug_label` early and folded every later command into the wrong
    /// region of the capture tree. The pass has to remember whether it owns a
    /// label, not merely whether one is open.
    render_pass_label: bool,
    /// The same, for the open compute pass.
    compute_pass_label: bool,
    /// Raw device objects the recorded commands use; handed to the device at
    /// `finish` so a submission can keep destroyed-but-referenced objects parked
    /// in the deletion queue until it completes.
    references: Vec<u64>,
}

impl core::fmt::Debug for VkCommandEncoder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VkCommandEncoder")
            .field("failed", &self.failed.is_some())
            .field("in_render_pass", &self.in_render_pass)
            .finish_non_exhaustive()
    }
}

impl VkCommandEncoder {
    pub(crate) fn new(device: Arc<DeviceInner>, desc: &CommandEncoderDesc<'_>) -> Self {
        let mut encoder = Self {
            device,
            pool: vk::CommandPool::null(),
            raw: vk::CommandBuffer::null(),
            failed: None,
            in_render_pass: false,
            in_compute_pass: false,
            label_depth: 0,
            render_pass_label: false,
            compute_pass_label: false,
            references: Vec::new(),
        };
        if let Err(error) = encoder.begin(desc) {
            encoder.failed = Some(error);
        }
        encoder
    }

    fn begin(&mut self, desc: &CommandEncoderDesc<'_>) -> Result<(), HalError> {
        let family = self.device.queue_family(desc.queue)?;
        let info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(family)
            // Every buffer from this pool is recorded once and thrown away, so
            // `TRANSIENT` is the honest hint and the one that lets a driver use
            // a bump allocator.
            .flags(vk::CommandPoolCreateFlags::TRANSIENT);
        // SAFETY: `family` is a queue family this device created a queue on.
        self.pool = unsafe { self.device.raw.create_command_pool(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateCommandPool", error))?;
        self.device.set_object_name(self.pool, desc.label);

        let allocate = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: `self.pool` was just created by this device.
        let buffers = unsafe { self.device.raw.allocate_command_buffers(&allocate) }
            .map_err(|error| conv::hal_error("vkAllocateCommandBuffers", error))?;
        self.raw = buffers[0];
        self.device.set_object_name(self.raw, desc.label);

        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: `self.raw` was just allocated and is not recording.
        unsafe { self.device.raw.begin_command_buffer(self.raw, &begin) }
            .map_err(|error| conv::hal_error("vkBeginCommandBuffer", error))
    }

    /// Whether *new* commands may be recorded: nothing has failed yet and there
    /// is a buffer to record into.
    fn ok(&self) -> bool {
        self.failed.is_none() && self.recording()
    }

    /// Whether there is a live command buffer at all.
    ///
    /// Distinct from [`ok`](Self::ok) on purpose. Once recording has failed, no
    /// further *commands* are emitted — but the scopes and labels already open
    /// must still be closed, because `vkEndCommandBuffer` with a
    /// `vkCmdBeginRendering` outstanding is itself a validation error, and
    /// because a label pop that never runs is a `while` loop in
    /// [`finish`](CommandEncoder::finish) that never terminates. That hang was
    /// real: `begin_render_pass(label)` followed by any failing call used to
    /// spin forever with no output.
    fn recording(&self) -> bool {
        self.raw != vk::CommandBuffer::null()
    }

    /// Records the first failure and drops every later command.
    fn fail(&mut self, error: HalError) {
        if self.failed.is_none() {
            log::error!("crcbl-vk: command recording failed: {error}");
            self.failed = Some(error);
        }
    }

    /// Records a raw device object the recorded commands use. Deduplicated so a
    /// buffer bound or copied many times costs one queue extension.
    fn use_object(&mut self, raw: u64) {
        if !self.references.contains(&raw) {
            self.references.push(raw);
        }
    }
}

impl CommandEncoder for VkCommandEncoder {
    // --- debug ---

    fn begin_debug_label(&mut self, label: &str) {
        if !self.ok() {
            return;
        }
        let (Some(debug_ext), Ok(name)) = (
            self.device.debug_ext.as_ref(),
            std::ffi::CString::new(label),
        ) else {
            // Accepted and dropped without the feature, per the seam: debug
            // instrumentation degrades, it does not break.
            return;
        };
        let info = vk::DebugUtilsLabelEXT::default().label_name(&name);
        // SAFETY: `self.raw` is recording and `name` outlives the call.
        unsafe { debug_ext.cmd_begin_debug_utils_label(self.raw, &info) };
        self.label_depth += 1;
    }

    fn end_debug_label(&mut self) {
        if !self.recording() || self.label_depth == 0 {
            return;
        }
        // Decrement first and unconditionally: the count must reach zero even
        // when there is no `VK_EXT_debug_utils` to pop against, or `finish`
        // loops forever.
        self.label_depth -= 1;
        let Some(debug_ext) = self.device.debug_ext.as_ref() else {
            return;
        };
        // SAFETY: `self.raw` is recording and a label is open.
        unsafe { debug_ext.cmd_end_debug_utils_label(self.raw) };
    }

    fn insert_debug_marker(&mut self, label: &str) {
        if !self.ok() {
            return;
        }
        let (Some(debug_ext), Ok(name)) = (
            self.device.debug_ext.as_ref(),
            std::ffi::CString::new(label),
        ) else {
            return;
        };
        let info = vk::DebugUtilsLabelEXT::default().label_name(&name);
        // SAFETY: `self.raw` is recording and `name` outlives the call.
        unsafe { debug_ext.cmd_insert_debug_utils_label(self.raw, &info) };
    }

    // --- sync ---

    fn pipeline_barrier(&mut self, barriers: &Barriers<'_>) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let mut buffer_barriers = Vec::with_capacity(barriers.buffers.len());
        for barrier in barriers.buffers {
            let Ok(raw) = self.device.buffer_raw(&state, barrier.buffer) else {
                drop(state);
                self.fail(HalError::invalid_handle("buffer", barrier.buffer));
                return;
            };
            let from = conv::state_masks(barrier.from);
            let to = conv::state_masks(barrier.to);
            let mut vk_barrier = vk::BufferMemoryBarrier2::default()
                .src_stage_mask(from.stage)
                .src_access_mask(from.access)
                .dst_stage_mask(to.stage)
                .dst_access_mask(to.access)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(raw)
                .offset(0)
                .size(vk::WHOLE_SIZE);
            if let Some(transfer) = barrier.queue_transfer {
                let families = (
                    self.device.queue_family(transfer.from),
                    self.device.queue_family(transfer.to),
                );
                let (Ok(from_family), Ok(to_family)) = families else {
                    drop(state);
                    self.fail(queue_transfer_failure("buffer", families));
                    return;
                };
                vk_barrier = vk_barrier
                    .src_queue_family_index(from_family)
                    .dst_queue_family_index(to_family);
            }
            buffer_barriers.push(vk_barrier);
        }

        let mut image_barriers = Vec::with_capacity(barriers.images.len());
        for barrier in barriers.images {
            let Ok((raw, format)) = self.device.image_raw(&state, barrier.image) else {
                drop(state);
                self.fail(HalError::invalid_handle("image", barrier.image));
                return;
            };
            let mut from = conv::state_masks(barrier.from);
            let to = conv::state_masks(barrier.to);
            // `Undefined` normally means "wait for nothing": the contents are
            // being discarded, and on a WSI image the acquire semaphore is what
            // separates this write from whatever read the image last. An
            // offscreen ring image has no such semaphore — the seam hands one
            // back with an implicit acquire — so with `NONE` here nothing at all
            // orders this layout transition against the previous trip round the
            // ring, whose last act is typically the copy a headless caller reads
            // its pixels back with. That is a write-after-read, and validation
            // reports it as one with `read_barriers: 0`, which is this mask.
            //
            // A barrier's first synchronisation scope covers everything
            // submitted earlier to the same queue, so widening the *stage* to
            // `ALL_COMMANDS` is exactly the missing dependency and nothing more:
            // the access mask stays empty, because a write-after-read needs
            // execution ordering and no cache flush, and the contents are still
            // discarded.
            if barrier.from == ResourceState::Undefined
                && self.device.image_ring_reuse(&state, barrier.image)
            {
                from.stage = vk::PipelineStageFlags2::ALL_COMMANDS;
            }
            // The range's aspect comes from the caller, but a caller that used
            // `ImageSubresourceRange::all(swapchain_format)` and a backend that
            // knows the real format must not disagree — so the image's own
            // format wins when the caller left the default in place.
            let mut range = conv::subresource_range(barrier.range);
            if range.aspect_mask.is_empty() {
                range.aspect_mask = conv::aspect(crcbl_hal::ImageAspect::of(format));
            }
            let mut vk_barrier = vk::ImageMemoryBarrier2::default()
                .src_stage_mask(from.stage)
                .src_access_mask(from.access)
                .dst_stage_mask(to.stage)
                .dst_access_mask(to.access)
                .old_layout(from.layout)
                .new_layout(to.layout)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(raw)
                .subresource_range(range);
            if let Some(transfer) = barrier.queue_transfer {
                let families = (
                    self.device.queue_family(transfer.from),
                    self.device.queue_family(transfer.to),
                );
                let (Ok(from_family), Ok(to_family)) = families else {
                    drop(state);
                    self.fail(queue_transfer_failure("image", families));
                    return;
                };
                vk_barrier = vk_barrier
                    .src_queue_family_index(from_family)
                    .dst_queue_family_index(to_family);
            }
            image_barriers.push(vk_barrier);
        }
        drop(state);

        // The raw values were moved into the barriers above, so read them back
        // for the reference list — this is the last point before the driver
        // call where every resolved object is still named by this command.
        for barrier in &buffer_barriers {
            self.use_object(barrier.buffer.as_raw());
        }
        for barrier in &image_barriers {
            self.use_object(barrier.image.as_raw());
        }

        // The sledgehammer the seam documents: a dependency the graph knows
        // exists but cannot attribute to a resource.
        let memory_barriers = if barriers.global {
            vec![
                vk::MemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                    .src_access_mask(vk::AccessFlags2::MEMORY_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                    .dst_access_mask(
                        vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE,
                    ),
            ]
        } else {
            Vec::new()
        };
        if memory_barriers.is_empty() && buffer_barriers.is_empty() && image_barriers.is_empty() {
            // An empty `Barriers` is legal and does nothing, says the seam.
            return;
        }

        let dependency = vk::DependencyInfo::default()
            .memory_barriers(&memory_barriers)
            .buffer_memory_barriers(&buffer_barriers)
            .image_memory_barriers(&image_barriers);
        // SAFETY: `self.raw` is recording, and every handle in `dependency` was
        // resolved against this device's tables above.
        unsafe { self.device.raw.cmd_pipeline_barrier2(self.raw, &dependency) };
    }

    // --- copies ---

    fn copy_buffer_to_buffer(&mut self, copy: &BufferCopy) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let (Ok(src), Ok(dst)) = (
            self.device.buffer_raw(&state, copy.src),
            self.device.buffer_raw(&state, copy.dst),
        ) else {
            drop(state);
            self.fail(HalError::invalid_handle("buffer", copy.src));
            return;
        };
        drop(state);
        self.use_object(src.as_raw());
        self.use_object(dst.as_raw());
        let region = vk::BufferCopy {
            src_offset: copy.src_offset,
            dst_offset: copy.dst_offset,
            size: copy.size,
        };
        // SAFETY: `self.raw` is recording and both buffers are live.
        unsafe {
            self.device
                .raw
                .cmd_copy_buffer(self.raw, src, dst, &[region]);
        }
    }

    fn copy_buffer_to_image(&mut self, copy: &BufferImageCopy) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let (Ok(buffer), Ok((image, _))) = (
            self.device.buffer_raw(&state, copy.buffer),
            self.device.image_raw(&state, copy.image),
        ) else {
            drop(state);
            self.fail(HalError::invalid_handle("buffer or image", copy.buffer));
            return;
        };
        drop(state);
        self.use_object(buffer.as_raw());
        self.use_object(image.as_raw());
        let region = buffer_image_region(copy);
        // SAFETY: `self.raw` is recording, both objects are live, and the image
        // is in `TRANSFER_DST_OPTIMAL` because the graph put it there — the
        // seam is explicit that this layer never infers a barrier.
        unsafe {
            self.device.raw.cmd_copy_buffer_to_image(
                self.raw,
                buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
        }
    }

    fn copy_image_to_buffer(&mut self, copy: &BufferImageCopy) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let (Ok(buffer), Ok((image, _))) = (
            self.device.buffer_raw(&state, copy.buffer),
            self.device.image_raw(&state, copy.image),
        ) else {
            drop(state);
            self.fail(HalError::invalid_handle("buffer or image", copy.buffer));
            return;
        };
        drop(state);
        self.use_object(image.as_raw());
        self.use_object(buffer.as_raw());
        let region = buffer_image_region(copy);
        // SAFETY: as above, with the image in `TRANSFER_SRC_OPTIMAL`.
        unsafe {
            self.device.raw.cmd_copy_image_to_buffer(
                self.raw,
                image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer,
                &[region],
            );
        }
    }

    fn copy_image_to_image(&mut self, copy: &ImageCopy) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let (Ok((src, _)), Ok((dst, _))) = (
            self.device.image_raw(&state, copy.src),
            self.device.image_raw(&state, copy.dst),
        ) else {
            drop(state);
            self.fail(HalError::invalid_handle("image", copy.src));
            return;
        };
        drop(state);
        self.use_object(src.as_raw());
        self.use_object(dst.as_raw());
        let region = vk::ImageCopy::default()
            .src_subresource(conv::subresource_layers(copy.src_subresource))
            .src_offset(vk::Offset3D {
                x: copy.src_offset.x,
                y: copy.src_offset.y,
                z: copy.src_offset.z,
            })
            .dst_subresource(conv::subresource_layers(copy.dst_subresource))
            .dst_offset(vk::Offset3D {
                x: copy.dst_offset.x,
                y: copy.dst_offset.y,
                z: copy.dst_offset.z,
            })
            .extent(vk::Extent3D {
                width: copy.extent.width,
                height: copy.extent.height,
                depth: copy.extent.depth_or_layers,
            });
        // SAFETY: `self.raw` is recording; both images are live and in the
        // layouts the graph transitioned them to.
        unsafe {
            self.device.raw.cmd_copy_image(
                self.raw,
                src,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                dst,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
        }
    }

    fn fill_buffer(&mut self, buffer: BufferHandle, offset: u64, size: u64, value: u32) {
        if !self.ok() {
            return;
        }
        // `vkCmdFillBuffer` requires both to be multiples of 4 — it writes
        // `u32`s. Unchecked, a misaligned zeroing of an indirect count buffer
        // is a VU violation rather than the descriptor error the seam promises.
        // `VK_WHOLE_SIZE` is the one legal non-multiple.
        if !offset.is_multiple_of(4) || (size != vk::WHOLE_SIZE && !size.is_multiple_of(4)) {
            self.fail(HalError::InvalidDescriptor(format!(
                "fill_buffer offset {offset} and size {size} must both be multiples of 4"
            )));
            return;
        }
        let state = self.device.state();
        let Ok(raw) = self.device.buffer_raw(&state, buffer) else {
            drop(state);
            self.fail(HalError::invalid_handle("buffer", buffer));
            return;
        };
        drop(state);
        self.use_object(raw.as_raw());
        // SAFETY: `self.raw` is recording and `raw` is live.
        unsafe {
            self.device
                .raw
                .cmd_fill_buffer(self.raw, raw, offset, size, value);
        }
    }

    // --- render scope ---

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

        let state = self.device.state();
        let mut colors = Vec::with_capacity(desc.color_attachments.len());
        for attachment in desc.color_attachments {
            let Ok((view, _)) = self.device.view_raw(&state, attachment.view) else {
                drop(state);
                self.fail(HalError::invalid_handle("image view", attachment.view));
                return;
            };
            let resolve = match attachment.resolve {
                Some(handle) => match self.device.view_raw(&state, handle) {
                    Ok((raw, _)) => Some(raw),
                    Err(_) => {
                        drop(state);
                        self.fail(HalError::invalid_handle("image view", handle));
                        return;
                    }
                },
                None => None,
            };
            let mut info = vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(conv::load_op(attachment.load))
                .store_op(conv::store_op(attachment.store))
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: attachment.clear.color,
                    },
                });
            if let Some(resolve) = resolve {
                info = info
                    .resolve_mode(vk::ResolveModeFlags::AVERAGE)
                    .resolve_image_view(resolve)
                    .resolve_image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
            }
            colors.push(info);
        }

        let depth_stencil = match desc.depth_stencil_attachment {
            Some(attachment) => match self.device.view_raw(&state, attachment.view) {
                Ok((view, format)) => Some((view, format, attachment)),
                Err(_) => {
                    drop(state);
                    self.fail(HalError::invalid_handle("image view", attachment.view));
                    return;
                }
            },
            None => None,
        };
        drop(state);

        // The views went into the rendering info above; read them back for the
        // reference list, plus the depth/stencil view if the pass has one.
        for info in &colors {
            self.use_object(info.image_view.as_raw());
            if !info.resolve_image_view.is_null() {
                self.use_object(info.resolve_image_view.as_raw());
            }
        }
        if let Some((view, _, _)) = &depth_stencil {
            self.use_object(view.as_raw());
        }

        // **The layout must match the one the image is actually in.** The graph
        // transitions a read-only depth attachment to
        // `DEPTH_STENCIL_READ_ONLY_OPTIMAL` (`conv::state_masks` for
        // `ResourceState::DepthStencilRead`, which `PassBuilder::depth_read`
        // emits), and hardcoding `DEPTH_STENCIL_ATTACHMENT_OPTIMAL` here began
        // rendering with an attachment layout that did not match — a VU
        // violation that breaks the "zero validation errors" gate and leaves
        // the attachment contents undefined. `DepthStencilAttachment::read_only`
        // is how the seam says which of the two states it declared.
        let depth_layout = |read_only: bool| {
            if read_only {
                vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL
            } else {
                vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL
            }
        };
        let depth_info = depth_stencil.map(|(view, _, attachment)| {
            vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(depth_layout(attachment.read_only))
                .load_op(conv::load_op(attachment.depth_load))
                .store_op(conv::store_op(attachment.depth_store))
                .clear_value(vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        // Reversed-Z: the seam's default is `depth::CLEAR`, and
                        // this backend passes it through untouched. Clearing to
                        // 1.0 with a `Greater` compare renders nothing at all.
                        depth: attachment.clear.depth,
                        stencil: attachment.clear.stencil,
                    },
                })
        });
        let stencil_info = depth_stencil
            // Keyed off the **format**, not the ops. A `D32Float` attachment
            // has no stencil plane to load or store however the caller set
            // `stencil_load`/`stencil_store`, and handing `pStencilAttachment`
            // a depth-only view is a VU violation — which is exactly what a
            // caller filling the stencil fields unconditionally used to get.
            .filter(|(_, format, _)| format.has_stencil())
            .map(|(view, _, attachment)| {
                vk::RenderingAttachmentInfo::default()
                    .image_view(view)
                    .image_layout(depth_layout(attachment.read_only))
                    .load_op(conv::load_op(attachment.stencil_load))
                    .store_op(conv::store_op(attachment.stencil_store))
                    .clear_value(vk::ClearValue {
                        depth_stencil: vk::ClearDepthStencilValue {
                            depth: attachment.clear.depth,
                            stencil: attachment.clear.stencil,
                        },
                    })
            });

        let mut info = vk::RenderingInfo::default()
            .render_area(vk::Rect2D {
                offset: vk::Offset2D {
                    x: desc.render_area.x,
                    y: desc.render_area.y,
                },
                extent: vk::Extent2D {
                    width: desc.render_area.width,
                    height: desc.render_area.height,
                },
            })
            .layer_count(1)
            .color_attachments(&colors);
        if let Some(depth) = depth_info.as_ref() {
            info = info.depth_attachment(depth);
        }
        if let Some(stencil) = stencil_info.as_ref() {
            info = info.stencil_attachment(stencil);
        }

        if let Some(label) = desc.label {
            self.begin_debug_label(label);
            self.render_pass_label = true;
        }
        // SAFETY: `self.raw` is recording outside any pass, every view is live,
        // and the attachments are in the layouts the graph transitioned them to
        // — which the seam makes the caller's responsibility explicitly.
        unsafe { self.device.raw.cmd_begin_rendering(self.raw, &info) };
        self.in_render_pass = true;
    }

    fn end_render_pass(&mut self) {
        // `recording`, not `ok`: an already-failed encoder still has to close
        // its rendering scope before `vkEndCommandBuffer`.
        if !self.recording() || !self.in_render_pass {
            return;
        }
        // SAFETY: `self.raw` is recording inside a rendering scope.
        unsafe { self.device.raw.cmd_end_rendering(self.raw) };
        self.in_render_pass = false;
        if core::mem::take(&mut self.render_pass_label) {
            self.end_debug_label();
        }
    }

    fn set_viewport(&mut self, viewport: &Viewport) {
        if !self.ok() {
            return;
        }
        // Negative height flips Y so the seam's coordinate convention survives
        // Vulkan's inverted clip space. `depth_min`/`depth_max` are passed
        // through unchanged: reversed-Z comes from the *projection matrix*, and
        // inverting the viewport range here would apply it twice.
        let raw = vk::Viewport {
            x: viewport.x,
            y: viewport.y + viewport.height,
            width: viewport.width,
            height: -viewport.height,
            min_depth: viewport.depth_min,
            max_depth: viewport.depth_max,
        };
        // SAFETY: `self.raw` is recording.
        unsafe { self.device.raw.cmd_set_viewport(self.raw, 0, &[raw]) };
    }

    fn set_scissor(&mut self, rect: &Rect2d) {
        if !self.ok() {
            return;
        }
        let raw = vk::Rect2D {
            offset: vk::Offset2D {
                x: rect.x,
                y: rect.y,
            },
            extent: vk::Extent2D {
                width: rect.width,
                height: rect.height,
            },
        };
        // SAFETY: `self.raw` is recording.
        unsafe { self.device.raw.cmd_set_scissor(self.raw, 0, &[raw]) };
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        if !self.ok() {
            return;
        }
        // SAFETY: `self.raw` is recording.
        unsafe {
            self.device.raw.cmd_set_stencil_reference(
                self.raw,
                vk::StencilFaceFlags::FRONT_AND_BACK,
                reference,
            );
        }
    }

    fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        let Some(raw) = self.resolve_pipeline(pipeline.cast(), "graphics pipeline") else {
            return;
        };
        // SAFETY: `self.raw` is recording inside a rendering scope and `raw` is
        // a live graphics pipeline of this device.
        unsafe {
            self.device
                .raw
                .cmd_bind_pipeline(self.raw, vk::PipelineBindPoint::GRAPHICS, raw);
        }
    }

    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64, format: IndexFormat) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let Ok(raw) = self.device.buffer_raw(&state, buffer) else {
            drop(state);
            self.fail(HalError::invalid_handle("buffer", buffer));
            return;
        };
        drop(state);
        self.use_object(raw.as_raw());
        let index_type = match format {
            IndexFormat::Uint16 => vk::IndexType::UINT16,
            IndexFormat::Uint32 => vk::IndexType::UINT32,
        };
        // SAFETY: `self.raw` is recording inside a render pass and `raw` is a
        // live buffer with `INDEX` usage.
        unsafe {
            self.device
                .raw
                .cmd_bind_index_buffer(self.raw, raw, offset, index_type);
        }
    }

    // --- bindings ---

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
        let bind_point = self.current_bind_point();
        let state = self.device.state();
        let (record, pool, group_layout) =
            match crate::device::lookup(&state.bind_groups, "bind group", group, &self.device) {
                Ok(entry) => (entry.raw, entry.pool, entry.layout),
                Err(error) => {
                    drop(state);
                    self.fail(error);
                    return;
                }
            };
        // The alignments `adapter.rs` reports and nothing used to read. A
        // dynamic offset is added to the descriptor's own offset before the
        // shader sees it, so a misaligned one is
        // `VUID-vkCmdBindDescriptorSets-pDynamicOffsets-01971` in the driver
        // rather than the descriptor error the seam promises. The seam says
        // `dynamic_offsets` is "in binding order", so the layout's own
        // dynamic-offset bindings, in that order, say which alignment each
        // offset must satisfy.
        let dynamic_kinds = match crate::device::lookup(
            &state.bind_group_layouts,
            "bind group layout",
            group_layout,
            &self.device,
        ) {
            Ok(record) => {
                let mut kinds: Vec<crcbl_hal::BindGroupLayoutEntry> = record
                    .entries
                    .iter()
                    .copied()
                    .filter(|entry| {
                        matches!(
                            entry.kind,
                            crcbl_hal::BindingKind::UniformBuffer { dynamic: true }
                                | crcbl_hal::BindingKind::StorageBuffer { dynamic: true, .. }
                        )
                    })
                    .collect();
                kinds.sort_by_key(|entry| entry.binding);
                kinds
            }
            Err(error) => {
                drop(state);
                self.fail(error);
                return;
            }
        };
        if let Err(error) =
            check_dynamic_offsets(&self.device.caps.limits, &dynamic_kinds, dynamic_offsets)
        {
            drop(state);
            self.fail(error);
            return;
        }
        let layout_raw = match crate::device::lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            layout,
            &self.device,
        ) {
            Ok(record) => record.raw,
            Err(error) => {
                drop(state);
                self.fail(error);
                return;
            }
        };
        drop(state);
        // The descriptor-set raw itself is never parked — `destroy_bind_group`
        // frees the pool the set came from — so the pool is the object whose
        // retirement this bind must extend, alongside the pipeline layout.
        self.use_object(pool.as_raw());
        self.use_object(layout_raw.as_raw());
        // SAFETY: `self.raw` is recording, `record` is a live descriptor set,
        // and `layout_raw` is the layout this group was created for.
        unsafe {
            self.device.raw.cmd_bind_descriptor_sets(
                self.raw,
                bind_point,
                layout_raw,
                slot,
                &[record],
                dynamic_offsets,
            );
        }
    }

    fn push_constants(
        &mut self,
        stages: ShaderStages,
        offset: u32,
        data: &[u8],
        layout: PipelineLayoutHandle,
    ) {
        if !self.ok() {
            return;
        }
        if data.is_empty() {
            return;
        }
        if !data.len().is_multiple_of(4) || !offset.is_multiple_of(4) {
            self.fail(HalError::InvalidDescriptor(format!(
                "push_constants writes {} bytes at offset {offset}; both must be multiples of 4",
                data.len()
            )));
            return;
        }
        let state = self.device.state();
        let layout_entry = match crate::device::lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            layout,
            &self.device,
        ) {
            Ok(entry) => entry,
            Err(error) => {
                drop(state);
                self.fail(error);
                return;
            }
        };
        let push_constant_stages = layout_entry.push_constant_stages;
        let push_constant_size = layout_entry.push_constant_size;
        let layout_raw = layout_entry.raw;
        drop(state);

        if push_constant_stages.is_empty() {
            // The seam is explicit that a backend must fail loudly rather than
            // dropping the write.
            self.fail(HalError::InvalidDescriptor(
                "push_constants on a layout that declares no push-constant range".to_string(),
            ));
            return;
        }
        let _ = stages;
        #[allow(clippy::cast_possible_truncation)]
        let end = offset.saturating_add(data.len() as u32);
        if end > push_constant_size {
            self.fail(HalError::InvalidDescriptor(format!(
                "push_constants writes {offset}..{end}, but the pipeline layout declares only \
                 {} bytes",
                push_constant_size
            )));
            return;
        }
        self.use_object(layout_raw.as_raw());
        // The layout's stages, not the caller's: `vkCmdPushConstants` requires
        // the mask to match the declared range exactly, while the seam lets a
        // caller name a subset. Passing the caller's would be a validation error
        // for a call that is, at the seam's level, correct.
        // SAFETY: `self.raw` is recording, `layout_raw` is live, the stage
        // mask is the one the layout declared, and the range was checked to be
        // 4-byte aligned. Its upper bound was checked against
        // `max_push_constant_size` when the layout was created.
        unsafe {
            self.device.raw.cmd_push_constants(
                self.raw,
                layout_raw,
                push_constant_stages,
                offset,
                data,
            );
        }
    }

    // --- draws ---

    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        if !self.ok() {
            return;
        }
        // SAFETY: `self.raw` is recording inside a render pass with a graphics
        // pipeline bound. The seam makes both the caller's responsibility, and
        // the validation layer is what holds them to it — this layer never
        // infers a pass or a pipeline.
        unsafe {
            self.device.raw.cmd_draw(
                self.raw,
                vertices.len() as u32,
                instances.len() as u32,
                vertices.start,
                instances.start,
            );
        }
    }

    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        if !self.ok() {
            return;
        }
        // SAFETY: as `draw`.
        unsafe {
            self.device.raw.cmd_draw_indexed(
                self.raw,
                indices.len() as u32,
                instances.len() as u32,
                indices.start,
                base_vertex,
                instances.start,
            );
        }
    }

    fn draw_indirect(&mut self, draw: &DrawIndirect) {
        self.indirect(draw, false);
    }

    fn draw_indexed_indirect(&mut self, draw: &DrawIndirect) {
        self.indirect(draw, true);
    }

    fn draw_indirect_count(&mut self, draw: &DrawIndirectCount) {
        self.indirect_count(draw, false);
    }

    fn draw_indexed_indirect_count(&mut self, draw: &DrawIndirectCount) {
        self.indirect_count(draw, true);
    }

    // --- compute scope ---

    fn begin_compute_pass(&mut self, desc: &ComputePassDesc<'_>) {
        if !self.ok() {
            return;
        }
        if self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(
                "begin_compute_pass inside a compute pass; passes do not nest".to_string(),
            ));
            return;
        }
        // A compute pass has no Vulkan object: it exists in the seam purely to
        // give the pass a name and a timestamp boundary.
        if let Some(label) = desc.label {
            self.begin_debug_label(label);
            self.compute_pass_label = true;
        }
        self.in_compute_pass = true;
    }

    fn end_compute_pass(&mut self) {
        if !self.in_compute_pass {
            return;
        }
        // A compute pass has no Vulkan scope to close, only a label.
        self.in_compute_pass = false;
        if core::mem::take(&mut self.compute_pass_label) {
            self.end_debug_label();
        }
    }

    fn bind_compute_pipeline(&mut self, pipeline: ComputePipelineHandle) {
        let Some(raw) = self.resolve_pipeline(pipeline.cast(), "compute pipeline") else {
            return;
        };
        // SAFETY: `self.raw` is recording and `raw` is a live compute pipeline
        // of this device.
        unsafe {
            self.device
                .raw
                .cmd_bind_pipeline(self.raw, vk::PipelineBindPoint::COMPUTE, raw);
        }
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        if !self.ok() {
            return;
        }
        // SAFETY: `self.raw` is recording with a compute pipeline bound, which
        // the seam makes the caller's responsibility, as for `draw`.
        unsafe { self.device.raw.cmd_dispatch(self.raw, x, y, z) };
    }

    fn dispatch_indirect(&mut self, args: BufferHandle, offset: u64) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let Ok(raw) = self.device.buffer_raw(&state, args) else {
            drop(state);
            self.fail(HalError::invalid_handle("buffer", args));
            return;
        };
        drop(state);
        self.use_object(raw.as_raw());
        // SAFETY: `self.raw` is recording and `raw` is a live buffer.
        unsafe {
            self.device.raw.cmd_dispatch_indirect(self.raw, raw, offset);
        }
    }

    // --- queries ---

    fn reset_query_set(&mut self, set: QuerySetHandle, range: Range<u32>) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let Ok((raw, _, count)) = self.device.query_set_raw(&state, set) else {
            drop(state);
            self.fail(HalError::invalid_handle("query set", set));
            return;
        };
        if range.end > count {
            drop(state);
            self.fail(HalError::InvalidDescriptor(format!(
                "query range {}..{} exceeds the set's {count} queries",
                range.start, range.end
            )));
            return;
        }
        drop(state);
        self.use_object(raw.as_raw());
        // SAFETY: `self.raw` is recording outside a pass, `raw` is live, and
        // the range was bounds-checked against the pool's query count.
        unsafe {
            self.device
                .raw
                .cmd_reset_query_pool(self.raw, raw, range.start, range.len() as u32);
        }
    }

    fn write_timestamp(&mut self, set: QuerySetHandle, index: u32) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let Ok((raw, _, count)) = self.device.query_set_raw(&state, set) else {
            drop(state);
            // Accepted and dropped without the feature — but a *bad handle* is
            // still a bug worth reporting.
            self.fail(HalError::invalid_handle("query set", set));
            return;
        };
        if index >= count {
            drop(state);
            self.fail(HalError::InvalidDescriptor(format!(
                "query index {index} exceeds the set's {count} queries"
            )));
            return;
        }
        drop(state);
        self.use_object(raw.as_raw());
        // SAFETY: `self.raw` is recording, `raw` is a live timestamp pool
        // whose query `index` was bounds-checked against its count, and the
        // query has been reset.
        unsafe {
            self.device.raw.cmd_write_timestamp2(
                self.raw,
                vk::PipelineStageFlags2::ALL_COMMANDS,
                raw,
                index,
            );
        }
    }

    fn resolve_query_set(
        &mut self,
        set: QuerySetHandle,
        range: Range<u32>,
        dst: BufferHandle,
        dst_offset: u64,
    ) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let (Ok((raw, _, count)), Ok(buffer)) = (
            self.device.query_set_raw(&state, set),
            self.device.buffer_raw(&state, dst),
        ) else {
            drop(state);
            self.fail(HalError::invalid_handle("query set or buffer", dst));
            return;
        };
        if range.end > count {
            drop(state);
            self.fail(HalError::InvalidDescriptor(format!(
                "query range {}..{} exceeds the set's {count} queries",
                range.start, range.end
            )));
            return;
        }
        drop(state);
        self.use_object(raw.as_raw());
        self.use_object(buffer.as_raw());
        // SAFETY: `self.raw` is recording outside a pass; both objects are
        // live, the range was bounds-checked against the pool's query count,
        // and `dst` has `QUERY_RESOLVE`/`TRANSFER_DST` usage.
        unsafe {
            self.device.raw.cmd_copy_query_pool_results(
                self.raw,
                raw,
                range.start,
                range.len() as u32,
                buffer,
                dst_offset,
                core::mem::size_of::<u64>() as u64,
                vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
            );
        }
    }

    // --- finish ---

    fn finish(mut self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
        if self.in_render_pass {
            self.fail(HalError::InvalidDescriptor(
                "finish with a render pass still open".to_string(),
            ));
            // Report it *and* close it: an unclosed `vkCmdBeginRendering` makes
            // `vkEndCommandBuffer` itself illegal, which would bury the real
            // error under a second, less useful one.
            self.end_render_pass();
        }
        if self.in_compute_pass {
            self.fail(HalError::InvalidDescriptor(
                "finish with a compute pass still open".to_string(),
            ));
            self.end_compute_pass();
        }
        while self.label_depth > 0 {
            self.end_debug_label();
        }
        if self.raw != vk::CommandBuffer::null() {
            // SAFETY: `self.raw` is recording and is ended exactly once.
            if let Err(error) = unsafe { self.device.raw.end_command_buffer(self.raw) } {
                self.fail(conv::hal_error("vkEndCommandBuffer", error));
            }
        }
        if let Some(error) = self.failed.take() {
            return Err(error);
        }

        // Ownership of the pool moves into the device's table, so `Drop` must
        // not free it as well.
        let pool = core::mem::replace(&mut self.pool, vk::CommandPool::null());
        let raw = self.raw;
        let references = core::mem::take(&mut self.references);
        Ok(self
            .device
            .stamp(
                self.device
                    .state()
                    .command_buffers_mut()
                    .insert(CommandBufferEntry {
                        owner: self.device.id,
                        raw,
                        pool,
                        references,
                    }),
            ))
    }
}

impl Drop for VkCommandEncoder {
    fn drop(&mut self) {
        // Only reached when `finish` was never called or failed: a successful
        // `finish` hands the pool to the device's table and nulls this one.
        if self.pool == vk::CommandPool::null() {
            return;
        }
        // Nothing was submitted — a command buffer that never reached a queue
        // cannot be executing — so freeing inline is safe and immediate.
        // SAFETY: the pool was created by this device and holds only this
        // never-submitted buffer.
        unsafe { self.device.raw.destroy_command_pool(self.pool, None) };
    }
}

impl VkCommandEncoder {
    /// Resolves a pipeline handle to its `VkPipeline`.
    ///
    /// Nothing is remembered about it any more: `bind_group` and
    /// `push_constants` take an explicit [`PipelineLayoutHandle`], so the
    /// bound-pipeline tracking that used to live here — and the
    /// `PipelineEntry::layout` it copied — had no reader left.
    fn resolve_pipeline(
        &mut self,
        pipeline: Handle<crcbl_hal::GraphicsPipeline>,
        kind: &'static str,
    ) -> Option<vk::Pipeline> {
        if !self.ok() {
            return None;
        }
        let state = self.device.state();
        let resolved = crate::device::lookup(&state.pipelines, kind, pipeline, &self.device)
            .map(|entry| entry.raw);
        drop(state);
        match resolved {
            Ok(resolved) => {
                self.use_object(resolved.as_raw());
                Some(resolved)
            }
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    /// Which bind point `bind_group` and `push_constants` address.
    ///
    /// The open scope is the only signal the seam carries — see the module
    /// docs. Outside a compute pass this is graphics, which is also the right
    /// answer for a bind issued before `begin_render_pass`.
    fn current_bind_point(&self) -> vk::PipelineBindPoint {
        if self.in_compute_pass {
            vk::PipelineBindPoint::COMPUTE
        } else {
            vk::PipelineBindPoint::GRAPHICS
        }
    }

    fn indirect(&mut self, draw: &DrawIndirect, indexed: bool) {
        if !self.ok() {
            return;
        }
        let state = self.device.state();
        let Ok(args) = self.device.buffer_raw(&state, draw.args) else {
            drop(state);
            self.fail(HalError::invalid_handle("buffer", draw.args));
            return;
        };
        drop(state);
        self.use_object(args.as_raw());
        // SAFETY: `self.raw` is recording inside a render pass and `args` is a
        // live buffer with `INDIRECT` usage in `IndirectArgument` state.
        unsafe {
            if indexed {
                self.device.raw.cmd_draw_indexed_indirect(
                    self.raw,
                    args,
                    draw.offset,
                    draw.draw_count,
                    draw.stride,
                );
            } else {
                self.device.raw.cmd_draw_indirect(
                    self.raw,
                    args,
                    draw.offset,
                    draw.draw_count,
                    draw.stride,
                );
            }
        }
    }

    fn indirect_count(&mut self, draw: &DrawIndirectCount, indexed: bool) {
        if !self.ok() {
            return;
        }
        if !self
            .device
            .caps
            .features
            .contains(crcbl_hal::Features::DRAW_INDIRECT_COUNT)
        {
            self.fail(HalError::Unsupported {
                backend: crcbl_hal::BackendKind::Vulkan,
                what: "this device has no draw_indirect_count; use the Tier B path",
            });
            return;
        }
        let state = self.device.state();
        let (Ok(args), Ok(count)) = (
            self.device.buffer_raw(&state, draw.args),
            self.device.buffer_raw(&state, draw.count_buffer),
        ) else {
            drop(state);
            self.fail(HalError::invalid_handle("buffer", draw.args));
            return;
        };
        drop(state);
        self.use_object(args.as_raw());
        self.use_object(count.as_raw());
        // SAFETY: `self.raw` is recording inside a render pass; both buffers
        // are live, have `INDIRECT` usage and are in `IndirectArgument` state.
        unsafe {
            if indexed {
                self.device.raw.cmd_draw_indexed_indirect_count(
                    self.raw,
                    args,
                    draw.args_offset,
                    count,
                    draw.count_offset,
                    draw.max_draw_count,
                    draw.stride,
                );
            } else {
                self.device.raw.cmd_draw_indirect_count(
                    self.raw,
                    args,
                    draw.args_offset,
                    count,
                    draw.count_offset,
                    draw.max_draw_count,
                    draw.stride,
                );
            }
        }
    }
}

/// Checks the dynamic offsets a bind names against the layout that declared the
/// slots they fill.
///
/// Pure, so the alignment policy — which is one of the two `Limits` fields the
/// backend populated and never read — has a unit test rather than a GPU.
fn check_dynamic_offsets(
    limits: &crcbl_hal::Limits,
    dynamic: &[crcbl_hal::BindGroupLayoutEntry],
    offsets: &[u32],
) -> Result<(), HalError> {
    if offsets.len() != dynamic.len() {
        return Err(HalError::InvalidDescriptor(format!(
            "bind_group was given {} dynamic offset(s) but the layout declares {} \
             dynamic-offset binding(s)",
            offsets.len(),
            dynamic.len()
        )));
    }
    for (entry, offset) in dynamic.iter().zip(offsets) {
        let alignment = match entry.kind {
            crcbl_hal::BindingKind::UniformBuffer { .. } => {
                limits.min_uniform_buffer_offset_alignment
            }
            _ => limits.min_storage_buffer_offset_alignment,
        };
        if alignment > 1 && !u64::from(*offset).is_multiple_of(alignment) {
            return Err(HalError::InvalidDescriptor(format!(
                "dynamic offset {offset} for binding {} is not a multiple of this device's \
                 {alignment}-byte alignment",
                entry.binding
            )));
        }
    }
    Ok(())
}

/// The error a barrier reports when a requested queue-ownership transfer names
/// a queue this device does not have.
///
/// Dropping it silently is the one outcome that must not happen. The barrier
/// would go out with `QUEUE_FAMILY_IGNORED` on both sides — no transfer at all
/// — and the resource would then be used on the other family with **undefined
/// contents**, which is a corruption bug that reproduces on exactly the
/// machines that have a transfer queue. Every other resolution failure in this
/// file calls `fail`; so does this one now.
fn queue_transfer_failure(
    what: &'static str,
    families: (Result<u32, HalError>, Result<u32, HalError>),
) -> HalError {
    match families {
        (Err(error), _) | (_, Err(error)) => HalError::InvalidDescriptor(format!(
            "a {what} barrier asked for a queue-family transfer this device cannot perform: \
             {error}"
        )),
        (Ok(_), Ok(_)) => unreachable!("only called when one of the two failed"),
    }
}

/// The shared body of the two buffer↔image copies.
fn buffer_image_region(copy: &BufferImageCopy) -> vk::BufferImageCopy {
    vk::BufferImageCopy {
        buffer_offset: copy.buffer_offset,
        buffer_row_length: copy.buffer_row_length,
        buffer_image_height: copy.buffer_image_height,
        image_subresource: conv::subresource_layers(copy.image_subresource),
        image_offset: vk::Offset3D {
            x: copy.image_offset.x,
            y: copy.image_offset.y,
            z: copy.image_offset.z,
        },
        image_extent: vk::Extent3D {
            width: copy.image_extent.width,
            height: copy.image_extent.height,
            depth: copy.image_extent.depth_or_layers,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{BindGroupLayoutEntry, BindingKind, Limits, ShaderStages};

    fn dynamic(binding: u32, uniform: bool) -> BindGroupLayoutEntry {
        BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::ALL,
            kind: if uniform {
                BindingKind::UniformBuffer { dynamic: true }
            } else {
                BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: true,
                }
            },
            count: 1,
            flags: crcbl_hal::BindingFlags::empty(),
        }
    }

    /// The two `Limits` fields `adapter.rs` populated and nothing read. A
    /// misaligned dynamic offset used to reach the driver as a VU violation
    /// instead of an `InvalidDescriptor` naming the binding.
    #[test]
    fn dynamic_offsets_are_checked_against_the_right_alignment() {
        let limits = Limits::desktop();
        assert_eq!(limits.min_uniform_buffer_offset_alignment, 64);
        assert_eq!(limits.min_storage_buffer_offset_alignment, 16);
        let slots = [dynamic(0, true), dynamic(1, false)];

        check_dynamic_offsets(&limits, &slots, &[128, 32]).expect("both are aligned");

        // 32 is a legal *storage* offset and an illegal uniform one, which is
        // exactly why one shared alignment would be wrong.
        let error = check_dynamic_offsets(&limits, &slots, &[32, 32])
            .expect_err("32 is not a multiple of 64");
        assert!(error.to_string().contains("binding 0"), "{error}");

        let error = check_dynamic_offsets(&limits, &slots, &[128, 8])
            .expect_err("8 is not a multiple of 16");
        assert!(error.to_string().contains("binding 1"), "{error}");
    }

    /// A count mismatch is a caller bug too: `dynamic_offsets` is positional,
    /// so one too few silently shifts every later offset onto the wrong slot.
    #[test]
    fn the_offset_count_must_match_the_layouts_dynamic_bindings() {
        let limits = Limits::desktop();
        let slots = [dynamic(0, true)];
        assert!(check_dynamic_offsets(&limits, &slots, &[]).is_err());
        assert!(check_dynamic_offsets(&limits, &slots, &[0, 0]).is_err());
        check_dynamic_offsets(&limits, &[], &[]).expect("no dynamic bindings, no offsets");
    }

    /// A transfer that cannot be performed must be reported, never dropped:
    /// the barrier would otherwise go out with `QUEUE_FAMILY_IGNORED` on both
    /// sides and the resource be used on the other family with undefined
    /// contents.
    #[test]
    fn a_queue_transfer_that_cannot_resolve_names_the_failure() {
        let missing = HalError::invalid_handle::<crcbl_hal::Queue>(
            "queue",
            crcbl_core::Handle::from_bits(1 << 32).expect("non-zero generation"),
        );
        let error = queue_transfer_failure("image", (Ok(0), Err(missing)));
        let text = error.to_string();
        assert!(text.contains("image"), "{text}");
        assert!(text.contains("transfer"), "{text}");
    }
}
