//! The wgpu `Device` implementation — P5.2 resource mappings.

use crcbl_hal::{
    self as hal, AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, CommandBufferHandle,
    CommandEncoderDesc, ComputePipelineDesc, ComputePipelineHandle, Device, DeviceCaps, DeviceDesc,
    GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle, ImageViewDesc,
    ImageViewHandle, PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo, QuerySetDesc,
    QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc, ReadbackHandle, ReadbackState,
    SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, ShaderModuleDesc,
    ShaderModuleHandle, SubmitInfo, SurfaceError, SwapchainDesc, SwapchainHandle,
};

use crate::conv;
use crate::resources::Pools;

pub struct WgpuDevice {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    caps: DeviceCaps,
    graphics_queue: QueueHandle,
    pools: Pools,
}

impl std::fmt::Debug for WgpuDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuDevice").finish_non_exhaustive()
    }
}

impl WgpuDevice {
    pub(crate) fn new(adapter: &wgpu::Adapter, _desc: &DeviceDesc<'_>) -> Result<Self, HalError> {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|_e| HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "device creation failed",
        })?;

        Ok(Self {
            device,
            queue,
            caps: DeviceCaps {
                features: hal::Features::empty(),
                limits: hal::Limits::desktop(),
            },
            graphics_queue: QueueHandle::from_bits(1).expect("handle 1 is valid"),
            pools: Pools::new(),
        })
    }

    fn unsupported(what: &'static str) -> HalError {
        HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what,
        }
    }
}

impl Device for WgpuDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Wgpu
    }
    fn caps(&self) -> DeviceCaps {
        self.caps
    }
    fn queue(&self, _kind: QueueKind) -> Option<QueueHandle> {
        Some(self.graphics_queue)
    }

    // ---------- buffers ----------
    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        let b = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: desc.label,
            size: desc.size,
            usage: conv::map_buffer_usage(desc.usage),
            mapped_at_creation: desc.memory == hal::MemoryLocation::HostUpload,
        });
        Ok(self.pools.buffers.lock().unwrap().insert(b).cast())
    }
    fn destroy_buffer(&self, h: BufferHandle) {
        self.pools.buffers.lock().unwrap().remove(h.cast());
    }
    fn write_buffer(&self, h: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError> {
        let pools = self.pools.buffers.lock().unwrap();
        let buf = pools
            .get(h.cast())
            .ok_or_else(|| Self::unsupported("stale buffer"))?;
        self.queue.write_buffer(buf, offset, data);
        Ok(())
    }
    fn request_readback(&self, _d: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        Err(Self::unsupported("readback"))
    }
    fn poll_readback(
        &self,
        _r: ReadbackHandle,
        _out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        Err(Self::unsupported("poll_readback"))
    }
    fn destroy_readback(&self, _r: ReadbackHandle) {}

    // ---------- images ----------
    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: desc.label,
            size: wgpu::Extent3d {
                width: desc.extent.width,
                height: desc.extent.height,
                depth_or_array_layers: desc.extent.depth_or_layers,
            },
            mip_level_count: desc.mip_levels,
            sample_count: desc.samples,
            dimension: match desc.image_type {
                hal::ImageType::D1 => wgpu::TextureDimension::D1,
                hal::ImageType::D2 => wgpu::TextureDimension::D2,
                hal::ImageType::D3 => wgpu::TextureDimension::D3,
            },
            format: conv::map_format(desc.format),
            usage: conv::map_image_usage(desc.usage),
            view_formats: &[conv::map_format(desc.format)],
        });
        Ok(self.pools.images.lock().unwrap().insert(tex).cast())
    }
    fn destroy_image(&self, h: ImageHandle) {
        self.pools.images.lock().unwrap().remove(h.cast());
    }
    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError> {
        let images = self.pools.images.lock().unwrap();
        let tex = images
            .get(desc.image.cast())
            .ok_or_else(|| Self::unsupported("stale image"))?;
        let view = tex.create_view(&wgpu::TextureViewDescriptor {
            label: desc.label,
            format: Some(conv::map_format(desc.format)),
            dimension: Some(match desc.view_type {
                hal::ImageViewType::D1 => wgpu::TextureViewDimension::D1,
                hal::ImageViewType::D2 => wgpu::TextureViewDimension::D2,
                hal::ImageViewType::D2Array => wgpu::TextureViewDimension::D2Array,
                hal::ImageViewType::Cube => wgpu::TextureViewDimension::Cube,
                hal::ImageViewType::CubeArray => wgpu::TextureViewDimension::CubeArray,
                hal::ImageViewType::D3 => wgpu::TextureViewDimension::D3,
            }),
            base_mip_level: desc.range.base_mip,
            mip_level_count: Some(desc.range.mip_count),
            base_array_layer: desc.range.base_layer,
            array_layer_count: Some(desc.range.layer_count),
            ..Default::default()
        });
        Ok(self.pools.image_views.lock().unwrap().insert(view).cast())
    }
    fn destroy_image_view(&self, h: ImageViewHandle) {
        self.pools.image_views.lock().unwrap().remove(h.cast());
    }

    // ---------- samplers ----------
    fn create_sampler(&self, desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError> {
        let s = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: desc.label,
            address_mode_u: conv::map_address(desc.address_mode[0]),
            address_mode_v: conv::map_address(desc.address_mode[1]),
            address_mode_w: conv::map_address(desc.address_mode[2]),
            mag_filter: conv::map_filter(desc.mag_filter),
            min_filter: conv::map_filter(desc.min_filter),
            mipmap_filter: match desc.mip_filter {
                hal::FilterMode::Nearest => wgpu::MipmapFilterMode::Nearest,
                hal::FilterMode::Linear => wgpu::MipmapFilterMode::Linear,
            },
            lod_min_clamp: desc.lod_min,
            lod_max_clamp: desc.lod_max,
            compare: desc.compare.map(conv::map_compare),
            anisotropy_clamp: desc.anisotropy as u16,
            ..Default::default()
        });
        Ok(self.pools.samplers.lock().unwrap().insert(s).cast())
    }
    fn destroy_sampler(&self, h: SamplerHandle) {
        self.pools.samplers.lock().unwrap().remove(h.cast());
    }

    // ---------- shader modules ----------
    fn create_shader_module(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        // SAFETY: we pass trusted SPIR-V from the engine's own shader crate
        let sm = unsafe {
            self.device.create_shader_module_trusted(
                wgpu::ShaderModuleDescriptor {
                    label: desc.label,
                    source: wgpu::ShaderSource::SpirV(std::borrow::Cow::Borrowed(desc.spirv)),
                },
                wgpu::ShaderRuntimeChecks::unchecked(),
            )
        };
        Ok(self.pools.shader_modules.lock().unwrap().insert(sm).cast())
    }
    fn destroy_shader_module(&self, h: ShaderModuleHandle) {
        self.pools.shader_modules.lock().unwrap().remove(h.cast());
    }

    // ---------- bind group layouts ----------
    fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        let entries: Vec<wgpu::BindGroupLayoutEntry> = desc
            .entries
            .iter()
            .map(|e| wgpu::BindGroupLayoutEntry {
                binding: e.binding,
                visibility: conv::map_shader_stages(e.visibility),
                ty: conv::map_binding_kind(e.kind),
                count: if e.count > 1 {
                    std::num::NonZero::new(e.count)
                } else {
                    None
                },
            })
            .collect();
        let layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: desc.label,
                entries: &entries,
            });
        Ok(self
            .pools
            .bind_group_layouts
            .lock()
            .unwrap()
            .insert(layout)
            .cast())
    }
    fn destroy_bind_group_layout(&self, h: BindGroupLayoutHandle) {
        self.pools
            .bind_group_layouts
            .lock()
            .unwrap()
            .remove(h.cast());
    }

    // ---------- bind groups ----------
    fn create_bind_group(&self, desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError> {
        let layout = {
            let layouts = self.pools.bind_group_layouts.lock().unwrap();
            let l = layouts
                .get(desc.layout.cast())
                .ok_or_else(|| Self::unsupported("stale layout"))?;
            l.clone()
        };

        // Collect cloned wgpu resources so MutexGuards can be dropped.
        let mut resolved: Vec<(u32, wgpu::Buffer, u64, Option<std::num::NonZeroU64>)> = Vec::new();
        let mut tex_views: Vec<(u32, wgpu::TextureView)> = Vec::new();
        let mut samplers: Vec<(u32, wgpu::Sampler)> = Vec::new();

        for e in desc.entries {
            match e.resource {
                hal::BindingResource::Buffer {
                    buffer,
                    offset,
                    size,
                } => {
                    let bufs = self.pools.buffers.lock().unwrap();
                    let buf = bufs.get(buffer.cast()).expect("stale buffer").clone();
                    let sz = (size != hal::BindingResource::WHOLE_BUFFER)
                        .then(|| std::num::NonZero::new(size).unwrap());
                    resolved.push((e.binding, buf, offset, sz));
                }
                hal::BindingResource::ImageView(view) => {
                    let views = self.pools.image_views.lock().unwrap();
                    tex_views.push((
                        e.binding,
                        views.get(view.cast()).expect("stale view").clone(),
                    ));
                }
                hal::BindingResource::Sampler(sampler) => {
                    let ss = self.pools.samplers.lock().unwrap();
                    samplers.push((
                        e.binding,
                        ss.get(sampler.cast()).expect("stale sampler").clone(),
                    ));
                }
            }
        }

        let entries: Vec<wgpu::BindGroupEntry<'_>> = resolved
            .iter()
            .map(|(b, buf, off, sz)| wgpu::BindGroupEntry {
                binding: *b,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: buf,
                    offset: *off,
                    size: *sz,
                }),
            })
            .chain(tex_views.iter().map(|(b, v)| wgpu::BindGroupEntry {
                binding: *b,
                resource: wgpu::BindingResource::TextureView(v),
            }))
            .chain(samplers.iter().map(|(b, s)| wgpu::BindGroupEntry {
                binding: *b,
                resource: wgpu::BindingResource::Sampler(s),
            }))
            .collect();

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: desc.label,
            layout: &layout,
            entries: &entries,
        });
        Ok(self.pools.bind_groups.lock().unwrap().insert(bg).cast())
    }
    fn update_bind_group(
        &self,
        _g: BindGroupHandle,
        _e: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        Err(Self::unsupported("update_bind_group"))
    }
    fn destroy_bind_group(&self, h: BindGroupHandle) {
        self.pools.bind_groups.lock().unwrap().remove(h.cast());
    }

    // ---------- pipeline layouts ----------
    fn create_pipeline_layout(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        let layouts = self.pools.bind_group_layouts.lock().unwrap();
        let bgls: Vec<Option<&wgpu::BindGroupLayout>> = desc
            .bind_group_layouts
            .iter()
            .map(|h| {
                layouts
                    .get(h.cast())
                    .ok_or_else(|| Self::unsupported("stale bgl"))
                    .map(Some)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let pl = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: desc.label,
                bind_group_layouts: &bgls,
                immediate_size: 0,
            });
        drop(layouts);
        Ok(self
            .pools
            .pipeline_layouts
            .lock()
            .unwrap()
            .insert(pl)
            .cast())
    }
    fn destroy_pipeline_layout(&self, h: PipelineLayoutHandle) {
        self.pools.pipeline_layouts.lock().unwrap().remove(h.cast());
    }

    // ---------- graphics pipelines ----------
    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let layout = {
            let pls = self.pools.pipeline_layouts.lock().unwrap();
            pls.get(desc.layout.cast())
                .ok_or_else(|| Self::unsupported("stale pipeline layout"))?
                .clone()
        };
        let (vs, fs_entry) = {
            let sms = self.pools.shader_modules.lock().unwrap();
            let vs = sms
                .get(desc.vertex.module.cast())
                .ok_or_else(|| Self::unsupported("stale shader"))?
                .clone();
            let fs = desc
                .fragment
                .map(|f| {
                    sms.get(f.module.cast())
                        .ok_or_else(|| Self::unsupported("stale shader"))
                        .map(|m| (m.clone(), f.entry_point))
                })
                .transpose()?;
            (vs, fs)
        };

        let targets: Vec<Option<wgpu::ColorTargetState>> = desc
            .color_targets
            .iter()
            .map(|ct| {
                let blend = ct.blend.map(|b| wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: conv::map_blend_factor(b.color_src),
                        dst_factor: conv::map_blend_factor(b.color_dst),
                        operation: conv::map_blend_op(b.color_op),
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: conv::map_blend_factor(b.alpha_src),
                        dst_factor: conv::map_blend_factor(b.alpha_dst),
                        operation: conv::map_blend_op(b.alpha_op),
                    },
                });
                Some(wgpu::ColorTargetState {
                    format: conv::map_format(ct.format),
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })
            })
            .collect();

        let ds = desc.depth_stencil.map(|ds| wgpu::DepthStencilState {
            format: conv::map_format(ds.format),
            depth_write_enabled: Some(ds.depth_write),
            depth_compare: Some(conv::map_compare(ds.depth_compare)),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: ds.bias.constant as i32,
                slope_scale: ds.bias.slope_scale,
                clamp: ds.bias.clamp,
            },
        });

        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: desc.label,
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &vs,
                    entry_point: Some(desc.vertex.entry_point),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    topology: conv::map_topology(desc.primitive.topology),
                    strip_index_format: None,
                    front_face: conv::map_front_face(desc.primitive.front_face),
                    cull_mode: conv::map_cull_mode(desc.primitive.cull_mode),
                    polygon_mode: conv::map_polygon_mode(desc.primitive.polygon_mode),
                    unclipped_depth: desc.primitive.depth_clamp,
                    conservative: false,
                },
                depth_stencil: ds,
                multisample: wgpu::MultisampleState {
                    count: desc.multisample.samples,
                    mask: desc.multisample.mask as u64,
                    alpha_to_coverage_enabled: desc.multisample.alpha_to_coverage,
                },
                fragment: fs_entry.as_ref().map(|(m, e)| wgpu::FragmentState {
                    module: m,
                    entry_point: Some(e),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                multiview_mask: None,
                cache: None,
            });
        Ok(self
            .pools
            .graphics_pipelines
            .lock()
            .unwrap()
            .insert(pipeline)
            .cast())
    }
    fn destroy_graphics_pipeline(&self, h: GraphicsPipelineHandle) {
        self.pools
            .graphics_pipelines
            .lock()
            .unwrap()
            .remove(h.cast());
    }

    // ---------- compute pipelines ----------
    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        let layout = {
            let pls = self.pools.pipeline_layouts.lock().unwrap();
            pls.get(desc.layout.cast())
                .ok_or_else(|| Self::unsupported("stale pipeline layout"))?
                .clone()
        };
        let cs = {
            let sms = self.pools.shader_modules.lock().unwrap();
            sms.get(desc.compute.module.cast())
                .ok_or_else(|| Self::unsupported("stale shader"))?
                .clone()
        };
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: desc.label,
                layout: Some(&layout),
                module: &cs,
                entry_point: Some(desc.compute.entry_point),
                compilation_options: Default::default(),
                cache: None,
            });
        Ok(self
            .pools
            .compute_pipelines
            .lock()
            .unwrap()
            .insert(pipeline)
            .cast())
    }
    fn destroy_compute_pipeline(&self, h: ComputePipelineHandle) {
        self.pools
            .compute_pipelines
            .lock()
            .unwrap()
            .remove(h.cast());
    }

    // ---------- queries ----------
    fn create_query_set(&self, _d: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        Err(Self::unsupported("query_set"))
    }
    fn destroy_query_set(&self, _s: QuerySetHandle) {}
    fn query_results(&self, _s: QuerySetHandle, _f: u32, _out: &mut [u64]) -> Result<(), HalError> {
        Err(Self::unsupported("query_results"))
    }

    // ---------- sync ----------
    fn create_semaphore(&self, desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        let v = match desc.kind {
            hal::SemaphoreKind::Binary => 0,
            hal::SemaphoreKind::Timeline { initial_value } => initial_value,
        };
        Ok(self
            .pools
            .semaphores
            .lock()
            .unwrap()
            .insert(crate::resources::SemaphoreSlot { value: v })
            .cast())
    }
    fn destroy_semaphore(&self, h: SemaphoreHandle) {
        self.pools.semaphores.lock().unwrap().remove(h.cast());
    }
    fn semaphore_value(&self, h: SemaphoreHandle) -> Result<u64, HalError> {
        self.pools
            .semaphores
            .lock()
            .unwrap()
            .get(h.cast())
            .map(|s| s.value)
            .ok_or_else(|| Self::unsupported("stale semaphore"))
    }
    fn wait_semaphores(&self, _w: &[hal::SemaphoreWait], _t: u64) -> Result<bool, HalError> {
        Ok(true)
    }
    fn wait_idle(&self) -> Result<(), HalError> {
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        Ok(())
    }

    // ---------- commands ----------
    fn create_command_encoder(
        &self,
        desc: &CommandEncoderDesc<'_>,
    ) -> Box<dyn hal::CommandEncoder> {
        let enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: desc.label });
        Box::new(crate::command::WgpuCommandEncoder::new(enc))
    }
    fn destroy_command_buffer(&self, _b: CommandBufferHandle) {}

    // ---------- submit ----------
    fn submit(&self, _queue: QueueHandle, submit: &SubmitInfo<'_>) -> Result<(), HalError> {
        let mut cmds = self.pools.command_buffers.lock().unwrap();
        let wgpu_cmds: Vec<wgpu::CommandBuffer> = submit
            .command_buffers
            .iter()
            .filter_map(|h| cmds.remove(h.cast()).and_then(|s| s.buffer))
            .collect();
        self.queue.submit(wgpu_cmds);
        Ok(())
    }

    // ---------- swapchain (stubs) ----------
    fn create_swapchain(&self, _d: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        Err(SurfaceError::Lost)
    }
    fn reconfigure_swapchain(
        &self,
        _s: SwapchainHandle,
        _d: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        Err(SurfaceError::Lost)
    }
    fn destroy_swapchain(&self, _s: SwapchainHandle) {}
    fn acquire_next_frame(&self, _s: SwapchainHandle) -> Result<AcquiredFrame, SurfaceError> {
        Err(SurfaceError::OutOfDate)
    }
    fn present(&self, _q: QueueHandle, _p: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        Err(SurfaceError::OutOfDate)
    }
}
