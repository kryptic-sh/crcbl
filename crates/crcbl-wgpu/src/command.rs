//! The wgpu `CommandEncoder` implementation — P5.2 stubs.

use crcbl_hal::CommandEncoder;

/// The wgpu command encoder. Most methods are stubs for P5.2.
#[derive(Debug)]
pub struct WgpuCommandEncoder {
    encoder: wgpu::CommandEncoder,
}

impl WgpuCommandEncoder {
    pub fn new(encoder: wgpu::CommandEncoder) -> Self {
        Self { encoder }
    }
}

impl CommandEncoder for WgpuCommandEncoder {
    fn begin_debug_label(&mut self, _label: &str) {}
    fn end_debug_label(&mut self) {}
    fn insert_debug_marker(&mut self, _label: &str) {}
    fn pipeline_barrier(&mut self, _barriers: &crcbl_hal::Barriers<'_>) {}
    fn copy_buffer_to_buffer(&mut self, _copy: &crcbl_hal::BufferCopy) {}
    fn copy_buffer_to_image(&mut self, _copy: &crcbl_hal::BufferImageCopy) {}
    fn copy_image_to_buffer(&mut self, _copy: &crcbl_hal::BufferImageCopy) {}
    fn copy_image_to_image(&mut self, _copy: &crcbl_hal::ImageCopy) {}
    fn fill_buffer(
        &mut self,
        _buffer: crcbl_hal::BufferHandle,
        _offset: u64,
        _size: u64,
        _value: u32,
    ) {
    }
    fn begin_render_pass(&mut self, _desc: &crcbl_hal::RenderPassDesc<'_>) {}
    fn end_render_pass(&mut self) {}
    fn set_viewport(&mut self, _viewport: &crcbl_hal::Viewport) {}
    fn set_scissor(&mut self, _rect: &crcbl_hal::Rect2d) {}
    fn set_stencil_reference(&mut self, _reference: u32) {}
    fn bind_graphics_pipeline(&mut self, _pipeline: crcbl_hal::GraphicsPipelineHandle) {}
    fn bind_index_buffer(
        &mut self,
        _buffer: crcbl_hal::BufferHandle,
        _offset: u64,
        _format: crcbl_hal::IndexFormat,
    ) {
    }
    fn bind_group(
        &mut self,
        _slot: u32,
        _group: crcbl_hal::BindGroupHandle,
        _dynamic_offsets: &[u32],
        _layout: crcbl_hal::PipelineLayoutHandle,
    ) {
    }
    fn push_constants(
        &mut self,
        _stages: crcbl_hal::ShaderStages,
        _offset: u32,
        _data: &[u8],
        _layout: crcbl_hal::PipelineLayoutHandle,
    ) {
    }
    fn draw(&mut self, _vertices: std::ops::Range<u32>, _instances: std::ops::Range<u32>) {}
    fn draw_indexed(
        &mut self,
        _indices: std::ops::Range<u32>,
        _base_vertex: i32,
        _instances: std::ops::Range<u32>,
    ) {
    }
    fn draw_indirect(&mut self, _draw: &crcbl_hal::DrawIndirect) {}
    fn draw_indexed_indirect(&mut self, _draw: &crcbl_hal::DrawIndirect) {}
    fn draw_indirect_count(&mut self, _draw: &crcbl_hal::DrawIndirectCount) {}
    fn draw_indexed_indirect_count(&mut self, _draw: &crcbl_hal::DrawIndirectCount) {}
    fn begin_compute_pass(&mut self, _desc: &crcbl_hal::ComputePassDesc<'_>) {}
    fn end_compute_pass(&mut self) {}
    fn bind_compute_pipeline(&mut self, _pipeline: crcbl_hal::ComputePipelineHandle) {}
    fn dispatch(&mut self, _x: u32, _y: u32, _z: u32) {}
    fn dispatch_indirect(&mut self, _args: crcbl_hal::BufferHandle, _offset: u64) {}
    fn reset_query_set(&mut self, _set: crcbl_hal::QuerySetHandle, _range: std::ops::Range<u32>) {}
    fn write_timestamp(&mut self, _set: crcbl_hal::QuerySetHandle, _index: u32) {}
    fn resolve_query_set(
        &mut self,
        _set: crcbl_hal::QuerySetHandle,
        _range: std::ops::Range<u32>,
        _dst: crcbl_hal::BufferHandle,
        _dst_offset: u64,
    ) {
    }
    fn finish(self: Box<Self>) -> Result<crcbl_hal::CommandBufferHandle, crcbl_hal::HalError> {
        let _buffer = self.encoder.finish();
        Err(crcbl_hal::HalError::Unsupported {
            backend: crcbl_hal::BackendKind::Wgpu,
            what: "command_buffer finish (P5.2)",
        })
    }
}
