//! The encoder: HAL calls in, one wasm-owned byte buffer out.

use core::ops::Range;

use crcbl_hal::{
    BindGroupHandle, BufferDesc, BufferHandle, ClearValue, ColorAttachment, DepthStencilAttachment,
    DeviceDesc, GraphicsPipelineHandle, PipelineLayoutHandle, Rect2d, RenderPassDesc, ShaderStages,
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

    fn put_color_attachment(&mut self, attachment: &ColorAttachment) {
        self.put_handle(attachment.view);
        self.put_opt_handle(attachment.resolve);
        self.put_u8(tag::load_op_code(attachment.load));
        self.put_u8(tag::store_op_code(attachment.store));
        self.put_clear_value(attachment.clear);
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

    // ── Destruction ──────────────────────────────────────────────────────────

    /// [`Device::destroy_buffer`](crcbl_hal::Device::destroy_buffer).
    pub fn destroy_buffer(&mut self, buffer: BufferHandle) -> u64 {
        let sequence = self.push_tag(tag::DESTROY_BUFFER_TAG);
        self.bytes.put_handle(buffer);
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
