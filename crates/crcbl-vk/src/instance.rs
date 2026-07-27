//! [`VkInstance`]: the loader, the instance, adapter enumeration and surfaces.

use core::ffi::{CStr, c_void};
use std::sync::{Arc, Mutex, MutexGuard};

use ash::{ext, khr, vk};

use crcbl_core::{Pool, SurfaceTarget};
use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, CompositeAlpha, Device, DeviceDesc, Format, HalError,
    Instance, PresentMode, SurfaceCaps, SurfaceHandle,
};

use crate::adapter::{self, AdapterRecord};
use crate::conv;
use crate::debug::{self, ValidationReport, ValidationSink};

/// The Vulkan API version this backend targets.
///
/// `docs/plan/02-vulkan-backend.md`: "Full HAL implementation on Vulkan 1.3
/// (dynamic rendering, sync2, no legacy render passes)". Asking for 1.3 in
/// `VkApplicationInfo` is what makes `vkCmdBeginRendering`,
/// `vkCmdPipelineBarrier2` and `vkQueueSubmit2` core entry points rather than
/// extension ones, so nothing below has to carry a promoted/unpromoted branch.
pub const API_VERSION: u32 = vk::API_VERSION_1_3;

/// Process-wide source of owner ids.
///
/// `crcbl-hal`'s [`device`](crcbl_hal::device) rule 3 obliges every backend to
/// stamp an owner identity into its own side table, because a
/// [`Handle`] has no room for one and two instances genuinely do issue
/// identical bits. A counter is enough and is cheaper to compare than a
/// `VkInstance` pointer.
static NEXT_OWNER_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) fn next_owner_id() -> u64 {
    NEXT_OWNER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A live `VkSurfaceKHR` and the bookkeeping obligation 2 requires.
#[derive(Debug)]
struct SurfaceEntry {
    raw: vk::SurfaceKHR,
    /// Instance that created it. Surfaces are checked against the *instance*
    /// id, so any device from that instance may use them.
    owner: u64,
    /// Which `SurfaceTarget` variant it came from, for logs.
    platform: &'static str,
    /// Swapchains still configured on it. `vkDestroySurfaceKHR` with a live
    /// swapchain is undefined behaviour in the driver, so the object outlives
    /// the handle by exactly this count.
    swapchains: usize,
}

/// The surface table, plus the surfaces whose handle is dead but whose driver
/// object is not.
#[derive(Debug, Default)]
struct Surfaces {
    live: Pool<SurfaceEntry>,
    /// Destroyed by the caller, still referenced by a swapchain. See
    /// `crcbl-hal`'s obligation 2: "the handle dies when the caller says so,
    /// the object dies when it is safe".
    zombies: Vec<SurfaceEntry>,
}

/// Everything a device needs to keep alive from the instance that made it.
///
/// `crcbl-hal`'s obligation 1: a `Device` may outlive its `Instance`, and the
/// backend **must** keep the instance-level state alive internally. This is that
/// `Arc`. Dropping the public [`VkInstance`] while a device is open therefore
/// does not call `vkDestroyInstance`.
pub(crate) struct InstanceInner {
    /// The dynamically loaded entry points. Held because `ash::Entry` owns the
    /// `dlopen` handle: dropping it unloads `libvulkan.so.1` out from under
    /// every function pointer in this struct.
    pub(crate) entry: ash::Entry,
    pub(crate) raw: ash::Instance,
    pub(crate) surface_ext: Option<khr::surface::Instance>,
    pub(crate) wayland_ext: Option<khr::wayland_surface::Instance>,
    pub(crate) xcb_ext: Option<khr::xcb_surface::Instance>,
    pub(crate) debug_ext: Option<ext::debug_utils::Instance>,
    messenger: vk::DebugUtilsMessengerEXT,
    messenger_user_data: *mut c_void,
    pub(crate) sink: ValidationSink,
    pub(crate) validation_enabled: bool,
    pub(crate) id: u64,
    pub(crate) adapters: Vec<AdapterRecord>,
    surfaces: Mutex<Surfaces>,
}

// SAFETY: every field is either `Send + Sync` already (`ash::Entry`,
// `ash::Instance` and the extension tables are all documented as such) or is
// protected by the `Mutex`. The one raw pointer, `messenger_user_data`, is an
// `Arc::into_raw` of a `Sync` allocation that is only ever read by the driver's
// callback and reclaimed once, in `Drop`.
unsafe impl Send for InstanceInner {}
// SAFETY: as above.
unsafe impl Sync for InstanceInner {}

impl core::fmt::Debug for InstanceInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InstanceInner")
            .field("id", &self.id)
            .field("adapters", &self.adapters.len())
            .field("validation", &self.validation_enabled)
            .finish_non_exhaustive()
    }
}

impl InstanceInner {
    fn surfaces(&self) -> MutexGuard<'_, Surfaces> {
        self.surfaces
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Resolves a surface handle against *this* instance, per obligation 3.
    pub(crate) fn surface_raw(&self, surface: SurfaceHandle) -> Result<vk::SurfaceKHR, HalError> {
        let surfaces = self.surfaces();
        match surfaces.live.get(surface.cast()) {
            Some(entry) if entry.owner == self.id => Ok(entry.raw),
            Some(_) => Err(HalError::ForeignObject {
                kind: "surface",
                bits: surface.to_bits(),
            }),
            None => Err(HalError::invalid_handle("surface", surface)),
        }
    }

    /// Notes that a swapchain now references `surface`.
    ///
    /// The count exists only to defer `vkDestroySurfaceKHR`, so an offscreen
    /// surface — which has no driver object — is deliberately not counted.
    /// Counting it would leave a zombie nothing could ever release, because
    /// [`release_surface`](Self::release_surface) is keyed on the raw handle
    /// and every offscreen surface's is null.
    pub(crate) fn retain_surface(&self, surface: SurfaceHandle) -> Result<(), HalError> {
        let mut surfaces = self.surfaces();
        match surfaces.live.get_mut(surface.cast()) {
            Some(entry) if entry.owner == self.id => {
                if entry.raw != vk::SurfaceKHR::null() {
                    entry.swapchains += 1;
                }
                Ok(())
            }
            Some(_) => Err(HalError::ForeignObject {
                kind: "surface",
                bits: surface.to_bits(),
            }),
            None => Err(HalError::invalid_handle("surface", surface)),
        }
    }

    /// Notes that a swapchain no longer references `raw`, destroying the driver
    /// object if that was the last one and the caller has already let the
    /// handle go.
    pub(crate) fn release_surface(&self, raw: vk::SurfaceKHR) {
        if raw == vk::SurfaceKHR::null() {
            return;
        }
        let mut surfaces = self.surfaces();
        for (_, entry) in surfaces.live.iter_mut() {
            if entry.raw == raw {
                entry.swapchains = entry.swapchains.saturating_sub(1);
                return;
            }
        }
        let Some(index) = surfaces
            .zombies
            .iter()
            .position(|entry| entry.raw == raw && entry.swapchains > 0)
        else {
            return;
        };
        surfaces.zombies[index].swapchains -= 1;
        if surfaces.zombies[index].swapchains == 0 {
            let entry = surfaces.zombies.swap_remove(index);
            self.destroy_surface_object(entry.raw);
        }
    }

    fn destroy_surface_object(&self, raw: vk::SurfaceKHR) {
        // An offscreen "surface" has no driver object at all — see
        // `crate::swapchain`. Nothing to destroy, and nothing to defer.
        if raw == vk::SurfaceKHR::null() {
            return;
        }
        let Some(surface_ext) = self.surface_ext.as_ref() else {
            return;
        };
        // SAFETY: `raw` came from this instance's `vkCreate*SurfaceKHR`, it has
        // not been destroyed before (each path removes its entry first), and
        // every swapchain configured on it has already been destroyed —
        // which is the whole reason this call is deferred.
        unsafe { surface_ext.destroy_surface(raw, None) };
    }
}

impl Drop for InstanceInner {
    fn drop(&mut self) {
        // Anything still referenced here is a caller leak, not a reason to skip
        // the free: the process is going away either way, and leaving a surface
        // behind hides the leak from the driver's own validation.
        let mut surfaces = self.surfaces();
        let leaked = surfaces.live.len() + surfaces.zombies.len();
        if leaked > 0 {
            log::warn!(
                "crcbl-vk: {leaked} surface(s) still alive at instance teardown; \
                 the seam's teardown order is swapchain, surface, device, instance"
            );
        }
        let raws: Vec<vk::SurfaceKHR> = surfaces
            .live
            .iter()
            .map(|(_, entry)| entry.raw)
            .chain(surfaces.zombies.iter().map(|entry| entry.raw))
            .collect();
        surfaces.live.clear();
        surfaces.zombies.clear();
        drop(surfaces);
        for raw in raws {
            self.destroy_surface_object(raw);
        }

        if let Some(debug_ext) = self.debug_ext.as_ref()
            && self.messenger != vk::DebugUtilsMessengerEXT::null()
        {
            // SAFETY: the messenger was created from this instance and is
            // destroyed exactly once. It must go before the instance, and
            // before its user data is reclaimed below.
            unsafe { debug_ext.destroy_debug_utils_messenger(self.messenger, None) };
        }
        // SAFETY: no device or surface remains (both are gone above; a live
        // device would still be holding an `Arc` to this struct and `Drop`
        // would not be running).
        unsafe { self.raw.destroy_instance(None) };
        // Strictly after the messenger, which is the only thing that could
        // still dereference it.
        // SAFETY: this is the pointer `debug::messenger_user_data` produced in
        // `VkInstance::open`, reclaimed once, with the messenger already gone.
        unsafe { debug::drop_messenger_user_data(self.messenger_user_data) };
    }
}

/// The Vulkan backend's entry point.
///
/// One per process is the intended shape, but nothing enforces it: two
/// instances get distinct owner ids and their handles are foreign to each other,
/// which is exactly what `crcbl-hal`'s obligation 3 requires.
#[derive(Clone, Debug)]
pub struct VkInstance {
    inner: Arc<InstanceInner>,
}

/// Why the Vulkan backend could not start.
///
/// Separate from [`HalError`] only at the constructor, because "there is no
/// Vulkan loader on this machine" is a *selection* answer — the registry above
/// moves to the next backend — rather than a device failure.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// `libvulkan.so.1` (or the platform equivalent) could not be `dlopen`ed.
    ///
    /// The expected outcome on a machine with no Vulkan runtime at all, which
    /// is why this backend loads dynamically — see the crate docs.
    #[error("no Vulkan loader: {0}")]
    NoLoader(String),

    /// The loader is present but reports an API version below 1.3.
    #[error(
        "the Vulkan loader reports {major}.{minor}, and crcbl-vk targets 1.3 \
         (dynamic rendering, synchronization2, timeline semaphores)"
    )]
    ApiTooOld {
        /// Major version the loader reported.
        major: u32,
        /// Minor version the loader reported.
        minor: u32,
    },

    /// `vkCreateInstance` failed.
    #[error("vkCreateInstance failed: {0}")]
    Instance(String),

    /// The loader works but enumerated no physical device.
    #[error("the Vulkan loader found no physical device")]
    NoAdapters,
}

impl From<OpenError> for HalError {
    fn from(error: OpenError) -> Self {
        Self::Backend(error.to_string())
    }
}

impl VkInstance {
    /// Loads Vulkan, creates an instance and enumerates adapters.
    ///
    /// # Errors
    ///
    /// [`OpenError::NoLoader`] when there is no Vulkan runtime — the case a
    /// backend registry falls through on — or the other variants when there is
    /// one and it refused.
    pub fn open() -> Result<Self, OpenError> {
        // The loading decision, in one call. See the crate docs for why this is
        // `load()` and not `linked()`.
        //
        // SAFETY: `Entry::load` `dlopen`s the system Vulkan loader and reads
        // its exported symbols. It is `unsafe` because a hostile or mismatched
        // `libvulkan.so.1` on the search path could export the right names with
        // the wrong signatures; that is the same trust the process already
        // extends to every other system library it resolves, and it is exactly
        // the trust `crcbl-shell` extends to `libwayland-client.so.0`.
        let entry = unsafe { ash::Entry::load() }
            .map_err(|error| OpenError::NoLoader(error.to_string()))?;

        // SAFETY: `entry` is a freshly loaded, valid entry table.
        let loader_version = unsafe { entry.try_enumerate_instance_version() }
            .map_err(|error| OpenError::Instance(format!("{error:?}")))?
            .unwrap_or(vk::API_VERSION_1_0);
        if vk::api_version_major(loader_version) < 1
            || (vk::api_version_major(loader_version) == 1
                && vk::api_version_minor(loader_version) < 3)
        {
            return Err(OpenError::ApiTooOld {
                major: vk::api_version_major(loader_version),
                minor: vk::api_version_minor(loader_version),
            });
        }

        // SAFETY: `entry` is valid; the call only reads the loader's tables.
        let available_extensions = unsafe { entry.enumerate_instance_extension_properties(None) }
            .map_err(|error| OpenError::Instance(format!("{error:?}")))?;
        let has_extension = |name: &CStr| {
            available_extensions.iter().any(|properties| {
                properties
                    .extension_name_as_c_str()
                    .is_ok_and(|available| available == name)
            })
        };

        // SAFETY: as above.
        let available_layers = unsafe { entry.enumerate_instance_layer_properties() }
            .map_err(|error| OpenError::Instance(format!("{error:?}")))?;
        let validation_available = available_layers.iter().any(|properties| {
            properties
                .layer_name_as_c_str()
                .is_ok_and(|name| name == debug::VALIDATION_LAYER_C)
        });
        let want_validation = debug::validation_wanted();
        if want_validation && !validation_available {
            // A warning, never a failure: a machine without the layers package
            // must still run the engine. The *tests* refuse a clean report from
            // a disabled layer, which is where the strictness belongs.
            log::warn!(
                "crcbl-vk: {} was requested but is not installed; validation is off",
                debug::VALIDATION_LAYER
            );
        }
        let validation_enabled = want_validation && validation_available;

        // Only ever the intersection of wanted and available. Requesting an
        // absent extension fails `vkCreateInstance` outright, and this backend
        // must degrade on a headless container with no WSI at all rather than
        // refusing to start — offscreen rendering needs none of these.
        let mut extensions: Vec<*const core::ffi::c_char> = Vec::new();
        let mut enabled_names: Vec<&CStr> = Vec::new();
        for name in [
            khr::surface::NAME,
            khr::wayland_surface::NAME,
            khr::xcb_surface::NAME,
        ] {
            if has_extension(name) {
                extensions.push(name.as_ptr());
                enabled_names.push(name);
            }
        }
        let debug_utils = has_extension(ext::debug_utils::NAME);
        if debug_utils {
            extensions.push(ext::debug_utils::NAME.as_ptr());
            enabled_names.push(ext::debug_utils::NAME);
        }
        let want_sync_validation = validation_enabled && debug::sync_validation_wanted();
        // **`VK_EXT_validation_features` is a *layer* extension, not an instance
        // one.** `vkEnumerateInstanceExtensionProperties(NULL, …)` returns only
        // what the loader and the ICDs provide, and the validation layer's own
        // extensions are invisible to it — so probing the implicit list alone
        // finds nothing, and synchronisation validation is silently never
        // enabled. `CRCBL_VK_SYNC_VALIDATION=1` then buys a log line and no
        // checking, on the developer's machine *and* in CI, which is precisely
        // the vacuous-gate failure `ValidationReport::assert_clean` exists to
        // prevent one level up.
        //
        // Found at P1.2: `docs/plan/02-vulkan-backend.md` names sync bugs as
        // this stage's headline risk and sync validation as the mitigation, and
        // the mitigation had never run.
        let layer_extensions = if validation_enabled {
            // SAFETY: `entry` is valid and `VALIDATION_LAYER_C` is a NUL-
            // terminated name the loader just reported as available.
            unsafe {
                entry.enumerate_instance_extension_properties(Some(debug::VALIDATION_LAYER_C))
            }
            .unwrap_or_default()
        } else {
            Vec::new()
        };
        let has_layer_extension = |name: &CStr| {
            layer_extensions.iter().any(|properties| {
                properties
                    .extension_name_as_c_str()
                    .is_ok_and(|available| available == name)
            })
        };
        let validation_features_ext = want_sync_validation
            && (has_extension(ext::validation_features::NAME)
                || has_layer_extension(ext::validation_features::NAME));
        if validation_features_ext {
            extensions.push(ext::validation_features::NAME.as_ptr());
            enabled_names.push(ext::validation_features::NAME);
        } else if want_sync_validation {
            log::warn!(
                "crcbl-vk: synchronisation validation was asked for but {} is absent from both \
                 the loader's extensions and {}'s",
                ext::validation_features::NAME.to_string_lossy(),
                debug::VALIDATION_LAYER
            );
        }
        if validation_features_ext {
            // At info, not debug: "sync validation is on" is the fact that makes
            // a green run mean something, and a run where it is off must not
            // look identical in the log to one where it is on.
            log::info!("crcbl-vk: synchronisation validation enabled");
        }
        log::debug!(
            "crcbl-vk: instance extensions {:?}",
            enabled_names
                .iter()
                .map(|name| name.to_string_lossy())
                .collect::<Vec<_>>()
        );

        let layers: Vec<*const core::ffi::c_char> = if validation_enabled {
            vec![debug::VALIDATION_LAYER_C.as_ptr()]
        } else {
            Vec::new()
        };

        let sink = ValidationSink::new();
        let user_data = if validation_enabled && debug_utils {
            debug::messenger_user_data(&sink)
        } else {
            core::ptr::null_mut()
        };

        let application = vk::ApplicationInfo::default()
            .application_name(c"crcbl")
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(c"crucible")
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(API_VERSION);

        // Chained into `VkInstanceCreateInfo` rather than only created
        // afterwards, because a messenger created after `vkCreateInstance`
        // cannot report anything that went wrong *during* it — and instance
        // creation is exactly where a bad extension list shows up.
        let mut messenger_info = debug::messenger_create_info(user_data);
        let sync_enables = [vk::ValidationFeatureEnableEXT::SYNCHRONIZATION_VALIDATION];
        let mut validation_features =
            vk::ValidationFeaturesEXT::default().enabled_validation_features(&sync_enables);

        let mut create_info = vk::InstanceCreateInfo::default()
            .application_info(&application)
            .enabled_layer_names(&layers)
            .enabled_extension_names(&extensions);
        if validation_enabled && debug_utils {
            create_info = create_info.push_next(&mut messenger_info);
        }
        if validation_features_ext {
            create_info = create_info.push_next(&mut validation_features);
        }

        // SAFETY: every pointer in `create_info` borrows a local that outlives
        // this call, both name arrays are NUL-terminated `&CStr` pointers, and
        // the chained structs are `push_next`ed exactly once each.
        let raw = match unsafe { entry.create_instance(&create_info, None) } {
            Ok(raw) => raw,
            Err(error) => {
                // SAFETY: the messenger was never created, so nothing can call
                // back through this pointer.
                unsafe { debug::drop_messenger_user_data(user_data) };
                return Err(OpenError::Instance(format!("{error:?}")));
            }
        };

        let debug_ext = debug_utils.then(|| ext::debug_utils::Instance::new(&entry, &raw));
        let messenger = match debug_ext.as_ref().filter(|_| validation_enabled) {
            // SAFETY: `raw` is a live instance created with `VK_EXT_debug_utils`
            // enabled, and `messenger_info` is the same descriptor the chained
            // one used.
            Some(debug_ext) => {
                unsafe { debug_ext.create_debug_utils_messenger(&messenger_info, None) }
                    .unwrap_or_else(|error| {
                        log::warn!("crcbl-vk: could not create the debug messenger: {error:?}");
                        vk::DebugUtilsMessengerEXT::null()
                    })
            }
            None => vk::DebugUtilsMessengerEXT::null(),
        };

        let surface_ext =
            has_extension(khr::surface::NAME).then(|| khr::surface::Instance::new(&entry, &raw));
        let wayland_ext = has_extension(khr::wayland_surface::NAME)
            .then(|| khr::wayland_surface::Instance::new(&entry, &raw));
        let xcb_ext = has_extension(khr::xcb_surface::NAME)
            .then(|| khr::xcb_surface::Instance::new(&entry, &raw));

        let adapters = adapter::enumerate(&raw, debug_utils);
        if adapters.is_empty() {
            // Tear the instance down by hand: `InstanceInner` does not exist
            // yet, so there is no `Drop` to do it.
            if let Some(debug_ext) = debug_ext.as_ref()
                && messenger != vk::DebugUtilsMessengerEXT::null()
            {
                // SAFETY: created just above from this instance, destroyed once.
                unsafe { debug_ext.destroy_debug_utils_messenger(messenger, None) };
            }
            // SAFETY: nothing was created from this instance.
            unsafe { raw.destroy_instance(None) };
            // SAFETY: the messenger is gone, so nothing can call back.
            unsafe { debug::drop_messenger_user_data(user_data) };
            return Err(OpenError::NoAdapters);
        }
        for record in &adapters {
            log::info!(
                "crcbl-vk: adapter {} — {} ({:?}), {}, tier {:?}",
                record.info.id.0,
                record.info.name,
                record.info.device_type,
                record.info.driver,
                record.info.caps.tier()
            );
        }

        Ok(Self {
            inner: Arc::new(InstanceInner {
                entry,
                raw,
                surface_ext,
                wayland_ext,
                xcb_ext,
                debug_ext,
                messenger,
                messenger_user_data: user_data,
                sink,
                validation_enabled,
                id: next_owner_id(),
                adapters,
                surfaces: Mutex::new(Surfaces::default()),
            }),
        })
    }

    /// Everything the validation layer has said so far.
    ///
    /// The P1 exit criterion "zero validation errors/warnings" is checked with
    /// [`ValidationReport::assert_clean`] on this, which is what turns it from
    /// a thing someone reads in a log into a thing that fails a test.
    #[must_use]
    pub fn validation_report(&self) -> ValidationReport {
        self.inner.sink.report(self.inner.validation_enabled)
    }

    /// Whether the validation layer is actually loaded.
    #[must_use]
    pub fn validation_enabled(&self) -> bool {
        self.inner.validation_enabled
    }

    /// The Vulkan version the *loader* reports, as `(major, minor, patch)`.
    ///
    /// Worth logging alongside the adapter's own `apiVersion`: the two differ
    /// routinely (a 1.4 loader in front of a 1.3 driver, or the reverse), and
    /// "which 1.3 am I actually getting" is the first question a driver bug
    /// raises.
    #[must_use]
    pub fn loader_version(&self) -> (u32, u32, u32) {
        // SAFETY: `entry` is the live entry table this instance was created
        // from; the call only reads the loader's own version.
        let raw = unsafe { self.inner.entry.try_enumerate_instance_version() }
            .ok()
            .flatten()
            .unwrap_or(vk::API_VERSION_1_0);
        (
            vk::api_version_major(raw),
            vk::api_version_minor(raw),
            vk::api_version_patch(raw),
        )
    }

    fn record(&self, adapter: AdapterId) -> Result<&AdapterRecord, HalError> {
        self.inner
            .adapters
            .get(adapter.0 as usize)
            .ok_or(HalError::NoSuchAdapter(adapter.0))
    }
}

impl Instance for VkInstance {
    fn backend(&self) -> BackendKind {
        BackendKind::Vulkan
    }

    fn adapters(&self) -> Vec<AdapterInfo> {
        self.inner
            .adapters
            .iter()
            .map(|record| record.info.clone())
            .collect()
    }

    unsafe fn create_surface(&self, target: &SurfaceTarget) -> Result<SurfaceHandle, HalError> {
        let unsupported = |what| {
            Err(HalError::Unsupported {
                backend: BackendKind::Vulkan,
                what,
            })
        };
        let raw = match *target {
            SurfaceTarget::Wayland { display, surface } => {
                let Some(ext) = self.inner.wayland_ext.as_ref() else {
                    return unsupported("VK_KHR_wayland_surface is not available");
                };
                let info = vk::WaylandSurfaceCreateInfoKHR::default()
                    .display(display.as_ptr())
                    .surface(surface.as_ptr());
                // SAFETY: the trait's contract puts this on the caller — the
                // two pointers name a live `wl_display*` and `wl_surface*` of
                // exactly those kinds, and both outlive the surface returned
                // here. Nothing in this crate can check that; only the shell
                // that created the window knows.
                unsafe { ext.create_wayland_surface(&info, None) }
                    .map_err(|error| conv::hal_error("vkCreateWaylandSurfaceKHR", error))?
            }
            SurfaceTarget::Xcb {
                connection,
                window,
                visual_id: _,
            } => {
                let Some(ext) = self.inner.xcb_ext.as_ref() else {
                    return unsupported("VK_KHR_xcb_surface is not available");
                };
                let info = vk::XcbSurfaceCreateInfoKHR::default()
                    .connection(connection.as_ptr())
                    .window(window);
                // SAFETY: as for Wayland — the caller promises `connection` is
                // a live `xcb_connection_t*` and `window` a live XID on it, and
                // that both outlive the surface.
                unsafe { ext.create_xcb_surface(&info, None) }
                    .map_err(|error| conv::hal_error("vkCreateXcbSurfaceKHR", error))?
            }
            // No window system at all: the "surface" is a token, and the
            // swapchain built on it is a ring of plain images. See
            // `crate::swapchain`.
            SurfaceTarget::Offscreen => vk::SurfaceKHR::null(),
            SurfaceTarget::Win32 { .. } => return unsupported("Win32 surfaces land at P14"),
            SurfaceTarget::AppKit { .. } => return unsupported("AppKit surfaces land at P14"),
            SurfaceTarget::Web { .. } => {
                return unsupported("a canvas is crcbl-wgpu's target, not Vulkan's");
            }
        };

        let handle: SurfaceHandle = self
            .inner
            .surfaces()
            .live
            .insert(SurfaceEntry {
                raw,
                owner: self.inner.id,
                platform: target.platform_name(),
                swapchains: 0,
            })
            .cast();
        log::debug!(
            "crcbl-vk: created a {} surface {:?}",
            target.platform_name(),
            handle
        );
        Ok(handle)
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        let mut surfaces = self.inner.surfaces();
        // Only this instance's surfaces: another instance's handle may collide
        // on bits and must not be destroyed by mistake.
        if !surfaces
            .live
            .get(surface.cast())
            .is_some_and(|entry| entry.owner == self.inner.id)
        {
            return;
        }
        let Some(entry) = surfaces.live.remove(surface.cast()) else {
            return;
        };
        if entry.swapchains == 0 {
            let raw = entry.raw;
            drop(surfaces);
            self.inner.destroy_surface_object(raw);
        } else {
            // Obligation 2: the handle is dead now, the object is not.
            log::debug!(
                "crcbl-vk: {} surface handle destroyed with {} swapchain(s) still on it; \
                 deferring the driver object",
                entry.platform,
                entry.swapchains
            );
            surfaces.zombies.push(entry);
        }
    }

    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        let record = self.record(adapter)?;
        let raw = self.inner.surface_raw(surface)?;
        if raw == vk::SurfaceKHR::null() {
            return Ok(crate::swapchain::offscreen_surface_caps());
        }
        let Some(surface_ext) = self.inner.surface_ext.as_ref() else {
            return Err(HalError::Unsupported {
                backend: BackendKind::Vulkan,
                what: "VK_KHR_surface is not available",
            });
        };

        // An adapter that cannot present to this surface is a real and common
        // case — a discrete GPU under an X server with no DRI3, a second GPU in
        // a laptop — and `vkGetPhysicalDeviceSurfaceFormatsKHR` on one is not
        // meaningfully defined. So it is refused here, before the query.
        //
        // The seam has no *value* for "this pairing does not work": `formats:
        // []` would collide with `SurfaceCaps::preferred_format`'s documented
        // "the backend should have failed earlier", and `present_modes: []`
        // would break its promise that `Fifo` is always there. `Unsupported` is
        // the honest answer, and it obliges a caller doing adapter selection to
        // treat an `Err` from this call as "try the next one" rather than as
        // fatal — which `apps/sandbox` does, and which the seam should probably
        // say out loud. See the crate docs.
        let mut presentable = false;
        // SAFETY: `record.physical` came from this instance.
        let families = unsafe {
            self.inner
                .raw
                .get_physical_device_queue_family_properties(record.physical)
        };
        for family in 0..families.len() {
            #[allow(clippy::cast_possible_truncation)]
            let family = family as u32;
            // SAFETY: `record.physical` and `raw` both came from this instance.
            if unsafe {
                surface_ext.get_physical_device_surface_support(record.physical, family, raw)
            }
            .unwrap_or(false)
            {
                presentable = true;
                break;
            }
        }
        if !presentable {
            return Err(HalError::Unsupported {
                backend: BackendKind::Vulkan,
                what: "no queue family on this adapter can present to this surface",
            });
        }

        // SAFETY: `raw` is a live surface from this instance and
        // `record.physical` a physical device enumerated from it.
        let capabilities = unsafe {
            surface_ext.get_physical_device_surface_capabilities(record.physical, raw)
        }
        .map_err(|error| conv::hal_error("vkGetPhysicalDeviceSurfaceCapabilitiesKHR", error))?;
        // SAFETY: as above.
        let formats =
            unsafe { surface_ext.get_physical_device_surface_formats(record.physical, raw) }
                .map_err(|error| conv::hal_error("vkGetPhysicalDeviceSurfaceFormatsKHR", error))?;
        // SAFETY: as above.
        let present_modes =
            unsafe { surface_ext.get_physical_device_surface_present_modes(record.physical, raw) }
                .map_err(|error| {
                    conv::hal_error("vkGetPhysicalDeviceSurfacePresentModesKHR", error)
                })?;

        Ok(build_surface_caps(&capabilities, &formats, &present_modes))
    }

    fn create_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn Device>, HalError> {
        let record = self.record(desc.adapter)?;
        let present_surface = match desc.compatible_surface {
            Some(surface) => Some((surface, self.inner.surface_raw(surface)?)),
            None => None,
        };
        let device = crate::device::VkDevice::open(
            Arc::clone(&self.inner),
            record,
            desc,
            present_surface.map(|(_, raw)| raw),
        )?;
        Ok(Box::new(device))
    }
}

/// Builds the seam's [`SurfaceCaps`] from the three Vulkan queries.
///
/// Pure, and the reason it is a free function: this is where **obligation 2 of
/// the extent rule** is discharged, and it is the single most testable decision
/// in the whole backend.
///
/// * `currentExtent` of `(0xFFFFFFFF, 0xFFFFFFFF)` is Wayland saying "you
///   choose", and the seam is explicit that the sentinel **must not** escape —
///   it becomes `None`.
/// * Formats the seam has no name for are dropped rather than approximated,
///   and sRGB ones are listed first so [`SurfaceCaps::preferred_format`] finds
///   one without depending on driver order.
/// * `maxImageCount == 0` means "no limit" in Vulkan, which the seam has no way
///   to say, so it becomes a large-but-finite number a caller can clamp to.
#[must_use]
pub(crate) fn build_surface_caps(
    capabilities: &vk::SurfaceCapabilitiesKHR,
    formats: &[vk::SurfaceFormatKHR],
    present_modes: &[vk::PresentModeKHR],
) -> SurfaceCaps {
    let mut mapped: Vec<Format> = formats
        .iter()
        // Only the standard non-linear sRGB colour space: the engine tonemaps
        // into a display-referred swapchain, and an HDR colour space is a P7
        // decision with its own metadata, not something to pick up by accident.
        .filter(|format| format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR)
        .filter_map(|format| conv::format_from_vk(format.format))
        .collect();
    mapped.dedup();
    // "Best first", per the seam. sRGB first is what the tonemap pass wants,
    // and stable ordering means two drivers listing the same formats in
    // different orders pick the same one.
    mapped.sort_by_key(|format| (!format.is_srgb(), *format));
    mapped.dedup();

    let mut modes: Vec<PresentMode> = present_modes
        .iter()
        .filter_map(|mode| conv::present_mode_from_vk(*mode))
        .collect();
    // `PresentMode` has no `Ord` — it is a set of names, not a scale — so the
    // order is stated here: the seam's own preference order, so two drivers
    // listing the same modes differently produce the same list.
    modes.sort_by_key(|mode| match mode {
        PresentMode::Fifo => 0,
        PresentMode::FifoRelaxed => 1,
        PresentMode::Mailbox => 2,
        PresentMode::Immediate => 3,
    });
    modes.dedup();
    if !modes.contains(&PresentMode::Fifo) {
        // The seam promises `Fifo` is always present, and Vulkan requires every
        // surface to support it, so a driver that omitted it is confused rather
        // than authoritative.
        modes.insert(0, PresentMode::Fifo);
    }

    let mut alpha = conv::composite_alpha_from_vk(capabilities.supported_composite_alpha);
    if alpha.is_empty() {
        alpha.push(CompositeAlpha::Opaque);
    }

    SurfaceCaps {
        formats: mapped,
        present_modes: modes,
        composite_alpha: alpha,
        min_image_count: capabilities.min_image_count,
        max_image_count: if capabilities.max_image_count == 0 {
            // Vulkan's "unlimited". The seam wants a number; anything a caller
            // would clamp against does, and this one cannot overflow a `+ 1`.
            u32::MAX - 1
        } else {
            capabilities.max_image_count
        },
        current_extent: crate::swapchain::resolve_current_extent(capabilities.current_extent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface_format(format: vk::Format) -> vk::SurfaceFormatKHR {
        vk::SurfaceFormatKHR {
            format,
            color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
        }
    }

    fn capabilities(current: vk::Extent2D) -> vk::SurfaceCapabilitiesKHR {
        vk::SurfaceCapabilitiesKHR {
            min_image_count: 2,
            max_image_count: 4,
            current_extent: current,
            min_image_extent: vk::Extent2D {
                width: 1,
                height: 1,
            },
            max_image_extent: vk::Extent2D {
                width: 16384,
                height: 16384,
            },
            supported_composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
            ..Default::default()
        }
    }

    /// The headline obligation, in the exact shape Wayland produces it:
    /// `0xFFFFFFFF` means "no opinion", and it must become `None` rather than
    /// travelling into the seam as a four-billion-pixel window.
    #[test]
    fn the_wayland_sentinel_never_escapes_into_the_seam() {
        let caps = build_surface_caps(
            &capabilities(vk::Extent2D {
                width: u32::MAX,
                height: u32::MAX,
            }),
            &[surface_format(vk::Format::B8G8R8A8_SRGB)],
            &[vk::PresentModeKHR::FIFO],
        );
        assert_eq!(caps.current_extent, None);
    }

    /// X11's half of the same rule: the server knows the size, so it is
    /// reported — as a cross-check the caller may log, never as the source of
    /// truth.
    #[test]
    fn a_real_extent_is_reported_as_a_cross_check() {
        let caps = build_surface_caps(
            &capabilities(vk::Extent2D {
                width: 1280,
                height: 720,
            }),
            &[surface_format(vk::Format::B8G8R8A8_SRGB)],
            &[vk::PresentModeKHR::FIFO],
        );
        assert_eq!(caps.current_extent, Some((1280, 720)));
    }

    #[test]
    fn srgb_formats_are_listed_first_whatever_order_the_driver_used() {
        let caps = build_surface_caps(
            &capabilities(vk::Extent2D {
                width: 1,
                height: 1,
            }),
            &[
                surface_format(vk::Format::B8G8R8A8_UNORM),
                surface_format(vk::Format::B8G8R8A8_SRGB),
            ],
            &[vk::PresentModeKHR::FIFO],
        );
        assert_eq!(caps.preferred_format(), Some(Format::Bgra8UnormSrgb));
        assert_eq!(caps.formats.first(), Some(&Format::Bgra8UnormSrgb));
    }

    /// A format the seam has no name for is dropped, not approximated. A
    /// silently substituted format is a colour-space bug that looks like
    /// "slightly wrong on one backend".
    #[test]
    fn formats_the_seam_cannot_name_are_dropped() {
        let caps = build_surface_caps(
            &capabilities(vk::Extent2D {
                width: 1,
                height: 1,
            }),
            &[
                surface_format(vk::Format::R5G6B5_UNORM_PACK16),
                surface_format(vk::Format::B8G8R8A8_SRGB),
            ],
            &[vk::PresentModeKHR::FIFO],
        );
        assert_eq!(caps.formats, vec![Format::Bgra8UnormSrgb]);
    }

    /// An HDR colour space is a P7 decision with its own metadata. Picking one
    /// up by accident because the driver listed it would silently change what
    /// the tonemap pass is writing into.
    #[test]
    fn only_the_standard_srgb_colour_space_is_offered() {
        let caps = build_surface_caps(
            &capabilities(vk::Extent2D {
                width: 1,
                height: 1,
            }),
            &[
                vk::SurfaceFormatKHR {
                    format: vk::Format::R16G16B16A16_SFLOAT,
                    color_space: vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT,
                },
                surface_format(vk::Format::B8G8R8A8_SRGB),
            ],
            &[vk::PresentModeKHR::FIFO],
        );
        assert_eq!(caps.formats, vec![Format::Bgra8UnormSrgb]);
    }

    /// Vulkan's `maxImageCount == 0` means unlimited; the seam has no such
    /// value, and a caller doing `min_image_count + 1 .min(max)` must not get
    /// zero.
    #[test]
    fn an_unlimited_image_count_becomes_a_usable_number() {
        let mut raw = capabilities(vk::Extent2D {
            width: 1,
            height: 1,
        });
        raw.max_image_count = 0;
        let caps = build_surface_caps(
            &raw,
            &[surface_format(vk::Format::B8G8R8A8_SRGB)],
            &[vk::PresentModeKHR::FIFO],
        );
        assert!(caps.max_image_count > caps.min_image_count);
        assert_eq!(
            caps.min_image_count
                .saturating_add(1)
                .min(caps.max_image_count),
            3
        );
    }

    /// The seam promises `Fifo` is always available and Vulkan requires it, so
    /// a driver that omits it gets it back rather than breaking the promise.
    #[test]
    fn fifo_is_always_offered_and_modes_are_deduplicated() {
        let caps = build_surface_caps(
            &capabilities(vk::Extent2D {
                width: 1,
                height: 1,
            }),
            &[surface_format(vk::Format::B8G8R8A8_SRGB)],
            &[
                vk::PresentModeKHR::MAILBOX,
                vk::PresentModeKHR::MAILBOX,
                vk::PresentModeKHR::SHARED_DEMAND_REFRESH,
            ],
        );
        assert!(caps.supports_present_mode(PresentMode::Fifo));
        assert_eq!(
            caps.present_modes
                .iter()
                .filter(|mode| **mode == PresentMode::Mailbox)
                .count(),
            1
        );
        assert_eq!(
            caps.choose_present_mode(&[PresentMode::Mailbox, PresentMode::Fifo]),
            PresentMode::Mailbox
        );
    }

    #[test]
    fn a_driver_offering_no_composite_alpha_still_gets_opaque() {
        let mut raw = capabilities(vk::Extent2D {
            width: 1,
            height: 1,
        });
        raw.supported_composite_alpha = vk::CompositeAlphaFlagsKHR::empty();
        let caps = build_surface_caps(
            &raw,
            &[surface_format(vk::Format::B8G8R8A8_SRGB)],
            &[vk::PresentModeKHR::FIFO],
        );
        assert_eq!(caps.composite_alpha, vec![CompositeAlpha::Opaque]);
    }

    /// The registry above this crate falls through on `NoLoader`, so the error
    /// must stay distinguishable after the conversion into `HalError`.
    #[test]
    fn open_errors_survive_conversion_with_their_reason_intact() {
        let error: HalError = OpenError::NoLoader("libvulkan.so.1: not found".to_string()).into();
        assert!(error.to_string().contains("no Vulkan loader"), "{error}");
        let error: HalError = OpenError::ApiTooOld { major: 1, minor: 2 }.into();
        assert!(error.to_string().contains("1.2"), "{error}");
        assert!(error.to_string().contains("1.3"), "{error}");
    }
}
