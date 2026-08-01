//! The wgpu `Device` implementation — P5.2 resource mappings.

use crcbl_hal::{
    self as hal, AcquiredFrame, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, CommandBufferHandle,
    CommandEncoderDesc, ComputePipelineDesc, ComputePipelineHandle, Device, DeviceCaps, DeviceDesc,
    GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc, ImageHandle, ImageViewDesc,
    ImageViewHandle, PipelineLayoutDesc, PipelineLayoutHandle, PresentInfo, QuerySetDesc,
    QuerySetHandle, QueueHandle, QueueKind, ReadbackDesc, ReadbackHandle, ReadbackState,
    SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, ShaderModuleDesc,
    ShaderModuleHandle, ShaderSources, SubmitInfo, SurfaceError, SwapchainDesc, SwapchainHandle,
};

use crate::cell::{Lock, Shared};

use crcbl_core::Pool;

use crate::conv;
use crate::resources::{
    BufferSlot, CommandBufferSlot, PendingSignal, Pools, SurfaceSlot, SwapchainSlot,
};

/// wgpu requires buffer copy offsets and sizes to be multiples of this.
const COPY_ALIGNMENT: u64 = wgpu::COPY_BUFFER_ALIGNMENT;

pub struct WgpuDevice {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    caps: DeviceCaps,
    /// What was actually enabled, so command recording can refuse what the
    /// device cannot do instead of letting wgpu panic.
    enabled: wgpu::Features,
    graphics_queue: QueueHandle,
    pools: Shared<Pools>,
}

impl std::fmt::Debug for WgpuDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuDevice").finish_non_exhaustive()
    }
}

/// The `requestDevice` future, boxed so a [`WgpuPendingDevice`] can outlive the
/// call that started it.
///
/// `Send` on native (`HalThreadSafe` demands it, and wgpu's future is
/// `WasmNotSend`), unbounded on wasm32 where the web types are `!Send`.
#[cfg(not(target_arch = "wasm32"))]
type DeviceFuture = std::pin::Pin<
    Box<dyn Future<Output = Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError>> + Send>,
>;

/// See the native [`DeviceFuture`].
#[cfg(target_arch = "wasm32")]
type DeviceFuture = std::pin::Pin<
    Box<dyn Future<Output = Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError>>>,
>;

/// A wgpu device request in flight: the `requestDevice` future plus everything
/// needed to build the [`WgpuDevice`] once it resolves.
///
/// The whole reason the seam is poll-shaped. On the web `requestDevice` returns
/// a `Promise` that resolves on a later turn of the event loop, and the main
/// thread — where the rAF loop lives — must not block on it. Each
/// [`poll`](crcbl_hal::PendingDevice::poll) polls the future once with a no-op
/// waker: the promise is driven by the browser's own event loop, so the waker
/// has nothing to do and the next poll simply observes the result. On native the
/// future is `core::future::ready`, so the first poll completes.
pub(crate) struct WgpuPendingDevice {
    /// `None` once the device has been handed over, so a second poll is the
    /// caller bug it is rather than a second device.
    future: Lock<Option<DeviceFuture>>,
    surfaces: Shared<Lock<Pool<SurfaceSlot>>>,
}

impl std::fmt::Debug for WgpuPendingDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuPendingDevice").finish_non_exhaustive()
    }
}

impl crcbl_hal::PendingDevice for WgpuPendingDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Wgpu
    }

    fn poll(&mut self) -> Result<crcbl_hal::DeviceRequestState, HalError> {
        let mut slot = self.future.lock().unwrap();
        let Some(future) = slot.as_mut() else {
            return Err(HalError::InvalidDescriptor(
                "this device request already produced its device".to_string(),
            ));
        };
        // A no-op waker is correct here and not a shortcut: nothing in this
        // engine parks a thread on this future. Native completes on the first
        // poll; on the web the `Promise`'s `then` callback runs on the browser's
        // microtask queue whether or not a waker was registered, so the poll
        // after it lands sees the result. The caller's rAF loop is the executor.
        let waker = core::task::Waker::noop();
        let mut cx = core::task::Context::from_waker(waker);
        match future.as_mut().poll(&mut cx) {
            core::task::Poll::Pending => Ok(crcbl_hal::DeviceRequestState::Pending),
            core::task::Poll::Ready(result) => {
                *slot = None;
                let (device, queue) = result.map_err(|error| {
                    HalError::DeviceLost(format!("wgpu device creation failed: {error}"))
                })?;
                drop(slot);
                let device = WgpuDevice::from_open(device, queue, self.surfaces.clone());
                Ok(crcbl_hal::DeviceRequestState::Ready(Box::new(device)))
            }
        }
    }
}

impl WgpuDevice {
    /// Starts opening a device, returning as soon as the request is in flight.
    ///
    /// Everything decidable without the driver — a missing required feature —
    /// is decided here, per the seam's contract; only the open itself is
    /// deferred.
    pub(crate) fn request(
        adapter: &wgpu::Adapter,
        desc: &DeviceDesc<'_>,
        surfaces: Shared<Lock<Pool<SurfaceSlot>>>,
    ) -> Result<WgpuPendingDevice, HalError> {
        let advertised = crate::instance::adapter_caps(adapter);
        let missing = advertised.missing(desc.required_features);
        if !missing.is_empty() {
            return Err(HalError::UnsupportedFeatures { missing });
        }

        // Required, plus whichever optional ones this adapter actually has.
        let wanted = desc
            .required_features
            .union(desc.optional_features.intersection(advertised.features));
        let required_features = crate::instance::wgpu_features_for(wanted, adapter.features());

        // Limits are the adapter's own, except that asking for a non-zero
        // immediate budget without the feature is a validation error.
        let mut required_limits = adapter.limits();
        if !required_features.contains(wgpu::Features::IMMEDIATES) {
            required_limits.max_immediate_size = 0;
        }

        // The borrow of `adapter` and of `desc.label` ends with this call:
        // wgpu's future owns everything it needs, which is what lets the
        // returned `PendingDevice` be `'static`.
        let future = adapter.request_device(&wgpu::DeviceDescriptor {
            label: desc.label,
            required_features,
            required_limits,
            ..Default::default()
        });

        Ok(WgpuPendingDevice {
            future: Lock::new(Some(Box::pin(future))),
            surfaces,
        })
    }

    /// Wraps an opened wgpu device and its queue.
    fn from_open(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surfaces: Shared<Lock<Pool<SurfaceSlot>>>,
    ) -> Self {
        // What the device *has*, not what was asked for: an optional feature
        // the caller did not request is simply not in `enabled`, and one that
        // needs no enabling at all (compute, debug markers) is always there.
        // The seam's contract is "absent ones are simply not enabled; check
        // `Device::caps` afterwards to find out".
        let enabled = device.features();
        let caps = DeviceCaps {
            features: crate::instance::hal_features_for(enabled),
            limits: crate::instance::hal_limits_for(&device.limits(), enabled),
        };

        Self {
            device,
            queue,
            caps,
            enabled,
            // A handle's high 32 bits are its generation and zero is the "never
            // issued" sentinel, so the synthesised queue handle needs a
            // non-zero one — `from_bits(1)` is index 1, generation 0, which is
            // no handle at all and panicked on the first real device this
            // backend ever opened. Index 0, generation 1, exactly as
            // `crcbl_hal::null` synthesises its own queue handles.
            graphics_queue: QueueHandle::from_bits(1 << 32).expect("generation 1 is non-zero"),
            pools: Shared::new(Pools::new(surfaces)),
        }
    }

    fn unsupported(what: &'static str) -> HalError {
        HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what,
        }
    }

    /// Promotes every recorded signal whose submission has finished.
    ///
    /// wgpu has no semaphores, so a "timeline value" here is the completion of
    /// the submission that was asked to signal it. Probed with a zero-timeout
    /// wait, which is wgpu's non-blocking way to ask about one submission.
    fn resolve_semaphores(&self) {
        let mut semaphores = self.pools.semaphores.lock().unwrap();
        for (_, slot) in semaphores.iter_mut() {
            slot.pending.retain(|signal| {
                let done = self
                    .device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(signal.submission.clone()),
                        timeout: Some(core::time::Duration::ZERO),
                    })
                    .is_ok();
                if done {
                    slot.value = slot.value.max(signal.value);
                }
                !done
            });
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
    fn queue(&self, kind: QueueKind) -> Option<QueueHandle> {
        // One queue: wgpu exposes exactly one, and the seam says a caller with
        // no async-compute or transfer feature falls back to `Graphics`.
        match kind {
            QueueKind::Graphics => Some(self.graphics_queue),
            QueueKind::Compute | QueueKind::Transfer => None,
        }
    }

    // ---------- buffers ----------
    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError> {
        if desc.size == 0 {
            return Err(HalError::InvalidDescriptor(
                "a buffer must have a non-zero size".to_string(),
            ));
        }
        // wgpu writes and copies in four-byte units. Rounding the *allocation*
        // up keeps a 3-byte texture upload legal without making every caller
        // pad; the seam-visible size stays what was asked for, so bounds checks
        // do not quietly grow with it.
        let allocated = desc.size.next_multiple_of(COPY_ALIGNMENT);
        let b = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: desc.label,
            size: allocated,
            usage: conv::map_buffer_usage(desc.usage, desc.memory),
            // Never: a buffer left mapped at creation stays CPU-owned until it
            // is unmapped, and the first GPU use of one errors inside wgpu.
            // `write_buffer` is `Queue::write_buffer`, which needs no mapping.
            mapped_at_creation: false,
        });
        Ok(self
            .pools
            .buffers
            .lock()
            .unwrap()
            .insert(BufferSlot {
                buffer: b,
                size: desc.size,
                memory: desc.memory,
            })
            .cast())
    }
    fn destroy_buffer(&self, h: BufferHandle) {
        self.pools.buffers.lock().unwrap().remove(h.cast());
    }
    fn write_buffer(&self, h: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError> {
        let pools = self.pools.buffers.lock().unwrap();
        let slot = pools
            .get(h.cast())
            .ok_or_else(|| HalError::invalid_handle("buffer", h))?;
        if !slot.memory.is_mappable() {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer needs a host-visible buffer; this one is {:?}",
                slot.memory
            )));
        }
        let end = offset.checked_add(data.len() as u64).ok_or_else(|| {
            HalError::InvalidDescriptor("write_buffer range overflows".to_string())
        })?;
        if end > slot.size {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer range {offset}..{end} exceeds the buffer's {} bytes",
                slot.size
            )));
        }
        if !offset.is_multiple_of(COPY_ALIGNMENT) {
            return Err(HalError::InvalidDescriptor(format!(
                "write_buffer offset {offset} is not a multiple of {COPY_ALIGNMENT}, which wgpu \
                 requires"
            )));
        }
        if data.is_empty() {
            return Ok(());
        }
        // The tail is padded rather than refused: `create_buffer` already
        // rounded the allocation up, so the extra bytes land in padding the
        // caller cannot address.
        if data.len().is_multiple_of(COPY_ALIGNMENT as usize) {
            self.queue.write_buffer(&slot.buffer, offset, data);
        } else {
            let mut padded = data.to_vec();
            padded.resize(data.len().next_multiple_of(COPY_ALIGNMENT as usize), 0);
            self.queue.write_buffer(&slot.buffer, offset, &padded);
        }
        Ok(())
    }
    fn request_readback(&self, _d: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError> {
        Err(Self::unsupported(
            "readback: wgpu's map_async completes on a later turn of the event loop and this \
             backend has no polling ring for it yet",
        ))
    }
    fn poll_readback(&self, r: ReadbackHandle, _out: &mut [u8]) -> Result<ReadbackState, HalError> {
        // No readback can have been created, so any handle is stale.
        Err(HalError::invalid_handle("readback", r))
    }
    fn destroy_readback(&self, _r: ReadbackHandle) {}

    // ---------- images ----------
    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError> {
        let usage = conv::map_image_usage(desc.usage);
        if usage.is_empty() {
            return Err(HalError::InvalidDescriptor(format!(
                "image usage {:?} maps to no wgpu texture usage at all",
                desc.usage
            )));
        }
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
            usage,
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
            .ok_or_else(|| HalError::invalid_handle("image", desc.image))?;
        // `ImageSubresourceRange::ALL` means "every remaining level/layer",
        // which is exactly what wgpu spells `None`. Passing `u32::MAX` through
        // is an out-of-range count, and it is what every graph-owned view was
        // built with.
        let mip_level_count = remaining(desc.range.mip_count);
        let array_layer_count = remaining(desc.range.layer_count);
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
            mip_level_count,
            base_array_layer: desc.range.base_layer,
            array_layer_count,
            ..Default::default()
        });
        drop(images);
        Ok(self.pools.image_views.lock().unwrap().insert(view).cast())
    }
    fn destroy_image_view(&self, h: ImageViewHandle) {
        self.pools.image_views.lock().unwrap().remove(h.cast());
    }

    // ---------- samplers ----------
    fn create_sampler(&self, desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError> {
        let anisotropy = desc
            .anisotropy
            .clamp(1.0, self.caps.limits.max_sampler_anisotropy);
        let s = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: desc.label,
            address_mode_u: conv::map_address(desc.address_mode[0]),
            address_mode_v: conv::map_address(desc.address_mode[1]),
            address_mode_w: conv::map_address(desc.address_mode[2]),
            mag_filter: conv::map_filter(desc.mag_filter),
            min_filter: conv::map_filter(desc.min_filter),
            mipmap_filter: conv::map_mip_filter(desc.mip_filter),
            lod_min_clamp: desc.lod_min,
            lod_max_clamp: desc.lod_max,
            compare: desc.compare.map(conv::map_compare),
            anisotropy_clamp: anisotropy as u16,
            ..Default::default()
        });
        Ok(self.pools.samplers.lock().unwrap().insert(s).cast())
    }
    fn destroy_sampler(&self, h: SamplerHandle) {
        self.pools.samplers.lock().unwrap().remove(h.cast());
    }

    // ---------- shader modules ----------
    /// WGSL first, SPIR-V only if that is all there is.
    ///
    /// wgpu reaches both formats through `naga`, but not equally. Its WGSL
    /// frontend is the one WebGPU itself is specified in and the one wgpu's own
    /// tests exercise; its SPIR-V frontend implements a subset, and the subset
    /// excludes `DrawParameters` — which every artifact `crcbl-shaders` emits
    /// declares, because Slang lowers `SV_VertexID` to
    /// `gl_VertexIndex - gl_BaseVertex`. Before this preference existed, this
    /// function had never successfully created a module on any target.
    ///
    /// So the SPIR-V path is kept, and kept honest: a caller who supplies only
    /// SPIR-V gets it handed to naga, and gets naga's error — an
    /// `UnsupportedCapability` panic through wgpu's error handler for anything
    /// the frontend does not implement. It is a fallback for shaders that
    /// happen to stay inside naga's subset, not a supported path for the
    /// engine's own.
    fn create_shader_module(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        let sm = if let Some(wgsl) = desc.wgsl {
            // Not `create_shader_module_trusted`: WGSL arrives as source that
            // naga will parse and bounds-check, and skipping those checks buys
            // nothing here — the artifact is compiled once at start-up.
            self.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: desc.label,
                    source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(wgsl)),
                })
        } else if desc.spirv.is_empty() {
            return Err(desc.unusable(ShaderSources::WGSL | ShaderSources::SPIRV));
        } else {
            // SAFETY: `ShaderRuntimeChecks::unchecked` removes the injected
            // bounds checks, so the module must not index a binding out of
            // range. `desc.spirv` is a build-time artifact of this workspace,
            // compiled from a Slang source in `crcbl-shaders` and hash-pinned
            // by its manifest — it is not attacker-supplied and not loaded from
            // disk at run time.
            unsafe {
                self.device.create_shader_module_trusted(
                    wgpu::ShaderModuleDescriptor {
                        label: desc.label,
                        source: wgpu::ShaderSource::SpirV(std::borrow::Cow::Borrowed(desc.spirv)),
                    },
                    wgpu::ShaderRuntimeChecks::unchecked(),
                )
            }
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
            .map(|e| {
                Ok(wgpu::BindGroupLayoutEntry {
                    binding: e.binding,
                    visibility: conv::map_shader_stages(e.visibility),
                    ty: conv::map_binding_kind(e.kind)?,
                    count: if e.count > 1 {
                        std::num::NonZero::new(e.count)
                    } else {
                        None
                    },
                })
            })
            .collect::<Result<Vec<_>, HalError>>()?;
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
                .ok_or_else(|| HalError::invalid_handle("bind group layout", desc.layout))?;
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
                    let buf = bufs
                        .get(buffer.cast())
                        .ok_or_else(|| HalError::invalid_handle("buffer", buffer))?
                        .buffer
                        .clone();
                    let sz = if size == hal::BindingResource::WHOLE_BUFFER {
                        None
                    } else {
                        Some(std::num::NonZero::new(size).ok_or_else(|| {
                            HalError::InvalidDescriptor(format!(
                                "binding {} names a zero-sized range of a buffer; use \
                                 BindingResource::whole_buffer for the whole thing",
                                e.binding
                            ))
                        })?)
                    };
                    resolved.push((e.binding, buf, offset, sz));
                }
                hal::BindingResource::ImageView(view) => {
                    let views = self.pools.image_views.lock().unwrap();
                    tex_views.push((
                        e.binding,
                        views
                            .get(view.cast())
                            .ok_or_else(|| HalError::invalid_handle("image view", view))?
                            .clone(),
                    ));
                }
                hal::BindingResource::Sampler(sampler) => {
                    let ss = self.pools.samplers.lock().unwrap();
                    samplers.push((
                        e.binding,
                        ss.get(sampler.cast())
                            .ok_or_else(|| HalError::invalid_handle("sampler", sampler))?
                            .clone(),
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
        Err(Self::unsupported(
            "update_bind_group: WebGPU bind groups are immutable once created and wgpu exposes no \
             update-after-bind path; rebuild the group instead",
        ))
    }
    fn destroy_bind_group(&self, h: BindGroupHandle) {
        self.pools.bind_groups.lock().unwrap().remove(h.cast());
    }

    // ---------- pipeline layouts ----------
    fn create_pipeline_layout(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        // The seam requires this to fail loudly rather than have
        // `push_constants` drop the writes later.
        let immediate_size = match desc.push_constants {
            None => 0,
            Some(range) => {
                if !self.enabled.contains(wgpu::Features::IMMEDIATES) {
                    return Err(Self::unsupported(
                        "push constants: this device did not enable wgpu's IMMEDIATES feature",
                    ));
                }
                range.offset + range.size
            }
        };
        let layouts = self.pools.bind_group_layouts.lock().unwrap();
        let bgls: Vec<Option<&wgpu::BindGroupLayout>> = desc
            .bind_group_layouts
            .iter()
            .map(|h| {
                layouts
                    .get(h.cast())
                    .ok_or_else(|| HalError::invalid_handle("bind group layout", *h))
                    .map(Some)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let pl = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: desc.label,
                bind_group_layouts: &bgls,
                immediate_size,
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
                .ok_or_else(|| HalError::invalid_handle("pipeline layout", desc.layout))?
                .clone()
        };
        let (vs, fs_entry) = {
            let sms = self.pools.shader_modules.lock().unwrap();
            let vs = sms
                .get(desc.vertex.module.cast())
                .ok_or_else(|| HalError::invalid_handle("shader module", desc.vertex.module))?
                .clone();
            let fs = desc
                .fragment
                .map(|f| {
                    sms.get(f.module.cast())
                        .ok_or_else(|| HalError::invalid_handle("shader module", f.module))
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
                    write_mask: conv::map_color_writes(ct.write_mask),
                })
            })
            .collect();

        let ds = desc
            .depth_stencil
            .map(|ds| -> Result<wgpu::DepthStencilState, HalError> {
                // wgpu's constant bias is an integer count of depth units; the
                // seam's is a float. Rounding is fine, but rounding a non-zero
                // bias to zero silently disables it, which is a shadow-acne
                // debugging session rather than an error message.
                let constant = ds.bias.constant.round();
                if ds.bias.constant != 0.0 && constant == 0.0 {
                    return Err(HalError::InvalidDescriptor(format!(
                        "depth bias constant {} rounds to zero: wgpu's DepthBiasState.constant is \
                         an integer, so a sub-unit constant bias cannot be expressed",
                        ds.bias.constant
                    )));
                }
                Ok(wgpu::DepthStencilState {
                    format: conv::map_format(ds.format),
                    depth_write_enabled: Some(ds.depth_write),
                    depth_compare: Some(conv::map_compare(ds.depth_compare)),
                    stencil: ds
                        .stencil
                        .map_or_else(wgpu::StencilState::default, |stencil| wgpu::StencilState {
                            front: conv::map_stencil_face(stencil.front),
                            back: conv::map_stencil_face(stencil.back),
                            read_mask: stencil.read_mask,
                            write_mask: stencil.write_mask,
                        }),
                    bias: wgpu::DepthBiasState {
                        constant: constant as i32,
                        slope_scale: ds.bias.slope_scale,
                        clamp: ds.bias.clamp,
                    },
                })
            })
            .transpose()?;

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
                    mask: u64::from(desc.multisample.mask),
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
                .ok_or_else(|| HalError::invalid_handle("pipeline layout", desc.layout))?
                .clone()
        };
        let cs = {
            let sms = self.pools.shader_modules.lock().unwrap();
            sms.get(desc.compute.module.cast())
                .ok_or_else(|| HalError::invalid_handle("shader module", desc.compute.module))?
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
        Err(Self::unsupported(
            "query sets: this backend reports neither TIMESTAMP_QUERY nor the statistics features",
        ))
    }
    fn destroy_query_set(&self, _s: QuerySetHandle) {}
    fn query_results(&self, s: QuerySetHandle, _f: u32, _out: &mut [u64]) -> Result<(), HalError> {
        Err(HalError::invalid_handle("query set", s))
    }

    // ---------- sync ----------
    fn create_semaphore(&self, desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError> {
        let value = match desc.kind {
            hal::SemaphoreKind::Binary => 0,
            hal::SemaphoreKind::Timeline { initial_value } => initial_value,
        };
        Ok(self
            .pools
            .semaphores
            .lock()
            .unwrap()
            .insert(crate::resources::SemaphoreSlot {
                kind: desc.kind,
                value,
                pending: Vec::new(),
            })
            .cast())
    }
    fn destroy_semaphore(&self, h: SemaphoreHandle) {
        self.pools.semaphores.lock().unwrap().remove(h.cast());
    }
    fn semaphore_value(&self, h: SemaphoreHandle) -> Result<u64, HalError> {
        self.resolve_semaphores();
        let semaphores = self.pools.semaphores.lock().unwrap();
        let slot = semaphores
            .get(h.cast())
            .ok_or_else(|| HalError::invalid_handle("semaphore", h))?;
        match slot.kind {
            hal::SemaphoreKind::Timeline { .. } => Ok(slot.value),
            hal::SemaphoreKind::Binary => Err(Self::unsupported(
                "semaphore_value on a binary semaphore, which has no value to read",
            )),
        }
    }
    fn wait_semaphores(
        &self,
        waits: &[hal::SemaphoreWait],
        timeout_ns: u64,
    ) -> Result<bool, HalError> {
        self.resolve_semaphores();
        let deadline = core::time::Duration::from_nanos(timeout_ns);
        for wait in waits {
            // Look the submission up, then drop the guard: waiting on it can
            // take the whole timeout and `resolve_semaphores` wants the lock.
            let submission = {
                let semaphores = self.pools.semaphores.lock().unwrap();
                let slot = semaphores
                    .get(wait.semaphore.cast())
                    .ok_or_else(|| HalError::invalid_handle("semaphore", wait.semaphore))?;
                if slot.value >= wait.value {
                    continue;
                }
                match slot
                    .pending
                    .iter()
                    .filter(|signal| signal.value >= wait.value)
                    .min_by_key(|signal| signal.value)
                {
                    Some(signal) => signal.submission.clone(),
                    // Nothing submitted will ever reach this value, so the wait
                    // cannot be satisfied by waiting — it would hang forever.
                    None => {
                        return Err(Self::unsupported(
                            "a wait on a timeline value no submitted work signals: wgpu has no \
                             standalone semaphore to signal it later",
                        ));
                    }
                }
            };
            if self
                .device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(deadline),
                })
                .is_err()
            {
                return Ok(false);
            }
        }
        self.resolve_semaphores();
        Ok(true)
    }
    fn wait_idle(&self) -> Result<(), HalError> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| HalError::DeviceLost(format!("wgpu device poll failed: {e}")))?;
        self.resolve_semaphores();
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
        // Pre-allocate a handle and a slot for the finished command buffer.
        let handle = {
            let mut cmds = self.pools.command_buffers.lock().unwrap();
            cmds.insert(CommandBufferSlot {
                buffer: None,
                label: desc.label.unwrap_or("<unnamed>").to_string(),
            })
            .cast()
        };
        Box::new(crate::command::WgpuCommandEncoder::new(
            enc,
            handle,
            self.pools.clone(),
            self.enabled.contains(wgpu::Features::IMMEDIATES),
            self.enabled
                .contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT),
        ))
    }
    fn destroy_command_buffer(&self, b: CommandBufferHandle) {
        self.pools.command_buffers.lock().unwrap().remove(b.cast());
    }

    // ---------- submit ----------
    fn submit(&self, queue: QueueHandle, submit: &SubmitInfo<'_>) -> Result<(), HalError> {
        if queue != self.graphics_queue {
            return Err(HalError::invalid_handle("queue", queue));
        }
        self.resolve_semaphores();

        // Validate everything before taking anything, so a rejected submission
        // leaves the caller's command buffers exactly as it found them.
        {
            let cmds = self.pools.command_buffers.lock().unwrap();
            for h in submit.command_buffers {
                let slot = cmds
                    .get(h.cast())
                    .ok_or_else(|| HalError::invalid_handle("command buffer", *h))?;
                if slot.buffer.is_none() {
                    return Err(HalError::InvalidDescriptor(format!(
                        "command buffer `{}` was never finished, or has already been submitted",
                        slot.label
                    )));
                }
            }
        }
        {
            let semaphores = self.pools.semaphores.lock().unwrap();
            for wait in submit.waits {
                let slot = semaphores
                    .get(wait.semaphore.cast())
                    .ok_or_else(|| HalError::invalid_handle("semaphore", wait.semaphore))?;
                // wgpu executes one queue's submissions in order and inserts
                // its own hazard barriers, so a wait on work already submitted
                // is satisfied by construction. A wait on anything else has no
                // object to block on.
                let reachable = slot
                    .pending
                    .iter()
                    .map(|signal| signal.value)
                    .max()
                    .unwrap_or(slot.value)
                    .max(slot.value);
                if wait.value > reachable {
                    return Err(Self::unsupported(
                        "a submit-time wait on a timeline value nothing submitted will signal: \
                         wgpu has no queue-side wait to express it",
                    ));
                }
            }
            for signal in submit.signals {
                let slot = semaphores
                    .get(signal.semaphore.cast())
                    .ok_or_else(|| HalError::invalid_handle("semaphore", signal.semaphore))?;
                if matches!(slot.kind, hal::SemaphoreKind::Timeline { .. })
                    && signal.value <= slot.value
                {
                    return Err(HalError::InvalidDescriptor(format!(
                        "a timeline signal must exceed the semaphore's current value {}, got {}",
                        slot.value, signal.value
                    )));
                }
            }
        }

        let wgpu_cmds: Vec<wgpu::CommandBuffer> = {
            let mut cmds = self.pools.command_buffers.lock().unwrap();
            submit
                .command_buffers
                .iter()
                .filter_map(|h| cmds.get_mut(h.cast()).and_then(|s| s.buffer.take()))
                .collect()
        };
        let submission = self.queue.submit(wgpu_cmds);

        let mut semaphores = self.pools.semaphores.lock().unwrap();
        for signal in submit.signals {
            if let Some(slot) = semaphores.get_mut(signal.semaphore.cast()) {
                slot.pending.push(PendingSignal {
                    submission: submission.clone(),
                    value: signal.value,
                });
            }
        }
        Ok(())
    }

    // ---------- swapchain ----------
    fn create_swapchain(&self, desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError> {
        let surfaces = self.pools.surfaces.lock().unwrap();
        let surface_slot = surfaces
            .get(desc.surface.cast())
            .ok_or(SurfaceError::Lost)?;

        let config = swapchain_config(desc);
        surface_slot.surface.configure(&self.device, &config);

        let slot = SwapchainSlot {
            surface_handle: desc.surface,
            config: Some(config),
            acquired: None,
            frame_image: None,
            frame_view: None,
            extent: desc.extent,
            format: desc.format,
            suboptimal: false,
        };
        drop(surfaces); // release lock before locking swapchains

        let mut swapchains = self.pools.swapchains.lock().unwrap();
        Ok(swapchains.insert(slot).cast())
    }

    fn reconfigure_swapchain(
        &self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError> {
        let surfaces = self.pools.surfaces.lock().unwrap();
        let mut swapchains = self.pools.swapchains.lock().unwrap();
        let slot = swapchains
            .get_mut(swapchain.cast())
            .ok_or(SurfaceError::Lost)?;

        // Drop any pending acquired texture before reconfiguring, and release
        // the handles it put in the pools.
        slot.acquired = None;
        let (stale_image, stale_view) = (slot.frame_image.take(), slot.frame_view.take());

        let surface_slot = surfaces
            .get(slot.surface_handle.cast())
            .ok_or(SurfaceError::Lost)?;

        let config = swapchain_config(desc);
        surface_slot.surface.configure(&self.device, &config);

        slot.config = Some(config);
        slot.extent = desc.extent;
        slot.format = desc.format;
        drop(swapchains);
        drop(surfaces);
        self.release_frame_handles(stale_image, stale_view);

        Ok(())
    }

    fn destroy_swapchain(&self, swapchain: SwapchainHandle) {
        let stale = self
            .pools
            .swapchains
            .lock()
            .unwrap()
            .remove(swapchain.cast())
            .map(|mut slot| (slot.frame_image.take(), slot.frame_view.take()));
        if let Some((image, view)) = stale {
            self.release_frame_handles(image, view);
        }
    }

    fn acquire_next_frame(
        &self,
        swapchain: SwapchainHandle,
    ) -> Result<AcquiredFrame, SurfaceError> {
        let surfaces = self.pools.surfaces.lock().unwrap();
        let mut swapchains = self.pools.swapchains.lock().unwrap();
        let slot = swapchains
            .get_mut(swapchain.cast())
            .ok_or(SurfaceError::Lost)?;

        let surface_slot = surfaces
            .get(slot.surface_handle.cast())
            .ok_or(SurfaceError::Lost)?;

        let (surface_texture, suboptimal) = match surface_slot.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout => return Err(SurfaceError::Timeout),
            wgpu::CurrentSurfaceTexture::Outdated => return Err(SurfaceError::OutOfDate),
            wgpu::CurrentSurfaceTexture::Lost => return Err(SurfaceError::Lost),
            _ => return Err(SurfaceError::Lost),
        };

        // Clone the texture (Arc-backed) so we can store it separately.
        let texture = surface_texture.texture.clone();
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("swapchain_view"),
            ..Default::default()
        });

        // The previous frame's handles die here: a swapchain texture is
        // re-acquired every frame, so keeping them would leak two pool slots
        // per frame for the life of the process.
        let (stale_image, stale_view) = (slot.frame_image.take(), slot.frame_view.take());

        let image: ImageHandle = self.pools.images.lock().unwrap().insert(texture).cast();
        let view: ImageViewHandle = self.pools.image_views.lock().unwrap().insert(view).cast();

        slot.acquired = Some(surface_texture);
        slot.frame_image = Some(image);
        slot.frame_view = Some(view);
        slot.suboptimal = suboptimal;
        let extent = slot.extent;

        drop(swapchains);
        drop(surfaces);
        self.release_frame_handles(stale_image, stale_view);

        Ok(AcquiredFrame {
            image,
            view,
            extent,
            index: 0,
            acquire_semaphore: None,
            present_semaphore: None,
            suboptimal,
        })
    }

    fn present(&self, queue: QueueHandle, present: &PresentInfo<'_>) -> Result<(), SurfaceError> {
        if queue != self.graphics_queue {
            return Err(SurfaceError::Hal(HalError::invalid_handle("queue", queue)));
        }
        let (image, view) = {
            let mut swapchains = self.pools.swapchains.lock().unwrap();
            let slot = swapchains
                .get_mut(present.swapchain.cast())
                .ok_or(SurfaceError::Lost)?;

            // Dropping the SurfaceTexture auto-presents.
            slot.acquired = None;
            (slot.frame_image.take(), slot.frame_view.take())
        };
        self.release_frame_handles(image, view);
        Ok(())
    }
}

impl WgpuDevice {
    /// Drops the pool entries an acquire created. Separate so every caller
    /// releases the pool locks first.
    fn release_frame_handles(&self, image: Option<ImageHandle>, view: Option<ImageViewHandle>) {
        if let Some(view) = view {
            self.pools.image_views.lock().unwrap().remove(view.cast());
        }
        if let Some(image) = image {
            self.pools.images.lock().unwrap().remove(image.cast());
        }
    }
}

/// `ImageSubresourceRange::ALL` → wgpu's "all remaining", which is `None`.
fn remaining(count: u32) -> Option<u32> {
    (count != hal::ImageSubresourceRange::ALL).then_some(count)
}

fn swapchain_config(desc: &SwapchainDesc<'_>) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: conv::map_format(desc.format),
        color_space: wgpu::SurfaceColorSpace::Auto,
        width: desc.extent.0,
        height: desc.extent.1,
        present_mode: match desc.present_mode {
            hal::PresentMode::Fifo => wgpu::PresentMode::Fifo,
            hal::PresentMode::FifoRelaxed => wgpu::PresentMode::FifoRelaxed,
            hal::PresentMode::Mailbox => wgpu::PresentMode::Mailbox,
            hal::PresentMode::Immediate => wgpu::PresentMode::Immediate,
        },
        desired_maximum_frame_latency: 2,
        alpha_mode: match desc.composite_alpha {
            hal::CompositeAlpha::Opaque => wgpu::CompositeAlphaMode::Opaque,
            hal::CompositeAlpha::PreMultiplied => wgpu::CompositeAlphaMode::PreMultiplied,
            hal::CompositeAlpha::PostMultiplied => wgpu::CompositeAlphaMode::PostMultiplied,
            hal::CompositeAlpha::Inherit => wgpu::CompositeAlphaMode::Inherit,
        },
        view_formats: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sentinel is the value every graph-owned view is built with, and
    /// passing it through is out of range in wgpu.
    #[test]
    fn the_all_remaining_sentinel_becomes_none() {
        assert_eq!(remaining(hal::ImageSubresourceRange::ALL), None);
        assert_eq!(remaining(1), Some(1));
        assert_eq!(remaining(0), Some(0));
    }

    /// The seam's thread-safety marker is `Send + Sync` on native, and a
    /// `PendingDevice` has to satisfy it like every other trait object. The
    /// boxed `requestDevice` future is `Send` but not `Sync`, which is exactly
    /// why it lives behind [`Lock`] — if that wrapper is ever removed, this
    /// stops compiling instead of failing at the `Box<dyn PendingDevice>` coercion
    /// in `WgpuInstance::request_device`.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_pending_device_is_thread_safe_on_native() {
        fn assert_thread_safe<T: Send + Sync>() {}
        assert_thread_safe::<WgpuPendingDevice>();
    }

    /// And it is a `PendingDevice` — the coercion the instance performs, made
    /// explicit so a signature drift is a compile error here rather than an
    /// error inside `instance.rs`.
    #[test]
    fn a_pending_device_is_a_seam_pending_device() {
        fn boxes(_: fn(WgpuPendingDevice) -> Box<dyn crcbl_hal::PendingDevice>) {}
        boxes(|pending| Box::new(pending));
    }

    /// Adapterless machines run this too: with no wgpu adapter there is nothing
    /// to request, and the test says so rather than silently passing. With one,
    /// the **polled** path — never `create_device` — must produce a working
    /// device, because that is the path the browser will take.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_polled_path_opens_a_real_device_when_an_adapter_exists() {
        use crcbl_hal::Instance as _;

        let Some(instance) = crate::create_native() else {
            // No adapter in this environment; `crcbl-wgpu`'s own suite must not
            // fail for that, and CI covers the adapter case on lavapipe. Say so
            // out loud: a check that silently did not run is not a check.
            eprintln!("SKIPPED: no wgpu adapter here, so the polled open was not exercised");
            return;
        };
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "create_native returned Some");

        let mut pending = instance
            .request_device(&hal::DeviceDesc {
                label: Some("polled"),
                adapter: adapters[0].id,
                required_features: hal::Features::empty(),
                optional_features: hal::Features::empty(),
                compatible_surface: None,
            })
            .expect("an adapter that enumerated can be asked for a device");

        let mut polls = 0;
        let device = loop {
            polls += 1;
            assert!(polls < 1024, "the native future must complete promptly");
            match pending.poll().expect("polling a healthy request") {
                hal::DeviceRequestState::Pending => {}
                hal::DeviceRequestState::Ready(device) => break device,
            }
        };
        assert_eq!(device.backend(), BackendKind::Wgpu);
        assert_eq!(
            device.caps().tier(),
            hal::RendererTier::B,
            "wgpu has no buffer device address"
        );

        // The device it produced is real, not a token.
        let buffer = device
            .create_buffer(&hal::BufferDesc {
                label: Some("from a polled wgpu device"),
                size: 256,
                usage: hal::BufferUsage::STORAGE,
                memory: hal::MemoryLocation::DeviceLocal,
            })
            .expect("a polled device creates resources");
        device.destroy_buffer(buffer);

        // And the request is spent.
        assert!(pending.poll().is_err(), "the device was already taken");
    }

    /// An adapter index the instance never issued is refused by
    /// `request_device` itself, not deferred to a poll — the seam's rule that
    /// only what depends on the driver answering may be late.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn an_unknown_adapter_is_refused_before_any_poll() {
        use crcbl_hal::Instance as _;

        let Some(instance) = crate::create_native() else {
            eprintln!("SKIPPED: no wgpu adapter here, so request_device was not exercised");
            return;
        };
        let bogus = hal::AdapterId(u32::from(u16::MAX));
        match instance.request_device(&hal::DeviceDesc::for_adapter(bogus)) {
            Err(HalError::NoSuchAdapter(index)) => assert_eq!(index, bogus.0),
            Err(other) => panic!("wrong error: {other}"),
            Ok(_) => panic!("an adapter that does not exist must not be requestable"),
        }
    }
}
