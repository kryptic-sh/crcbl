//! The Metal [`Instance`] implementation — adapter enumeration, device
//! creation, and the refusals that still name themselves.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, DeviceDesc, DeviceRequestState, HalError, Instance,
    PendingDevice, SurfaceCaps, SurfaceHandle, SurfaceTarget,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLCopyAllDevices, MTLDevice};

use crate::adapter;
use crate::device::MetalDevice;

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
/// There is no instance-level owner id here. Obligation 3 splits ownership two
/// ways — surfaces are checked against the *instance*, everything else against
/// the *device* — and this instance issues no surfaces, so an instance id would
/// be a field nothing compares. The surface slice adds it along with the
/// surfaces it has to check.
#[derive(Debug)]
pub(crate) struct InstanceInner {
    pub(crate) adapters: Vec<AdapterRecord>,
}

/// Every Metal device on the machine, described before any of them is opened.
///
/// Holds the enumerated [`AdapterInfo`] and the `MTLDevice` each was read from,
/// behind an [`Arc`] shared with every [`MetalDevice`] opened from it. See
/// `InstanceInner` for the lifetime obligation that shape discharges, and the
/// crate docs for why none of it needs an `unsafe` marker.
#[derive(Debug)]
pub struct MetalInstance {
    inner: Arc<InstanceInner>,
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
            log::warn!("crcbl-mtl: the system reports no Metal device");
            return None;
        }
        Some(Self {
            inner: Arc::new(InstanceInner { adapters }),
        })
    }

    /// The refusal this backend hands back for a slice that has not arrived,
    /// with `what` naming that slice.
    ///
    /// One constructor rather than a literal per call site so every entry point
    /// refuses in the same voice, and so the reader can see at a glance which
    /// ones still do.
    pub(crate) fn not_yet(what: &'static str) -> HalError {
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
            // Not `ForeignObject`: this instance has issued no surface at all
            // (`create_surface` refuses), so every handle offered here is one
            // that never resolved rather than one belonging to somebody else.
            return Err(HalError::invalid_handle("surface", surface));
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

    unsafe fn create_surface(&self, _target: &SurfaceTarget) -> Result<SurfaceHandle, HalError> {
        // Nothing is dereferenced, so the trait's safety contract is discharged
        // trivially here — this slice creates no `CAMetalLayer` binding and
        // never reads `target`. The surface slice is where obligation 1 (the
        // handles really are what they say) starts to matter.
        Err(Self::not_yet("surface creation (the Metal surface slice)"))
    }

    fn destroy_surface(&self, _surface: SurfaceHandle) {
        // A no-op that cannot be reached with a live handle: `create_surface`
        // above never returns one, so this instance has issued no surface for a
        // caller to destroy. The signature returns `()` and so has no way to
        // report that, which is precisely why the refusal lives in
        // `create_surface` — a caller cannot get far enough to need one here.
    }

    fn surface_caps(
        &self,
        _surface: SurfaceHandle,
        _adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        // Deliberately refused before either argument is checked. The trait
        // documents `InvalidHandle` for a stale surface and `NoSuchAdapter` for
        // an unknown adapter, and neither branch is reachable: no
        // `SurfaceHandle` exists, so every handle is stale and the adapter check
        // could only ever be answering about a surface that was never made.
        //
        // What must never happen here is the failure the trait calls out by
        // name — reporting a non-presentable adapter as empty `formats` or empty
        // `present_modes`. This slice cannot present at all, so it says so.
        Err(Self::not_yet(
            "surface capability queries (the Metal surface slice)",
        ))
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
    use crcbl_hal::{Features, Limits};

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
    /// [`DeviceDesc::for_adapter`] asks for [`Features::TIER_A`], which this
    /// backend does not report until the command slice picks Metal's indirect
    /// path — see `the_default_device_desc_is_refused_for_the_tier_a_gap`
    /// below, which is the test that pins that.
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
    fn every_adapter_names_itself_its_driver_and_its_backend() {
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
    fn adapter_ids_are_their_enumeration_positions() {
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
    fn reported_limits_come_from_the_device_and_agree_with_the_features() {
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
    fn a_device_opens_on_the_first_poll_and_only_once() {
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

    /// The seam's convenience constructor asks for Tier A, and this backend is
    /// Tier B until the command slice picks Metal's indirect path — so
    /// `for_adapter` is refused, by name, on real hardware.
    ///
    /// This is the visible consequence of the tier decision the crate docs
    /// argue for; if the backend ever starts reporting the two indirect
    /// features, this test says so rather than letting the change go unnoticed.
    #[test]
    fn the_default_device_desc_is_refused_for_the_tier_a_gap() {
        let instance = open();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");

        let error = instance
            .request_device(&DeviceDesc::for_adapter(adapters[0].id))
            .expect_err("this backend does not report Tier A");
        let HalError::UnsupportedFeatures { missing } = error else {
            panic!("expected a feature gap, got {error:?}");
        };
        assert!(
            missing.contains(Features::DRAW_INDIRECT_COUNT | Features::MULTI_DRAW_INDIRECT),
            "the two indirect features are what keeps this backend at Tier B: {missing:?}"
        );
    }

    /// An out-of-range adapter is a distinct contract from a feature gap, and
    /// it must not be swallowed by one.
    #[test]
    fn an_unknown_adapter_is_refused_as_such() {
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

    /// A device asked to be compatible with a surface is refused on the
    /// surface, not on the device — no surface exists to be compatible with.
    #[test]
    fn a_compatible_surface_is_refused_as_an_unresolvable_handle() {
        let instance = open();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");

        let mut with_surface = desc(adapters[0].id);
        // `create_surface` issues nothing, so any handle at all is one that
        // never resolved. `Handle::from_bits` is the only way to build one.
        with_surface.compatible_surface =
            Some(crcbl_core::Handle::from_bits(1 << 32).expect("generation 1 is non-zero"));

        let error = instance
            .request_device(&with_surface)
            .expect_err("no surface handle can resolve on this backend");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "surface"),
            "{error:?}"
        );
    }

    /// Surfaces are refused in the same voice, including the offscreen target
    /// that needs no window at all.
    #[test]
    fn surfaces_are_refused_and_the_refusal_names_metal() {
        let instance = open();
        // SAFETY: `SurfaceTarget::Offscreen` names no platform object, so the
        // trait's obligations about live handles are vacuous for it.
        let error = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect_err("this slice creates no surfaces");
        let text = error.to_string();
        assert!(text.contains("metal"), "{text}");
        assert!(text.contains("surface creation"), "{text}");
    }

    /// Obligation 1, made observable: the device must survive its instance
    /// being dropped, and must still work afterwards.
    #[test]
    fn a_device_outlives_the_instance_that_made_it() {
        let device = {
            let instance = open();
            let adapters = instance.adapters();
            assert!(!adapters.is_empty(), "nothing to check");
            instance
                .create_device(&desc(adapters[0].id))
                .expect("a Metal device opens with no required features")
        };
        // The instance is gone; the `Arc` inside the device is what keeps its
        // `MTLDevice` alive. A buffer round trip is the cheapest proof that the
        // device object is still usable rather than merely still addressable.
        assert_eq!(device.backend(), BackendKind::Metal);
        let buffer = device
            .create_buffer(&crcbl_hal::BufferDesc {
                label: Some("outlives its instance"),
                size: 256,
                usage: crcbl_hal::BufferUsage::STORAGE,
                memory: crcbl_hal::MemoryLocation::HostUpload,
            })
            .expect("the MTLDevice is still live");
        device.destroy_buffer(buffer);
    }
}
