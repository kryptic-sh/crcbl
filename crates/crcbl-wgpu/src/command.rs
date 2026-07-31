//! The wgpu `CommandEncoder` implementation.

use std::sync::Arc;

use crcbl_hal::{
    self as hal, CommandBufferHandle, CommandEncoder, GraphicsPipelineHandle, HalError,
    IndexFormat, LoadOp,
};

use crate::resources::Pools;

pub struct WgpuCommandEncoder {
    encoder: Option<wgpu::CommandEncoder>,
    handle: CommandBufferHandle,
    pools: Arc<Pools>,
    render_pass: Option<wgpu::RenderPass<'static>>,
    compute_pass: Option<wgpu::ComputePass<'static>>,
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
        pools: Arc<Pools>,
    ) -> Self {
        Self {
            encoder: Some(encoder),
            handle,
            pools,
            render_pass: None,
            compute_pass: None,
        }
    }

    /// Must be called at end of scope — errors if called inside a pass.
    fn encoder_mut(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder
            .as_mut()
            .expect("encoder consumed before finish")
    }
}

fn map_color_load_op(op: LoadOp, clear: [f64; 4]) -> wgpu::LoadOp<wgpu::Color> {
    match op {
        LoadOp::Load => wgpu::LoadOp::Load,
        LoadOp::Clear => wgpu::LoadOp::Clear(wgpu::Color {
            r: clear[0],
            g: clear[1],
            b: clear[2],
            a: clear[3],
        }),
        LoadOp::DontCare => wgpu::LoadOp::Clear(wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }),
    }
}

fn map_depth_load_op(op: LoadOp, clear: f32) -> wgpu::LoadOp<f32> {
    match op {
        LoadOp::Load => wgpu::LoadOp::Load,
        LoadOp::Clear => wgpu::LoadOp::Clear(clear),
        LoadOp::DontCare => wgpu::LoadOp::Clear(0.0),
    }
}

fn map_stencil_load_op(op: LoadOp, clear: u32) -> wgpu::LoadOp<u32> {
    match op {
        LoadOp::Load => wgpu::LoadOp::Load,
        LoadOp::Clear => wgpu::LoadOp::Clear(clear),
        LoadOp::DontCare => wgpu::LoadOp::Clear(0),
    }
}

impl CommandEncoder for WgpuCommandEncoder {
    fn begin_debug_label(&mut self, label: &str) {
        self.encoder_mut().push_debug_group(label);
    }
    fn end_debug_label(&mut self) {
        self.encoder_mut().pop_debug_group();
    }
    fn insert_debug_marker(&mut self, label: &str) {
        self.encoder_mut().insert_debug_marker(label);
    }

    fn pipeline_barrier(&mut self, _barriers: &hal::Barriers<'_>) {}
    fn copy_buffer_to_buffer(&mut self, _copy: &hal::BufferCopy) {}
    fn copy_buffer_to_image(&mut self, _copy: &hal::BufferImageCopy) {}
    fn copy_image_to_buffer(&mut self, _copy: &hal::BufferImageCopy) {}
    fn copy_image_to_image(&mut self, _copy: &hal::ImageCopy) {}
    fn fill_buffer(&mut self, _b: hal::BufferHandle, _off: u64, _sz: u64, _v: u32) {}

    fn begin_render_pass(&mut self, desc: &hal::RenderPassDesc<'_>) {
        // Resolve all attachment views from pools before building descriptor.
        let color_views: Vec<Option<wgpu::TextureView>> = {
            let views = self.pools.image_views.lock().unwrap();
            desc.color_attachments
                .iter()
                .map(|a| views.get(a.view.cast()).cloned())
                .collect()
        };
        let depth_view: Option<wgpu::TextureView> = desc.depth_stencil_attachment.and_then(|ds| {
            let views = self.pools.image_views.lock().unwrap();
            views.get(ds.view.cast()).cloned()
        });

        let color_attachments: Vec<_> = desc
            .color_attachments
            .iter()
            .zip(color_views.iter())
            .map(|(att, view)| {
                let clear_color: [f64; 4] = att.clear.color.map(|c| c as f64);
                Some(wgpu::RenderPassColorAttachment {
                    view: view.as_ref().unwrap(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: map_color_load_op(att.load, clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })
            })
            .collect();

        let depth_stencil_attachment =
            desc.depth_stencil_attachment
                .map(|ds| wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view.as_ref().unwrap(),
                    depth_ops: Some(wgpu::Operations {
                        load: map_depth_load_op(ds.depth_load, ds.clear.depth),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: map_stencil_load_op(ds.stencil_load, ds.clear.stencil),
                        store: wgpu::StoreOp::Store,
                    }),
                });

        let wgpu_desc = wgpu::RenderPassDescriptor {
            label: desc.label,
            color_attachments: &color_attachments,
            depth_stencil_attachment,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        };

        let rpass = self
            .encoder_mut()
            .begin_render_pass(&wgpu_desc)
            .forget_lifetime();

        // Also apply render area as scissor+viewport defaults.
        // NOTE: wgpu doesn't have per-pass render area — caller should set viewport/scissor.
        self.render_pass = Some(rpass);
    }

    fn end_render_pass(&mut self) {
        if let Some(rpass) = self.render_pass.take() {
            drop(rpass);
        }
    }

    fn set_viewport(&mut self, vp: &hal::Viewport) {
        if let Some(ref mut rp) = self.render_pass {
            rp.set_viewport(vp.x, vp.y, vp.width, vp.height, vp.depth_min, vp.depth_max);
        }
    }

    fn set_scissor(&mut self, rect: &hal::Rect2d) {
        if let Some(ref mut rp) = self.render_pass {
            rp.set_scissor_rect(rect.x as u32, rect.y as u32, rect.width, rect.height);
        }
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        if let Some(ref mut rp) = self.render_pass {
            rp.set_stencil_reference(reference);
        }
    }

    fn bind_graphics_pipeline(&mut self, pipeline: GraphicsPipelineHandle) {
        let rp = self
            .pools
            .graphics_pipelines
            .lock()
            .unwrap()
            .get(pipeline.cast())
            .cloned();
        if let (Some(ref mut pass), Some(rp)) = (self.render_pass.as_mut(), rp) {
            pass.set_pipeline(&rp);
        }
    }

    fn bind_index_buffer(&mut self, buffer: hal::BufferHandle, offset: u64, format: IndexFormat) {
        let buf = self
            .pools
            .buffers
            .lock()
            .unwrap()
            .get(buffer.cast())
            .cloned();
        let idx_format = crate::conv::map_index_format(format);
        if let (Some(ref mut pass), Some(buf)) = (self.render_pass.as_mut(), buf) {
            pass.set_index_buffer(buf.slice(offset..), idx_format);
        }
    }

    fn bind_group(
        &mut self,
        slot: u32,
        group: hal::BindGroupHandle,
        dynamic_offsets: &[u32],
        _layout: hal::PipelineLayoutHandle,
    ) {
        let bg = self
            .pools
            .bind_groups
            .lock()
            .unwrap()
            .get(group.cast())
            .cloned();
        let Some(ref bg) = bg else { return };
        if let Some(ref mut pass) = self.render_pass {
            pass.set_bind_group(slot, bg, dynamic_offsets);
        }
        if let Some(ref mut pass) = self.compute_pass {
            pass.set_bind_group(slot, bg, dynamic_offsets);
        }
    }

    fn push_constants(
        &mut self,
        _stages: hal::ShaderStages,
        _offset: u32,
        _data: &[u8],
        _layout: hal::PipelineLayoutHandle,
    ) {
        // wgpu 30 does not expose push constants directly.
        // Dynamic offsets on bind groups are the Tier B substitute (see hal docs).
    }

    fn draw(&mut self, vertices: std::ops::Range<u32>, instances: std::ops::Range<u32>) {
        if let Some(ref mut rp) = self.render_pass {
            rp.draw(vertices, instances);
        }
    }

    fn draw_indexed(
        &mut self,
        indices: std::ops::Range<u32>,
        base_vertex: i32,
        instances: std::ops::Range<u32>,
    ) {
        if let Some(ref mut rp) = self.render_pass {
            rp.draw_indexed(indices, base_vertex, instances);
        }
    }

    fn draw_indirect(&mut self, _draw: &hal::DrawIndirect) {}
    fn draw_indexed_indirect(&mut self, _draw: &hal::DrawIndirect) {}
    fn draw_indirect_count(&mut self, _draw: &hal::DrawIndirectCount) {}
    fn draw_indexed_indirect_count(&mut self, _draw: &hal::DrawIndirectCount) {}

    fn begin_compute_pass(&mut self, desc: &hal::ComputePassDesc<'_>) {
        let wgpu_desc = wgpu::ComputePassDescriptor {
            label: desc.label,
            timestamp_writes: None,
        };
        let cpass = self
            .encoder_mut()
            .begin_compute_pass(&wgpu_desc)
            .forget_lifetime();
        self.compute_pass = Some(cpass);
    }

    fn end_compute_pass(&mut self) {
        if let Some(cpass) = self.compute_pass.take() {
            drop(cpass);
        }
    }

    fn bind_compute_pipeline(&mut self, pipeline: hal::ComputePipelineHandle) {
        let cp = self
            .pools
            .compute_pipelines
            .lock()
            .unwrap()
            .get(pipeline.cast())
            .cloned();
        if let (Some(ref mut pass), Some(cp)) = (self.compute_pass.as_mut(), cp) {
            pass.set_pipeline(&cp);
        }
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        if let Some(ref mut cp) = self.compute_pass {
            cp.dispatch_workgroups(x, y, z);
        }
    }

    fn dispatch_indirect(&mut self, _args: hal::BufferHandle, _offset: u64) {}

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

    fn finish(self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
        let this = *self;
        // Drop any active passes before finishing.
        drop(this.render_pass);
        drop(this.compute_pass);
        match this.encoder {
            Some(enc) => {
                let wgpu_cb = enc.finish();
                let mut cmds = this.pools.command_buffers.lock().unwrap();
                let slot = cmds
                    .get_mut(this.handle.cast())
                    .expect("pre-allocated handle");
                slot.buffer = Some(wgpu_cb);
                Ok(this.handle)
            }
            None => Err(HalError::Unsupported {
                backend: hal::BackendKind::Wgpu,
                what: "encoder already finished",
            }),
        }
    }
}
