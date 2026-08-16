//! `impl CommandEncoder for WebGpuCommandEncoder` — recording onto the stream.
//!
//! # The honesty mechanism for unit-returning ops
//!
//! A recording method that has a [`StreamWriter`](crate::StreamWriter) command
//! encodes it. One that does not — and several draws, the query writes and the
//! viewport/scissor state have no command yet — returns `()` and cannot report
//! an error inline. Silently dropping it would let a scene that hits it replay a
//! command buffer missing the op and render **subtly wrong**, which is the
//! worst outcome the seam has.
//!
//! So an unwired op records its name onto the encoder instead, and
//! [`finish`](CommandEncoder::finish) — which *can* return an error — refuses,
//! naming the first one. A frame that stays inside the wired ops finishes
//! normally; one that reaches an unwired op fails loudly at the boundary, which
//! is where a fix goes to wire the missing command.

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
}

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
}

impl CommandEncoder for WebGpuCommandEncoder {
    // --- debug ---

    fn begin_debug_label(&mut self, label: &str) {
        self.channel
            .with(|c| c.encode(|stream| stream.begin_debug_label(label)));
    }

    fn end_debug_label(&mut self) {
        // The stream carries `BeginDebugLabel` but has no command to close a
        // label yet, so ending one is unwired.
        self.record_unsupported("end_debug_label is not yet wired into the WebGPU stream");
    }

    fn insert_debug_marker(&mut self, _label: &str) {
        self.record_unsupported("insert_debug_marker is not yet wired into the WebGPU stream");
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

    fn set_viewport(&mut self, _viewport: &Viewport) {
        self.record_unsupported("set_viewport is not yet wired into the WebGPU stream");
    }

    fn set_scissor(&mut self, _rect: &Rect2d) {
        self.record_unsupported("set_scissor is not yet wired into the WebGPU stream");
    }

    fn set_stencil_reference(&mut self, _reference: u32) {
        self.record_unsupported("set_stencil_reference is not yet wired into the WebGPU stream");
    }

    fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        self.channel
            .with(|c| c.encode(|stream| stream.bind_graphics_pipeline(pipeline)));
    }

    fn bind_index_buffer(&mut self, _buffer: BufferHandle, _offset: u64, _format: IndexFormat) {
        self.record_unsupported("bind_index_buffer is not yet wired into the WebGPU stream");
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
        self.channel
            .with(|c| c.encode(|stream| stream.push_constants(stages, offset, data, layout)));
    }

    // --- draws ---

    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.channel
            .with(|c| c.encode(|stream| stream.draw(vertices, instances)));
    }

    fn draw_indexed(&mut self, _indices: Range<u32>, _base_vertex: i32, _instances: Range<u32>) {
        self.record_unsupported("draw_indexed is not yet wired into the WebGPU stream");
    }

    fn draw_indirect(&mut self, _draw: &DrawIndirect) {
        self.record_unsupported("draw_indirect is not yet wired into the WebGPU stream");
    }

    fn draw_indexed_indirect(&mut self, _draw: &DrawIndirect) {
        self.record_unsupported("draw_indexed_indirect is not yet wired into the WebGPU stream");
    }

    fn draw_indirect_count(&mut self, _draw: &DrawIndirectCount) {
        self.record_unsupported("draw_indirect_count is not yet wired into the WebGPU stream");
    }

    fn draw_indexed_indirect_count(&mut self, _draw: &DrawIndirectCount) {
        self.record_unsupported(
            "draw_indexed_indirect_count is not yet wired into the WebGPU stream",
        );
    }

    fn draw_mesh_tasks(&mut self, _x: u32, _y: u32, _z: u32) {
        self.record_unsupported("draw_mesh_tasks is not yet wired into the WebGPU stream");
    }

    fn draw_mesh_tasks_indirect(&mut self, _draw: &DrawIndirect) {
        self.record_unsupported("draw_mesh_tasks_indirect is not yet wired into the WebGPU stream");
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

    fn dispatch_indirect(&mut self, _args: BufferHandle, _offset: u64) {
        self.record_unsupported("dispatch_indirect is not yet wired into the WebGPU stream");
    }

    // --- queries ---

    fn reset_query_set(&mut self, _set: QuerySetHandle, _range: Range<u32>) {
        self.record_unsupported("reset_query_set is not yet wired into the WebGPU stream");
    }

    fn write_timestamp(&mut self, _set: QuerySetHandle, _index: u32) {
        self.record_unsupported("write_timestamp is not yet wired into the WebGPU stream");
    }

    fn resolve_query_set(
        &mut self,
        _set: QuerySetHandle,
        _range: Range<u32>,
        _dst: BufferHandle,
        _dst_offset: u64,
    ) {
        self.record_unsupported("resolve_query_set is not yet wired into the WebGPU stream");
    }

    // --- finish ---

    fn finish(self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
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
