//! The Metal [`Instance`] implementation — adapter enumeration, device
//! creation, and the refusals that still name themselves.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crcbl_core::Pool;
use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, DeviceDesc, DeviceRequestState, HalError, Instance,
    PendingDevice, SurfaceCaps, SurfaceHandle, SurfaceTarget,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLCopyAllDevices, MTLDevice};

use crate::adapter;
use crate::device::{MetalDevice, Owner, device_tag, lookup};
use crate::swapchain::SurfaceEntry;

/// Process-wide source of owner ids.
///
/// `crcbl-hal`'s [`device`](crcbl_hal::device) obligation 3 obliges every
/// backend to stamp an owner identity into its own side table, because a
/// [`Handle`](crcbl_core::Handle) has no room for one and two devices genuinely
/// do issue identical bits. A counter is enough, and is cheaper to compare than
/// an object pointer — which on Metal would additionally be the wrong key,
/// since `MTLCopyAllDevices` may hand back the same `MTLDevice` object to two
/// instances.
static NEXT_OWNER_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_owner_id() -> u64 {
    NEXT_OWNER_ID.fetch_add(1, Ordering::Relaxed)
}

/// One enumerated adapter: what the seam was told about it, and the object it
/// was read from.
///
/// MTL1 kept only the [`AdapterInfo`] and dropped the `MTLDevice`, because
/// nothing needed it. [`Instance::request_device`] needs it — `MTLDevice` *is*
/// the thing a Metal device is opened from, there being no separate
/// `vkCreateDevice` step — so it is kept now that there is a caller.
pub(crate) struct AdapterRecord {
    pub(crate) raw: Retained<ProtocolObject<dyn MTLDevice>>,
    pub(crate) info: AdapterInfo,
}

impl core::fmt::Debug for AdapterRecord {
    /// The `MTLDevice`'s own `description` is long and says nothing the
    /// [`AdapterInfo`] does not, so only the latter is printed.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AdapterRecord")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

/// Everything a device needs to keep alive from the instance that made it.
///
/// `crcbl-hal`'s **obligation 1**: a `Device` may outlive its `Instance`, and
/// the backend *must* keep the instance-level state alive internally rather
/// than borrowing it. This is that state, and [`MetalDevice`] holds an
/// [`Arc`] of it, so dropping the public [`MetalInstance`] while a device is
/// open releases nothing the device is still using.
///
/// On Metal the state in question is the `MTLDevice` objects themselves. That
/// makes the obligation cheaper to honour here than in Vulkan — there is no
/// instance object with a destruction order to respect — but not free: an
/// `MTLDevice` outliving its `MTLCommandQueue` is the ordering that matters,
/// and it is guaranteed by the `Arc` rather than by a rule anyone has to
/// follow.
///
/// Obligation 3 splits ownership two ways — surfaces are checked against the
/// *instance*, everything else against the *device* — so the surface slice
/// brought the instance's own id and tag with the surfaces they identify. They
/// are the same pair a device carries and are compared the same way; see
/// `crcbl_mtl::device`'s handle-tagging section for why the owner id alone is
/// not enough.
pub(crate) struct InstanceInner {
    pub(crate) adapters: Vec<AdapterRecord>,
    pub(crate) id: u64,
    /// This instance's stamp on every surface handle it issues. Never zero.
    tag: u32,
    /// The surfaces this instance has issued. One lock, for the reason
    /// `crcbl_mtl::device` gives for its own: a surface is created once per
    /// window, so there is no contention to design around and a second lock
    /// would only add an ordering to get wrong.
    surfaces: Mutex<Pool<SurfaceEntry>>,
}

// SAFETY: `Instance` requires `HalThreadSafe`, and this type held only
// `MTLDevice` objects — declared `Send + Sync` upstream — until the surface
// slice put a `CAMetalLayer` in `surfaces`. The argument for that object is the
// one `crcbl_mtl::device`'s marker impls make in full: every access is under
// this `Mutex`, the only selectors this crate ever sends a layer are the
// Metal-facing ones, no `NSView`/`NSWindow`/`NSScreen` is ever reached from
// here, and Objective-C reference counting is atomic. It is deliberately not
// widened past that — `Instance::create_surface` still requires the window's
// own thread, and this crate does not claim otherwise.
unsafe impl Send for InstanceInner {}
// SAFETY: as above.
unsafe impl Sync for InstanceInner {}

impl core::fmt::Debug for InstanceInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InstanceInner")
            .field("id", &self.id)
            .field("adapters", &self.adapters.len())
            .finish_non_exhaustive()
    }
}

impl Owner for InstanceInner {
    fn owner_id(&self) -> u64 {
        self.id
    }

    fn tag(&self) -> u32 {
        self.tag
    }
}

impl InstanceInner {
    pub(crate) fn surfaces(&self) -> MutexGuard<'_, Pool<SurfaceEntry>> {
        self.surfaces
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether `surface` is one this instance issued, without saying anything
    /// about what it is.
    ///
    /// [`Instance::request_device`]'s
    /// [`compatible_surface`](DeviceDesc::compatible_surface) is the only
    /// caller: on Metal every device can present to every surface (see
    /// `crcbl_mtl::swapchain`), so the question is purely obligation 3's — is
    /// this handle ours — and the answer is [`lookup`]'s three-way one.
    fn check_surface(&self, surface: SurfaceHandle) -> Result<(), HalError> {
        let surfaces = self.surfaces();
        lookup(&surfaces, "surface", surface, self).map(|_| ())
    }
}

/// Every Metal device on the machine, described before any of them is opened.
///
/// Holds the enumerated [`AdapterInfo`] and the `MTLDevice` each was read from,
/// behind an [`Arc`] shared with every [`MetalDevice`] opened from it. See
/// `InstanceInner` for the lifetime obligation that shape discharges, and the
/// crate docs for why none of it needs an `unsafe` marker.
#[derive(Debug)]
pub struct MetalInstance {
    pub(crate) inner: Arc<InstanceInner>,
}

impl MetalInstance {
    /// Enumerates every Metal device, filling in each one's capabilities.
    ///
    /// `None` when the system reports no Metal device at all — the case a
    /// backend registry falls through on, and the same shape
    /// `crcbl_wgpu::WgpuInstance::new_native` uses for it. On a Mac this does
    /// not happen: `MTLCopyAllDevices` returns at least the built-in GPU.
    ///
    /// Enumeration is `MTLCopyAllDevices` rather than
    /// `MTLCreateSystemDefaultDevice`, and both halves of that matter. The seam
    /// requires a caller to see *every* adapter and pick — `surface_caps` is
    /// documented as the only way to learn which of them can present to a given
    /// window, so a single "default" device is not a shortcut but a bug on the
    /// second machine. And Apple documents `MTLCopyAllDevices` as leaving the
    /// choice of GPU to the application without switching devices, which is
    /// exactly the property enumeration needs: asking what exists must not
    /// change what is in use.
    #[must_use]
    pub fn open() -> Option<Self> {
        let driver = adapter::driver_string();
        let devices = MTLCopyAllDevices();
        let adapters: Vec<AdapterRecord> = devices
            .iter()
            .enumerate()
            // `info` is initialised first on purpose: it borrows the device to
            // read it, and `raw` then moves the same `Retained` in.
            .map(|(index, device)| AdapterRecord {
                info: adapter::adapter_info(index as u32, &device, driver.clone()),
                raw: device,
            })
            .collect();

        if adapters.is_empty() {
            crcbl_core::log::warn!("crcbl-mtl: the system reports no Metal device");
            return None;
        }
        let id = next_owner_id();
        Some(Self {
            inner: Arc::new(InstanceInner {
                adapters,
                id,
                tag: device_tag(id),
                surfaces: Mutex::new(Pool::new()),
            }),
        })
    }

    /// The refusal this backend hands back for a slice that has not arrived,
    /// with `what` naming that slice.
    ///
    /// One constructor rather than a literal per call site so every entry point
    /// refuses in the same voice, and so the reader can see at a glance which
    /// ones still do.
    pub(crate) fn not_yet(what: &'static str) -> HalError {
        Self::unsupported(what)
    }

    /// The refusal for something **Metal itself** has not got, with `what`
    /// naming the API that is missing.
    ///
    /// The same variant as [`not_yet`](Self::not_yet) and a different sentence,
    /// which is the split `Device::supports` already draws for this crate: "the
    /// byte-wide fill is the API, and no slice will change it" is not "the mesh
    /// slice is owed". Both are [`HalError::Unsupported`] because a caller does
    /// the same thing with either — take the fallback — and neither is
    /// [`HalError::InvalidDescriptor`], which is reserved for a descriptor field
    /// the caller can correct.
    pub(crate) fn unsupported(what: &'static str) -> HalError {
        HalError::Unsupported {
            backend: BackendKind::Metal,
            what,
        }
    }

    /// Opens a device, returning this crate's own type.
    ///
    /// [`Instance::request_device`] wraps this in a [`PendingDevice`]; the
    /// crate's tests call it directly, because a `Box<dyn Device>` cannot be
    /// asked about the pools underneath it.
    pub(crate) fn open_device(&self, desc: &DeviceDesc<'_>) -> Result<MetalDevice, HalError> {
        let Some(record) = self
            .inner
            .adapters
            .iter()
            .find(|record| record.info.id == desc.adapter)
        else {
            return Err(HalError::NoSuchAdapter(desc.adapter.0));
        };
        // Everything decidable now is decided now, per `request_device`'s
        // contract, and in the order the seam lists: the adapter first, then
        // the features, then the surface.
        let missing = record.info.caps.missing(desc.required_features);
        if !missing.is_empty() {
            return Err(HalError::UnsupportedFeatures { missing });
        }
        if let Some(surface) = desc.compatible_surface {
            // Obligation 3's instance half, and the whole of what this check
            // can be on Metal: a surface from *this* instance is compatible
            // with every device opened from it, because presenting is
            // `setDevice:` and there is no per-device support query to fail.
            // What is left is telling a stale handle from another instance's,
            // which is exactly what `lookup` answers.
            self.inner.check_surface(surface)?;
        }
        MetalDevice::open(Arc::clone(&self.inner), record, desc)
    }
}

impl Instance for MetalInstance {
    fn backend(&self) -> BackendKind {
        BackendKind::Metal
    }

    fn adapters(&self) -> Vec<AdapterInfo> {
        self.inner
            .adapters
            .iter()
            .map(|record| record.info.clone())
            .collect()
    }

    /// Creates a surface from a `CAMetalLayer`, or from nothing at all for
    /// [`SurfaceTarget::Offscreen`]. See `crcbl_mtl::swapchain`.
    ///
    /// # Safety
    ///
    /// The trait's contract, discharged in `crcbl_mtl::swapchain`'s
    /// `create_surface_impl`, which is where the `CAMetalLayer` pointer is
    /// actually retained.
    unsafe fn create_surface(&self, target: &SurfaceTarget) -> Result<SurfaceHandle, HalError> {
        // SAFETY: forwarded verbatim — this function adds no assumption of its
        // own, and the callee documents the same three obligations the trait
        // does.
        unsafe { self.create_surface_impl(target) }
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        self.destroy_surface_impl(surface);
    }

    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        self.surface_caps_impl(surface, adapter)
    }

    /// Opens the device *now* and hands it over on the first poll.
    ///
    /// Metal device creation is synchronous — the `MTLDevice` already exists
    /// from enumeration and `newCommandQueue` returns before this call does —
    /// so there is nothing to wait for and this backend does not pretend
    /// otherwise. The seam is poll-shaped because WebGPU's `requestDevice` is a
    /// promise (see `crcbl_hal::device`); `crcbl-vk` completes on its first
    /// poll for the same reason, and so does this.
    fn request_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn PendingDevice>, HalError> {
        let device = self.open_device(desc)?;
        Ok(Box::new(MetalPendingDevice {
            device: Some(Box::new(device)),
        }))
    }
}

/// A Metal device request — already finished before it is returned.
///
/// See [`Instance::request_device`] above for why this exists at all on a
/// backend whose device creation is synchronous.
#[derive(Debug)]
struct MetalPendingDevice {
    /// `None` once handed over, so a second poll is the caller bug it is rather
    /// than a second device.
    device: Option<Box<dyn crcbl_hal::Device>>,
}

impl PendingDevice for MetalPendingDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Metal
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crcbl_hal::{BindingModel, Features, GeometryPath, LightingPath, Limits};

    /// Opening the backend, with the failure loud.
    ///
    /// Every test below goes through this rather than tolerating `None`: a run
    /// on a machine with no Metal device must fail the suite, because a suite
    /// that skips itself into green is how "compile-verified only" backends
    /// happen — the trap `docs/plan/09-backends-metal-dx12.md` names in its
    /// risk list.
    pub(crate) fn open() -> MetalInstance {
        MetalInstance::open().expect("a Mac reports at least one Metal device")
    }

    /// A device request this backend can actually satisfy today.
    ///
    /// [`DeviceDesc::for_adapter`] requires compute and a timeline semaphore
    /// and asks for the rest optionally, so it opens here too — see
    /// `the_default_device_desc_opens_and_the_rest_degrades` below. This helper
    /// requires nothing at all, which is what the tests below are checking.
    pub(crate) fn desc(adapter: AdapterId) -> DeviceDesc<'static> {
        DeviceDesc {
            label: Some("crcbl-mtl test device"),
            adapter,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: None,
        }
    }

    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn enumeration_finds_at_least_one_metal_device() {
        let adapters = open().adapters();
        assert!(
            !adapters.is_empty(),
            "MTLCopyAllDevices returned nothing on a machine that has a GPU"
        );
    }

    /// Every adapter identifies itself. Asserted per adapter, after the list is
    /// known non-empty — a loop over an empty vector passes whatever the body
    /// says, so the emptiness check is its own assertion above rather than an
    /// assumption here.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn every_metal_adapter_names_itself_its_driver_and_its_backend() {
        let adapters = open().adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        for adapter in &adapters {
            assert_eq!(adapter.backend, BackendKind::Metal, "{adapter:?}");
            assert!(
                !adapter.name.is_empty(),
                "MTLDevice::name came back empty: {adapter:?}"
            );
            assert!(!adapter.driver.is_empty(), "no driver string: {adapter:?}");
        }
    }

    /// [`AdapterId`] is documented as the position in the enumeration, and
    /// `request_device` resolves it by search — so a mismatch would make every
    /// id but the first name the wrong GPU on a dual-GPU Mac.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn adapter_ids_are_their_positions_in_the_metal_device_list() {
        let adapters = open().adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        for (index, adapter) in adapters.iter().enumerate() {
            assert_eq!(adapter.id, AdapterId(index as u32), "{adapter:?}");
        }
    }

    /// The limits that are read off the device really were read off it, and the
    /// ones keyed off a feature agree with the feature.
    ///
    /// The first two checks are what make a fabricated `Limits` red, and the
    /// second is doing the work: `>=` the floor would pass for
    /// [`Limits::minimum`] *and* for [`Limits::desktop`], since both sit at or
    /// above it. **Equality of the two buffer ranges is the thing only this
    /// backend produces** — `limits_of` fills both from one `maxBufferLength`
    /// because Metal has no uniform/storage distinction, while every preset in
    /// the seam pairs a large storage range with a 64 KiB uniform one. Strictly
    /// greater than the floor then catches an `NSUInteger` conversion that
    /// collapsed, which equality alone would not.
    ///
    /// The last pair is the invariant `crcbl-wgpu` learnt the hard way: a
    /// bindless capacity without the bindless feature, or a push-constant budget
    /// without push constants, is a promise no call can keep.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn reported_limits_come_from_metal_and_agree_with_the_features() {
        let adapters = open().adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        let floor = Limits::minimum();
        for adapter in &adapters {
            let limits = adapter.caps.limits;
            assert_eq!(
                limits.max_storage_buffer_range, limits.max_uniform_buffer_range,
                "both ranges are one maxBufferLength, so a preset leaked in: {adapter:?}"
            );
            assert!(
                limits.max_storage_buffer_range > floor.max_storage_buffer_range,
                "maxBufferLength did not clear the seam's floor: {adapter:?}"
            );
            assert!(
                limits.max_sample_count.is_power_of_two() && limits.max_sample_count <= 64,
                "a sample count is a mask underneath: {adapter:?}"
            );
            assert!(
                limits
                    .max_compute_workgroup_size
                    .iter()
                    .all(|&size| size > 0),
                "maxThreadsPerThreadgroup came back zero: {adapter:?}"
            );

            let bindless = adapter
                .caps
                .features
                .contains(Features::DESCRIPTOR_INDEXING);
            assert_eq!(
                bindless,
                limits.max_bindless_descriptors > 0,
                "bindless capacity and the bindless feature disagree: {adapter:?}"
            );
            assert_eq!(
                adapter.caps.features.contains(Features::PUSH_CONSTANTS),
                limits.max_push_constant_size > 0,
                "the push-constant budget and the push-constant feature disagree: {adapter:?}"
            );
            assert_eq!(
                adapter.caps.features.contains(Features::SAMPLER_ANISOTROPY),
                limits.max_sampler_anisotropy > 1.0,
                "the anisotropy cap and the anisotropy feature disagree: {adapter:?}"
            );
        }
    }

    /// A device now opens, and arrives through exactly the request/poll pair
    /// the seam specifies — including the second poll being a caller bug rather
    /// than a second device.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_metal_device_opens_on_the_first_poll_and_only_once() {
        let instance = open();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");

        let mut pending = instance
            .request_device(&desc(adapters[0].id))
            .expect("a Metal device opens with no required features");
        assert_eq!(pending.backend(), BackendKind::Metal);

        let device = pending
            .poll()
            .expect("Metal device creation is synchronous")
            .into_device()
            .expect("the first poll must complete a synchronous backend");
        assert_eq!(device.backend(), BackendKind::Metal);

        let error = pending
            .poll()
            .expect_err("the device was already handed over");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// The seam's convenience constructor requires only compute and a timeline
    /// semaphore, both of which this backend reports — so `for_adapter`
    /// **opens** on real hardware and everything else it asks for degrades.
    ///
    /// This is the case topic 39 was written for: Metal used to be refused
    /// outright over `DRAW_INDIRECT_COUNT`, a flag absent from the API rather
    /// than unimplemented, while having the rest of the set.
    ///
    /// **The gap is now reported rather than refused, and this test is what
    /// records where it sits.** [`Features::MULTI_DRAW_INDIRECT`] is reported,
    /// because `crcbl_mtl::draw`'s loop is the call behind it;
    /// [`Features::DESCRIPTOR_INDEXING`] has been *withdrawn*, because bind
    /// groups bind Metal's flat argument tables and refuse every bindless
    /// layout; and [`Features::DRAW_INDIRECT_COUNT`] was never reported and
    /// still cannot be. All three are asserted rather than only the absent
    /// ones, so a change in either direction fails here.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn the_default_device_desc_opens_and_the_rest_degrades() {
        let instance = open();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");

        let mut pending = instance
            .request_device(&DeviceDesc::for_adapter(adapters[0].id))
            .expect("compute and a timeline semaphore are all the headless default requires");
        let device = pending
            .poll()
            .expect("Metal device creation is synchronous")
            .into_device()
            .expect("the first poll must complete a synchronous backend");

        let caps = device.caps();
        assert!(
            !caps.supports(Features::DRAW_INDIRECT_COUNT),
            "a GPU-side draw count is what Metal cannot express at all: {:?}",
            caps.features
        );
        assert!(
            !caps.supports(Features::DESCRIPTOR_INDEXING),
            "bind groups are flat argument tables, so bindless stays withdrawn: {:?}",
            caps.features
        );
        assert!(
            caps.supports(Features::MULTI_DRAW_INDIRECT),
            "the indirect loop earns this one: {:?}",
            caps.features
        );

        // The paths those absences select, named rather than implied: this is
        // what the renderer would record on this device today.
        assert_eq!(caps.geometry_path(), GeometryPath::IndirectPerBatch);
        assert_eq!(caps.binding_model(), BindingModel::ArrayPages);
        assert_eq!(caps.lighting_path(), LightingPath::Rasterised);
    }

    /// An out-of-range adapter is a distinct contract from a feature gap, and
    /// it must not be swallowed by one.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn an_unknown_metal_adapter_is_refused_as_such() {
        let instance = open();
        let past_the_end = AdapterId(instance.adapters().len() as u32);

        let error = instance
            .request_device(&desc(past_the_end))
            .expect_err("there is no adapter one past the last");
        assert!(
            matches!(error, HalError::NoSuchAdapter(id) if id == past_the_end.0),
            "{error:?}"
        );
    }

    /// A device asked to be compatible with a handle nobody issued is refused
    /// on the surface, not on the device.
    ///
    /// The surface slice made the other half of this real —
    /// `a_device_opens_against_its_own_instances_surface` in
    /// `crcbl_mtl::swapchain` is the case where a live surface is accepted — so
    /// what is left here is the handle that never resolved.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_compatible_surface_is_refused_by_metal_as_an_unresolvable_handle() {
        let instance = open();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");

        let mut with_surface = desc(adapters[0].id);
        // `Handle::from_bits` is the only way to build a handle no instance
        // issued, and it carries no owner tag, so it is stale rather than
        // foreign.
        with_surface.compatible_surface =
            Some(crcbl_core::Handle::from_bits(1 << 32).expect("generation 1 is non-zero"));

        let error = instance
            .request_device(&with_surface)
            .expect_err("no instance issued that handle");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "surface"),
            "{error:?}"
        );
    }

    /// The targets this backend has no window system for are refused by name,
    /// and the refusal says which backend is speaking.
    ///
    /// `Offscreen` and `AppKit` are **not** here any more — the surface slice
    /// implements both, and `crcbl_mtl::swapchain`'s tests cover them. What is
    /// left is the three platforms Metal genuinely is not, and a wildcard arm
    /// added to `create_surface_impl` would make this pass while quietly
    /// swallowing a new one.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn the_platforms_metal_is_not_are_refused_by_name() {
        let instance = open();
        let dangling = core::ptr::NonNull::dangling();
        let elsewhere = [
            SurfaceTarget::Wayland {
                display: dangling,
                surface: dangling,
            },
            SurfaceTarget::Xcb {
                connection: dangling,
                window: 1,
                visual_id: 2,
            },
            SurfaceTarget::Win32 {
                hinstance: dangling,
                hwnd: dangling,
            },
            SurfaceTarget::Web { canvas_id: 0 },
        ];
        for target in elsewhere {
            // SAFETY: every arm below is refused before the target is read —
            // `create_surface_impl` matches on the variant and returns — so the
            // dangling pointers are never dereferenced. That is what makes it
            // safe to name a platform this backend does not implement.
            let error = unsafe { instance.create_surface(&target) }
                .expect_err("Metal has no window system but macOS's");
            let text = error.to_string();
            assert!(text.contains("metal"), "{target:?}: {text}");
            assert!(text.contains("surfaces"), "{target:?}: {text}");
        }
    }
}
