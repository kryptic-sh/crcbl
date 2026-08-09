//! The wgpu `Instance` implementation — adapter enumeration, surfaces.

use crate::cell::{Lock, Shared};

use crcbl_core::Pool;
use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, CompositeAlpha, DeviceCaps, DeviceDesc, DeviceType,
    Features, Format, HalError, Instance, Limits, PresentMode, SurfaceCaps, SurfaceHandle,
    SurfaceTarget,
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
    /// Enumerates adapters, blocking on the enumeration future.
    ///
    /// Not available on wasm32: the browser main thread cannot block. See the
    /// crate docs for what that means for this backend on the web.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_native() -> Option<Self> {
        pollster::block_on(Self::new_async())
    }

    /// Enumerates adapters without blocking.
    ///
    /// Available on every target, including wasm32, because
    /// `Instance::enumerate_adapters` is a future on the WebGPU backend.
    pub async fn new_async() -> Option<Self> {
        let instance = wgpu::Instance::default();

        let adapters: Vec<_> = instance
            .enumerate_adapters(wgpu::Backends::all())
            .await
            .into_iter()
            .enumerate()
            .map(|(index, adapter)| {
                let info = adapter.get_info();
                let caps = adapter_caps(&adapter);
                (
                    AdapterInfo {
                        id: AdapterId(index as u32),
                        name: info.name,
                        vendor_id: info.vendor,
                        device_id: info.device,
                        device_type: map_device_type(info.device_type),
                        driver: format!("{} ({})", info.driver, info.driver_info),
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

fn map_device_type(kind: wgpu::DeviceType) -> DeviceType {
    match kind {
        wgpu::DeviceType::Cpu => DeviceType::Cpu,
        wgpu::DeviceType::IntegratedGpu => DeviceType::Integrated,
        wgpu::DeviceType::DiscreteGpu => DeviceType::Discrete,
        wgpu::DeviceType::VirtualGpu => DeviceType::Virtual,
        wgpu::DeviceType::Other => DeviceType::Other,
    }
}

/// Everything an adapter says about itself, in the seam's vocabulary.
pub(crate) fn adapter_caps(adapter: &wgpu::Adapter) -> DeviceCaps {
    let features = adapter.features();
    DeviceCaps {
        features: hal_features_for(features),
        limits: hal_limits_for(&adapter.limits(), features),
    }
}

/// wgpu capabilities → seam features.
///
/// Only what this backend actually implements is reported. Query sets are
/// [`HalError::Unsupported`] here, so `TIMESTAMP_QUERY` and the statistics
/// flags stay absent even on an adapter that has them — a renderer that turned
/// its profiler HUD on from a flag the backend then refuses would be worse off
/// than one that never saw the flag.
pub(crate) fn hal_features_for(features: wgpu::Features) -> Features {
    let mut out = Features::empty();

    // Compute, multi-draw-indirect and debug markers are unconditional in wgpu.
    out |= Features::COMPUTE;
    out |= Features::MULTI_DRAW_INDIRECT;
    out |= Features::DEBUG_MARKERS;
    // `SamplerDescriptor::anisotropy_clamp` and `DepthBiasState::clamp` are
    // core wgpu, not optional features.
    out |= Features::SAMPLER_ANISOTROPY;
    out |= Features::DEPTH_BIAS_CLAMP;
    // `Device::submit` records signals against the submission that performs
    // them and `semaphore_value` resolves them, so the seam's timeline is real
    // even though wgpu has no semaphore object.
    out |= Features::TIMELINE_SEMAPHORE;

    // Bindless needs all three halves: unbounded arrays, non-uniform indexing,
    // and partial binding. Two of the three is not a bindless device.
    if features.contains(wgpu::Features::TEXTURE_BINDING_ARRAY)
        && features
            .contains(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
        && features.contains(wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY)
    {
        out |= Features::DESCRIPTOR_INDEXING;
    }
    if features.contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT) {
        out |= Features::DRAW_INDIRECT_COUNT;
    }
    if features.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE) {
        out |= Features::INDIRECT_FIRST_INSTANCE;
    }
    // `Features::PUSH_CONSTANTS` is deliberately never reported, even when the
    // adapter has `IMMEDIATES` — which it does on native wgpu over Vulkan.
    //
    // A capability on this seam is a promise about what this backend can be
    // *asked to do*, and the answer is bounded by the shader language it
    // ingests, not by the adapter alone. This backend prefers WGSL
    // (`create_shader_module`), and WGSL has no push constants: Slang's WGSL
    // target lowers `[[vk::push_constant]]` to a module-scope `var<uniform>`
    // with no `@group`/`@binding`, which naga rejects outright. SPIR-V is not a
    // way out either — every artifact this workspace ships declares
    // `OpCapability DrawParameters`, which naga does not implement.
    //
    // Reporting the adapter's answer here made `crcbl_render::ConstantDelivery`
    // choose the push-constant path on native wgpu and then fail to create the
    // UI shader module, which is exactly the confusion the capability exists to
    // prevent. Tier B's uniform-buffer path is the one this backend can serve,
    // so it is the one it asks for.
    let _ = wgpu::Features::IMMEDIATES;
    if features.contains(wgpu::Features::DEPTH_CLIP_CONTROL) {
        out |= Features::DEPTH_CLAMP;
    }
    if features.contains(wgpu::Features::POLYGON_MODE_LINE) {
        out |= Features::POLYGON_MODE_LINE;
    }
    if features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
        out |= Features::TEXTURE_COMPRESSION_BC;
    }

    // Deliberately never reported: `BUFFER_DEVICE_ADDRESS` (wgpu has no raw GPU
    // pointers), the query features (see above), `ASYNC_COMPUTE_QUEUE` and
    // `TRANSFER_QUEUE` (wgpu exposes one queue), and `SHADER_DEBUG_PRINTF`.
    out
}

/// The seam features a caller asked for, as the wgpu features that provide
/// them — intersected with what the adapter actually has, so an optional
/// feature the adapter lacks is simply not enabled.
pub(crate) fn wgpu_features_for(wanted: Features, available: wgpu::Features) -> wgpu::Features {
    let mut out = wgpu::Features::empty();
    if wanted.contains(Features::DESCRIPTOR_INDEXING) {
        out |= wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
            | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY;
    }
    if wanted.contains(Features::DRAW_INDIRECT_COUNT) {
        out |= wgpu::Features::MULTI_DRAW_INDIRECT_COUNT;
    }
    if wanted.contains(Features::INDIRECT_FIRST_INSTANCE) {
        out |= wgpu::Features::INDIRECT_FIRST_INSTANCE;
    }
    if wanted.contains(Features::PUSH_CONSTANTS) {
        out |= wgpu::Features::IMMEDIATES;
    }
    if wanted.contains(Features::DEPTH_CLAMP) {
        out |= wgpu::Features::DEPTH_CLIP_CONTROL;
    }
    if wanted.contains(Features::POLYGON_MODE_LINE) {
        out |= wgpu::Features::POLYGON_MODE_LINE;
    }
    if wanted.contains(Features::TEXTURE_COMPRESSION_BC) {
        out |= wgpu::Features::TEXTURE_COMPRESSION_BC;
    }
    out.intersection(available)
}

/// wgpu limits → seam limits, kept consistent with the reported features.
///
/// The two that used to disagree are the ones keyed off a feature here:
/// reporting a 128-byte push-constant budget while `PUSH_CONSTANTS` is absent
/// is a promise no call can keep.
pub(crate) fn hal_limits_for(limits: &wgpu::Limits, features: wgpu::Features) -> Limits {
    let hal_features = hal_features_for(features);
    Limits {
        max_image_2d: limits.max_texture_dimension_2d,
        max_image_3d: limits.max_texture_dimension_3d,
        max_image_array_layers: limits.max_texture_array_layers,
        max_storage_buffer_range: limits.max_storage_buffer_binding_size,
        max_uniform_buffer_range: limits.max_uniform_buffer_binding_size,
        max_bind_groups: limits.max_bind_groups,
        max_bindless_descriptors: if hal_features.contains(Features::DESCRIPTOR_INDEXING) {
            limits.max_binding_array_elements_per_shader_stage
        } else {
            0
        },
        max_push_constant_size: if hal_features.contains(Features::PUSH_CONSTANTS) {
            limits.max_immediate_size
        } else {
            0
        },
        max_color_attachments: limits.max_color_attachments,
        // WebGPU guarantees 1x and 4x and exposes nothing above: `sampleCount`
        // on a `GPUTextureDescriptor` is specified to be one of exactly those
        // two values, and wgpu has no limit to read it from.
        max_sample_count: 4,
        max_draw_indirect_count: if hal_features.contains(Features::DRAW_INDIRECT_COUNT) {
            u32::MAX
        } else {
            1
        },
        max_compute_workgroup_size: [
            limits.max_compute_workgroup_size_x,
            limits.max_compute_workgroup_size_y,
            limits.max_compute_workgroup_size_z,
        ],
        max_compute_invocations_per_workgroup: limits.max_compute_invocations_per_workgroup,
        max_compute_workgroups_per_dimension: limits.max_compute_workgroups_per_dimension,
        min_uniform_buffer_offset_alignment: u64::from(limits.min_uniform_buffer_offset_alignment),
        min_storage_buffer_offset_alignment: u64::from(limits.min_storage_buffer_offset_alignment),
        // wgpu wants a buffer↔image copy's rows padded to this, which is the
        // coarsest alignment such a copy has to satisfy.
        optimal_buffer_copy_offset_alignment: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
        // `SamplerDescriptor::anisotropy_clamp` is a `u16` wgpu clamps to 16.
        max_sampler_anisotropy: 16.0,
        // No timestamp queries, so no tick period to report.
        timestamp_period_ns: 0.0,
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
            .ok_or_else(|| HalError::invalid_handle("surface", surface))?;

        let Some(surface_ref) = slot.surface.as_ref() else {
            // The adapter still has to exist — a caller doing adapter selection
            // must not get an answer about an adapter that does not.
            self.adapters
                .iter()
                .find(|(info, _)| info.id == adapter)
                .ok_or(HalError::NoSuchAdapter(adapter.0))?;
            return Ok(offscreen_surface_caps());
        };

        let (_, wgpu_adapter) = self
            .adapters
            .iter()
            .find(|(info, _)| info.id == adapter)
            .ok_or(HalError::NoSuchAdapter(adapter.0))?;

        let caps = surface_ref.get_capabilities(wgpu_adapter);

        // An adapter that cannot present to this surface must be an error, not
        // an empty format list — `Instance::surface_caps` says so, and callers
        // doing adapter selection read `Err` as "try the next one".
        if caps.formats.is_empty() || caps.present_modes.is_empty() {
            return Err(HalError::Unsupported {
                backend: BackendKind::Wgpu,
                what: "this adapter cannot present to this surface",
            });
        }

        let formats: Vec<_> = caps
            .formats
            .iter()
            .filter_map(|f| crate::conv::unmap_format(*f))
            .collect();
        let formats = if slot.srgb_view_only {
            with_srgb_views(formats)
        } else {
            formats
        };

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

        if formats.is_empty() || present_modes.is_empty() {
            return Err(HalError::Unsupported {
                backend: BackendKind::Wgpu,
                what: "this surface supports no format or present mode the seam can name",
            });
        }

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
        // Offscreen names no window-system object, so there is nothing for wgpu
        // to wrap: the surface is a marker, and `create_swapchain` builds a ring
        // of plain textures on it. Same shape as `crcbl-vk`, which stores a null
        // `VkSurfaceKHR` for exactly this case.
        if matches!(target, SurfaceTarget::Offscreen) {
            let mut surfaces = self.surfaces.lock().unwrap();
            return Ok(surfaces
                .insert(SurfaceSlot {
                    surface: None,
                    platform: "offscreen",
                    srgb_view_only: false,
                })
                .cast());
        }

        let (raw_window, raw_display, platform) = unsafe { map_surface_target(target)? };

        let surface = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: raw_display,
                    raw_window_handle: raw_window,
                })
        }
        .map_err(|e| HalError::Backend(format!("wgpu create_surface on {platform}: {e}")))?;

        let mut surfaces = self.surfaces.lock().unwrap();
        Ok(surfaces
            .insert(SurfaceSlot {
                surface: Some(surface),
                platform,
                srgb_view_only: matches!(target, SurfaceTarget::Web { .. }),
            })
            .cast())
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        if let Some(slot) = self.surfaces.lock().unwrap().remove(surface.cast()) {
            log::debug!("crcbl-wgpu: destroyed the {} surface", slot.platform);
        }
    }

    fn request_device(
        &self,
        desc: &DeviceDesc<'_>,
    ) -> Result<Box<dyn crcbl_hal::PendingDevice>, HalError> {
        let (_info, adapter) = self
            .adapters
            .iter()
            .find(|(info, _)| info.id == desc.adapter)
            .ok_or(HalError::NoSuchAdapter(desc.adapter.0))?;

        WgpuDevice::request(adapter, desc, self.surface_pool())
            .map(|pending| Box::new(pending) as Box<dyn crcbl_hal::PendingDevice>)
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
        #[cfg(target_arch = "wasm32")]
        SurfaceTarget::Web { canvas_id } => {
            use raw_window_handle::{WebDisplayHandle, WebWindowHandle};
            // The shell's JS shim stamps `data-raw-handle="<canvas_id>"` on the
            // canvas; that attribute is what `WebWindowHandle` names.
            let window = RawWindowHandle::Web(WebWindowHandle::new(canvas_id));
            let display = RawDisplayHandle::Web(WebDisplayHandle::new());
            Ok((window, Some(display), "web"))
        }
        #[cfg(not(target_arch = "wasm32"))]
        SurfaceTarget::Web { .. } => unsupported("a Web canvas surface outside a wasm32 build"),
        SurfaceTarget::Win32 { .. } => unsupported("Win32 (P14)"),
        SurfaceTarget::AppKit { .. } => unsupported("AppKit (P14)"),
        // Handled before this function is reached: an offscreen "surface" has
        // no raw handle pair to map, which is the whole of what this maps.
        SurfaceTarget::Offscreen => unsupported("Offscreen has no raw window handle to map"),
    }
}

/// `formats` plus the sRGB counterparts a view format can reach.
///
/// **For a WebGPU canvas, and the reason the browser build is not dark.** The
/// spec's supported context formats are all linear — `getPreferredCanvasFormat`
/// returns `rgba8unorm` or `bgra8unorm`, and `configure` refuses an `-srgb`
/// one — so wgpu's list alone would hand
/// [`SurfaceCaps::preferred_format`](crcbl_hal::SurfaceCaps::preferred_format)
/// a linear target. Every pass above the seam writes display-referred values
/// and leaves the encode to the hardware, so a linear target skips the encode
/// and the frame presents far too dark: the horde's grass came out black in a
/// browser while it was right on Vulkan.
///
/// The encode is still free. An `-srgb` view of the same image is what
/// `viewFormats` exists for — the pair differ in nothing else — and
/// `Device::create_swapchain` configures the canvas linear and views it sRGB.
///
/// Appended rather than substituted, and only where the surface did not offer
/// the counterpart outright, so the linear formats stay askable and a list that
/// already had both is unchanged. Order matters: `preferred_format` takes the
/// first sRGB entry, and appending puts the counterpart of the browser's own
/// preferred canvas format there.
fn with_srgb_views(mut formats: Vec<Format>) -> Vec<Format> {
    for index in 0..formats.len() {
        if let Some(srgb) = crate::conv::srgb_counterpart(formats[index])
            && !formats.contains(&srgb)
        {
            formats.push(srgb);
        }
    }
    formats
}

/// The capabilities an offscreen "surface" reports.
///
/// There is no window system to ask, so this is a statement of what the ring
/// supports rather than a query — and it is deliberately the same statement
/// `crcbl-vk`'s `offscreen_surface_caps` makes, so a caller that picks a format
/// from these caps gets the same frame out of either backend. `Fifo` because the
/// seam promises it always exists; `Rgba8UnormSrgb` first because
/// [`SurfaceCaps::preferred_format`] picks the first sRGB format and a golden
/// image wants a display-referred one. `current_extent: None` because nothing
/// here has an opinion about the size.
#[must_use]
pub(crate) fn offscreen_surface_caps() -> SurfaceCaps {
    SurfaceCaps {
        formats: vec![
            Format::Rgba8UnormSrgb,
            Format::Bgra8UnormSrgb,
            Format::Rgba8Unorm,
            Format::Bgra8Unorm,
            Format::Rgba16Float,
        ],
        present_modes: vec![PresentMode::Fifo, PresentMode::Immediate],
        composite_alpha: vec![CompositeAlpha::Opaque],
        min_image_count: 1,
        max_image_count: 8,
        current_extent: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A canvas's linear-only format list still yields an sRGB target.**
    ///
    /// The list is what WebGPU actually reports — the two 8-bit formats with
    /// the browser's preferred one first, and `Rgba16Float` where the canvas
    /// takes it. Without the counterparts `preferred_format` falls through to
    /// the first entry, which is linear, and the whole frame presents dark.
    #[test]
    fn a_web_canvas_is_offered_an_srgb_format_to_view_it_through() {
        let reported = vec![Format::Bgra8Unorm, Format::Rgba8Unorm, Format::Rgba16Float];
        let eight_bit = 2;
        let offered = with_srgb_views(reported.clone());

        let caps = SurfaceCaps {
            formats: offered.clone(),
            present_modes: vec![PresentMode::Fifo],
            composite_alpha: vec![CompositeAlpha::Opaque],
            min_image_count: 2,
            max_image_count: 3,
            current_extent: None,
        };
        assert_eq!(
            caps.preferred_format(),
            Some(Format::Bgra8UnormSrgb),
            "the sRGB counterpart of the canvas's *preferred* format has to win",
        );
        for format in &reported {
            assert!(offered.contains(format), "{format:?} stopped being askable",);
        }
        assert_eq!(
            offered.len(),
            reported.len() + eight_bit,
            "one counterpart per 8-bit format and none for the float: {offered:?}",
        );
    }

    /// Nothing is added where the surface already offers the counterpart, and
    /// nothing at all for a format that has none — a native surface's list must
    /// come back exactly as it was.
    #[test]
    fn a_list_that_already_has_its_srgb_formats_is_left_alone() {
        let native = vec![
            Format::Bgra8UnormSrgb,
            Format::Bgra8Unorm,
            Format::Rgba8UnormSrgb,
            Format::Rgba8Unorm,
            Format::Rgba16Float,
        ];
        assert_eq!(with_srgb_views(native.clone()), native);
        assert_eq!(
            with_srgb_views(vec![Format::Rgba16Float]),
            vec![Format::Rgba16Float],
        );
    }

    /// The reported limits must agree with the reported features: a
    /// push-constant budget on a device without push constants is a promise no
    /// call can keep, and it is what the fabricated `Limits::desktop()` used to
    /// report on every adapter.
    #[test]
    fn limits_agree_with_features() {
        let bare = hal_limits_for(&wgpu::Limits::defaults(), wgpu::Features::empty());
        assert_eq!(bare.max_push_constant_size, 0);
        assert_eq!(bare.max_bindless_descriptors, 0);
        assert_eq!(
            bare.max_draw_indirect_count, 1,
            "without a count buffer, one indirect call emits its own draws only"
        );
        assert!(!hal_features_for(wgpu::Features::empty()).contains(Features::PUSH_CONSTANTS));
    }

    /// Push constants are never advertised, however capable the adapter is.
    ///
    /// This backend ingests WGSL, and WGSL has no push constants — so an
    /// adapter that has `IMMEDIATES` still cannot be handed a pipeline that
    /// uses them, because the shader for it cannot be compiled. Reporting the
    /// adapter's answer instead made `ConstantDelivery` pick the Tier A path on
    /// native wgpu and fail at `create_shader_module`. The limit must agree with
    /// the feature, which is what makes the pair checkable at all.
    #[test]
    fn push_constants_are_never_advertised_even_when_the_adapter_has_them() {
        for features in [wgpu::Features::IMMEDIATES, wgpu::Features::all()] {
            let hal = hal_features_for(features);
            assert!(
                !hal.contains(Features::PUSH_CONSTANTS),
                "WGSL cannot express a push constant, so the seam must not offer one"
            );
            assert_eq!(
                hal_limits_for(&wgpu::Limits::defaults(), features).max_push_constant_size,
                0,
                "a non-zero budget for a feature that is absent is a promise no call can keep"
            );
        }
    }

    /// Nothing wgpu cannot do may be advertised — most importantly buffer
    /// device address, which no wgpu backend exposes at all.
    #[test]
    fn wgpu_never_claims_what_it_cannot_do() {
        let everything = hal_features_for(wgpu::Features::all());
        assert!(!everything.contains(Features::BUFFER_DEVICE_ADDRESS));
        assert!(!everything.contains(Features::TIMESTAMP_QUERY));
        let caps = DeviceCaps {
            features: everything,
            limits: hal_limits_for(&wgpu::Limits::defaults(), wgpu::Features::all()),
        };
        assert!(
            caps.missing(Features::GPU_DRIVEN)
                .contains(Features::BUFFER_DEVICE_ADDRESS),
            "even asking wgpu for everything leaves the bundle unsatisfied: {:?}",
            caps.features
        );
    }

    /// A caller asking for an optional feature the adapter lacks gets a device,
    /// not an error — and the feature simply is not enabled.
    #[test]
    fn optional_features_are_filtered_by_the_adapter() {
        let requested = Features::PUSH_CONSTANTS | Features::TEXTURE_COMPRESSION_BC;
        let available = wgpu::Features::IMMEDIATES;
        let enabled = wgpu_features_for(requested, available);
        assert_eq!(enabled, wgpu::Features::IMMEDIATES);
    }
}
