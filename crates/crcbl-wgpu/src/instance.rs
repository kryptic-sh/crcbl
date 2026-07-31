//! The wgpu `Instance` implementation — adapter enumeration, device creation.

use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, DeviceCaps, DeviceDesc, DeviceType, Features, HalError,
    Instance, Limits, SurfaceCaps, SurfaceHandle, SurfaceTarget,
};

use crate::device::WgpuDevice;

#[derive(Debug)]
pub struct WgpuInstance {
    #[allow(dead_code)]
    instance: wgpu::Instance,
    adapters: Vec<(AdapterInfo, wgpu::Adapter)>,
}

impl WgpuInstance {
    pub fn new_native() -> Option<Self> {
        let instance = wgpu::Instance::default();

        let adapters: Vec<_> =
            pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
                .into_iter()
                .enumerate()
                .map(|(index, adapter)| {
                    let info = adapter.get_info();
                    let caps = DeviceCaps {
                        features: Features::empty(),
                        limits: Limits::desktop(),
                    };
                    (
                        AdapterInfo {
                            id: AdapterId(index as u32),
                            name: info.name,
                            vendor_id: info.vendor,
                            device_id: info.device,
                            device_type: DeviceType::Other,
                            driver: format!("wgpu {}", info.driver),
                            backend: BackendKind::Vulkan,
                            caps,
                        },
                        adapter,
                    )
                })
                .collect();

        if adapters.is_empty() {
            log::warn!("crcbl-wgpu: no adapters found");
            return None;
        }

        Some(Self { instance, adapters })
    }
}

impl Instance for WgpuInstance {
    fn backend(&self) -> BackendKind {
        BackendKind::Vulkan
    }

    fn adapters(&self) -> Vec<AdapterInfo> {
        self.adapters.iter().map(|(info, _)| info.clone()).collect()
    }

    fn surface_caps(
        &self,
        _surface: SurfaceHandle,
        _adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "surface_caps (P5.2)",
        })
    }

    unsafe fn create_surface(&self, _target: &SurfaceTarget) -> Result<SurfaceHandle, HalError> {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "create_surface (P5.2)",
        })
    }

    fn destroy_surface(&self, _surface: SurfaceHandle) {}

    fn create_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn crcbl_hal::Device>, HalError> {
        let (_info, adapter) = self
            .adapters
            .iter()
            .find(|(info, _)| info.id == desc.adapter)
            .ok_or(HalError::NoSuchAdapter(desc.adapter.0))?;

        WgpuDevice::new(adapter, desc).map(|d| Box::new(d) as Box<dyn crcbl_hal::Device>)
    }
}
