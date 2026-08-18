//! `impl CommandEncoder for WebGpuCommandEncoder` — recording onto the stream.
//!
//! # The honesty mechanism for unit-returning ops
//!
//! A recording method that has a [`StreamWriter`](crate::StreamWriter) command
//! encodes it. One that does not — the count-buffer and mesh draws, and
//! [`write_timestamp`](CommandEncoder::write_timestamp) — returns `()` and
//! cannot report an error inline. Silently dropping it would let a scene that
//! hits it replay a command buffer missing the op and render **subtly wrong**,
//! which is the worst outcome the seam has.
//!
//! So an unwired op records its name onto the encoder instead, and
//! [`finish`](CommandEncoder::finish) — which *can* return an error — refuses,
//! naming the first one. A frame that stays inside the wired ops finishes
//! normally; one that reaches an unwired op fails loudly at the boundary, which
//! is where a fix goes to wire the missing command.
//!
//! A *wired* op with a field past the stream's caps takes the same route, for a
//! different reason: the writer would assert, and on wasm an assert is
//! `unreachable` — the module dies mid-frame with nothing to read. See the
//! `super::bounds` module.

use core::ops::Range;

use crcbl_hal::{
    BackendKind, Barriers, BindGroupHandle, BufferCopy, BufferHandle, BufferImageCopy,
    CommandBufferHandle, CommandEncoder, CommandEncoderDesc, ComputePassDesc,
    ComputePipelineHandle, DrawIndirect, DrawIndirectCount, GraphicsPipelineHandle, HalError,
    ImageCopy, IndexFormat, PipelineLayoutHandle, QuerySetHandle, Rect2d, RenderPassDesc,
    ShaderStages, Viewport,
};

use super::channel::{HandlePool, SharedChannel};

/// Records commands into the shared stream and produces one command buffer.
#[derive(Debug)]
pub struct WebGpuCommandEncoder {
    channel: SharedChannel,
    pool: HandlePool,
    /// The first unwired op recorded, or `None`. `finish` fails naming it — see
    /// the [module docs](self).
    unsupported: Option<&'static str>,
    /// The first field this encoder refused for being past the stream's caps,
    /// or `None`.
    ///
    /// The same mechanism [`unsupported`](Self::unsupported) is, for the other
    /// reason a unit-returning recording method cannot go through: the op is
    /// wired, but the bytes it was handed are longer than a stream field
    /// carries. Left to the writer that is an assert — `unreachable` on wasm,
    /// so the module dies mid-frame — and here it is a message
    /// [`finish`](CommandEncoder::finish) reports.
    refused: Option<String>,
}

/// WebGPU's answer for [`Capability::DrawIndirectCount`], shared by the
/// encoder's refusal and the device's declaration so the two cannot drift.
///
/// `GPURenderPassEncoder.drawIndirect` takes a buffer and an offset and reads
/// one tightly packed argument structure. There is no count-buffer form in the
/// core specification — the count-buffer draws are Dawn extensions — so this is
/// the API rather than the slice.
pub(super) const NO_COUNT_BUFFER_DRAW: &str =
    "WebGPU has no count-buffer draw, and the stream has no tag for one";

/// WebGPU's answer for [`Capability::MeshShading`] and
/// [`Capability::TaskShaderStage`], shared for the same reason.
pub(super) const NO_MESH_STAGE: &str = "WebGPU has no mesh stage";

impl WebGpuCommandEncoder {
    /// A fresh encoder. Encodes `create_command_encoder`, whose replayer builds
    /// the implicit-current encoder every recording method then targets.
    pub(crate) fn new(
        channel: SharedChannel,
        pool: HandlePool,
        desc: &CommandEncoderDesc<'_>,
    ) -> Self {
        channel.with(|c| c.encode(|stream| stream.create_command_encoder(desc)));
        Self {
            channel,
            pool,
            unsupported: None,
            refused: None,
        }
    }

    /// Record that an unwired op was reached. The first one wins: it is the
    /// earliest place the recording went past what the stream can carry, and
    /// the one a fix should wire next.
    fn record_unsupported(&mut self, op: &'static str) {
        if self.unsupported.is_none() {
            self.unsupported = Some(op);
        }
    }

    /// Record that a field was past a stream cap. The first one wins, for
    /// [`record_unsupported`](Self::record_unsupported)'s reason.
    fn record_refused(&mut self, why: &HalError) {
        if self.refused.is_none() {
            self.refused = Some(why.to_string());
        }
    }
}

impl CommandEncoder for WebGpuCommandEncoder {
    // --- debug ---

    fn begin_debug_label(&mut self, label: &str) {
        self.channel
            .with(|c| c.encode(|stream| stream.begin_debug_label(label)));
    }

    fn end_debug_label(&mut self) {
        self.channel
            .with(|c| c.encode(crate::StreamWriter::end_debug_label));
    }

    fn insert_debug_marker(&mut self, label: &str) {
        self.channel
            .with(|c| c.encode(|stream| stream.insert_debug_marker(label)));
    }

    // --- sync ---

    fn pipeline_barrier(&mut self, barriers: &Barriers<'_>) {
        self.channel
            .with(|c| c.encode(|stream| stream.pipeline_barrier(barriers)));
    }

    // --- copies ---

    fn copy_buffer_to_buffer(&mut self, copy: &BufferCopy) {
        self.channel
            .with(|c| c.encode(|stream| stream.copy_buffer_to_buffer(copy)));
    }

    fn copy_buffer_to_image(&mut self, copy: &BufferImageCopy) {
        self.channel
            .with(|c| c.encode(|stream| stream.copy_buffer_to_image(copy)));
    }

    fn copy_image_to_buffer(&mut self, copy: &BufferImageCopy) {
        self.channel
            .with(|c| c.encode(|stream| stream.copy_image_to_buffer(copy)));
    }

    fn copy_image_to_image(&mut self, copy: &ImageCopy) {
        self.channel
            .with(|c| c.encode(|stream| stream.copy_image_to_image(copy)));
    }

    fn fill_buffer(&mut self, buffer: BufferHandle, offset: u64, size: u64, value: u32) {
        self.channel
            .with(|c| c.encode(|stream| stream.fill_buffer(buffer, offset, size, value)));
    }

    // --- render scope ---

    fn begin_render_pass(&mut self, desc: &RenderPassDesc<'_>) {
        self.channel
            .with(|c| c.encode(|stream| stream.begin_render_pass(desc)));
    }

    fn end_render_pass(&mut self) {
        self.channel
            .with(|c| c.encode(crate::StreamWriter::end_render_pass));
    }

    fn set_viewport(&mut self, viewport: &Viewport) {
        self.channel
            .with(|c| c.encode(|stream| stream.set_viewport(viewport)));
    }

    fn set_scissor(&mut self, rect: &Rect2d) {
        self.channel
            .with(|c| c.encode(|stream| stream.set_scissor(rect)));
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        self.channel
            .with(|c| c.encode(|stream| stream.set_stencil_reference(reference)));
    }

    fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        self.channel
            .with(|c| c.encode(|stream| stream.bind_graphics_pipeline(pipeline)));
    }

    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: u64, format: IndexFormat) {
        self.channel
            .with(|c| c.encode(|stream| stream.bind_index_buffer(buffer, offset, format)));
    }

    // --- bindings, shared by both scopes ---

    fn bind_group(
        &mut self,
        slot: u32,
        group: BindGroupHandle,
        dynamic_offsets: &[u32],
        layout: PipelineLayoutHandle,
    ) {
        self.channel
            .with(|c| c.encode(|stream| stream.bind_group(slot, group, dynamic_offsets, layout)));
    }

    fn push_constants(
        &mut self,
        stages: ShaderStages,
        offset: u32,
        data: &[u8],
        layout: PipelineLayoutHandle,
    ) {
        // Wired: the command crosses whole. WebGPU has no push constants, so the
        // replayer refuses it by name — which is where the refusal belongs, per
        // the stream's own rule that the writer carries what the caller gives.
        //
        // The block is caller data, though, so its length is measured first:
        // past a stream field the writer would assert, and this method has no
        // `Result` to say so through, so `finish` reports it.
        if let Err(error) = super::bounds::field("push_constants data", data.len()) {
            self.record_refused(&error);
            return;
        }
        self.channel
            .with(|c| c.encode(|stream| stream.push_constants(stages, offset, data, layout)));
    }

    // --- draws ---

    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.channel
            .with(|c| c.encode(|stream| stream.draw(vertices, instances)));
    }

    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        self.channel
            .with(|c| c.encode(|stream| stream.draw_indexed(indices, base_vertex, instances)));
    }

    fn draw_indirect(&mut self, draw: &DrawIndirect) {
        self.channel.with(|c| {
            c.encode(|stream| {
                stream.draw_indirect(draw.args, draw.offset, draw.draw_count, draw.stride)
            })
        });
    }

    fn draw_indexed_indirect(&mut self, draw: &DrawIndirect) {
        self.channel.with(|c| {
            c.encode(|stream| {
                stream.draw_indexed_indirect(draw.args, draw.offset, draw.draw_count, draw.stride)
            })
        });
    }

    // These four said "not yet wired into the WebGPU stream", which promises a
    // slice that cannot exist: `crcbl_hal::DIVERGENCES` classifies all four as
    // API absences, and a caller reading "not yet" would wait for work nobody
    // can do. Each now carries the sentence the device's `supports` declares,
    // through one constant, so the refusal and the declaration cannot drift.
    fn draw_indirect_count(&mut self, _draw: &DrawIndirectCount) {
        self.record_unsupported(NO_COUNT_BUFFER_DRAW);
    }

    fn draw_indexed_indirect_count(&mut self, _draw: &DrawIndirectCount) {
        self.record_unsupported(NO_COUNT_BUFFER_DRAW);
    }

    fn draw_mesh_tasks(&mut self, _x: u32, _y: u32, _z: u32) {
        self.record_unsupported(NO_MESH_STAGE);
    }

    fn draw_mesh_tasks_indirect(&mut self, _draw: &DrawIndirect) {
        self.record_unsupported(NO_MESH_STAGE);
    }

    // --- compute scope ---

    fn begin_compute_pass(&mut self, desc: &ComputePassDesc<'_>) {
        self.channel
            .with(|c| c.encode(|stream| stream.begin_compute_pass(desc)));
    }

    fn end_compute_pass(&mut self) {
        self.channel
            .with(|c| c.encode(crate::StreamWriter::end_compute_pass));
    }

    fn bind_compute_pipeline(&mut self, pipeline: ComputePipelineHandle) {
        self.channel
            .with(|c| c.encode(|stream| stream.bind_compute_pipeline(pipeline)));
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        self.channel
            .with(|c| c.encode(|stream| stream.dispatch(x, y, z)));
    }

    fn dispatch_indirect(&mut self, args: BufferHandle, offset: u64) {
        self.channel
            .with(|c| c.encode(|stream| stream.dispatch_indirect(args, offset)));
    }

    // --- queries ---

    fn reset_query_set(&mut self, set: QuerySetHandle, range: Range<u32>) {
        // Wired, and the command it becomes is a no-op: WebGPU has no reset, and
        // an unwritten query resolves to zero by specification, so there is
        // nothing for one to do. It is carried rather than dropped here because
        // this method returns `()` — a range naming a set the replayer does not
        // hold then reaches the device error queue by name instead of vanishing.
        // See `Command::ResetQuerySet`.
        self.channel
            .with(|c| c.encode(|stream| stream.reset_query_set(set, range)));
    }

    fn write_timestamp(&mut self, _set: QuerySetHandle, _index: u32) {
        // The one query verb that stays unwired, and not for want of a slice:
        // WebGPU takes timestamps only at pass boundaries, through a render or
        // compute pass's `timestampWrites`, and this call names an arbitrary
        // point in the command stream. `create_query_set` refuses
        // `QueryKind::Timestamp` for the same reason and with the same sentence,
        // so no live handle can reach here at all — this is the second lock on a
        // door, not the first.
        self.record_unsupported(super::device::NO_TIMESTAMP_SET);
    }

    fn resolve_query_set(
        &mut self,
        set: QuerySetHandle,
        range: Range<u32>,
        dst: BufferHandle,
        dst_offset: u64,
    ) {
        self.channel
            .with(|c| c.encode(|stream| stream.resolve_query_set(set, range, dst, dst_offset)));
    }

    // --- finish ---

    fn finish(self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
        // A refusal comes first: it names bytes the caller actually handed over,
        // which is a fix in the caller, where an unwired op is a fix here.
        if let Some(why) = self.refused {
            return Err(HalError::InvalidDescriptor(why));
        }
        if let Some(op) = self.unsupported {
            return Err(HalError::Unsupported {
                backend: BackendKind::WebGpu,
                what: op,
            });
        }
        let handle: CommandBufferHandle = self.pool.alloc();
        self.channel
            .with(|c| c.encode(|stream| stream.finish(handle)));
        Ok(handle)
    }
}
