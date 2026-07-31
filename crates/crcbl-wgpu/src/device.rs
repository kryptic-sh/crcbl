//! The wgpu `Device` implementation — P5.1 stubs.

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

#[derive(Debug)]
pub struct WgpuDevice {
    #[allow(dead_code)]
    device: wgpu::Device,
    #[allow(dead_code)]
    queue: wgpu::Queue,
    caps: DeviceCaps,
    graphics_queue: QueueHandle,
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
        })
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

    fn create_buffer(&self, _desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "buffer (P5.2)",
        })
    }
    fn destroy_buffer(&self, _buffer: BufferHandle) {}
    fn write_buffer(
        &self,
        _buffer: BufferHandle,
        _offset: u64,
        _data: &[u8],
    ) -> Result<(), HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "write_buffer (P5.2)",
        })
    }
    fn request_readback(&self, _desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "readback (P5.2)",
        })
    }
    fn poll_readback(
        &self,
        _readback: ReadbackHandle,
        _out: &mut [u8],
    ) -> Result<ReadbackState, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "poll_readback (P5.2)",
        })
    }
    fn destroy_readback(&self, _readback: ReadbackHandle) {}
    fn create_image(&self, _desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "image (P5.2)",
        })
    }
    fn destroy_image(&self, _image: ImageHandle) {}
    fn create_image_view(&self, _desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "image_view (P5.2)",
        })
    }
    fn destroy_image_view(&self, _view: ImageViewHandle) {}
    fn create_sampler(&self, _desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "sampler (P5.2)",
        })
    }
    fn destroy_sampler(&self, _sampler: SamplerHandle) {}
    fn create_shader_module(
        &self,
        _desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "shader_module (P5.3)",
        })
    }
    fn destroy_shader_module(&self, _module: ShaderModuleHandle) {}
    fn create_bind_group_layout(
        &self,
        _desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "bind_group_layout (P5.2)",
        })
    }
    fn destroy_bind_group_layout(&self, _layout: BindGroupLayoutHandle) {}
    fn create_bind_group(&self, _desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "bind_group (P5.2)",
        })
    }
    fn update_bind_group(
        &self,
        _group: BindGroupHandle,
        _entries: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "update_bind_group (P5.2)",
        })
    }
    fn destroy_bind_group(&self, _group: BindGroupHandle) {}
    fn create_pipeline_layout(
        &self,
        _desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "pipeline_layout (P5.2)",
        })
    }
    fn destroy_pipeline_layout(&self, _layout: PipelineLayoutHandle) {}
    fn create_graphics_pipeline(
        &self,
        _desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "graphics_pipeline (P5.2)",
        })
    }
    fn destroy_graphics_pipeline(&self, _pipeline: GraphicsPipelineHandle) {}
    fn create_compute_pipeline(
        &self,
        _desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "compute_pipeline (P5.2)",
        })
    }
    fn destroy_compute_pipeline(&self, _pipeline: ComputePipelineHandle) {}
    fn create_query_set(&self, _desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "query_set (P5.2)",
        })
    }
    fn destroy_query_set(&self, _set: QuerySetHandle) {}
    fn query_results(
        &self,
        _set: QuerySetHandle,
        _first_query: u32,
        _out: &mut [u64],
    ) -> Result<(), HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "query_results (P5.2)",
        })
    }
    fn create_semaphore(&self, _desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "semaphore (P5.2)",
        })
    }
    fn destroy_semaphore(&self, _semaphore: SemaphoreHandle) {}
    fn semaphore_value(&self, _semaphore: SemaphoreHandle) -> Result<u64, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "semaphore_value (P5.2)",
        })
    }
    fn wait_semaphores(
        &self,
        _waits: &[hal::SemaphoreWait],
        _timeout_ns: u64,
    ) -> Result<bool, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "wait_semaphores (P5.2)",
        })
    }
    fn wait_idle(&self) -> Result<(), HalError> {
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        Ok(())
    }
    fn create_command_encoder(
        &self,
        _desc: &CommandEncoderDesc<'_>,
    ) -> Box<dyn hal::CommandEncoder> {
        panic!("command_encoder stub (P5.2)")
    }
    fn destroy_command_buffer(&self, _buffer: CommandBufferHandle) {}
    fn submit(&self, _queue: QueueHandle, _submit: &SubmitInfo<'_>) -> Result<(), HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "submit (P5.2)",
        })
    }
    fn create_swapchain(&self, _desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        Err(SurfaceError::Lost)
    }
    fn reconfigure_swapchain(
        &self,
        _swapchain: SwapchainHandle,
        _desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        Err(SurfaceError::Lost)
    }
    fn destroy_swapchain(&self, _swapchain: SwapchainHandle) {}
    fn acquire_next_frame(
        &self,
        _swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        Err(SurfaceError::OutOfDate)
    }
    fn present(&self, _queue: QueueHandle, _present: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        Err(SurfaceError::OutOfDate)
    }
}
