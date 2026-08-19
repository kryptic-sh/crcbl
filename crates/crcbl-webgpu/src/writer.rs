//! The encoder: HAL calls in, one wasm-owned byte buffer out.

use core::ops::Range;

use crcbl_hal::{
    Barriers, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindGroupLayoutHandle, BindingKind, BindingResource, BlendState,
    BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferImageCopy, ClearValue,
    ColorAttachment, ColorTargetState, CommandBufferHandle, CommandEncoderDesc, ComputePassDesc,
    ComputePipelineDesc, ComputePipelineHandle, DepthStencilAttachment, DepthStencilState,
    DeviceDesc, Extent3d, GraphicsPipelineDesc, GraphicsPipelineHandle, ImageBarrier, ImageCopy,
    ImageDesc, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange, ImageViewDesc,
    ImageViewHandle, IndexFormat, MultisampleState, Offset3d, PassTimestampWrites,
    PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo, PrimitiveState, QuerySetDesc,
    QuerySetHandle, QueueTransfer, ReadbackDesc, ReadbackHandle, Rect2d, RenderPassDesc,
    SamplerDesc, SamplerHandle, SemaphoreSignal, SemaphoreWait, ShaderModuleDesc,
    ShaderModuleHandle, ShaderStages, StencilFaceState, SubmitInfo, SurfaceHandle, SwapchainDesc,
    SwapchainHandle, Viewport,
};

use crate::bytes::ByteWriter;
use crate::tag;

// ── The command stream's own field writers ────────────────────────────────────
//
// [`ByteWriter`] and its primitives live in [`crate::bytes`], shared with the
// reply direction; these are shaped by `crcbl-hal`'s descriptors and belong to
// the command stream alone. Each is the exact counterpart of a `read_*` in
// [`crate::reader`], which is the pairing the round-trip suite exists to hold.

impl ByteWriter {
    fn put_clear_value(&mut self, clear: ClearValue) {
        for channel in clear.color {
            self.put_f32(channel);
        }
        self.put_f32(clear.depth);
        self.put_u32(clear.stencil);
    }

    fn put_rect(&mut self, rect: Rect2d) {
        self.put_i32(rect.x);
        self.put_i32(rect.y);
        self.put_u32(rect.width);
        self.put_u32(rect.height);
    }

    fn put_extent(&mut self, extent: Extent3d) {
        self.put_u32(extent.width);
        self.put_u32(extent.height);
        self.put_u32(extent.depth_or_layers);
    }

    fn put_subresource_range(&mut self, range: ImageSubresourceRange) {
        self.put_u32(range.aspect.bits());
        self.put_u32(range.base_mip);
        self.put_u32(range.mip_count);
        self.put_u32(range.base_layer);
        self.put_u32(range.layer_count);
    }

    /// One [`ImageSubresourceLayers`] — a single mip level across a layer range,
    /// which is what a copy addresses, distinct from the full range above. The
    /// aspect rides the same `bits()` primitive every bitflags field on this
    /// stream uses, so a plane no aspect claims is refused by the reader rather
    /// than truncated.
    fn put_subresource_layers(&mut self, layers: ImageSubresourceLayers) {
        self.put_u32(layers.aspect.bits());
        self.put_u32(layers.mip);
        self.put_u32(layers.base_layer);
        self.put_u32(layers.layer_count);
    }

    /// One [`Offset3d`], as three signed texel offsets — `x`, `y`, `z` — through
    /// [`put_i32`](Self::put_i32), the same primitive [`put_rect`](Self::put_rect)
    /// uses for a rectangle's origin.
    fn put_offset(&mut self, offset: Offset3d) {
        self.put_i32(offset.x);
        self.put_i32(offset.y);
        self.put_i32(offset.z);
    }

    /// One [`SemaphoreWait`]: the handle, then the `u64` value. Shared by
    /// [`SubmitInfo::waits`](crcbl_hal::SubmitInfo::waits) and
    /// [`ReadbackDesc::after`](crcbl_hal::ReadbackDesc::after), which carry the
    /// same pair. The replayer refuses any of these WebGPU cannot express, but
    /// the pair still crosses whole so it round-trips in Rust.
    fn put_semaphore_wait(&mut self, wait: SemaphoreWait) {
        self.put_handle(wait.semaphore);
        self.put_u64(wait.value);
    }

    /// One [`SemaphoreSignal`], on [`put_semaphore_wait`](Self::put_semaphore_wait)'s
    /// terms — a handle and a `u64` value, its own writer only because the HAL
    /// keeps `SemaphoreSignal` and `SemaphoreWait` distinct.
    fn put_semaphore_signal(&mut self, signal: SemaphoreSignal) {
        self.put_handle(signal.semaphore);
        self.put_u64(signal.value);
    }

    /// A barrier's optional [`QueueTransfer`]: a presence byte, then the
    /// releasing and acquiring queue handles if present. The house rule for an
    /// optional field that is not a handle — a `Some` names a queue-family
    /// transfer WebGPU has no queue to honour, but it crosses whole so the batch
    /// round-trips, exactly as a [`ReadbackDesc::after`](crcbl_hal::ReadbackDesc::after)
    /// semaphore does.
    fn put_queue_transfer(&mut self, transfer: Option<QueueTransfer>) {
        match transfer {
            None => self.put_u8(tag::ABSENT),
            Some(transfer) => {
                self.put_u8(tag::PRESENT);
                self.put_handle(transfer.from);
                self.put_handle(transfer.to);
            }
        }
    }

    /// One [`BufferBarrier`]: the buffer, its `from`/`to` states as codes, then
    /// the optional queue transfer.
    fn put_buffer_barrier(&mut self, barrier: &BufferBarrier) {
        self.put_handle(barrier.buffer);
        self.put_u8(tag::resource_state_code(barrier.from));
        self.put_u8(tag::resource_state_code(barrier.to));
        self.put_queue_transfer(barrier.queue_transfer);
    }

    /// One [`ImageBarrier`]: the image, the subresource range it covers, its
    /// `from`/`to` states as codes, then the optional queue transfer. The range
    /// rides the same [`put_subresource_range`](Self::put_subresource_range) every
    /// image command on this stream uses.
    fn put_image_barrier(&mut self, barrier: &ImageBarrier) {
        self.put_handle(barrier.image);
        self.put_subresource_range(barrier.range);
        self.put_u8(tag::resource_state_code(barrier.from));
        self.put_u8(tag::resource_state_code(barrier.to));
        self.put_queue_transfer(barrier.queue_transfer);
    }

    fn put_color_attachment(&mut self, attachment: &ColorAttachment) {
        self.put_handle(attachment.view);
        self.put_opt_handle(attachment.resolve);
        self.put_u8(tag::load_op_code(attachment.load));
        self.put_u8(tag::store_op_code(attachment.store));
        self.put_clear_value(attachment.clear);
    }

    /// One [`BindingKind`]: its code, then that variant's own fields.
    ///
    /// A code plus a body rather than a code alone, because this is the one HAL
    /// enum on this stream whose variants carry data — and the bodies are
    /// different lengths, which is what makes the code byte load-bearing for the
    /// *cursor* and not only for the meaning. A reader that folded two codes
    /// together would go on to read the wrong number of bytes and land inside
    /// the next entry.
    fn put_binding_kind(&mut self, kind: BindingKind) {
        self.put_u8(tag::binding_kind_code(kind));
        match kind {
            BindingKind::UniformBuffer { dynamic } => self.put_bool(dynamic),
            BindingKind::StorageBuffer { read_only, dynamic } => {
                self.put_bool(read_only);
                self.put_bool(dynamic);
            }
            BindingKind::SampledImage {
                view_type,
                sample_type,
            } => {
                self.put_u8(tag::image_view_type_code(view_type));
                self.put_u8(tag::sample_type_code(sample_type));
            }
            BindingKind::StorageImage {
                read_only,
                view_type,
                format,
            } => {
                self.put_bool(read_only);
                self.put_u8(tag::image_view_type_code(view_type));
                self.put_u8(tag::format_code(format));
            }
            BindingKind::Sampler { comparison } => self.put_bool(comparison),
        }
    }

    /// One [`BindGroupLayoutEntry`], in the order the struct declares its fields.
    ///
    /// **`count` and `flags` cross verbatim**, sentinel and all. `count`'s
    /// [`u32::MAX`] means "as many as this device can" and is resolved through
    /// [`BindGroupLayoutEntry::resolved_count`](crcbl_hal::BindGroupLayoutEntry::resolved_count)
    /// against a device's own limits — which is the far side's, not this one's,
    /// exactly as `docs/plan/41-webgpu-stream.md` requires of every sentinel.
    fn put_bind_group_layout_entry(&mut self, entry: &BindGroupLayoutEntry) {
        self.put_u32(entry.binding);
        self.put_u32(entry.visibility.bits());
        self.put_binding_kind(entry.kind);
        self.put_u32(entry.count);
        self.put_u32(entry.flags.bits());
    }

    /// One [`BindingResource`]: its code, then that variant's own fields.
    ///
    /// A code plus a body, and the second such shape on this stream after
    /// [`BindingKind`] — the codes are load-bearing for the *cursor* as well as
    /// for the meaning, because [`BindingResource::Buffer`] carries sixteen bytes
    /// the other two do not. See [`tag::binding_resource_code`].
    fn put_binding_resource(&mut self, resource: BindingResource) {
        self.put_u8(tag::binding_resource_code(resource));
        match resource {
            BindingResource::Buffer {
                buffer,
                offset,
                size,
            } => {
                self.put_handle(buffer);
                self.put_u64(offset);
                self.put_u64(size);
            }
            BindingResource::ImageView(view) => self.put_handle(view),
            BindingResource::Sampler(sampler) => self.put_handle(sampler),
        }
    }

    /// One [`BindGroupEntry`], in the order the struct declares its fields.
    ///
    /// `array_index` crosses beside `binding` and is not folded into it: it is the
    /// bindless write path, and two entries that share a binding and differ only
    /// in it are two distinct assignments the wire must keep apart.
    fn put_bind_group_entry(&mut self, entry: &BindGroupEntry) {
        self.put_u32(entry.binding);
        self.put_u32(entry.array_index);
        self.put_binding_resource(entry.resource);
    }

    /// The optional [`PassTimestampWrites`] a pass descriptor ends with.
    ///
    /// A presence byte then, when present, the set and the two query indices —
    /// the convention every other optional non-handle field on this wire uses.
    /// Shared by both pass commands so the two cannot encode it differently.
    fn put_timestamp_writes(&mut self, writes: Option<PassTimestampWrites>) {
        match writes {
            None => self.put_u8(tag::ABSENT),
            Some(writes) => {
                self.put_u8(tag::PRESENT);
                self.put_handle(writes.set);
                self.put_u32(writes.beginning_of_pass);
                self.put_u32(writes.end_of_pass);
            }
        }
    }

    fn put_depth_stencil_attachment(&mut self, attachment: &DepthStencilAttachment) {
        self.put_handle(attachment.view);
        self.put_bool(attachment.read_only);
        self.put_u8(tag::load_op_code(attachment.depth_load));
        self.put_u8(tag::store_op_code(attachment.depth_store));
        self.put_u8(tag::load_op_code(attachment.stencil_load));
        self.put_u8(tag::store_op_code(attachment.stencil_store));
        self.put_clear_value(attachment.clear);
    }

    /// One [`PrimitiveState`], in the order the struct declares its fields.
    ///
    /// Four leaf enums and a `bool`, all leaf codes: the topology, the winding,
    /// the cull mode and the fill mode each go over as their own table's code, so
    /// a byte read out of position lands on a real variant of the wrong field
    /// rather than on an error — which is what makes the field order load-bearing
    /// and why it is spelled out one line at a time.
    fn put_primitive_state(&mut self, primitive: &PrimitiveState) {
        self.put_u8(tag::primitive_topology_code(primitive.topology));
        self.put_u8(tag::front_face_code(primitive.front_face));
        self.put_u8(tag::cull_mode_code(primitive.cull_mode));
        self.put_u8(tag::polygon_mode_code(primitive.polygon_mode));
        self.put_bool(primitive.depth_clamp);
    }

    /// One [`StencilFaceState`]: its compare op, then its three [`StencilOp`]s in
    /// the order the struct declares them — `fail_op`, `depth_fail_op`, `pass_op`.
    ///
    /// The three ops are the same table three times in a row, so their order is
    /// pinned here the way the sampler's three filters are: any two read in the
    /// wrong order still decodes to a face state that writes the stencil buffer
    /// the wrong way.
    fn put_stencil_face_state(&mut self, face: &StencilFaceState) {
        self.put_u8(tag::compare_op_code(face.compare));
        self.put_u8(tag::stencil_op_code(face.fail_op));
        self.put_u8(tag::stencil_op_code(face.depth_fail_op));
        self.put_u8(tag::stencil_op_code(face.pass_op));
    }

    /// One [`DepthStencilState`], in the order the struct declares its fields.
    ///
    /// **The deepest optional chain on the seam.** The stencil rides a presence
    /// byte — the house rule for an optional field that is not a handle — and,
    /// when present, its `front` and `back` faces go over in that order, distinct
    /// so a front/back swap is visible, followed by the three masks. The bias's
    /// three floats cross as bit patterns through [`f32::to_le_bytes`], as every
    /// float on this stream does.
    ///
    /// **There is no stencil `reference` on the wire.** WebGPU has no such
    /// pipeline member and neither does the seam — the value a draw compares
    /// against is pass state, and
    /// [`set_stencil_reference`](Self::set_stencil_reference) is the only thing
    /// that carries it. The stencil block therefore ends at `write_mask`, and
    /// the bias floats follow immediately; see
    /// [`STREAM_VERSION`](tag::STREAM_VERSION), which this moved.
    fn put_depth_stencil_state(&mut self, state: &DepthStencilState) {
        self.put_u8(tag::format_code(state.format));
        self.put_bool(state.depth_write);
        self.put_u8(tag::compare_op_code(state.depth_compare));
        match &state.stencil {
            None => self.put_u8(tag::ABSENT),
            Some(stencil) => {
                self.put_u8(tag::PRESENT);
                self.put_stencil_face_state(&stencil.front);
                self.put_stencil_face_state(&stencil.back);
                self.put_u32(stencil.read_mask);
                self.put_u32(stencil.write_mask);
            }
        }
        self.put_f32(state.bias.constant);
        self.put_f32(state.bias.slope_scale);
        self.put_f32(state.bias.clamp);
    }

    /// One [`MultisampleState`], in the order the struct declares its fields.
    ///
    /// A `u32` count and a coverage flag, with **no sample mask between them**:
    /// the seam dropped that word, so the replayer fills
    /// `GPUMultisampleState.mask` with every bit itself. An older decoder would
    /// read the count's four bytes and then take the `alpha_to_coverage` byte
    /// and three bytes of the next field as a mask, which is why
    /// [`STREAM_VERSION`](tag::STREAM_VERSION) moved with it.
    fn put_multisample_state(&mut self, multisample: &MultisampleState) {
        self.put_u32(multisample.samples);
        self.put_bool(multisample.alpha_to_coverage);
    }

    /// One [`BlendState`]: colour source/dest/op, then alpha source/dest/op.
    ///
    /// Four [`BlendFactor`](crcbl_hal::BlendFactor) codes and two
    /// [`BlendOp`](crcbl_hal::BlendOp) codes interleaved, all from tables whose
    /// unclaimed codes the reader refuses. The order is the struct's, and it is
    /// load-bearing for the same reason the sampler's is: every one of the six is
    /// a value the other five could hold, so the *colour* half read as the
    /// *alpha* half still builds a target that blends with the wrong equation.
    fn put_blend_state(&mut self, blend: &BlendState) {
        self.put_u8(tag::blend_factor_code(blend.color_src));
        self.put_u8(tag::blend_factor_code(blend.color_dst));
        self.put_u8(tag::blend_op_code(blend.color_op));
        self.put_u8(tag::blend_factor_code(blend.alpha_src));
        self.put_u8(tag::blend_factor_code(blend.alpha_dst));
        self.put_u8(tag::blend_op_code(blend.alpha_op));
    }

    /// One [`ColorTargetState`]: format, an optional blend, and the write mask.
    ///
    /// The blend rides a presence byte — the house rule — so `None` (blending
    /// disabled) is a distinct, shorter body than a `Some` and cannot be confused
    /// with an all-zero [`BlendState`]. `write_mask` crosses as its
    /// [`ColorWrites`](crcbl_hal::ColorWrites) bits, the way every bitflags field
    /// on this stream does, so a bit no channel claims is refused rather than
    /// truncated.
    fn put_color_target_state(&mut self, target: &ColorTargetState) {
        self.put_u8(tag::format_code(target.format));
        match &target.blend {
            None => self.put_u8(tag::ABSENT),
            Some(blend) => {
                self.put_u8(tag::PRESENT);
                self.put_blend_state(blend);
            }
        }
        self.put_u32(target.write_mask.bits());
    }
}

/// Appends commands into the byte buffer wasm owns and JS reads in place.
///
/// One writer per frame's worth of recording: encode during the frame, hand
/// [`bytes`](Self::bytes) to JS at the `requestAnimationFrame` boundary, then
/// [`clear`](Self::clear) for the next frame.
///
/// # Sequence numbers
///
/// Every command has a monotonically increasing sequence number, which is what
/// carries error attribution across the boundary: a WebGPU validation error
/// names the replayer, so without it the Rust that encoded the command cannot be
/// found again. Each encode method returns the number it assigned, for the side
/// map from sequence to opcode and label that renders the pair
/// [`Device::take_error`](crcbl_hal::Device::take_error) hands back.
///
/// **The number is not written per command.** Commands are decoded in order from
/// a buffer that starts at a known point, so the *n*th command's sequence is
/// [`base_sequence`](Self::base_sequence)` + n` — exact, and the only figure on
/// the wire is the base, in the header. A per-command field would cost four
/// bytes times every command in every frame to restate something already
/// implied, and would introduce a second source of truth that can disagree with
/// the first. The counter is 64-bit for the same reason a wire field could not
/// be: at a few thousand commands a frame, 32 bits wraps within hours of play.
///
/// # Panics
///
/// Every method here panics on a field longer than [`tag::MAX_FIELD_BYTES`] or
/// an array longer than [`tag::MAX_ELEMENT_COUNT`]. Those are the caps the
/// reader enforces, so encoding past one would produce a buffer this crate
/// refuses to decode, and a silent truncation would bury the bug rather than
/// report it.
///
/// **This is the layer that asserts, not the layer a caller's data reaches.** On
/// wasm a `panic!` is `unreachable`: the module dies mid-frame and the page is
/// left with a trap where a diagnosis should be. So the HAL seam measures every
/// caller-supplied field whose length is *data* before a byte of the command is
/// written — the `hal::bounds` module — and answers a
/// [`HalError`](crcbl_hal::HalError) the caller can report. The one upload with
/// no bound at all is [`write_buffer`](Self::write_buffer)'s, which splits
/// itself across as many commands as the field cap needs rather than refusing.
/// What is left asserting here is the fields whose length the program that
/// wrote them fixes: a debug label, an entry point name, an attachment list the
/// device's own limits bound.
#[derive(Debug)]
pub struct StreamWriter {
    bytes: ByteWriter,
    /// Sequence of the first command in `buf`. Written into the header, so the
    /// reader can name every command without a per-command field.
    base_sequence: u64,
    /// Sequence the next command will be assigned.
    next_sequence: u64,
}

impl Default for StreamWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamWriter {
    /// A fresh writer holding nothing but a header.
    #[must_use]
    pub fn new() -> Self {
        let mut writer = Self {
            bytes: ByteWriter::with_capacity(tag::HEADER_BYTES),
            base_sequence: 0,
            next_sequence: 0,
        };
        writer.write_header();
        writer
    }

    /// The encoded stream, header included.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes.bytes()
    }

    /// The sequence number of the first command in the current buffer.
    ///
    /// The reader recovers it from the header; a caller needs it to turn the
    /// replayer's "the command at position *n* failed" back into the sequence
    /// its side map is keyed on.
    #[must_use]
    pub const fn base_sequence(&self) -> u64 {
        self.base_sequence
    }

    /// Drops the encoded commands, keeping the sequence counter where it is.
    ///
    /// Sequence numbers are monotonic across frames, not within one: an error
    /// raised by a replayed command surfaces through `take_error` a frame or
    /// more after the frame that encoded it, so a counter that restarted every
    /// frame would name several different commands with the same number.
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.base_sequence = self.next_sequence;
        self.write_header();
    }

    // ── Creation ─────────────────────────────────────────────────────────────

    /// [`Device::create_buffer`](crcbl_hal::Device::create_buffer), with the
    /// handle the caller allocated for it.
    ///
    /// A stream cannot answer during the call, so identity is positional: wasm
    /// takes the id from its own pool, writes it alongside the descriptor, and
    /// returns `Ok(handle)` at once. JS creates the object at replay time and
    /// stores it in the table at that id. Failure arrives out of band through
    /// [`Device::take_error`](crcbl_hal::Device::take_error).
    pub fn create_buffer(&mut self, buffer: BufferHandle, desc: &BufferDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::CREATE_BUFFER_TAG);
        self.bytes.put_handle(buffer);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_u64(desc.size);
        self.bytes.put_u32(desc.usage.bits());
        self.bytes.put_u8(tag::memory_location_code(desc.memory));
        sequence
    }

    /// [`Instance::create_surface`](crcbl_hal::Instance::create_surface), with
    /// the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and the surface is created against the canvas `canvas_id` names
    /// in the shell's JS-side registry.
    ///
    /// **The `u32` is the whole target.** A
    /// [`SurfaceTarget`](crcbl_core::SurfaceTarget) is not taken and cannot be:
    /// four of its variants carry `NonNull` pointers to platform objects, and a
    /// pointer transmitted as an integer is the one thing this seam refuses
    /// outright. Only `Web { canvas_id }` is reachable in a browser, so the
    /// caller unwraps it and the pointer variants stop at the HAL boundary
    /// rather than here.
    pub fn create_surface(&mut self, surface: SurfaceHandle, canvas_id: u32) -> u64 {
        let sequence = self.push_tag(tag::CREATE_SURFACE_TAG);
        self.bytes.put_handle(surface);
        self.bytes.put_u32(canvas_id);
        sequence
    }

    /// [`Instance::create_surface`](crcbl_hal::Instance::create_surface) for
    /// [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen), with
    /// the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason. **The handle is the whole body**, where
    /// [`create_surface`](Self::create_surface) also writes a `canvas_id`: an
    /// offscreen target names no canvas key, and it has no extent or format to
    /// carry either — those belong to the swapchain, not the surface, and reach
    /// the replayer through [`create_swapchain`](Self::create_swapchain). See
    /// [`Command::CreateOffscreenSurface`](crate::Command::CreateOffscreenSurface).
    pub fn create_offscreen_surface(&mut self, surface: SurfaceHandle) -> u64 {
        let sequence = self.push_tag(tag::CREATE_OFFSCREEN_SURFACE_TAG);
        self.bytes.put_handle(surface);
        sequence
    }

    /// [`Device::create_image`](crcbl_hal::Device::create_image), with the
    /// handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason. Fields go on the wire in the order
    /// [`ImageDesc`] declares them, behind the handle, so
    /// the two cannot drift apart unnoticed.
    ///
    /// **Nothing in the descriptor is validated here, and `mip_levels` and
    /// `samples` are the two that invite it**: zero is meaningless for both,
    /// and both cross verbatim anyway. The encoder takes what the caller gives
    /// it, the decoder is where a malformed *stream* is caught, and a
    /// descriptor that no device will accept is a creation failure — which this
    /// seam already has a route for, out of band through
    /// [`Device::take_error`](crcbl_hal::Device::take_error). Refusing here
    /// would have to be a panic, since the call returns `Ok(handle)` before the
    /// replayer has seen anything.
    ///
    /// **There is no memory location on the wire** because
    /// [`ImageDesc`] has no such field: every image the
    /// seam creates is device-local.
    pub fn create_image(&mut self, image: ImageHandle, desc: &ImageDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::CREATE_IMAGE_TAG);
        self.bytes.put_handle(image);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_u8(tag::image_type_code(desc.image_type));
        self.bytes.put_extent(desc.extent);
        self.bytes.put_u8(tag::format_code(desc.format));
        self.bytes.put_u32(desc.mip_levels);
        self.bytes.put_u32(desc.samples);
        self.bytes.put_u32(desc.usage.bits());
        sequence
    }

    /// [`Device::create_image_view`](crcbl_hal::Device::create_image_view), with
    /// the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and fields follow the descriptor's declaration order for
    /// [`create_image`](Self::create_image)'s.
    ///
    /// **`view` and `desc.image` are two handles a body could transpose**, and
    /// they mean opposite things: the first is the id being *filled in*, the
    /// second the id being *read*. The parameter is separate from the
    /// descriptor for the same reason it is on every other creation method
    /// here, so a call site cannot supply one where the other belongs.
    pub fn create_image_view(&mut self, view: ImageViewHandle, desc: &ImageViewDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::CREATE_IMAGE_VIEW_TAG);
        self.bytes.put_handle(view);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_handle(desc.image);
        self.bytes.put_u8(tag::image_view_type_code(desc.view_type));
        self.bytes.put_u8(tag::format_code(desc.format));
        self.bytes.put_subresource_range(desc.range);
        sequence
    }

    /// [`Device::create_sampler`](crcbl_hal::Device::create_sampler), with the
    /// handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and fields follow the descriptor's declaration order for
    /// [`create_image`](Self::create_image)'s — which matters more here than
    /// anywhere else on this stream, because three [`FilterMode`](crcbl_hal::FilterMode)
    /// bytes and three [`SamplerAddressMode`](crcbl_hal::SamplerAddressMode)
    /// bytes go over back to back and every one of the six is a value the other
    /// five could hold.
    ///
    /// **Every float crosses as its bit pattern**, through
    /// [`f32::to_le_bytes`], which is what [`ClearValue`] already does on this
    /// stream and what [`Limits`](crcbl_hal::Limits) does on the reply one. Not
    /// a decimal string and not a widening through `f64`: both are lossy in one
    /// direction or the other, and a mip clamp that arrived a half-ulp off is a
    /// sampler nobody can tell is wrong.
    ///
    /// **Nothing is resolved and nothing is validated.**
    /// [`SamplerDesc::lod_max`](crcbl_hal::SamplerDesc::lod_max) is
    /// [`f32::MAX`] by default and that is a *sentinel*, so it crosses as
    /// itself — the rule `docs/plan/41-webgpu-stream.md` sets for
    /// `WHOLE_BUFFER`. [`anisotropy`](crcbl_hal::SamplerDesc::anisotropy) is
    /// whatever the caller passed, fractional values and values past the
    /// device's cap included: WebGPU's `maxAnisotropy` is an integer and the
    /// narrowing belongs to the half that faces WebGPU, exactly as the usage
    /// words' does. See [`Command::CreateSampler`](crate::Command::CreateSampler).
    pub fn create_sampler(&mut self, sampler: SamplerHandle, desc: &SamplerDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::CREATE_SAMPLER_TAG);
        self.bytes.put_handle(sampler);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_u8(tag::filter_mode_code(desc.mag_filter));
        self.bytes.put_u8(tag::filter_mode_code(desc.min_filter));
        self.bytes.put_u8(tag::filter_mode_code(desc.mip_filter));
        for mode in desc.address_mode {
            self.bytes.put_u8(tag::sampler_address_mode_code(mode));
        }
        self.bytes.put_f32(desc.lod_min);
        self.bytes.put_f32(desc.lod_max);
        self.bytes.put_f32(desc.anisotropy);
        // A presence byte, because `CompareOp` is not a handle and has no niche
        // to spare — the house rule for every optional field that is not one.
        // Folding it into the code table as a reserved "absent" byte would make
        // a corrupt stream's `None` indistinguishable from a drifted table's.
        match desc.compare {
            None => self.bytes.put_u8(tag::ABSENT),
            Some(op) => {
                self.bytes.put_u8(tag::PRESENT);
                self.bytes.put_u8(tag::compare_op_code(op));
            }
        }
        sequence
    }

    /// [`Device::create_bind_group_layout`](crcbl_hal::Device::create_bind_group_layout),
    /// with the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and fields follow the descriptor's declaration order for
    /// [`create_image`](Self::create_image)'s.
    ///
    /// **The entries go over in the descriptor's own order and are not sorted.**
    /// [`BindGroupLayoutDesc::entries`] is order-sensitive — a
    /// [`BindingFlags::VARIABLE_COUNT`](crcbl_hal::BindingFlags::VARIABLE_COUNT)
    /// entry must be both last in the slice and highest-numbered — so the order
    /// is part of the value rather than a presentation of it.
    ///
    /// **Nothing is validated here, and this descriptor has the most to
    /// validate on the seam.**
    /// [`BindGroupLayoutDesc::check_entries`] is what
    /// enforces the ordering rule, the duplicate-binding rule, the zero-count
    /// rule, the bindless ceiling and the stage-capability rule — **and it is
    /// the caller's to run, before it ever reaches this method**. Every one of
    /// those is a value a `u32` or a `bits()` word legitimately holds, so a
    /// decoder refusing one would be refusing a buffer this writer produced,
    /// and this writer asserts only what the reader enforces. The replayer
    /// cannot run `check_entries` either — it has no `DeviceCaps` and no
    /// `crcbl-hal` — so `web/engine/gpu-replay.js` states, at the arm that
    /// builds the layout, which of these rules it re-checks for itself and
    /// which it leaves to the browser. An unenforced rule both sides assume the
    /// other checks is the failure this pair of comments exists to prevent.
    pub fn create_bind_group_layout(
        &mut self,
        layout: BindGroupLayoutHandle,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> u64 {
        let sequence = self.push_tag(tag::CREATE_BIND_GROUP_LAYOUT_TAG);
        self.bytes.put_handle(layout);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_count(desc.entries.len());
        for entry in desc.entries {
            self.bytes.put_bind_group_layout_entry(entry);
        }
        sequence
    }

    /// [`Device::create_bind_group`](crcbl_hal::Device::create_bind_group), with
    /// the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and fields follow the descriptor's declaration order for
    /// [`create_image`](Self::create_image)'s.
    ///
    /// **The entries go over in the descriptor's own order and are not sorted or
    /// keyed by binding.** [`array_index`](crcbl_hal::BindGroupEntry::array_index)
    /// is the bindless write path, so two entries may share a
    /// [`binding`](crcbl_hal::BindGroupEntry::binding) and a decoder that rebuilt
    /// the list from binding numbers would lose one.
    ///
    /// **Nothing is resolved and nothing is validated.** A
    /// [`BindingResource::Buffer`] whose `size` is
    /// [`BindingResource::WHOLE_BUFFER`] (`u64::MAX`) is a *sentinel* and crosses
    /// as itself — the rule
    /// `docs/plan/41-webgpu-stream.md` sets — because only the replayer can turn
    /// it into WebGPU's absent `GPUBufferBinding.size`. A non-zero `array_index`,
    /// and a `Some` [`variable_count`](crcbl_hal::BindGroupDesc::variable_count),
    /// are both values a `u32` legitimately holds, so refusing them is the
    /// replayer's job — WebGPU can express neither — not this writer's, which
    /// asserts only what the reader enforces. See
    /// [`Command::CreateBindGroup`](crate::Command::CreateBindGroup).
    pub fn create_bind_group(&mut self, group: BindGroupHandle, desc: &BindGroupDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::CREATE_BIND_GROUP_TAG);
        self.bytes.put_handle(group);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_handle(desc.layout);
        self.bytes.put_count(desc.entries.len());
        for entry in desc.entries {
            self.bytes.put_bind_group_entry(entry);
        }
        // A presence byte, because `variable_count` is an optional scalar and not
        // a handle — the house rule for every optional field with no niche to
        // spare, exactly as `create_sampler`'s `compare` does.
        match desc.variable_count {
            None => self.bytes.put_u8(tag::ABSENT),
            Some(count) => {
                self.bytes.put_u8(tag::PRESENT);
                self.bytes.put_u32(count);
            }
        }
        sequence
    }

    /// [`Device::create_shader_module`](crcbl_hal::Device::create_shader_module),
    /// with the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and **every field of [`ShaderModuleDesc`] crosses in the order the
    /// descriptor declares them** — `label`, `spirv`, `wgsl`, `msl`, `dxil` —
    /// because the encoding is the seam's rather than WebGPU's, and a decoder must
    /// traverse all four artifacts to reach the end of the command even though a
    /// browser reads only WGSL. Each carries its own absence convention over the
    /// wire, verbatim: `spirv` empty is absent, `wgsl`/`msl` keep `Some("")` apart
    /// from `None` through the shared `put_opt_str` primitive, and `dxil`'s empty
    /// list is absence while a pair with an empty container is a
    /// truncated artifact — see [`Command::CreateShaderModule`](crate::Command::CreateShaderModule).
    ///
    /// **Nothing is validated here**, which is [`create_image`](Self::create_image)'s
    /// rule: a descriptor carrying no format at all, or only formats a backend
    /// cannot compile, is a creation failure the replayer raises through
    /// [`Device::take_error`](crcbl_hal::Device::take_error) — see
    /// [`ShaderModuleDesc::unusable`](crcbl_hal::ShaderModuleDesc::unusable) — not
    /// a malformed stream this writer refuses.
    pub fn create_shader_module(
        &mut self,
        module: ShaderModuleHandle,
        desc: &ShaderModuleDesc<'_>,
    ) -> u64 {
        let sequence = self.push_tag(tag::CREATE_SHADER_MODULE_TAG);
        self.bytes.put_handle(module);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_words(desc.spirv);
        self.bytes.put_opt_str(desc.wgsl);
        self.bytes.put_opt_str(desc.msl);
        self.bytes.put_dxil(desc.dxil);
        sequence
    }

    /// [`Device::create_pipeline_layout`](crcbl_hal::Device::create_pipeline_layout),
    /// with the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and fields follow the descriptor's declaration order for
    /// [`create_image`](Self::create_image)'s.
    ///
    /// **The bind-group layouts go over as a counted list of bare handles, in
    /// set order and not sorted.** `docs/plan/41-webgpu-stream.md` states the
    /// shape — a `u32` count then that many `to_bits` words — and set order is
    /// part of the value: it is what a shader's `@group(n)` indexes, so a
    /// decoder that reordered it would build a layout binding the wrong set to
    /// the wrong slot. Each id names an existing bind-group layout the replayer
    /// looks up, not one it fills in.
    ///
    /// **Nothing is resolved and nothing is validated.** A
    /// [`Some`] [`push_constants`](crcbl_hal::PipelineLayoutDesc::push_constants)
    /// crosses whole — the [`PushConstantRange`](crcbl_hal::PushConstantRange)'s
    /// `stages`, `offset` and `size` — even though WebGPU has no push constants
    /// and the replayer refuses it: the writer carries what the caller gives,
    /// the replayer refuses what WebGPU can't do, exactly as a
    /// [`BufferUsage::DEVICE_ADDRESS`](crcbl_hal::BufferUsage::DEVICE_ADDRESS)
    /// buffer or a `VARIABLE_COUNT` layout does. `stages` rides the
    /// [`ShaderStages`] bitflags primitive every other stage field on this
    /// stream uses, so a stage this build has no name for is refused by the
    /// reader rather than truncated. See
    /// [`Command::CreatePipelineLayout`](crate::Command::CreatePipelineLayout).
    pub fn create_pipeline_layout(
        &mut self,
        layout: PipelineLayoutHandle,
        desc: &PipelineLayoutDesc<'_>,
    ) -> u64 {
        let sequence = self.push_tag(tag::CREATE_PIPELINE_LAYOUT_TAG);
        self.bytes.put_handle(layout);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_count(desc.bind_group_layouts.len());
        for bind_group_layout in desc.bind_group_layouts {
            self.bytes.put_handle(*bind_group_layout);
        }
        // A presence byte, because `PushConstantRange` is not a handle and has no
        // niche to spare — the house rule for every optional field that is not
        // one, exactly as `create_sampler`'s `compare` and `create_bind_group`'s
        // `variable_count` do.
        match desc.push_constants {
            None => self.bytes.put_u8(tag::ABSENT),
            Some(range) => {
                self.bytes.put_u8(tag::PRESENT);
                self.bytes.put_u32(range.stages.bits());
                self.bytes.put_u32(range.offset);
                self.bytes.put_u32(range.size);
            }
        }
        sequence
    }

    /// [`Device::create_compute_pipeline`](crcbl_hal::Device::create_compute_pipeline),
    /// with the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and fields follow the descriptor's declaration order for
    /// [`create_image`](Self::create_image)'s.
    ///
    /// **Two handles cross into two *different* tables, and they are not
    /// interchangeable.** `desc.layout` names a pipeline layout and
    /// `desc.compute.module` a shader module — the first command on this stream to
    /// resolve handles into two distinct non-buffer tables. A handle carries no
    /// kind, so the parameter and the two descriptor handles are all eight bytes
    /// the others could hold; the opcode and their position are what say which
    /// table each indexes. The replayer routes a miss on either to the error queue
    /// naming which one it was — see
    /// [`Command::CreateComputePipeline`](crate::Command::CreateComputePipeline).
    ///
    /// **[`ComputePipelineDesc::workgroup_size`]
    /// crosses whole, all three components**, even though a WebGPU replayer drops
    /// it: `GPUComputePipelineDescriptor` reads the workgroup size from the
    /// module's `@workgroup_size` attribute and has no member for it, but
    /// [`ComputePipelineDesc`] carries it because
    /// Metal has nowhere else to declare it. The writer carries what the caller
    /// gives; the replayer drops what WebGPU reads from the shader — and dropping
    /// it changes nothing, so unlike a push-constant range it is not refused. It
    /// round-trips in Rust so a transposition of the three components is visible.
    ///
    /// **`entry_point` is a bare length-prefixed string**, always written — the
    /// seam always supplies one rather than leaning on WebGPU's default-to-the-
    /// sole-entry-point rule. Nothing is validated here, which is
    /// [`create_image`](Self::create_image)'s rule: the workgroup-size and
    /// handle-resolution checks are the replayer's, because only it faces WebGPU.
    pub fn create_compute_pipeline(
        &mut self,
        pipeline: ComputePipelineHandle,
        desc: &ComputePipelineDesc<'_>,
    ) -> u64 {
        let sequence = self.push_tag(tag::CREATE_COMPUTE_PIPELINE_TAG);
        self.bytes.put_handle(pipeline);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_handle(desc.layout);
        self.bytes.put_handle(desc.compute.module);
        self.bytes.put_bytes(desc.compute.entry_point.as_bytes());
        for extent in desc.workgroup_size {
            self.bytes.put_u32(extent);
        }
        sequence
    }

    /// [`Device::create_graphics_pipeline`](crcbl_hal::Device::create_graphics_pipeline),
    /// with the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and fields follow the descriptor's declaration order for
    /// [`create_image`](Self::create_image)'s. **The largest descriptor on the
    /// seam**, and the one whose depth-stencil chain
    /// `docs/plan/41-webgpu-stream.md` names as the deepest: the whole tree is
    /// written out through the field writers above, so a stride wrong by a byte
    /// anywhere in it lands the cursor inside the next field rather than
    /// truncating.
    ///
    /// **The vertex stage is a module handle and an entry point and nothing
    /// else** — no vertex-buffer layout, because the engine pulls geometry from
    /// storage buffers, so `GPUVertexState.buffers` is the empty array and there
    /// is nothing on the wire for it. The fragment stage rides a presence byte:
    /// `None` is a depth-only pass and writes just [`ABSENT`](tag::ABSENT), a
    /// `Some` writes its module and entry point. The four state blocks —
    /// primitive, the optional depth-stencil, multisample, and the counted list
    /// of colour targets — follow.
    ///
    /// **Nothing is resolved and nothing is validated**, which is
    /// [`create_image`](Self::create_image)'s rule met by the deepest descriptor.
    /// A [`PolygonMode::Line`](crcbl_hal::PolygonMode::Line), a
    /// [`depth_clamp`](crcbl_hal::PrimitiveState::depth_clamp) the device cannot
    /// serve, a `samples` count WebGPU forbids, a fractional
    /// [`DepthBias.constant`](crcbl_hal::DepthBias::constant) — all
    /// cross verbatim, because each is a value the wire form claims and the
    /// replayer is the half that faces WebGPU. See
    /// [`Command::CreateGraphicsPipeline`](crate::Command::CreateGraphicsPipeline).
    pub fn create_graphics_pipeline(
        &mut self,
        pipeline: GraphicsPipelineHandle,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> u64 {
        let sequence = self.push_tag(tag::CREATE_GRAPHICS_PIPELINE_TAG);
        self.bytes.put_handle(pipeline);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_handle(desc.layout);
        self.bytes.put_handle(desc.vertex.module);
        self.bytes.put_bytes(desc.vertex.entry_point.as_bytes());
        // A presence byte, because a fragment stage is an optional field that is
        // not a handle — the house rule, exactly as `create_sampler`'s `compare`
        // uses. `None` is a depth-only pass and the replayer omits the whole
        // `GPUFragmentState` member.
        match &desc.fragment {
            None => self.bytes.put_u8(tag::ABSENT),
            Some(fragment) => {
                self.bytes.put_u8(tag::PRESENT);
                self.bytes.put_handle(fragment.module);
                self.bytes.put_bytes(fragment.entry_point.as_bytes());
            }
        }
        self.bytes.put_primitive_state(&desc.primitive);
        match &desc.depth_stencil {
            None => self.bytes.put_u8(tag::ABSENT),
            Some(depth_stencil) => {
                self.bytes.put_u8(tag::PRESENT);
                self.bytes.put_depth_stencil_state(depth_stencil);
            }
        }
        self.bytes.put_multisample_state(&desc.multisample);
        self.bytes.put_count(desc.color_targets.len());
        for target in desc.color_targets {
            self.bytes.put_color_target_state(target);
        }
        sequence
    }

    /// [`Device::create_query_set`](crcbl_hal::Device::create_query_set), with
    /// the handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and fields follow [`QuerySetDesc`]'s
    /// declaration order behind the handle. **Every
    /// [`QueryKind`](crcbl_hal::QueryKind) encodes**, including the two the
    /// replayer refuses: the writer carries what the caller gives, and the
    /// refusal belongs where the WebGPU vocabulary is. See
    /// [`Command::CreateQuerySet`](crate::Command::CreateQuerySet).
    pub fn create_query_set(&mut self, set: QuerySetHandle, desc: &QuerySetDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::CREATE_QUERY_SET_TAG);
        self.bytes.put_handle(set);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_u8(tag::query_kind_code(desc.kind));
        self.bytes.put_u32(desc.count);
        sequence
    }

    // ── Destruction ──────────────────────────────────────────────────────────

    /// [`Device::destroy_buffer`](crcbl_hal::Device::destroy_buffer).
    pub fn destroy_buffer(&mut self, buffer: BufferHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_BUFFER_TAG);
        self.bytes.put_handle(buffer);
        sequence
    }

    /// [`Instance::destroy_surface`](crcbl_hal::Instance::destroy_surface).
    pub fn destroy_surface(&mut self, surface: SurfaceHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_SURFACE_TAG);
        self.bytes.put_handle(surface);
        sequence
    }

    /// [`Device::destroy_image`](crcbl_hal::Device::destroy_image).
    pub fn destroy_image(&mut self, image: ImageHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_IMAGE_TAG);
        self.bytes.put_handle(image);
        sequence
    }

    /// [`Device::destroy_image_view`](crcbl_hal::Device::destroy_image_view).
    ///
    /// Its own opcode rather than a kind byte on one destroy, because the
    /// opcode is what says which table an id indexes: a view and the image it
    /// views are separate objects with separate ids, and the two tables
    /// genuinely issue identical bits.
    pub fn destroy_image_view(&mut self, view: ImageViewHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_IMAGE_VIEW_TAG);
        self.bytes.put_handle(view);
        sequence
    }

    /// [`Device::destroy_sampler`](crcbl_hal::Device::destroy_sampler).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason: the opcode is what says which table an id indexes, and a sampler
    /// shares its eight bytes with every other kind quite legitimately.
    pub fn destroy_sampler(&mut self, sampler: SamplerHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_SAMPLER_TAG);
        self.bytes.put_handle(sampler);
        sequence
    }

    /// [`Device::destroy_bind_group_layout`](crcbl_hal::Device::destroy_bind_group_layout).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason, and the one destroy here whose **empty slot is ordinary rather
    /// than exceptional**: the replayer refuses a layout it cannot express — a
    /// bindless entry, a stage WebGPU has no bit for — so the handle the caller
    /// pre-allocated is destroyed with nothing behind it every time that
    /// happens. That is the pattern the crate docs describe, met on the normal
    /// path.
    pub fn destroy_bind_group_layout(&mut self, layout: BindGroupLayoutHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_BIND_GROUP_LAYOUT_TAG);
        self.bytes.put_handle(layout);
        sequence
    }

    /// [`Device::destroy_bind_group`](crcbl_hal::Device::destroy_bind_group).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason: the opcode is what says which table an id indexes, and a bind
    /// group shares its eight bytes with every other kind quite legitimately.
    pub fn destroy_bind_group(&mut self, group: BindGroupHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_BIND_GROUP_TAG);
        self.bytes.put_handle(group);
        sequence
    }

    /// [`Device::destroy_shader_module`](crcbl_hal::Device::destroy_shader_module).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason: the opcode is what says which table an id indexes, and a shader
    /// module shares its eight bytes with every other kind quite legitimately.
    pub fn destroy_shader_module(&mut self, module: ShaderModuleHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_SHADER_MODULE_TAG);
        self.bytes.put_handle(module);
        sequence
    }

    /// [`Device::destroy_pipeline_layout`](crcbl_hal::Device::destroy_pipeline_layout).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason, and the second destroy here whose **empty slot is ordinary rather
    /// than exceptional** — like [`destroy_bind_group_layout`](Self::destroy_bind_group_layout):
    /// the replayer refuses a layout it cannot express — a `Some` push-constant
    /// range, a bind-group layout that resolves to nothing — so the handle the
    /// caller pre-allocated is destroyed with nothing behind it every time that
    /// happens.
    pub fn destroy_pipeline_layout(&mut self, layout: PipelineLayoutHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_PIPELINE_LAYOUT_TAG);
        self.bytes.put_handle(layout);
        sequence
    }

    /// [`Device::destroy_compute_pipeline`](crcbl_hal::Device::destroy_compute_pipeline).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason, and the third destroy here whose **empty slot is ordinary rather
    /// than exceptional** — like [`destroy_pipeline_layout`](Self::destroy_pipeline_layout):
    /// the replayer refuses a pipeline it cannot build (an unresolvable layout or
    /// module), so the handle the caller pre-allocated is destroyed with nothing
    /// behind it every time that happens.
    pub fn destroy_compute_pipeline(&mut self, pipeline: ComputePipelineHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_COMPUTE_PIPELINE_TAG);
        self.bytes.put_handle(pipeline);
        sequence
    }

    /// [`Device::destroy_graphics_pipeline`](crcbl_hal::Device::destroy_graphics_pipeline).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason, and — like the compute-pipeline destroy — the one whose **empty
    /// slot is ordinary rather than exceptional**: the replayer refuses a pipeline
    /// it cannot build (a `Line` polygon mode, an unresolvable layout or module, a
    /// forbidden `samples` count), so the handle the caller pre-allocated is
    /// destroyed with nothing behind it every time that happens.
    pub fn destroy_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_GRAPHICS_PIPELINE_TAG);
        self.bytes.put_handle(pipeline);
        sequence
    }

    /// [`Device::destroy_command_buffer`](crcbl_hal::Device::destroy_command_buffer).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason: the opcode is what says which table an id indexes, and a command
    /// buffer shares its eight bytes with every other kind quite legitimately.
    pub fn destroy_command_buffer(&mut self, command_buffer: CommandBufferHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_COMMAND_BUFFER_TAG);
        self.bytes.put_handle(command_buffer);
        sequence
    }

    /// [`Device::destroy_readback`](crcbl_hal::Device::destroy_readback).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason. On WebGPU the replayer's release of this id is the `unmap`.
    pub fn destroy_readback(&mut self, readback: ReadbackHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_READBACK_TAG);
        self.bytes.put_handle(readback);
        sequence
    }

    /// [`Device::destroy_query_set`](crcbl_hal::Device::destroy_query_set).
    ///
    /// Its own opcode for [`destroy_image_view`](Self::destroy_image_view)'s
    /// reason. Releasing a `GPUQuerySet` is its `destroy()`.
    pub fn destroy_query_set(&mut self, set: QuerySetHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_QUERY_SET_TAG);
        self.bytes.put_handle(set);
        sequence
    }

    // ── Encoder state ────────────────────────────────────────────────────────

    /// [`begin_debug_label`](crcbl_hal::CommandEncoder::begin_debug_label) — the
    /// region name, which the replayer pushes onto whichever scope is open. See
    /// [`Command::BeginDebugLabel`](crate::Command::BeginDebugLabel).
    pub fn begin_debug_label(&mut self, label: &str) -> u64 {
        let sequence = self.push_tag(tag::BEGIN_DEBUG_LABEL_TAG);
        self.bytes.put_bytes(label.as_bytes());
        sequence
    }

    /// [`end_debug_label`](crcbl_hal::CommandEncoder::end_debug_label).
    ///
    /// Body-less: it closes the region
    /// [`begin_debug_label`](Self::begin_debug_label) opened, without naming it.
    pub fn end_debug_label(&mut self) -> u64 {
        self.push_tag(tag::END_DEBUG_LABEL_TAG)
    }

    /// [`insert_debug_marker`](crcbl_hal::CommandEncoder::insert_debug_marker) —
    /// the marker name, carried exactly as
    /// [`begin_debug_label`](Self::begin_debug_label)'s is; only the tag says
    /// which of the two the replayer calls. See
    /// [`Command::InsertDebugMarker`](crate::Command::InsertDebugMarker).
    pub fn insert_debug_marker(&mut self, label: &str) -> u64 {
        let sequence = self.push_tag(tag::INSERT_DEBUG_MARKER_TAG);
        self.bytes.put_bytes(label.as_bytes());
        sequence
    }

    /// [`begin_render_pass`](crcbl_hal::CommandEncoder::begin_render_pass).
    pub fn begin_render_pass(&mut self, desc: &RenderPassDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::BEGIN_RENDER_PASS_TAG);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_count(desc.color_attachments.len());
        for attachment in desc.color_attachments {
            self.bytes.put_color_attachment(attachment);
        }
        match &desc.depth_stencil_attachment {
            None => self.bytes.put_u8(tag::ABSENT),
            Some(attachment) => {
                self.bytes.put_u8(tag::PRESENT);
                self.bytes.put_depth_stencil_attachment(attachment);
            }
        }
        self.bytes.put_rect(desc.render_area);
        self.bytes.put_timestamp_writes(desc.timestamp_writes);
        sequence
    }

    /// [`bind_graphics_pipeline`](crcbl_hal::CommandEncoder::bind_graphics_pipeline).
    pub fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) -> u64 {
        let sequence = self.push_tag(tag::BIND_GRAPHICS_PIPELINE_TAG);
        self.bytes.put_handle(pipeline);
        sequence
    }

    /// [`set_viewport`](crcbl_hal::CommandEncoder::set_viewport), on the open
    /// render pass — the whole of [`Viewport`] in declaration order: the
    /// rectangle's four floats, then the depth range's two. See
    /// [`Command::SetViewport`](crate::Command::SetViewport).
    pub fn set_viewport(&mut self, viewport: &Viewport) -> u64 {
        let sequence = self.push_tag(tag::SET_VIEWPORT_TAG);
        self.bytes.put_f32(viewport.x);
        self.bytes.put_f32(viewport.y);
        self.bytes.put_f32(viewport.width);
        self.bytes.put_f32(viewport.height);
        self.bytes.put_f32(viewport.depth_min);
        self.bytes.put_f32(viewport.depth_max);
        sequence
    }

    /// [`set_scissor`](crcbl_hal::CommandEncoder::set_scissor), on the open render
    /// pass — the whole of [`Rect2d`], the same primitive `begin_render_pass`'
    /// `render_area` uses. See [`Command::SetScissor`](crate::Command::SetScissor).
    pub fn set_scissor(&mut self, rect: &Rect2d) -> u64 {
        let sequence = self.push_tag(tag::SET_SCISSOR_TAG);
        self.bytes.put_rect(*rect);
        sequence
    }

    /// [`set_stencil_reference`](crcbl_hal::CommandEncoder::set_stencil_reference),
    /// on the open render pass — the whole `u32`, which is what WebGPU's
    /// `GPUStencilValue` is too. See
    /// [`Command::SetStencilReference`](crate::Command::SetStencilReference).
    pub fn set_stencil_reference(&mut self, reference: u32) -> u64 {
        let sequence = self.push_tag(tag::SET_STENCIL_REFERENCE_TAG);
        self.bytes.put_u32(reference);
        sequence
    }

    /// [`bind_index_buffer`](crcbl_hal::CommandEncoder::bind_index_buffer), on the
    /// open render pass — the buffer, its byte offset, then the index-format code.
    /// See [`Command::BindIndexBuffer`](crate::Command::BindIndexBuffer).
    pub fn bind_index_buffer(
        &mut self,
        buffer: BufferHandle,
        offset: u64,
        format: IndexFormat,
    ) -> u64 {
        let sequence = self.push_tag(tag::BIND_INDEX_BUFFER_TAG);
        self.bytes.put_handle(buffer);
        self.bytes.put_u64(offset);
        self.bytes.put_u8(tag::index_format_code(format));
        sequence
    }

    /// [`bind_group`](crcbl_hal::CommandEncoder::bind_group).
    ///
    /// Fields go on the wire in the order the HAL call takes them, `layout`
    /// included and last, so the two cannot drift apart unnoticed.
    pub fn bind_group(
        &mut self,
        slot: u32,
        group: BindGroupHandle,
        dynamic_offsets: &[u32],
        layout: PipelineLayoutHandle,
    ) -> u64 {
        let sequence = self.push_tag(tag::BIND_GROUP_TAG);
        self.bytes.put_u32(slot);
        self.bytes.put_handle(group);
        self.bytes.put_count(dynamic_offsets.len());
        for offset in dynamic_offsets {
            self.bytes.put_u32(*offset);
        }
        self.bytes.put_handle(layout);
        sequence
    }

    /// [`push_constants`](crcbl_hal::CommandEncoder::push_constants).
    ///
    /// `layout` is last, as in the HAL call; see [`bind_group`](Self::bind_group).
    pub fn push_constants(
        &mut self,
        stages: ShaderStages,
        offset: u32,
        data: &[u8],
        layout: PipelineLayoutHandle,
    ) -> u64 {
        let sequence = self.push_tag(tag::PUSH_CONSTANTS_TAG);
        self.bytes.put_u32(stages.bits());
        self.bytes.put_u32(offset);
        self.bytes.put_bytes(data);
        self.bytes.put_handle(layout);
        sequence
    }

    /// [`Device::create_command_encoder`](crcbl_hal::Device::create_command_encoder).
    ///
    /// **No handle on the wire.** The encoder is the replayer's implicit-current
    /// one, exactly as `crcbl-hal`'s recording methods assume no encoder
    /// receiver — so nothing is allocated here, unlike every `create_*`. The
    /// command buffer it will yield is named by [`finish`](Self::finish).
    ///
    /// **`queue` crosses and selects no queue.** WebGPU has one implicit queue,
    /// so the handle is carried for a transposition to be visible and dropped by
    /// the replayer — see [`Command::CreateCommandEncoder`](crate::Command::CreateCommandEncoder).
    pub fn create_command_encoder(&mut self, desc: &CommandEncoderDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::CREATE_COMMAND_ENCODER_TAG);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_handle(desc.queue);
        sequence
    }

    /// [`end_render_pass`](crcbl_hal::CommandEncoder::end_render_pass).
    ///
    /// Body-less, on the implicit-current encoder: it closes the pass
    /// [`begin_render_pass`](Self::begin_render_pass) opened.
    pub fn end_render_pass(&mut self) -> u64 {
        self.push_tag(tag::END_RENDER_PASS_TAG)
    }

    /// [`begin_compute_pass`](crcbl_hal::CommandEncoder::begin_compute_pass).
    ///
    /// The whole of [`ComputePassDesc`], which is a label and the two queries
    /// the pass is bracketed by — compute has no attachments.
    pub fn begin_compute_pass(&mut self, desc: &ComputePassDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::BEGIN_COMPUTE_PASS_TAG);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_timestamp_writes(desc.timestamp_writes);
        sequence
    }

    /// [`bind_compute_pipeline`](crcbl_hal::CommandEncoder::bind_compute_pipeline).
    pub fn bind_compute_pipeline(&mut self, pipeline: ComputePipelineHandle) -> u64 {
        let sequence = self.push_tag(tag::BIND_COMPUTE_PIPELINE_TAG);
        self.bytes.put_handle(pipeline);
        sequence
    }

    /// [`dispatch`](crcbl_hal::CommandEncoder::dispatch) — the three workgroup
    /// counts, in the order the HAL call takes them.
    pub fn dispatch(&mut self, x: u32, y: u32, z: u32) -> u64 {
        let sequence = self.push_tag(tag::DISPATCH_TAG);
        self.bytes.put_u32(x);
        self.bytes.put_u32(y);
        self.bytes.put_u32(z);
        sequence
    }

    /// [`dispatch_indirect`](crcbl_hal::CommandEncoder::dispatch_indirect), on
    /// the open compute pass — the argument buffer, then its byte offset.
    ///
    /// No count and no stride, unlike the indirect draws: WebGPU's
    /// `dispatchWorkgroupsIndirect` is a single dispatch and so is the HAL call,
    /// so there is nothing for the replayer to unroll. See
    /// [`Command::DispatchIndirect`](crate::Command::DispatchIndirect).
    pub fn dispatch_indirect(&mut self, buffer: BufferHandle, offset: u64) -> u64 {
        let sequence = self.push_tag(tag::DISPATCH_INDIRECT_TAG);
        self.bytes.put_handle(buffer);
        self.bytes.put_u64(offset);
        sequence
    }

    /// [`end_compute_pass`](crcbl_hal::CommandEncoder::end_compute_pass).
    ///
    /// Body-less, on the implicit-current encoder: it closes the pass
    /// [`begin_compute_pass`](Self::begin_compute_pass) opened.
    pub fn end_compute_pass(&mut self) -> u64 {
        self.push_tag(tag::END_COMPUTE_PASS_TAG)
    }

    // ── Queries ──────────────────────────────────────────────────────────────

    /// [`reset_query_set`](crcbl_hal::CommandEncoder::reset_query_set), on the
    /// implicit-current encoder — **the documented no-op**.
    ///
    /// The range crosses as a first index and a count rather than as a pair, so
    /// an empty range is `0` rather than something a decoder has to check for
    /// inversion, and so it is the same shape
    /// [`resolve_query_set`](Self::resolve_query_set) carries. See
    /// [`Command::ResetQuerySet`](crate::Command::ResetQuerySet) for why a
    /// command WebGPU cannot perform is still written.
    pub fn reset_query_set(&mut self, set: QuerySetHandle, range: Range<u32>) -> u64 {
        let sequence = self.push_tag(tag::RESET_QUERY_SET_TAG);
        self.bytes.put_handle(set);
        self.bytes.put_u32(range.start);
        self.bytes.put_u32(range.end.saturating_sub(range.start));
        sequence
    }

    /// [`resolve_query_set`](crcbl_hal::CommandEncoder::resolve_query_set), on
    /// the implicit-current encoder.
    ///
    /// The whole call: the set, the range as a first index and a count, then the
    /// destination and its byte offset. Nothing is checked here — the two rules
    /// WebGPU imposes on the destination are the replayer's, which is where the
    /// buffer's usage bits actually are; see
    /// [`Command::ResolveQuerySet`](crate::Command::ResolveQuerySet).
    pub fn resolve_query_set(
        &mut self,
        set: QuerySetHandle,
        range: Range<u32>,
        dst: BufferHandle,
        dst_offset: u64,
    ) -> u64 {
        let sequence = self.push_tag(tag::RESOLVE_QUERY_SET_TAG);
        self.bytes.put_handle(set);
        self.bytes.put_u32(range.start);
        self.bytes.put_u32(range.end.saturating_sub(range.start));
        self.bytes.put_handle(dst);
        self.bytes.put_u64(dst_offset);
        sequence
    }

    /// [`Device::query_results`](crcbl_hal::Device::query_results).
    ///
    /// **Answered**, so it goes through
    /// [`StreamChannel::encode_awaited`](crate::web::StreamChannel::encode_awaited)
    /// for [`poll_readback`](Self::poll_readback)'s reason: the
    /// [`Reply::QueryResults`](crate::Reply::QueryResults) naming the returned
    /// sequence is refused, whole buffer at a time, unless something is already
    /// waiting on it.
    ///
    /// `query_count` is the caller's `out.len()`. See
    /// [`Command::QueryResults`](crate::Command::QueryResults) for why the
    /// replayer needs a resolve, a copy and a map to serve it.
    pub fn query_results(
        &mut self,
        set: QuerySetHandle,
        first_query: u32,
        query_count: u32,
    ) -> u64 {
        let sequence = self.push_tag(tag::QUERY_RESULTS_TAG);
        self.bytes.put_handle(set);
        self.bytes.put_u32(first_query);
        self.bytes.put_u32(query_count);
        sequence
    }

    /// [`finish`](crcbl_hal::CommandEncoder::finish) — consumes the
    /// implicit-current encoder and produces the command buffer.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason: the [`CommandBufferHandle`] is caller-allocated, and the replayer
    /// files `encoder.finish()` under it.
    pub fn finish(&mut self, command_buffer: CommandBufferHandle) -> u64 {
        let sequence = self.push_tag(tag::FINISH_TAG);
        self.bytes.put_handle(command_buffer);
        sequence
    }

    // ── Copies ─────────────────────────────────────────────────────────────────

    /// [`copy_image_to_buffer`](crcbl_hal::CommandEncoder::copy_image_to_buffer),
    /// on the implicit-current encoder.
    ///
    /// The whole of [`BufferImageCopy`] in the order the struct declares it, and
    /// **the direction is the method, never a field**: the same struct serves
    /// both directions and the opcode is what says which, so the two cannot
    /// disagree.
    ///
    /// **`buffer_row_length` crosses in texels, `0` included, and is not
    /// resolved.** WebGPU's `bytesPerRow` is bytes and must be 256-aligned, so
    /// turning texels into bytes — and refusing a misalignment — is the
    /// replayer's, for the sentinel rule's reason: only it has the texture's
    /// format to multiply by. See
    /// [`Command::CopyImageToBuffer`](crate::Command::CopyImageToBuffer) and
    /// `web/engine/gpu-replay.js`.
    pub fn copy_image_to_buffer(&mut self, copy: &BufferImageCopy) -> u64 {
        let sequence = self.push_tag(tag::COPY_IMAGE_TO_BUFFER_TAG);
        self.bytes.put_handle(copy.buffer);
        self.bytes.put_u64(copy.buffer_offset);
        self.bytes.put_u32(copy.buffer_row_length);
        self.bytes.put_u32(copy.buffer_image_height);
        self.bytes.put_handle(copy.image);
        self.bytes.put_subresource_layers(copy.image_subresource);
        self.bytes.put_offset(copy.image_offset);
        self.bytes.put_extent(copy.image_extent);
        sequence
    }

    /// [`copy_buffer_to_buffer`](crcbl_hal::CommandEncoder::copy_buffer_to_buffer),
    /// on the implicit-current encoder.
    ///
    /// The whole of [`BufferCopy`] in the order the struct declares it — source
    /// and its offset, destination and its offset, then the size. It is the only
    /// way a dispatch's storage-buffer output is observed: a storage buffer cannot
    /// be `MAP_READ`, so it is copied here into a host-readable buffer the readback
    /// then maps.
    pub fn copy_buffer_to_buffer(&mut self, copy: &BufferCopy) -> u64 {
        let sequence = self.push_tag(tag::COPY_BUFFER_TO_BUFFER_TAG);
        self.bytes.put_handle(copy.src);
        self.bytes.put_u64(copy.src_offset);
        self.bytes.put_handle(copy.dst);
        self.bytes.put_u64(copy.dst_offset);
        self.bytes.put_u64(copy.size);
        sequence
    }

    /// [`copy_buffer_to_image`](crcbl_hal::CommandEncoder::copy_buffer_to_image),
    /// on the implicit-current encoder — the upload counterpart of
    /// [`copy_image_to_buffer`](Self::copy_image_to_buffer).
    ///
    /// The whole of [`BufferImageCopy`] in the same field order that method
    /// writes, since the HAL uses one struct for both directions; the opcode is
    /// what says which way the copy runs. `buffer_row_length` crosses in texels,
    /// `0` included, and is not resolved — the texel→byte conversion is the
    /// replayer's, exactly as for [`copy_image_to_buffer`](Self::copy_image_to_buffer).
    pub fn copy_buffer_to_image(&mut self, copy: &BufferImageCopy) -> u64 {
        let sequence = self.push_tag(tag::COPY_BUFFER_TO_IMAGE_TAG);
        self.bytes.put_handle(copy.buffer);
        self.bytes.put_u64(copy.buffer_offset);
        self.bytes.put_u32(copy.buffer_row_length);
        self.bytes.put_u32(copy.buffer_image_height);
        self.bytes.put_handle(copy.image);
        self.bytes.put_subresource_layers(copy.image_subresource);
        self.bytes.put_offset(copy.image_offset);
        self.bytes.put_extent(copy.image_extent);
        sequence
    }

    /// [`copy_image_to_image`](crcbl_hal::CommandEncoder::copy_image_to_image),
    /// on the implicit-current encoder.
    ///
    /// The whole of [`ImageCopy`] in the order the struct declares it — source,
    /// its subresource and offset, destination, its subresource and offset, then
    /// the shared extent — composed from the same subresource/offset/extent
    /// primitives every image copy on this stream uses.
    pub fn copy_image_to_image(&mut self, copy: &ImageCopy) -> u64 {
        let sequence = self.push_tag(tag::COPY_IMAGE_TO_IMAGE_TAG);
        self.bytes.put_handle(copy.src);
        self.bytes.put_subresource_layers(copy.src_subresource);
        self.bytes.put_offset(copy.src_offset);
        self.bytes.put_handle(copy.dst);
        self.bytes.put_subresource_layers(copy.dst_subresource);
        self.bytes.put_offset(copy.dst_offset);
        self.bytes.put_extent(copy.extent);
        sequence
    }

    /// [`fill_buffer`](crcbl_hal::CommandEncoder::fill_buffer), on the
    /// implicit-current encoder — the buffer, its offset and size, then the
    /// `u32` value.
    ///
    /// The value crosses verbatim even though WebGPU can only express a zero
    /// fill: the replayer refuses a non-zero value at the target, so it must be
    /// told what the value was. See
    /// [`Command::FillBuffer`](crate::Command::FillBuffer).
    pub fn fill_buffer(&mut self, buffer: BufferHandle, offset: u64, size: u64, value: u32) -> u64 {
        let sequence = self.push_tag(tag::FILL_BUFFER_TAG);
        self.bytes.put_handle(buffer);
        self.bytes.put_u64(offset);
        self.bytes.put_u64(size);
        self.bytes.put_u32(value);
        sequence
    }

    /// [`Device::write_buffer`](crcbl_hal::Device::write_buffer) — a host→buffer
    /// upload: the buffer, its byte offset, then the bytes.
    ///
    /// Not an encoder command — `write_buffer` is a [`Device`](crcbl_hal::Device)
    /// method, and the replayer submits it to the queue rather than recording it
    /// on the implicit-current encoder. See
    /// [`Command::WriteBuffer`](crate::Command::WriteBuffer).
    ///
    /// # An upload past the field cap crosses as several commands
    ///
    /// This is the one command whose payload is caller *data* rather than
    /// program text: a scene's vertex, index or instance upload is routinely
    /// larger than [`tag::MAX_FIELD_BYTES`], which bounds one length-prefixed
    /// field. Raising that cap would only move the wall — any fixed number is a
    /// scene size waiting to exceed it — so the upload is **split** instead.
    /// Each chunk is a whole `WriteBuffer` naming `offset` plus the bytes
    /// already sent, and the replayer's `queue.writeBuffer` applies them in
    /// order with no notion that they were ever one call. So the stream this
    /// crate encodes stays one it can decode, and neither half ever holds the
    /// whole upload as a single field.
    ///
    /// Every chunk consumes a sequence number of its own; the one returned is
    /// the **first**, so an upload occupies the consecutive run starting there.
    /// An empty upload is still one command carrying a zero-length field, which
    /// is what keeps `write_buffer(b, o, &[])` distinguishable from no call at
    /// all.
    ///
    /// # Panics
    ///
    /// If `offset + data.len()` would run past the end of the `u64` address
    /// space, which is the chunk arithmetic's precondition and no buffer is
    /// large enough to reach. It is a caller bug rather than a stream limit, and
    /// [`WebGpuDevice::write_buffer`](crate::hal::WebGpuDevice) refuses it with
    /// a [`HalError`](crcbl_hal::HalError) before it can get here.
    pub fn write_buffer(&mut self, buffer: BufferHandle, offset: u64, data: &[u8]) -> u64 {
        assert!(
            offset.checked_add(data.len() as u64).is_some(),
            "a {} byte upload at offset {offset} runs past the end of the u64 address space",
            data.len()
        );
        // `chunks` yields nothing for an empty slice, and an empty upload is
        // still one command — so the head is written unconditionally and the
        // rest of the split follows it.
        let (head, rest) = data.split_at(data.len().min(tag::MAX_FIELD_BYTES));
        let first = self.put_write_buffer(buffer, offset, head);
        let mut next_offset = offset + head.len() as u64;
        for chunk in rest.chunks(tag::MAX_FIELD_BYTES) {
            self.put_write_buffer(buffer, next_offset, chunk);
            next_offset += chunk.len() as u64;
        }
        first
    }

    /// One `WriteBuffer` command — the buffer, its byte offset, then the bytes.
    ///
    /// `data` is at most [`tag::MAX_FIELD_BYTES`]; splitting an upload that is
    /// longer is [`write_buffer`](Self::write_buffer)'s job.
    fn put_write_buffer(&mut self, buffer: BufferHandle, offset: u64, data: &[u8]) -> u64 {
        let sequence = self.push_tag(tag::WRITE_BUFFER_TAG);
        self.bytes.put_handle(buffer);
        self.bytes.put_u64(offset);
        self.bytes.put_bytes(data);
        sequence
    }

    /// [`pipeline_barrier`](crcbl_hal::CommandEncoder::pipeline_barrier), on the
    /// implicit-current encoder — the documented no-op.
    ///
    /// The whole [`Barriers`] batch crosses for wire fidelity: the counted
    /// buffer and image lists, each element with its `from`/`to` states and its
    /// optional queue transfer, then the `global` flag. WebGPU tracks resource
    /// state itself, so the replayer records nothing — but a barrier is a faithful
    /// transposition, so it must carry what it was told rather than resolve it
    /// away. See [`Command::PipelineBarrier`](crate::Command::PipelineBarrier).
    pub fn pipeline_barrier(&mut self, barriers: &Barriers<'_>) -> u64 {
        let sequence = self.push_tag(tag::PIPELINE_BARRIER_TAG);
        self.bytes.put_count(barriers.buffers.len());
        for barrier in barriers.buffers {
            self.bytes.put_buffer_barrier(barrier);
        }
        self.bytes.put_count(barriers.images.len());
        for barrier in barriers.images {
            self.bytes.put_image_barrier(barrier);
        }
        self.bytes.put_bool(barriers.global);
        sequence
    }

    // ── Draws ────────────────────────────────────────────────────────────────

    /// [`draw`](crcbl_hal::CommandEncoder::draw).
    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) -> u64 {
        let sequence = self.push_tag(tag::DRAW_TAG);
        self.bytes.put_u32(vertices.start);
        self.bytes.put_u32(vertices.end);
        self.bytes.put_u32(instances.start);
        self.bytes.put_u32(instances.end);
        sequence
    }

    /// [`draw_indexed`](crcbl_hal::CommandEncoder::draw_indexed), on the open
    /// render pass — the index range's start and end, the signed `base_vertex`,
    /// then the instance range's start and end. `base_vertex` sits between the two
    /// ranges exactly as the HAL call takes it. See
    /// [`Command::DrawIndexed`](crate::Command::DrawIndexed).
    pub fn draw_indexed(
        &mut self,
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    ) -> u64 {
        let sequence = self.push_tag(tag::DRAW_INDEXED_TAG);
        self.bytes.put_u32(indices.start);
        self.bytes.put_u32(indices.end);
        self.bytes.put_i32(base_vertex);
        self.bytes.put_u32(instances.start);
        self.bytes.put_u32(instances.end);
        sequence
    }

    /// [`draw_indirect`](crcbl_hal::CommandEncoder::draw_indirect), on the open
    /// render pass — the argument buffer, its byte offset, the CPU-known
    /// `draw_count`, then the `stride`. The replayer unrolls the count into that
    /// many single-draw WebGPU calls. See
    /// [`Command::DrawIndirect`](crate::Command::DrawIndirect).
    pub fn draw_indirect(
        &mut self,
        buffer: BufferHandle,
        offset: u64,
        draw_count: u32,
        stride: u32,
    ) -> u64 {
        let sequence = self.push_tag(tag::DRAW_INDIRECT_TAG);
        self.bytes.put_handle(buffer);
        self.bytes.put_u64(offset);
        self.bytes.put_u32(draw_count);
        self.bytes.put_u32(stride);
        sequence
    }

    /// [`draw_indexed_indirect`](crcbl_hal::CommandEncoder::draw_indexed_indirect),
    /// on the open render pass — the same four fields
    /// [`draw_indirect`](Self::draw_indirect) carries, in the same order; only the
    /// argument layout the replayer's single-draw call reads differs. See
    /// [`Command::DrawIndexedIndirect`](crate::Command::DrawIndexedIndirect).
    pub fn draw_indexed_indirect(
        &mut self,
        buffer: BufferHandle,
        offset: u64,
        draw_count: u32,
        stride: u32,
    ) -> u64 {
        let sequence = self.push_tag(tag::DRAW_INDEXED_INDIRECT_TAG);
        self.bytes.put_handle(buffer);
        self.bytes.put_u64(offset);
        self.bytes.put_u32(draw_count);
        self.bytes.put_u32(stride);
        sequence
    }

    // ── Submission and readback ──────────────────────────────────────────────

    /// [`Device::submit`](crcbl_hal::Device::submit).
    ///
    /// The whole of [`SubmitInfo`]: the command buffers in order, then the
    /// counted `waits` and `signals`.
    ///
    /// **The wait and signal lists cross whole, empty or not.** WebGPU has no
    /// semaphores, so a non-empty list is what the replayer refuses by name — a
    /// dropped wait is a synchronisation bug, and the engine's real frame will
    /// carry them. The writer carries what the caller gives; the replayer refuses
    /// what WebGPU can't do, exactly as a `Some` push-constant range is handled.
    pub fn submit(&mut self, submit: &SubmitInfo<'_>) -> u64 {
        let sequence = self.push_tag(tag::SUBMIT_TAG);
        self.bytes.put_count(submit.command_buffers.len());
        for command_buffer in submit.command_buffers {
            self.bytes.put_handle(*command_buffer);
        }
        self.bytes.put_count(submit.waits.len());
        for wait in submit.waits {
            self.bytes.put_semaphore_wait(*wait);
        }
        self.bytes.put_count(submit.signals.len());
        for signal in submit.signals {
            self.bytes.put_semaphore_signal(*signal);
        }
        sequence
    }

    /// [`Device::request_readback`](crcbl_hal::Device::request_readback), with the
    /// handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and fields follow [`ReadbackDesc`]'s declaration order behind the
    /// handle.
    ///
    /// **`after` rides a presence byte and crosses whole.** `None` is WebGPU's
    /// `mapAsync`; `Some` names a semaphore wait the replayer refuses — WebGPU
    /// cannot express it — so it is carried by name rather than resolved, the
    /// house rule for an optional field that is not a handle. See
    /// [`Command::RequestReadback`](crate::Command::RequestReadback).
    pub fn request_readback(&mut self, readback: ReadbackHandle, desc: &ReadbackDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::REQUEST_READBACK_TAG);
        self.bytes.put_handle(readback);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_handle(desc.buffer);
        self.bytes.put_u64(desc.offset);
        self.bytes.put_u64(desc.size);
        match desc.after {
            None => self.bytes.put_u8(tag::ABSENT),
            Some(wait) => {
                self.bytes.put_u8(tag::PRESENT);
                self.bytes.put_semaphore_wait(wait);
            }
        }
        sequence
    }

    /// [`Device::poll_readback`](crcbl_hal::Device::poll_readback).
    ///
    /// **The one command here whose answer comes back**, so — like
    /// [`enumerate_adapters`](Self::enumerate_adapters) — it must go through
    /// [`StreamChannel::encode_awaited`](crate::web::StreamChannel::encode_awaited)
    /// so something is waiting on the returned sequence before the frame ends,
    /// or the [`Reply::ReadbackReady`](crate::Reply::ReadbackReady) naming it is
    /// refused as an answer to a command nobody asked — and refused for the whole
    /// buffer.
    pub fn poll_readback(&mut self, readback: ReadbackHandle) -> u64 {
        let sequence = self.push_tag(tag::POLL_READBACK_TAG);
        self.bytes.put_handle(readback);
        sequence
    }

    /// [`Device::take_error`](crcbl_hal::Device::take_error).
    ///
    /// Body-less — the call takes nothing — and **answered**, so it goes through
    /// [`StreamChannel::encode_awaited`](crate::web::StreamChannel::encode_awaited)
    /// for [`poll_readback`](Self::poll_readback)'s reason: the
    /// [`Reply::DeviceErrors`](crate::Reply::DeviceErrors) naming the returned
    /// sequence is refused, whole buffer at a time, unless something is already
    /// waiting on it.
    ///
    /// **One at a time.** The reply carries the whole queue, so a second poll
    /// while the first is unanswered would ask for errors the first is already
    /// bringing; [`WebGpuDevice::take_error`](crate::hal::WebGpuDevice) is what
    /// holds to that.
    pub fn take_error(&mut self) -> u64 {
        self.push_tag(tag::TAKE_ERROR_TAG)
    }

    // ── Instance ─────────────────────────────────────────────────────────────

    /// [`Instance::adapters`](crcbl_hal::Instance::adapters).
    ///
    /// **The one command here whose answer comes back**, and the reason the
    /// returned sequence matters rather than merely existing: something has to
    /// be waiting on it before the frame ends, or the reply naming it is refused
    /// as an answer to a command nobody asked — and refused for the whole
    /// buffer. Reach it through
    /// [`StreamChannel::encode_awaited`](crate::web::StreamChannel::encode_awaited),
    /// which encodes and registers in one step;
    /// [`AdapterProbe::request`](crate::instance::AdapterProbe::request) is that
    /// call written out.
    ///
    /// The body is empty: the HAL call takes nothing, and what comes back is a
    /// reply rather than a field.
    pub fn enumerate_adapters(&mut self) -> u64 {
        self.push_tag(tag::ENUMERATE_ADAPTERS_TAG)
    }

    /// [`Instance::request_device`](crcbl_hal::Instance::request_device).
    ///
    /// The second command whose answer comes back, and it goes through
    /// [`StreamChannel::encode_awaited`](crate::web::StreamChannel::encode_awaited)
    /// for [`enumerate_adapters`](Self::enumerate_adapters)'s reason;
    /// [`DeviceProbe::request`](crate::device::DeviceProbe::request) is that call
    /// written out.
    ///
    /// **Both feature words go over as [`Features::bits`](crcbl_hal::Features::bits),
    /// whole.** Nothing is filtered here: a bit WebGPU cannot satisfy is still
    /// what the caller asked for, and dropping it in the encoder would turn a
    /// required feature the browser does not have into a device that opened
    /// without it. The replayer holds the WebGPU vocabulary and is where the
    /// refusal happens — see [`crate::device`].
    pub fn request_device(&mut self, desc: &DeviceDesc<'_>) -> u64 {
        let sequence = self.push_tag(tag::REQUEST_DEVICE_TAG);
        self.bytes.put_u32(desc.adapter.0);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_u64(desc.required_features.bits());
        self.bytes.put_u64(desc.optional_features.bits());
        self.bytes.put_opt_handle(desc.compatible_surface);
        sequence
    }

    /// [`Instance::surface_caps`](crcbl_hal::Instance::surface_caps).
    ///
    /// The third command whose answer comes back, and it goes through
    /// [`StreamChannel::encode_awaited`](crate::web::StreamChannel::encode_awaited)
    /// for [`enumerate_adapters`](Self::enumerate_adapters)'s reason: something
    /// has to be waiting on the returned sequence before the frame ends, or the
    /// [`Reply::SurfaceCaps`](crate::Reply::SurfaceCaps) naming it is refused as
    /// an answer to a command nobody asked — and refused for the whole buffer.
    ///
    /// **The body is empty, though the HAL call takes a surface and an
    /// adapter**: the record depends on neither, so the caller's two ids are
    /// validated where the tables are and never travel. See
    /// [`Command::SurfaceCaps`](crate::Command::SurfaceCaps).
    pub fn surface_caps(&mut self) -> u64 {
        self.push_tag(tag::SURFACE_CAPS_TAG)
    }

    // ── Presentation ─────────────────────────────────────────────────────────

    /// [`Device::create_swapchain`](crcbl_hal::Device::create_swapchain), with the
    /// handle the caller allocated for it.
    ///
    /// Identity is positional here for [`create_buffer`](Self::create_buffer)'s
    /// reason, and the descriptor's fields follow its declaration order behind the
    /// handle — the surface, the format, the extent's two components, the image
    /// count, then the present-mode and composite-alpha enum codes.
    ///
    /// **`image_count` and `present_mode` cross whole and the replayer drops
    /// them**: a browser only offers `fifo` and manages its own buffering, so the
    /// canvas configure has no knob for either, but a faithful transposition
    /// carries what it was told rather than resolving it away. See
    /// [`Command::CreateSwapchain`](crate::Command::CreateSwapchain).
    pub fn create_swapchain(
        &mut self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> u64 {
        let sequence = self.push_tag(tag::CREATE_SWAPCHAIN_TAG);
        self.put_swapchain_desc(swapchain, desc);
        sequence
    }

    /// [`Device::reconfigure_swapchain`](crcbl_hal::Device::reconfigure_swapchain)
    /// — re-configures an already-configured swapchain in place.
    ///
    /// **The same wire as [`create_swapchain`](Self::create_swapchain)** — one
    /// [`SwapchainDesc`] behind the handle — differing only in the tag and the
    /// semantic: `swapchain` names an existing configured swapchain, which stays
    /// valid across the reconfigure. See
    /// [`Command::ReconfigureSwapchain`](crate::Command::ReconfigureSwapchain).
    pub fn reconfigure_swapchain(
        &mut self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> u64 {
        let sequence = self.push_tag(tag::RECONFIGURE_SWAPCHAIN_TAG);
        self.put_swapchain_desc(swapchain, desc);
        sequence
    }

    /// The body [`create_swapchain`](Self::create_swapchain) and
    /// [`reconfigure_swapchain`](Self::reconfigure_swapchain) share after their
    /// tag: the handle, then [`SwapchainDesc`]'s fields in declaration order — the
    /// label, the surface, the format, the extent's two components, the image
    /// count, then the present-mode and composite-alpha enum codes. One place so
    /// the two commands cannot drift on what a swapchain descriptor is.
    fn put_swapchain_desc(&mut self, swapchain: SwapchainHandle, desc: &SwapchainDesc<'_>) {
        self.bytes.put_handle(swapchain);
        self.bytes.put_opt_str(desc.label);
        self.bytes.put_handle(desc.surface);
        self.bytes.put_u8(tag::format_code(desc.format));
        self.bytes.put_u32(desc.extent.0);
        self.bytes.put_u32(desc.extent.1);
        self.bytes.put_u32(desc.image_count);
        self.bytes.put_u8(tag::present_mode_code(desc.present_mode));
        self.bytes
            .put_u8(tag::composite_alpha_code(desc.composite_alpha));
    }

    /// [`Device::acquire_next_frame`](crcbl_hal::Device::acquire_next_frame) — the
    /// swapchain, then the two caller-allocated handles the acquired texture and
    /// its view are filed under.
    ///
    /// Positional identity for [`create_surface`](Self::create_surface)'s reason:
    /// acquire is synchronous and deterministic on WebGPU, so nothing is answered
    /// and wasm allocates the `image` and `view` ids. See
    /// [`Command::AcquireNextFrame`](crate::Command::AcquireNextFrame).
    pub fn acquire_next_frame(
        &mut self,
        swapchain: SwapchainHandle,
        image: ImageHandle,
        view: ImageViewHandle,
    ) -> u64 {
        let sequence = self.push_tag(tag::ACQUIRE_NEXT_FRAME_TAG);
        self.bytes.put_handle(swapchain);
        self.bytes.put_handle(image);
        self.bytes.put_handle(view);
        sequence
    }

    /// [`Device::present`](crcbl_hal::Device::present) — the documented no-op.
    ///
    /// The whole of [`PresentInfo`]: the swapchain, the counted `waits`, then the
    /// optional `present_id` behind a presence byte.
    ///
    /// **The wait list crosses whole, empty or not.** WebGPU has no semaphores, so
    /// a non-empty list is what the replayer refuses by name — a dropped wait is a
    /// synchronisation bug — exactly as a [`submit`](Self::submit)'s waits are. The
    /// writer carries what the caller gives; the replayer refuses what WebGPU can't
    /// do. See [`Command::Present`](crate::Command::Present).
    pub fn present(&mut self, present: &PresentInfo<'_>) -> u64 {
        let sequence = self.push_tag(tag::PRESENT_TAG);
        self.bytes.put_handle(present.swapchain);
        self.bytes.put_count(present.waits.len());
        for wait in present.waits {
            self.bytes.put_handle(*wait);
        }
        match present.present_id {
            None => self.bytes.put_u8(tag::ABSENT),
            Some(present_id) => {
                self.bytes.put_u8(tag::PRESENT);
                self.bytes.put_u64(present_id);
            }
        }
        sequence
    }

    /// [`Device::destroy_swapchain`](crcbl_hal::Device::destroy_swapchain).
    pub fn destroy_swapchain(&mut self, swapchain: SwapchainHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_SWAPCHAIN_TAG);
        self.bytes.put_handle(swapchain);
        sequence
    }

    // ── Header and tags ──────────────────────────────────────────────────────

    fn write_header(&mut self) {
        self.bytes
            .put_format_header(tag::STREAM_MAGIC, tag::STREAM_VERSION);
        self.bytes.put_u64(self.base_sequence);
    }

    /// Opens a command and assigns it its sequence number.
    ///
    /// **The number does not go on the wire** — see the type docs. It is
    /// assigned here so that the one place a command begins is the one place a
    /// sequence is spent, whatever the body turns out to be.
    fn push_tag(&mut self, opcode: u8) -> u64 {
        self.bytes.put_u8(opcode);
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        sequence
    }
}
