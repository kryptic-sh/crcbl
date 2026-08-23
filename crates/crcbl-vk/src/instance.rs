//! [`VkInstance`]: the loader, the instance, adapter enumeration and surfaces.

use core::ffi::{CStr, c_void};
use std::sync::{Arc, Mutex, MutexGuard};

use ash::{ext, khr, vk};

use crcbl_core::{Pool, SurfaceTarget};
use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, CompositeAlpha, Device, DeviceDesc, DeviceRequestState,
    Format, HalError, Instance, PresentMode, SurfaceCaps, SurfaceHandle,
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
/// [`Handle`](crcbl_core::Handle) has no room for one and two instances
/// genuinely do issue identical bits. A counter is enough and is cheaper to
/// compare than a `VkInstance` pointer.
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
///
/// Obligation 2b — defer `vkDestroySurfaceKHR` until the last swapchain on the
/// surface is gone — lives here as **pure bookkeeping**, separately from the
/// driver call it decides on. That split is what makes it testable at all: the
/// only surface this crate's e2e suite can create is
/// [`SurfaceTarget::Offscreen`], which
/// has no driver object to defer, so a headless run cannot reach the case whose
/// failure mode is undefined behaviour rather than a wrong error code.
#[derive(Debug, Default)]
struct Surfaces {
    live: Pool<SurfaceEntry>,
    /// Destroyed by the caller, still referenced by a swapchain. See
    /// `crcbl-hal`'s obligation 2: "the handle dies when the caller says so,
    /// the object dies when it is safe".
    zombies: Vec<SurfaceEntry>,
}

impl Surfaces {
    /// Issues a handle for an entry this instance just inserted, tagged with
    /// the instance that owns it.
    ///
    /// The tag is what makes obligation 3 checkable at all here. Every
    /// instance's surface pool is its own, so two `VkInstance`s hand out
    /// *identical* bits — index 0, generation 1, both of them — and an untagged
    /// lookup in the other one's pool then resolves to a real, wrong surface
    /// with a matching `owner`. It did: instance A accepted instance B's handle
    /// in `surface_caps` and freed its own surface when given it to destroy.
    fn insert(&mut self, entry: SurfaceEntry) -> SurfaceHandle {
        let owner = entry.owner;
        let slot = self.live.insert(entry);
        crate::device::stamp(crate::device::owner_tag(owner), slot, "surface")
    }

    /// Decodes a handle into this instance's own pool index, or says why it is
    /// not one of ours.
    fn local(
        &self,
        surface: SurfaceHandle,
        owner: u64,
    ) -> Result<crcbl_core::Handle<SurfaceEntry>, HalError> {
        let tag = crate::device::handle_tag(surface);
        if tag == crate::device::owner_tag(owner) {
            return Ok(crate::device::untag(surface));
        }
        // Tag zero was never issued by any instance — a hand-made handle, or
        // one whose pool index overflowed the tagged range.
        Err(if tag == 0 {
            HalError::invalid_handle("surface", surface)
        } else {
            HalError::ForeignObject {
                kind: "surface",
                bits: surface.to_bits(),
            }
        })
    }

    /// Resolves a handle against `owner`, per obligation 3.
    fn raw(&self, surface: SurfaceHandle, owner: u64) -> Result<vk::SurfaceKHR, HalError> {
        let local = self.local(surface, owner)?;
        match self.live.get(local) {
            Some(entry) if entry.owner == owner => Ok(entry.raw),
            Some(_) => Err(HalError::ForeignObject {
                kind: "surface",
                bits: surface.to_bits(),
            }),
            None => Err(HalError::invalid_handle("surface", surface)),
        }
    }

    /// Notes that a swapchain now references `surface`.
    ///
    /// An offscreen surface — which has no driver object — is deliberately not
    /// counted. Counting it would leave a zombie nothing could ever release,
    /// because [`Surfaces::release`] is keyed on the raw handle and every
    /// offscreen surface's is null.
    fn retain(&mut self, surface: SurfaceHandle, owner: u64) -> Result<(), HalError> {
        let local = self.local(surface, owner)?;
        match self.live.get_mut(local) {
            Some(entry) if entry.owner == owner => {
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

    /// Notes that a swapchain no longer references `raw`.
    ///
    /// Returns the driver object the caller must now destroy: `Some` only when
    /// this was the last swapchain on a surface whose *handle* the caller has
    /// already let go.
    fn release(&mut self, raw: vk::SurfaceKHR) -> Option<vk::SurfaceKHR> {
        if raw == vk::SurfaceKHR::null() {
            return None;
        }
        for (_, entry) in self.live.iter_mut() {
            if entry.raw == raw {
                entry.swapchains = entry.swapchains.saturating_sub(1);
                return None;
            }
        }
        let index = self
            .zombies
            .iter()
            .position(|entry| entry.raw == raw && entry.swapchains > 0)?;
        self.zombies[index].swapchains -= 1;
        if self.zombies[index].swapchains == 0 {
            return Some(self.zombies.swap_remove(index).raw);
        }
        None
    }

    /// Invalidates `surface`'s handle immediately, per obligation 2.
    ///
    /// Returns the driver object to destroy *now*, or `None` when a swapchain
    /// still references it and the object must outlive the handle.
    fn destroy(&mut self, surface: SurfaceHandle, owner: u64) -> Option<vk::SurfaceKHR> {
        // Only this instance's surfaces: another instance's handle carries a
        // different tag and, before it did, collided on bits and freed this
        // instance's own surface instead.
        let local = self.local(surface, owner).ok()?;
        if !self
            .live
            .get(local)
            .is_some_and(|entry| entry.owner == owner)
        {
            return None;
        }
        let entry = self.live.remove(local)?;
        if entry.swapchains == 0 {
            return Some(entry.raw);
        }
        // Obligation 2: the handle is dead now, the object is not.
        crcbl_core::log::debug!(
            "crcbl-vk: {} surface handle destroyed with {} swapchain(s) still on it; \
             deferring the driver object",
            entry.platform,
            entry.swapchains
        );
        self.zombies.push(entry);
        None
    }
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
    /// Whether [`crate::VkDevice`]'s `take_error` reports what the layer said,
    /// which is what turns a validation error into a failed run rather than a
    /// log line. Off unless [`debug::FATAL_VALIDATION_ENV_VAR`] asks for it —
    /// see that module's "Failing the run on an error".
    pub(crate) fatal_validation: bool,
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
        self.surfaces().raw(surface, self.id)
    }

    /// Notes that a swapchain now references `surface`.
    pub(crate) fn retain_surface(&self, surface: SurfaceHandle) -> Result<(), HalError> {
        self.surfaces().retain(surface, self.id)
    }

    /// Notes that a swapchain no longer references `raw`, destroying the driver
    /// object if that was the last one and the caller has already let the
    /// handle go.
    pub(crate) fn release_surface(&self, raw: vk::SurfaceKHR) {
        // The lock is dropped before the driver call, not held across it: the
        // decision and the destruction are deliberately separate steps.
        let orphaned = self.surfaces().release(raw);
        if let Some(raw) = orphaned {
            self.destroy_surface_object(raw);
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
            crcbl_core::log::warn!(
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

    /// A run asked for validation to be able to fail it, and the layer that
    /// would do the failing is not installed.
    ///
    /// The one place a missing layer is fatal, and it is fatal because of what
    /// was asked for rather than because the layer is missing: a caller who set
    /// [`debug::FATAL_VALIDATION_ENV_VAR`] said the run's result is only worth
    /// having if a specification violation could have stopped it. Opening
    /// anyway would hand back exactly the green-light-wired-to-nothing that
    /// variable exists to remove — and silently, since the only other signal is
    /// a warning nothing in a CI step reads. Every other caller still gets the
    /// warning and a working engine.
    #[error(
        "{0} is not installed, so {1}=1 cannot be honoured; install it (Arch: \
         vulkan-validation-layers, Debian/Ubuntu: vulkan-validationlayers) or \
         unset the variable"
    )]
    FatalValidationUnavailable(&'static str, &'static str),
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
            crcbl_core::log::warn!(
                "crcbl-vk: {} was requested but is not installed; validation is off",
                debug::VALIDATION_LAYER
            );
        }
        let validation_enabled = want_validation && validation_available;
        if debug::fatal_validation_wanted() && !validation_enabled {
            // **The one case where a missing layer is fatal.** Everywhere else
            // the warning above is the whole answer, because a machine without
            // the layers package must still run the engine. Here the caller has
            // said the run's result is only worth having if a violation could
            // have stopped it, and opening anyway would return a green light
            // wired to nothing — which is what the seven CI steps setting this
            // variable would then be.
            return Err(OpenError::FatalValidationUnavailable(
                debug::VALIDATION_LAYER,
                debug::FATAL_VALIDATION_ENV_VAR,
            ));
        }
        // `&&` rather than the flag alone so that the field answers the question
        // a reader has — "will an error stop this run" — instead of the one the
        // environment answered. The refusal above is what makes the two agree.
        let fatal_validation = validation_enabled && debug::fatal_validation_wanted();

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
            // Not used directly by anything here — it is a *dependency* of the
            // device-level `VK_EXT_present_timing`, which `vk.xml` declares as
            // `VK_KHR_swapchain+VK_KHR_present_id2+VK_KHR_get_surface_capabilities2+VK_KHR_calibrated_timestamps`.
            // An extension chain is not partially satisfiable, so a device that
            // wants present timing needs this one enabled on the instance under
            // it, and instance extensions can only be asked for here.
            khr::get_surface_capabilities2::NAME,
        ] {
            if has_extension(name) {
                extensions.push(name.as_ptr());
                enabled_names.push(name);
            }
        }
        // Enabled exactly when available, by the loop above, so the two are the
        // same question — and it is the *enabled* one the adapter probe needs,
        // because a device extension whose instance-level dependency was never
        // enabled must not be asked for.
        let surface_caps2 = has_extension(khr::get_surface_capabilities2::NAME);
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
            crcbl_core::log::warn!(
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
            crcbl_core::log::info!("crcbl-vk: synchronisation validation enabled");
        }
        crcbl_core::log::debug!(
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
                        crcbl_core::log::warn!(
                            "crcbl-vk: could not create the debug messenger: {error:?}"
                        );
                        vk::DebugUtilsMessengerEXT::null()
                    })
            }
            None => vk::DebugUtilsMessengerEXT::null(),
        };
        if messenger != vk::DebugUtilsMessengerEXT::null() {
            // **The half of `ValidationReport::assert_clean` a shell script can
            // read.** A harness that greps a run's log for validation errors is
            // vacuous unless the callback was wired, and until this line existed
            // the log said nothing either way: the layer being absent produces a
            // warning, a messenger that failed to be created produces another,
            // and a run where everything worked produced neither. Every gate
            // reading this log — `tools/run-samples-windowed.sh` and the two
            // shell e2e harnesses — asserts this line is present before it
            // believes the absence of errors below it. `debug` emits it rather
            // than this module so that one log filter governs the line and the
            // errors it vouches for; see `announce_messenger`.
            debug::announce_messenger();
        }

        let surface_ext =
            has_extension(khr::surface::NAME).then(|| khr::surface::Instance::new(&entry, &raw));
        let wayland_ext = has_extension(khr::wayland_surface::NAME)
            .then(|| khr::wayland_surface::Instance::new(&entry, &raw));
        let xcb_ext = has_extension(khr::xcb_surface::NAME)
            .then(|| khr::xcb_surface::Instance::new(&entry, &raw));

        let adapters = adapter::enumerate(&raw, debug_utils, surface_caps2);
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
            crcbl_core::log::info!(
                "crcbl-vk: adapter {} — {} ({:?}), {}, geometry {:?}, binding {:?}, lighting {:?}",
                record.info.id.0,
                record.info.name,
                record.info.device_type,
                record.info.driver,
                record.info.caps.geometry_path(),
                record.info.caps.binding_model(),
                record.info.caps.lighting_path()
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
                fatal_validation,
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
                return unsupported("a canvas is crcbl-webgpu's target, not Vulkan's");
            }
        };

        let handle: SurfaceHandle = self.inner.surfaces().insert(SurfaceEntry {
            raw,
            owner: self.inner.id,
            platform: target.platform_name(),
            swapchains: 0,
        });
        crcbl_core::log::debug!(
            "crcbl-vk: created a {} surface {:?}",
            target.platform_name(),
            handle
        );
        Ok(handle)
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        let now = self.inner.surfaces().destroy(surface, self.inner.id);
        if let Some(raw) = now {
            self.inner.destroy_surface_object(raw);
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

    /// Opens the device *now* and hands it over on the first poll.
    ///
    /// `vkCreateDevice` is synchronous, so there is nothing to wait for and this
    /// backend does not pretend otherwise: no simulated latency, no deferred
    /// work. The seam is poll-shaped because WebGPU's `requestDevice` is a
    /// promise (see `crcbl_hal::device`); a backend whose creation completes
    /// immediately says so by completing immediately.
    fn request_device(
        &self,
        desc: &DeviceDesc<'_>,
    ) -> Result<Box<dyn crcbl_hal::PendingDevice>, HalError> {
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
        Ok(Box::new(VkPendingDevice {
            device: Some(Box::new(device)),
        }))
    }
}

/// A Vulkan device request — already finished before it is returned.
///
/// See [`Instance::request_device`] above for why this exists at all on a
/// backend with a synchronous device-creation call.
#[derive(Debug)]
struct VkPendingDevice {
    /// `None` once handed over, so a second poll is the caller bug it is rather
    /// than a second device.
    device: Option<Box<dyn Device>>,
}

impl crcbl_hal::PendingDevice for VkPendingDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Vulkan
    }

    fn poll(&mut self) -> Result<DeviceRequestState, HalError> {
        self.device
            .take()
            .map(DeviceRequestState::Ready)
            .ok_or_else(|| {
                HalError::InvalidDescriptor(
                    "this device request already produced its device".to_string(),
                )
            })
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
    // "Best first", per the seam. sRGB first is what the tonemap pass wants,
    // and stable ordering means two drivers listing the same formats in
    // different orders pick the same one. Sorted *before* the dedup, which is
    // the only order in which `Vec::dedup` — which only ever removes
    // consecutive equals — deduplicates anything; the pass that used to run
    // here first was a no-op on an unsorted list.
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

    // `from_raw`, for building a surface handle that stands in for a windowed
    // one. `ash` puts it on a trait rather than the type.
    use ash::vk::Handle as _;

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

    /// A surface with a driver object, which
    /// [`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget::Offscreen) — the
    /// only kind this crate's headless suite can create — never has.
    fn windowed(surfaces: &mut Surfaces, owner: u64, raw: u64) -> SurfaceHandle {
        surfaces.insert(SurfaceEntry {
            raw: vk::SurfaceKHR::from_raw(raw),
            owner,
            platform: "test",
            swapchains: 0,
        })
    }

    /// **Obligation 2b.** `vkDestroySurfaceKHR` with a live swapchain is
    /// undefined behaviour in the driver, so the handle must die when the
    /// caller says so and the object only when the last swapchain on it does.
    ///
    /// Nothing checked this in any backend. It cannot be checked end to end
    /// here either — an offscreen surface has no driver object, so the deferral
    /// never engages for it, which is the case below — so the decision is
    /// separated from the `vkDestroySurfaceKHR` it drives and asserted directly:
    /// `Some(raw)` *is* "destroy it now".
    #[test]
    fn a_surface_with_a_live_swapchain_defers_its_driver_object() {
        let mut surfaces = Surfaces::default();
        let surface = windowed(&mut surfaces, 1, 0xBEEF);

        surfaces.retain(surface, 1).expect("a swapchain takes it");
        surfaces.retain(surface, 1).expect("and a second one");

        // The handle dies immediately, and the object does not.
        assert_eq!(surfaces.destroy(surface, 1), None, "two swapchains hold it");
        assert!(
            surfaces.raw(surface, 1).is_err(),
            "the handle is invalid the moment the caller destroys it"
        );
        assert_eq!(surfaces.zombies.len(), 1);

        // Releasing all but the last changes nothing.
        assert_eq!(surfaces.release(vk::SurfaceKHR::from_raw(0xBEEF)), None);
        assert_eq!(surfaces.zombies.len(), 1, "one swapchain still holds it");

        // The last one hands the object back to be destroyed, exactly once.
        assert_eq!(
            surfaces.release(vk::SurfaceKHR::from_raw(0xBEEF)),
            Some(vk::SurfaceKHR::from_raw(0xBEEF))
        );
        assert!(surfaces.zombies.is_empty());
        assert_eq!(
            surfaces.release(vk::SurfaceKHR::from_raw(0xBEEF)),
            None,
            "a double release must not destroy the object twice"
        );
    }

    /// The other half: with nothing on it, the object goes immediately, and a
    /// swapchain created *after* that cannot resurrect the handle.
    #[test]
    fn a_surface_with_no_swapchain_is_destroyed_at_once() {
        let mut surfaces = Surfaces::default();
        let surface = windowed(&mut surfaces, 1, 0xF00D);
        assert_eq!(
            surfaces.destroy(surface, 1),
            Some(vk::SurfaceKHR::from_raw(0xF00D))
        );
        assert!(surfaces.zombies.is_empty());
        assert!(surfaces.retain(surface, 1).is_err());
        assert_eq!(
            surfaces.destroy(surface, 1),
            None,
            "destroying twice must not hand the same object back twice"
        );
    }

    /// An offscreen surface has no driver object, so it is deliberately never
    /// counted — counting it would park a zombie that
    /// [`Surfaces::release`], which is keyed on the raw handle, could never
    /// release.
    #[test]
    fn an_offscreen_surface_is_never_deferred() {
        let mut surfaces = Surfaces::default();
        let surface = windowed(&mut surfaces, 1, 0);
        surfaces.retain(surface, 1).expect("a ring takes it");
        assert_eq!(surfaces.destroy(surface, 1), Some(vk::SurfaceKHR::null()));
        assert!(
            surfaces.zombies.is_empty(),
            "an offscreen surface must never become a zombie"
        );
        assert_eq!(surfaces.release(vk::SurfaceKHR::null()), None);
    }

    /// **Obligation 3, across two instances**, in the shape it actually occurs:
    /// each instance has its *own* surface pool, so both hand out slot 0 at
    /// generation 1. Before the handles carried an instance tag, one instance's
    /// handle therefore resolved to the other's surface with a matching
    /// `owner` — `surface_caps` answered for the wrong surface, and
    /// `destroy_surface` freed it.
    #[test]
    fn another_instances_surface_is_foreign_and_undestroyable() {
        let mut first = Surfaces::default();
        let mut second = Surfaces::default();
        let mine = windowed(&mut first, 1, 0xABCD);
        let theirs = windowed(&mut second, 2, 0xDCBA);
        assert_ne!(
            mine.to_bits(),
            theirs.to_bits(),
            "two instances' first surfaces sit in the same pool slot, so only \
             the tag can tell them apart"
        );

        assert!(matches!(
            first.raw(theirs, 1),
            Err(HalError::ForeignObject {
                kind: "surface",
                ..
            })
        ));
        assert!(matches!(
            first.retain(theirs, 1),
            Err(HalError::ForeignObject {
                kind: "surface",
                ..
            })
        ));
        assert_eq!(
            first.destroy(theirs, 1),
            None,
            "one instance must not be able to free another's surface — nor, \
             through a bit collision, its own"
        );
        assert!(
            first.raw(mine, 1).is_ok(),
            "and both surfaces are untouched"
        );
        assert!(second.raw(theirs, 2).is_ok());

        // A handle nobody stamped resolves nowhere rather than aliasing slot 0.
        let unstamped: SurfaceHandle =
            crcbl_core::Handle::from_bits(1 << 32).expect("generation 1");
        assert!(matches!(
            first.raw(unstamped, 1),
            Err(HalError::InvalidHandle {
                kind: "surface",
                ..
            })
        ));
    }

    /// **The owner id is what separates two owners whose tags collide**, and
    /// this is the only thing in the workspace that asserts it.
    ///
    /// A tag is one byte of the handle's index half, so ids
    /// [`OWNER_TAG_COUNT`](crate::device::OWNER_TAG_COUNT) apart stamp the same
    /// one — reachable after that many live owners in a process. Past the tag,
    /// `entry.owner == owner` is the whole of the check, and the test above
    /// cannot reach it: its two instances carry different tags, so every lookup
    /// there is refused a step earlier.
    ///
    /// The table below holds two owners' rows, which no real one ever does —
    /// every surface pool belongs to a single `VkInstance`, which is exactly
    /// why the id half is otherwise unexercised. A guard nothing can reach is a
    /// guard nobody can tell is still wired up, so the shared table is built
    /// here on purpose.
    #[test]
    fn a_surface_belongs_to_the_owner_that_filled_it_even_when_two_tags_collide() {
        const MINE: u64 = 1;
        const THEIRS: u64 = MINE + crate::device::OWNER_TAG_COUNT;
        assert_eq!(
            crate::device::owner_tag(MINE),
            crate::device::owner_tag(THEIRS),
            "the premise: these two ids stamp the same tag, so the tag cannot \
             tell them apart and only the id can"
        );

        let mut surfaces = Surfaces::default();
        let mine = windowed(&mut surfaces, MINE, 0xABCD);
        let theirs = windowed(&mut surfaces, THEIRS, 0xDCBA);
        assert_eq!(
            crate::device::handle_tag(mine),
            crate::device::handle_tag(theirs),
            "both handles carry that one tag, so both get past `local`"
        );

        assert!(matches!(
            surfaces.raw(mine, THEIRS),
            Err(HalError::ForeignObject {
                kind: "surface",
                ..
            })
        ));
        assert!(matches!(
            surfaces.retain(mine, THEIRS),
            Err(HalError::ForeignObject {
                kind: "surface",
                ..
            })
        ));
        assert_eq!(
            surfaces.destroy(mine, THEIRS),
            None,
            "and a colliding tag must not let one owner free the other's surface"
        );

        // The half that makes the refusal worth having: the row is still there
        // and still answers its own owner. A `destroy` that removed it first and
        // checked afterwards would pass every assertion above and fail here.
        assert_eq!(
            surfaces.raw(mine, MINE).expect("its owner still holds it"),
            vk::SurfaceKHR::from_raw(0xABCD)
        );
        assert_eq!(
            surfaces.raw(theirs, THEIRS).expect("and so does the other"),
            vk::SurfaceKHR::from_raw(0xDCBA)
        );
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
