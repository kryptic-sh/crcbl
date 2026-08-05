//! [`Dx12CommandEncoder`] — an encoder that records nothing and says so at
//! [`finish`](CommandEncoder::finish).
//!
//! # Why this type exists at all in a slice with no command recording
//!
//! [`Device::create_command_encoder`](crcbl_hal::Device::create_command_encoder)
//! returns a bare `Box<dyn CommandEncoder>`, not a `Result`. Every other entry
//! point this slice cannot serve refuses through its return type; this one has
//! no return type to refuse through, so the refusal has to move to the first
//! call that *does* have one — which is `finish`.
//!
//! **Nothing here is documented as working.** The recording methods accept their
//! arguments and drop them, exactly as they read, and the encoder cannot produce
//! a [`CommandBufferHandle`]: `finish` returns
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) naming the slice
//! that will implement it. A caller therefore gets a "not yet" at the point it
//! tries to *use* the recording rather than a command buffer that submits and
//! draws nothing — which is the failure
//! [`Device::take_error`](crcbl_hal::Device::take_error) exists to catch on
//! WebGPU, and which is worth not reproducing on purpose here.
//!
//! What lands in the command slice is the real thing: an
//! `ID3D12GraphicsCommandList` recorded into an `ID3D12CommandAllocator`, with
//! resource barriers, render passes and copies on it, and `finish` closing the
//! list and handing back a pooled handle. At that point this file is replaced
//! rather than extended. `crcbl-mtl` reached exactly this shape for exactly this
//! reason; this is that file with the nouns changed.

use core::ops::Range;

use crcbl_hal::{
    Barriers, BindGroupHandle, BufferCopy, BufferHandle, BufferImageCopy, CommandBufferHandle,
    CommandEncoder, ComputePassDesc, ComputePipelineHandle, DrawIndirect, DrawIndirectCount,
    GraphicsPipelineHandle, HalError, ImageCopy, IndexFormat, PipelineLayoutHandle, QuerySetHandle,
    Rect2d, RenderPassDesc, ShaderStages, Viewport,
};

use crate::instance::not_yet;

/// A command encoder that accepts recording calls and produces no command
/// buffer. See the module docs.
#[derive(Debug)]
pub(crate) struct Dx12CommandEncoder {
    /// Zero-sized state today. The field exists so the type is a `struct` with a
    /// private constructor rather than a unit type anything could build, and so
    /// the command slice has somewhere to put the command list.
    _private: (),
}

impl Dx12CommandEncoder {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }
}

impl CommandEncoder for Dx12CommandEncoder {
    fn begin_debug_label(&mut self, _label: &str) {}

    fn end_debug_label(&mut self) {}

    fn insert_debug_marker(&mut self, _label: &str) {}

    fn pipeline_barrier(&mut self, _barriers: &Barriers<'_>) {}

    fn copy_buffer_to_buffer(&mut self, _copy: &BufferCopy) {}

    fn copy_buffer_to_image(&mut self, _copy: &BufferImageCopy) {}

    fn copy_image_to_buffer(&mut self, _copy: &BufferImageCopy) {}

    fn copy_image_to_image(&mut self, _copy: &ImageCopy) {}

    fn fill_buffer(&mut self, _buffer: BufferHandle, _offset: u64, _size: u64, _value: u32) {}

    fn begin_render_pass(&mut self, _desc: &RenderPassDesc<'_>) {}

    fn end_render_pass(&mut self) {}

    fn set_viewport(&mut self, _viewport: &Viewport) {}

    fn set_scissor(&mut self, _rect: &Rect2d) {}

    fn set_stencil_reference(&mut self, _reference: u32) {}

    fn bind_graphics_pipeline(&mut self, _pipeline: GraphicsPipelineHandle) {}

    fn bind_index_buffer(&mut self, _buffer: BufferHandle, _offset: u64, _format: IndexFormat) {}

    fn bind_group(
        &mut self,
        _index: u32,
        _group: BindGroupHandle,
        _dynamic_offsets: &[u32],
        _layout: PipelineLayoutHandle,
    ) {
    }

    fn push_constants(
        &mut self,
        _stages: ShaderStages,
        _offset: u32,
        _data: &[u8],
        _layout: PipelineLayoutHandle,
    ) {
    }

    fn draw(&mut self, _vertices: Range<u32>, _instances: Range<u32>) {}

    fn draw_indexed(&mut self, _indices: Range<u32>, _base_vertex: i32, _instances: Range<u32>) {}

    fn draw_indirect(&mut self, _draw: &DrawIndirect) {}

    fn draw_indexed_indirect(&mut self, _draw: &DrawIndirect) {}

    fn draw_indirect_count(&mut self, _draw: &DrawIndirectCount) {}

    fn draw_indexed_indirect_count(&mut self, _draw: &DrawIndirectCount) {}

    fn begin_compute_pass(&mut self, _desc: &ComputePassDesc<'_>) {}

    fn end_compute_pass(&mut self) {}

    fn bind_compute_pipeline(&mut self, _pipeline: ComputePipelineHandle) {}

    fn dispatch(&mut self, _x: u32, _y: u32, _z: u32) {}

    fn dispatch_indirect(&mut self, _args: BufferHandle, _offset: u64) {}

    fn reset_query_set(&mut self, _set: QuerySetHandle, _range: Range<u32>) {}

    fn write_timestamp(&mut self, _set: QuerySetHandle, _index: u32) {}

    fn resolve_query_set(
        &mut self,
        _set: QuerySetHandle,
        _range: Range<u32>,
        _dst: BufferHandle,
        _dst_offset: u64,
    ) {
    }

    /// The refusal, at the one call on this trait that can carry one.
    fn finish(self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
        Err(not_yet("command recording (the DX12 command slice)"))
    }
}
