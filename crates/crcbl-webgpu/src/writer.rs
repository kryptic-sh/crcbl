//! The encoder: HAL calls in, one wasm-owned byte buffer out.

use core::ops::Range;

use crcbl_hal::{
    BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry, BindGroupLayoutHandle, BindingKind,
    BufferDesc, BufferHandle, ClearValue, ColorAttachment, DepthStencilAttachment, DeviceDesc,
    Extent3d, GraphicsPipelineHandle, ImageDesc, ImageHandle, ImageSubresourceRange, ImageViewDesc,
    ImageViewHandle, PipelineLayoutHandle, Rect2d, RenderPassDesc, SamplerDesc, SamplerHandle,
    ShaderStages, SurfaceHandle,
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
            BindingKind::StorageImage { read_only } => self.put_bool(read_only),
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

    fn put_depth_stencil_attachment(&mut self, attachment: &DepthStencilAttachment) {
        self.put_handle(attachment.view);
        self.put_bool(attachment.read_only);
        self.put_u8(tag::load_op_code(attachment.depth_load));
        self.put_u8(tag::store_op_code(attachment.depth_store));
        self.put_u8(tag::load_op_code(attachment.stencil_load));
        self.put_u8(tag::store_op_code(attachment.stencil_store));
        self.put_clear_value(attachment.clear);
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
/// refuses to decode; and no legitimate call can reach one — a push-constant
/// block is bounded by the device limit and a debug label is a string literal at
/// almost every call site. A caller that does hit one has a bug that a silent
/// truncation would bury.
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

    // ── Encoder state ────────────────────────────────────────────────────────

    /// [`begin_debug_label`](crcbl_hal::CommandEncoder::begin_debug_label).
    pub fn begin_debug_label(&mut self, label: &str) -> u64 {
        let sequence = self.push_tag(tag::BEGIN_DEBUG_LABEL_TAG);
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
        sequence
    }

    /// [`bind_graphics_pipeline`](crcbl_hal::CommandEncoder::bind_graphics_pipeline).
    pub fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) -> u64 {
        let sequence = self.push_tag(tag::BIND_GRAPHICS_PIPELINE_TAG);
        self.bytes.put_handle(pipeline);
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
