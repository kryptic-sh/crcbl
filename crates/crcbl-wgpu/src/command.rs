//! The wgpu `CommandEncoder` implementation.

use std::sync::{Arc, Mutex};

use crcbl_core::Pool;
use crcbl_hal::{self as hal, CommandBufferHandle, CommandEncoder};

use crate::resources::CommandBufferSlot;

pub struct WgpuCommandEncoder {
    encoder: Option<wgpu::CommandEncoder>,
    handle: CommandBufferHandle,
    command_buffers: Arc<Mutex<Pool<CommandBufferSlot>>>,
}

impl std::fmt::Debug for WgpuCommandEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuCommandEncoder").finish_non_exhaustive()
    }
}

impl WgpuCommandEncoder {
    pub fn new(
        encoder: wgpu::CommandEncoder,
        handle: CommandBufferHandle,
        command_buffers: Arc<Mutex<Pool<CommandBufferSlot>>>,
    ) -> Self {
        Self {
            encoder: Some(encoder),
            handle,
            command_buffers,
        }
    }
}

impl CommandEncoder for WgpuCommandEncoder {
    fn begin_debug_label(&mut self, label: &str) {
        if let Some(ref mut enc) = self.encoder {
            enc.push_debug_group(label);
        }
    }
    fn end_debug_label(&mut self) {
        if let Some(ref mut enc) = self.encoder {
            enc.pop_debug_group();
        }
    }
    fn insert_debug_marker(&mut self, label: &str) {
        if let Some(ref mut enc) = self.encoder {
            enc.insert_debug_marker(label);
        }
    }

    fn pipeline_barrier(&mut self, _barriers: &hal::Barriers<'_>) {}
    fn copy_buffer_to_buffer(&mut self, _copy: &hal::BufferCopy) {}
    fn copy_buffer_to_image(&mut self, _copy: &hal::BufferImageCopy) {}
    fn copy_image_to_buffer(&mut self, _copy: &hal::BufferImageCopy) {}
    fn copy_image_to_image(&mut self, _copy: &hal::ImageCopy) {}
    fn fill_buffer(&mut self, _b: hal::BufferHandle, _off: u64, _sz: u64, _v: u32) {}
    fn begin_render_pass(&mut self, _desc: &hal::RenderPassDesc<'_>) {}
    fn end_render_pass(&mut self) {}
    fn set_viewport(&mut self, _vp: &hal::Viewport) {}
    fn set_scissor(&mut self, _rect: &hal::Rect2d) {}
    fn set_stencil_reference(&mut self, _ref: u32) {}
    fn bind_graphics_pipeline(&mut self, _p: hal::GraphicsPipelineHandle) {}
    fn bind_index_buffer(&mut self, _b: hal::BufferHandle, _off: u64, _fmt: hal::IndexFormat) {}
    fn bind_group(
        &mut self,
        _s: u32,
        _g: hal::BindGroupHandle,
        _o: &[u32],
        _l: hal::PipelineLayoutHandle,
    ) {
    }
    fn push_constants(
        &mut self,
        _s: hal::ShaderStages,
        _o: u32,
        _d: &[u8],
        _l: hal::PipelineLayoutHandle,
    ) {
    }
    fn draw(&mut self, _v: std::ops::Range<u32>, _i: std::ops::Range<u32>) {}
    fn draw_indexed(&mut self, _i: std::ops::Range<u32>, _bv: i32, _inst: std::ops::Range<u32>) {}
    fn draw_indirect(&mut self, _d: &hal::DrawIndirect) {}
    fn draw_indexed_indirect(&mut self, _d: &hal::DrawIndirect) {}
    fn draw_indirect_count(&mut self, _d: &hal::DrawIndirectCount) {}
    fn draw_indexed_indirect_count(&mut self, _d: &hal::DrawIndirectCount) {}
    fn begin_compute_pass(&mut self, _desc: &hal::ComputePassDesc<'_>) {}
    fn end_compute_pass(&mut self) {}
    fn bind_compute_pipeline(&mut self, _p: hal::ComputePipelineHandle) {}
    fn dispatch(&mut self, _x: u32, _y: u32, _z: u32) {}
    fn dispatch_indirect(&mut self, _a: hal::BufferHandle, _o: u64) {}
    fn reset_query_set(&mut self, _s: hal::QuerySetHandle, _r: std::ops::Range<u32>) {}
    fn write_timestamp(&mut self, _s: hal::QuerySetHandle, _i: u32) {}
    fn resolve_query_set(
        &mut self,
        _s: hal::QuerySetHandle,
        _r: std::ops::Range<u32>,
        _d: hal::BufferHandle,
        _o: u64,
    ) {
    }

    fn finish(self: Box<Self>) -> Result<CommandBufferHandle, hal::HalError> {
        let this = *self;
        match this.encoder {
            Some(enc) => {
                let wgpu_cb = enc.finish();
                let mut cmds = this.command_buffers.lock().unwrap();
                let slot = cmds
                    .get_mut(this.handle.cast())
                    .expect("pre-allocated handle");
                slot.buffer = Some(wgpu_cb);
                Ok(this.handle)
            }
            None => Err(hal::HalError::Unsupported {
                backend: hal::BackendKind::Wgpu,
                what: "encoder already finished",
            }),
        }
    }
}
