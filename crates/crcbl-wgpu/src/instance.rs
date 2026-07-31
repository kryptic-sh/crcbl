//! The wgpu `Instance` implementation — adapter enumeration, surfaces.

use crate::cell::{Lock, Shared};

use crcbl_core::Pool;
use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, CompositeAlpha, DeviceCaps, DeviceDesc, DeviceType,
    Features, HalError, Instance, Limits, PresentMode, SurfaceCaps, SurfaceHandle, SurfaceTarget,
};

use crate::device::WgpuDevice;
use crate::resources::SurfaceSlot;

#[derive(Debug)]
pub struct WgpuInstance {
    instance: wgpu::Instance,
    adapters: Vec<(AdapterInfo, wgpu::Adapter)>,
    surfaces: Shared<Lock<Pool<SurfaceSlot>>>,
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
                            backend: BackendKind::Wgpu,
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

        Some(Self {
            instance,
            adapters,
            surfaces: Shared::new(Lock::new(Pool::new())),
        })
    }

    /// The surface pool shared with devices created by this instance.
    pub(crate) fn surface_pool(&self) -> Shared<Lock<Pool<SurfaceSlot>>> {
        self.surfaces.clone()
    }
}

impl Instance for WgpuInstance {
    fn backend(&self) -> BackendKind {
        BackendKind::Wgpu
    }

    fn adapters(&self) -> Vec<AdapterInfo> {
        self.adapters.iter().map(|(info, _)| info.clone()).collect()
    }

    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        let surfaces = self.surfaces.lock().unwrap();
        let slot = surfaces
            .get(surface.cast())
            .ok_or(HalError::InvalidHandle {
                kind: "surface",
                bits: surface.to_bits(),
            })?;

        let (_, wgpu_adapter) = self
            .adapters
            .iter()
            .find(|(info, _)| info.id == adapter)
            .ok_or(HalError::NoSuchAdapter(adapter.0))?;

        let caps = slot.surface.get_capabilities(wgpu_adapter);

        let formats: Vec<_> = caps
            .formats
            .iter()
            .map(|f| crate::conv::unmap_format(*f))
            .collect();

        let present_modes: Vec<_> = caps
            .present_modes
            .iter()
            .filter_map(|m| match m {
                wgpu::PresentMode::Fifo => Some(PresentMode::Fifo),
                wgpu::PresentMode::FifoRelaxed => Some(PresentMode::FifoRelaxed),
                wgpu::PresentMode::Mailbox => Some(PresentMode::Mailbox),
                wgpu::PresentMode::Immediate => Some(PresentMode::Immediate),
                // AutoVsync / AutoNoVsync are wgpu abstractions; HAL callers
                // pick a concrete mode from the capabilities list.
                wgpu::PresentMode::AutoVsync | wgpu::PresentMode::AutoNoVsync => None,
            })
            .collect();

        let composite_alpha: Vec<_> = caps
            .alpha_modes
            .iter()
            .map(|a| match a {
                wgpu::CompositeAlphaMode::Opaque => CompositeAlpha::Opaque,
                wgpu::CompositeAlphaMode::PreMultiplied => CompositeAlpha::PreMultiplied,
                wgpu::CompositeAlphaMode::PostMultiplied => CompositeAlpha::PostMultiplied,
                wgpu::CompositeAlphaMode::Inherit => CompositeAlpha::Inherit,
                wgpu::CompositeAlphaMode::Auto => CompositeAlpha::Opaque,
            })
            .collect();

        Ok(SurfaceCaps {
            formats,
            present_modes,
            composite_alpha,
            min_image_count: 2,
            max_image_count: 3,
            current_extent: None,
        })
    }

    unsafe fn create_surface(&self, target: &SurfaceTarget) -> Result<SurfaceHandle, HalError> {
        let (raw_window, raw_display, platform) = unsafe { map_surface_target(target)? };

        let surface = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: raw_display,
                    raw_window_handle: raw_window,
                })
        }
        .map_err(|_e| HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what: "create_surface",
        })?;

        let mut surfaces = self.surfaces.lock().unwrap();
        Ok(surfaces.insert(SurfaceSlot { surface, platform }).cast())
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        self.surfaces.lock().unwrap().remove(surface.cast());
    }

    fn create_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn crcbl_hal::Device>, HalError> {
        let (_info, adapter) = self
            .adapters
            .iter()
            .find(|(info, _)| info.id == desc.adapter)
            .ok_or(HalError::NoSuchAdapter(desc.adapter.0))?;

        WgpuDevice::new(adapter, desc, self.surface_pool())
            .map(|d| Box::new(d) as Box<dyn crcbl_hal::Device>)
    }
}

/// Map a `SurfaceTarget` to raw-window-handle types for wgpu.
///
/// Returns `(RawWindowHandle, Option<RawDisplayHandle>, platform_name)`.
unsafe fn map_surface_target(
    target: &SurfaceTarget,
) -> Result<
    (
        raw_window_handle::RawWindowHandle,
        Option<raw_window_handle::RawDisplayHandle>,
        &'static str,
    ),
    HalError,
> {
    let unsupported = |what: &'static str| {
        Err(HalError::Unsupported {
            backend: BackendKind::Wgpu,
            what,
        })
    };

    use raw_window_handle::{
        RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
        XcbDisplayHandle, XcbWindowHandle,
    };

    match *target {
        SurfaceTarget::Wayland { display, surface } => {
            let window = RawWindowHandle::Wayland(WaylandWindowHandle::new(surface));
            let display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display));
            Ok((window, Some(display), "wayland"))
        }
        SurfaceTarget::Xcb {
            connection,
            window,
            visual_id,
        } => {
            let win = core::num::NonZeroU32::new(window).ok_or(HalError::Unsupported {
                backend: BackendKind::Wgpu,
                what: "zero Xcb window ID",
            })?;
            let mut wh = XcbWindowHandle::new(win);
            wh.visual_id = core::num::NonZeroU32::new(visual_id);
            let window = RawWindowHandle::Xcb(wh);
            let display = RawDisplayHandle::Xcb(XcbDisplayHandle::new(Some(connection), 0));
            Ok((window, Some(display), "xcb"))
        }
        SurfaceTarget::Web { .. } => unsupported("Web (P5.3)"),
        SurfaceTarget::Win32 { .. } => unsupported("Win32 (P14)"),
        SurfaceTarget::AppKit { .. } => unsupported("AppKit (P14)"),
        SurfaceTarget::Offscreen => {
            // Offscreen: no real window system surface. wgpu doesn't have a
            // direct offscreen surface API, so we return an error for now.
            // The renderer can use a plain texture ring instead.
            unsupported("Offscreen (not yet wired)")
        }
    }
}
