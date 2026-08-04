//! The wgpu `CommandEncoder` implementation.
//!
//! # Errors are deferred to `finish`
//!
//! Most of [`CommandEncoder`]'s methods return `()`, so a recording step that
//! wgpu cannot express has nowhere to report. Rather than dropping it — a
//! silent no-op is exactly the failure this backend was reviewed for — the
//! first such step is remembered and [`CommandEncoder::finish`] returns it, so
//! the whole submission fails loudly instead of drawing from memory nothing
//! ever wrote.

use crate::cell::Shared;

use crcbl_hal::{
    self as hal, BackendKind, CommandBufferHandle, CommandEncoder, GraphicsPipelineHandle,
    HalError, IndexFormat, LoadOp, StoreOp,
};

use crate::resources::Pools;

pub struct WgpuCommandEncoder {
    encoder: Option<wgpu::CommandEncoder>,
    handle: CommandBufferHandle,
    pools: Shared<Pools>,
    render_pass: Option<wgpu::RenderPass<'static>>,
    compute_pass: Option<wgpu::ComputePass<'static>>,
    /// Whether the device enabled `wgpu::Features::IMMEDIATES`, which is what
    /// the seam calls `Features::PUSH_CONSTANTS`.
    immediates: bool,
    /// Whether the device enabled `wgpu::Features::MULTI_DRAW_INDIRECT_COUNT`.
    /// Without it wgpu's `multi_draw_*_indirect_count` panics, so the seam's
    /// `Unsupported` has to be produced before the call, not after.
    indirect_count: bool,
    /// The first thing this encoder was asked to record and could not.
    deferred: Option<HalError>,
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
        pools: Shared<Pools>,
        immediates: bool,
        indirect_count: bool,
    ) -> Self {
        Self {
            encoder: Some(encoder),
            handle,
            pools,
            render_pass: None,
            compute_pass: None,
            immediates,
            indirect_count,
            deferred: None,
        }
    }

    /// Records the first failure and drops the rest — the first one is the
    /// cause, and the ones after it are usually its consequences.
    fn fail(&mut self, error: HalError) {
        if self.deferred.is_none() {
            log::error!("crcbl-wgpu: command recording failed: {error}");
            self.deferred = Some(error);
        }
    }

    fn unsupported(&mut self, what: &'static str) {
        self.fail(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what,
        });
    }

    /// The encoder, outside any pass. `None` once `finish` has consumed it.
    fn encoder_mut(&mut self) -> Option<&mut wgpu::CommandEncoder> {
        self.encoder.as_mut()
    }

    fn buffer(&self, handle: hal::BufferHandle) -> Result<wgpu::Buffer, HalError> {
        self.pools
            .buffers
            .lock()
            .unwrap()
            .get(handle.cast())
            .map(|slot| slot.buffer.clone())
            .ok_or_else(|| HalError::invalid_handle("buffer", handle))
    }

    fn image(&self, handle: hal::ImageHandle) -> Result<wgpu::Texture, HalError> {
        self.pools
            .images
            .lock()
            .unwrap()
            .get(handle.cast())
            .cloned()
            .ok_or_else(|| HalError::invalid_handle("image", handle))
    }

    /// Bytes per row for a buffer↔image copy, from the seam's *texel* counts.
    ///
    /// `0` means tightly packed, per [`hal::BufferImageCopy`], and wgpu wants
    /// `None` when there is only one row.
    fn copy_layout(
        texture: &wgpu::Texture,
        copy: &hal::BufferImageCopy,
        aspect: wgpu::TextureAspect,
    ) -> Result<wgpu::TexelCopyBufferLayout, HalError> {
        let format = texture.format();
        let block = format.block_copy_size(Some(aspect)).ok_or_else(|| {
            HalError::InvalidDescriptor(format!(
                "{format:?} has no copy footprint for the {aspect:?} aspect"
            ))
        })?;
        let (block_width, block_height) = format.block_dimensions();
        let row_texels = if copy.buffer_row_length == 0 {
            copy.image_extent.width
        } else {
            copy.buffer_row_length
        };
        let image_rows = if copy.buffer_image_height == 0 {
            copy.image_extent.height
        } else {
            copy.buffer_image_height
        };
        let bytes_per_row = row_texels.div_ceil(block_width) * block;
        let rows_per_image = image_rows.div_ceil(block_height);
        let multi_row = copy.image_extent.height > 1 || copy.image_extent.depth_or_layers > 1;
        // wgpu requires the *row pitch* of a multi-row copy to be a multiple of
        // 256, which is what `Limits::optimal_buffer_copy_offset_alignment`
        // reports on this backend. A tightly packed source is legal on Vulkan
        // and not here, so the caller is told which number to pad to rather
        // than left with a validation message from inside wgpu.
        if multi_row && !bytes_per_row.is_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "a buffer↔image copy of {row_texels} texel(s) per row is {bytes_per_row} bytes, \
                 which wgpu requires to be a multiple of {}; pad the rows and set \
                 BufferImageCopy::buffer_row_length (see Limits::\
                 optimal_buffer_copy_offset_alignment)",
                wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
            )));
        }
        Ok(wgpu::TexelCopyBufferLayout {
            offset: copy.buffer_offset,
            bytes_per_row: multi_row.then_some(bytes_per_row),
            rows_per_image: (copy.image_extent.depth_or_layers > 1).then_some(rows_per_image),
        })
    }

    fn texture_info<'t>(
        texture: &'t wgpu::Texture,
        subresource: hal::ImageSubresourceLayers,
        offset: hal::Offset3d,
        aspect: wgpu::TextureAspect,
    ) -> Result<wgpu::TexelCopyTextureInfo<'t>, HalError> {
        let origin = wgpu::Origin3d {
            x: u32::try_from(offset.x).map_err(|_| negative_offset(offset))?,
            y: u32::try_from(offset.y).map_err(|_| negative_offset(offset))?,
            z: u32::try_from(offset.z).map_err(|_| negative_offset(offset))?,
        };
        Ok(wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: subresource.mip,
            origin: wgpu::Origin3d {
                z: origin.z + subresource.base_layer,
                ..origin
            },
            aspect,
        })
    }

    /// The argument buffer for an indirect draw, or `None` once the failure has
    /// been recorded.
    fn indirect_args(&mut self, draw: &hal::DrawIndirect) -> Option<wgpu::Buffer> {
        match self.buffer(draw.args) {
            Ok(buffer) => Some(buffer),
            Err(error) => {
                self.fail(error);
                None
            }
        }
    }

    /// The argument and count buffers for a GPU-counted indirect draw.
    ///
    /// `None` once the reason has been recorded — a missing handle, or a device
    /// that did not enable the count feature, in which case wgpu would panic
    /// rather than return.
    fn indirect_count_args(
        &mut self,
        draw: &hal::DrawIndirectCount,
    ) -> Option<(wgpu::Buffer, wgpu::Buffer)> {
        if !self.indirect_count {
            self.unsupported(
                "draw-indirect-count: the device did not enable wgpu's \
                 MULTI_DRAW_INDIRECT_COUNT feature",
            );
            return None;
        }
        match (self.buffer(draw.args), self.buffer(draw.count_buffer)) {
            (Ok(args), Ok(count)) => Some((args, count)),
            (Err(error), _) | (_, Err(error)) => {
                self.fail(error);
                None
            }
        }
    }
}

fn negative_offset(offset: hal::Offset3d) -> HalError {
    HalError::InvalidDescriptor(format!(
        "wgpu copy origins are unsigned; got {offset:?}, which no image contains"
    ))
}

/// wgpu has no `DontCare` load: every attachment is either loaded or cleared.
///
/// Clearing is the correct substitute — `DontCare` means "the pass does not
/// read what was there", so overwriting it is legal, while `Load` would not be
/// — and it is what wgpu's own guidance recommends. It is not free on a
/// tile-based GPU, which is the one place the seam's `DontCare` would have
/// saved bandwidth.
fn map_color_load_op(op: LoadOp, clear: [f64; 4]) -> wgpu::LoadOp<wgpu::Color> {
    match op {
        LoadOp::Load => wgpu::LoadOp::Load,
        LoadOp::Clear => wgpu::LoadOp::Clear(wgpu::Color {
            r: clear[0],
            g: clear[1],
            b: clear[2],
            a: clear[3],
        }),
        LoadOp::DontCare => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
    }
}

/// See [`map_color_load_op`]. `depth::CLEAR` is `0.0`, so a discarded depth
/// buffer is cleared to the reversed-Z far plane rather than to `1.0`.
fn map_depth_load_op(op: LoadOp, clear: f32) -> wgpu::LoadOp<f32> {
    match op {
        LoadOp::Load => wgpu::LoadOp::Load,
        LoadOp::Clear => wgpu::LoadOp::Clear(clear),
        LoadOp::DontCare => wgpu::LoadOp::Clear(hal::depth::CLEAR),
    }
}

fn map_stencil_load_op(op: LoadOp, clear: u32) -> wgpu::LoadOp<u32> {
    match op {
        LoadOp::Load => wgpu::LoadOp::Load,
        LoadOp::Clear => wgpu::LoadOp::Clear(clear),
        LoadOp::DontCare => wgpu::LoadOp::Clear(0),
    }
}

fn map_store_op(op: StoreOp) -> wgpu::StoreOp {
    match op {
        StoreOp::Store => wgpu::StoreOp::Store,
        StoreOp::Discard => wgpu::StoreOp::Discard,
    }
}

impl CommandEncoder for WgpuCommandEncoder {
    // Debug scopes belong to whichever scope is open: pushing a group on the
    // encoder while a pass is recording nests the two wrong, and wgpu rejects
    // an unbalanced pop.
    fn begin_debug_label(&mut self, label: &str) {
        if let Some(pass) = self.render_pass.as_mut() {
            pass.push_debug_group(label);
        } else if let Some(pass) = self.compute_pass.as_mut() {
            pass.push_debug_group(label);
        } else if let Some(encoder) = self.encoder_mut() {
            encoder.push_debug_group(label);
        }
    }
    fn end_debug_label(&mut self) {
        if let Some(pass) = self.render_pass.as_mut() {
            pass.pop_debug_group();
        } else if let Some(pass) = self.compute_pass.as_mut() {
            pass.pop_debug_group();
        } else if let Some(encoder) = self.encoder_mut() {
            encoder.pop_debug_group();
        }
    }
    fn insert_debug_marker(&mut self, label: &str) {
        if let Some(pass) = self.render_pass.as_mut() {
            pass.insert_debug_marker(label);
        } else if let Some(pass) = self.compute_pass.as_mut() {
            pass.insert_debug_marker(label);
        } else if let Some(encoder) = self.encoder_mut() {
            encoder.insert_debug_marker(label);
        }
    }

    /// A genuine no-op, not a stub.
    ///
    /// WebGPU has no barriers: the implementation tracks resource usage itself
    /// and inserts whatever transitions and memory dependencies a submission
    /// needs. The seam says so — see `crcbl_hal::command`'s "the backend
    /// expands one into … nothing at all on WebGPU". The graph still computes
    /// and records the transitions, which is what keeps the Vulkan backend
    /// honest and the null backend's assertions meaningful.
    fn pipeline_barrier(&mut self, _barriers: &hal::Barriers<'_>) {}

    fn copy_buffer_to_buffer(&mut self, copy: &hal::BufferCopy) {
        let (src, dst) = match (self.buffer(copy.src), self.buffer(copy.dst)) {
            (Ok(src), Ok(dst)) => (src, dst),
            (Err(error), _) | (_, Err(error)) => return self.fail(error),
        };
        let Some(encoder) = self.encoder_mut() else {
            return self.unsupported("copy after finish");
        };
        encoder.copy_buffer_to_buffer(
            &src,
            copy.src_offset,
            &dst,
            copy.dst_offset,
            Some(copy.size),
        );
    }

    fn copy_buffer_to_image(&mut self, copy: &hal::BufferImageCopy) {
        let aspect = match crate::conv::map_aspect(copy.image_subresource.aspect) {
            Ok(aspect) => aspect,
            Err(error) => return self.fail(error),
        };
        let (buffer, texture) = match (self.buffer(copy.buffer), self.image(copy.image)) {
            (Ok(buffer), Ok(texture)) => (buffer, texture),
            (Err(error), _) | (_, Err(error)) => return self.fail(error),
        };
        let layout = match Self::copy_layout(&texture, copy, aspect) {
            Ok(layout) => layout,
            Err(error) => return self.fail(error),
        };
        let destination =
            match Self::texture_info(&texture, copy.image_subresource, copy.image_offset, aspect) {
                Ok(info) => info,
                Err(error) => return self.fail(error),
            };
        let extent = wgpu::Extent3d {
            width: copy.image_extent.width,
            height: copy.image_extent.height,
            depth_or_array_layers: copy.image_extent.depth_or_layers,
        };
        let Some(encoder) = self.encoder.as_mut() else {
            return self.unsupported("copy after finish");
        };
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout,
            },
            destination,
            extent,
        );
    }

    fn copy_image_to_buffer(&mut self, copy: &hal::BufferImageCopy) {
        let aspect = match crate::conv::map_aspect(copy.image_subresource.aspect) {
            Ok(aspect) => aspect,
            Err(error) => return self.fail(error),
        };
        let (buffer, texture) = match (self.buffer(copy.buffer), self.image(copy.image)) {
            (Ok(buffer), Ok(texture)) => (buffer, texture),
            (Err(error), _) | (_, Err(error)) => return self.fail(error),
        };
        let layout = match Self::copy_layout(&texture, copy, aspect) {
            Ok(layout) => layout,
            Err(error) => return self.fail(error),
        };
        let source =
            match Self::texture_info(&texture, copy.image_subresource, copy.image_offset, aspect) {
                Ok(info) => info,
                Err(error) => return self.fail(error),
            };
        let extent = wgpu::Extent3d {
            width: copy.image_extent.width,
            height: copy.image_extent.height,
            depth_or_array_layers: copy.image_extent.depth_or_layers,
        };
        let Some(encoder) = self.encoder.as_mut() else {
            return self.unsupported("copy after finish");
        };
        encoder.copy_texture_to_buffer(
            source,
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout,
            },
            extent,
        );
    }

    fn copy_image_to_image(&mut self, copy: &hal::ImageCopy) {
        let (src_aspect, dst_aspect) = match (
            crate::conv::map_aspect(copy.src_subresource.aspect),
            crate::conv::map_aspect(copy.dst_subresource.aspect),
        ) {
            (Ok(src), Ok(dst)) => (src, dst),
            (Err(error), _) | (_, Err(error)) => return self.fail(error),
        };
        let (src, dst) = match (self.image(copy.src), self.image(copy.dst)) {
            (Ok(src), Ok(dst)) => (src, dst),
            (Err(error), _) | (_, Err(error)) => return self.fail(error),
        };
        let source =
            match Self::texture_info(&src, copy.src_subresource, copy.src_offset, src_aspect) {
                Ok(info) => info,
                Err(error) => return self.fail(error),
            };
        let destination =
            match Self::texture_info(&dst, copy.dst_subresource, copy.dst_offset, dst_aspect) {
                Ok(info) => info,
                Err(error) => return self.fail(error),
            };
        let extent = wgpu::Extent3d {
            width: copy.extent.width,
            height: copy.extent.height,
            depth_or_array_layers: copy.extent.depth_or_layers,
        };
        let Some(encoder) = self.encoder.as_mut() else {
            return self.unsupported("copy after finish");
        };
        encoder.copy_texture_to_texture(source, destination, extent);
    }

    fn fill_buffer(&mut self, buffer: hal::BufferHandle, offset: u64, size: u64, value: u32) {
        // `clear_buffer` is a zero fill and wgpu has nothing else: the seam's
        // headline use — zeroing an indirect count at the top of a frame — is
        // exactly the case it covers, and any other value is a write nothing
        // here can perform.
        if value != 0 {
            return self.unsupported(
                "fill_buffer with a non-zero value: wgpu only offers a zero fill (clear_buffer)",
            );
        }
        let target = match self.buffer(buffer) {
            Ok(buffer) => buffer,
            Err(error) => return self.fail(error),
        };
        let Some(encoder) = self.encoder_mut() else {
            return self.unsupported("fill_buffer after finish");
        };
        encoder.clear_buffer(&target, offset, Some(size));
    }

    fn begin_render_pass(&mut self, desc: &hal::RenderPassDesc<'_>) {
        // Resolve all attachment views from pools before building descriptor.
        let color_views: Vec<Option<wgpu::TextureView>> = {
            let views = self.pools.image_views.lock().unwrap();
            desc.color_attachments
                .iter()
                .map(|a| views.get(a.view.cast()).cloned())
                .collect()
        };
        for (attachment, view) in desc.color_attachments.iter().zip(&color_views) {
            if view.is_none() {
                return self.fail(HalError::invalid_handle("image view", attachment.view));
            }
        }
        // The resolve destinations, alongside the colour views: `att.resolve`
        // is read here and nowhere else, so a stale handle has to fail here
        // rather than silently dropping the resolve.
        let resolve_views: Vec<Option<wgpu::TextureView>> = {
            let views = self.pools.image_views.lock().unwrap();
            desc.color_attachments
                .iter()
                .map(|a| match a.resolve {
                    Some(handle) => views.get(handle.cast()).cloned(),
                    None => None,
                })
                .collect()
        };
        for (attachment, resolve) in desc.color_attachments.iter().zip(&resolve_views) {
            if let Some(handle) = attachment.resolve
                && resolve.is_none()
            {
                return self.fail(HalError::invalid_handle("image view", handle));
            }
        }
        let depth_view: Option<wgpu::TextureView> = match desc.depth_stencil_attachment {
            Some(ds) => {
                let views = self.pools.image_views.lock().unwrap();
                match views.get(ds.view.cast()).cloned() {
                    Some(view) => Some(view),
                    None => {
                        drop(views);
                        return self.fail(HalError::invalid_handle("image view", ds.view));
                    }
                }
            }
            None => None,
        };

        let color_attachments: Vec<_> = desc
            .color_attachments
            .iter()
            .zip(color_views.iter())
            .zip(resolve_views.iter())
            .map(|((att, view), resolve)| {
                let clear_color: [f64; 4] = att.clear.color.map(f64::from);
                view.as_ref().map(|view| wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve.as_ref(),
                    ops: wgpu::Operations {
                        load: map_color_load_op(att.load, clear_color),
                        store: map_store_op(att.store),
                    },
                })
            })
            .collect();

        let depth_stencil_attachment = desc.depth_stencil_attachment.and_then(|ds| {
            depth_view
                .as_ref()
                .map(|view| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: map_depth_load_op(ds.depth_load, ds.clear.depth),
                        store: map_store_op(ds.depth_store),
                    }),
                    // A `D32Float` attachment has no stencil plane, and wgpu
                    // rejects stencil ops on one that does not. The graph fills
                    // the stencil fields unconditionally, so the format — not
                    // the descriptor — is what decides.
                    stencil_ops: view.texture().format().has_stencil_aspect().then(|| {
                        wgpu::Operations {
                            load: map_stencil_load_op(ds.stencil_load, ds.clear.stencil),
                            store: map_store_op(ds.stencil_store),
                        }
                    }),
                })
        });

        let wgpu_desc = wgpu::RenderPassDescriptor {
            label: desc.label,
            color_attachments: &color_attachments,
            depth_stencil_attachment,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        };

        let Some(encoder) = self.encoder.as_mut() else {
            return self.unsupported("begin_render_pass after finish");
        };
        let rpass = encoder.begin_render_pass(&wgpu_desc).forget_lifetime();

        // wgpu has no per-pass render area; `CompiledGraph::execute` sets the
        // viewport and scissor from `pass.render_area` immediately after this.
        self.render_pass = Some(rpass);
    }

    fn end_render_pass(&mut self) {
        drop(self.render_pass.take());
    }

    fn set_viewport(&mut self, vp: &hal::Viewport) {
        if let Some(ref mut rp) = self.render_pass {
            rp.set_viewport(vp.x, vp.y, vp.width, vp.height, vp.depth_min, vp.depth_max);
        }
    }

    fn set_scissor(&mut self, rect: &hal::Rect2d) {
        let Some(ref mut rp) = self.render_pass else {
            return;
        };
        // wgpu's scissor is unsigned. A negative origin means the rectangle
        // starts off-screen; clipping it to the framebuffer is what every other
        // backend does, and casting would have turned `-1` into 4294967295.
        let x = rect.x.max(0);
        let y = rect.y.max(0);
        let clipped_x = u32::try_from(x - rect.x).unwrap_or(0);
        let clipped_y = u32::try_from(y - rect.y).unwrap_or(0);
        let width = rect.width.saturating_sub(clipped_x);
        let height = rect.height.saturating_sub(clipped_y);
        rp.set_scissor_rect(
            u32::try_from(x).unwrap_or(0),
            u32::try_from(y).unwrap_or(0),
            width,
            height,
        );
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
        let Some(rp) = rp else {
            return self.fail(HalError::invalid_handle("graphics pipeline", pipeline));
        };
        if let Some(ref mut pass) = self.render_pass {
            pass.set_pipeline(&rp);
        }
    }

    fn bind_index_buffer(&mut self, buffer: hal::BufferHandle, offset: u64, format: IndexFormat) {
        let buf = match self.buffer(buffer) {
            Ok(buffer) => buffer,
            Err(error) => return self.fail(error),
        };
        let idx_format = crate::conv::map_index_format(format);
        if let Some(ref mut pass) = self.render_pass {
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
        let Some(ref bg) = bg else {
            return self.fail(HalError::invalid_handle("bind group", group));
        };
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
        offset: u32,
        data: &[u8],
        _layout: hal::PipelineLayoutHandle,
    ) {
        // wgpu 30 calls them *immediates*, and they are one block shared by
        // every stage rather than a per-stage range — so the stage mask is
        // information wgpu does not need rather than information it loses.
        if !self.immediates {
            return self.unsupported(
                "push constants: the device did not enable wgpu's IMMEDIATES feature, so the \
                 pipeline layout has no immediate block to write",
            );
        }
        if let Some(ref mut pass) = self.render_pass {
            pass.set_immediates(offset, data);
        }
        if let Some(ref mut pass) = self.compute_pass {
            pass.set_immediates(offset, data);
        }
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

    fn draw_indirect(&mut self, draw: &hal::DrawIndirect) {
        // wgpu's multi_draw_indirect reads tightly packed DrawIndirect structs
        // (16 bytes); crcbl-vk honours `stride`, and a padded stride here would
        // read garbage silently.
        if draw.stride != core::mem::size_of::<wgpu::util::DrawIndirectArgs>() as u32 {
            return self
                .unsupported("draw_indirect: wgpu requires a tightly packed stride (16 bytes)");
        }
        let Some(args) = self.indirect_args(draw) else {
            return;
        };
        if let Some(ref mut rp) = self.render_pass {
            rp.multi_draw_indirect(&args, draw.offset, draw.draw_count);
        }
    }

    fn draw_indexed_indirect(&mut self, draw: &hal::DrawIndirect) {
        // wgpu's multi_draw_indexed_indirect reads tightly packed
        // DrawIndexedIndirect structs (20 bytes); crcbl-vk honours `stride`,
        // and a padded stride here would read garbage silently.
        if draw.stride != core::mem::size_of::<wgpu::util::DrawIndexedIndirectArgs>() as u32 {
            return self.unsupported(
                "draw_indexed_indirect: wgpu requires a tightly packed stride (20 bytes)",
            );
        }
        let Some(args) = self.indirect_args(draw) else {
            return;
        };
        if let Some(ref mut rp) = self.render_pass {
            rp.multi_draw_indexed_indirect(&args, draw.offset, draw.draw_count);
        }
    }

    fn draw_indirect_count(&mut self, draw: &hal::DrawIndirectCount) {
        // The count methods read the same tightly packed argument structs wgpu
        // requires of the CPU-counted draws, so the padded stride is refused
        // the same way — the count buffer only says *how many* draws, never how
        // far apart the arguments are.
        if draw.stride != core::mem::size_of::<wgpu::util::DrawIndirectArgs>() as u32 {
            return self.unsupported(
                "draw_indirect_count: wgpu requires a tightly packed stride (16 bytes)",
            );
        }
        let Some((args, count)) = self.indirect_count_args(draw) else {
            return;
        };
        if let Some(ref mut rp) = self.render_pass {
            rp.multi_draw_indirect_count(
                &args,
                draw.args_offset,
                &count,
                draw.count_offset,
                draw.max_draw_count,
            );
        }
    }

    fn draw_indexed_indirect_count(&mut self, draw: &hal::DrawIndirectCount) {
        if draw.stride != core::mem::size_of::<wgpu::util::DrawIndexedIndirectArgs>() as u32 {
            return self.unsupported(
                "draw_indexed_indirect_count: wgpu requires a tightly packed stride (20 bytes)",
            );
        }
        let Some((args, count)) = self.indirect_count_args(draw) else {
            return;
        };
        if let Some(ref mut rp) = self.render_pass {
            rp.multi_draw_indexed_indirect_count(
                &args,
                draw.args_offset,
                &count,
                draw.count_offset,
                draw.max_draw_count,
            );
        }
    }

    fn begin_compute_pass(&mut self, desc: &hal::ComputePassDesc<'_>) {
        let wgpu_desc = wgpu::ComputePassDescriptor {
            label: desc.label,
            timestamp_writes: None,
        };
        let Some(encoder) = self.encoder.as_mut() else {
            return self.unsupported("begin_compute_pass after finish");
        };
        let cpass = encoder.begin_compute_pass(&wgpu_desc).forget_lifetime();
        self.compute_pass = Some(cpass);
    }

    fn end_compute_pass(&mut self) {
        drop(self.compute_pass.take());
    }

    fn bind_compute_pipeline(&mut self, pipeline: hal::ComputePipelineHandle) {
        let cp = self
            .pools
            .compute_pipelines
            .lock()
            .unwrap()
            .get(pipeline.cast())
            .cloned();
        let Some(cp) = cp else {
            return self.fail(HalError::invalid_handle("compute pipeline", pipeline));
        };
        if let Some(ref mut pass) = self.compute_pass {
            pass.set_pipeline(&cp);
        }
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) {
        if let Some(ref mut cp) = self.compute_pass {
            cp.dispatch_workgroups(x, y, z);
        }
    }

    fn dispatch_indirect(&mut self, args: hal::BufferHandle, offset: u64) {
        let buffer = match self.buffer(args) {
            Ok(buffer) => buffer,
            Err(error) => return self.fail(error),
        };
        if let Some(ref mut cp) = self.compute_pass {
            cp.dispatch_workgroups_indirect(&buffer, offset);
        }
    }

    // Query sets are `Unsupported` on this backend — `Device::create_query_set`
    // refuses, so no handle these could name can exist. Recording against one
    // is therefore a stale handle, which the seam says is the caller's bug.
    fn reset_query_set(&mut self, set: hal::QuerySetHandle, _r: std::ops::Range<u32>) {
        self.fail(HalError::invalid_handle("query set", set));
    }
    fn write_timestamp(&mut self, _set: hal::QuerySetHandle, _index: u32) {
        // Accepted and dropped: the seam says a device without
        // `Features::TIMESTAMP_QUERY` degrades rather than breaks, and this
        // backend never reports it.
    }
    fn resolve_query_set(
        &mut self,
        set: hal::QuerySetHandle,
        _r: std::ops::Range<u32>,
        _d: hal::BufferHandle,
        _o: u64,
    ) {
        self.fail(HalError::invalid_handle("query set", set));
    }

    fn finish(self: Box<Self>) -> Result<CommandBufferHandle, HalError> {
        let this = *self;
        // Drop any active passes before finishing.
        drop(this.render_pass);
        drop(this.compute_pass);
        if let Some(error) = this.deferred {
            return Err(error);
        }
        match this.encoder {
            Some(enc) => {
                let wgpu_cb = enc.finish();
                let mut cmds = this.pools.command_buffers.lock().unwrap();
                let slot = cmds
                    .get_mut(this.handle.cast())
                    .ok_or_else(|| HalError::invalid_handle("command buffer", this.handle))?;
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
