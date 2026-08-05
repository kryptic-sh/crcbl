//! The Metal [`Instance`] implementation — adapter enumeration, and refusals
//! that name themselves.

use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, DeviceDesc, HalError, Instance, PendingDevice,
    SurfaceCaps, SurfaceHandle, SurfaceTarget,
};
use objc2_metal::MTLCopyAllDevices;

use crate::adapter;

/// Every Metal device on the machine, described before any of them is opened.
///
/// Holds owned [`AdapterInfo`] only; the `MTLDevice` objects are read during
/// [`MetalInstance::open`] and dropped there. See the crate docs for why, and
/// for why that has nothing to do with thread safety.
#[derive(Debug)]
pub struct MetalInstance {
    adapters: Vec<AdapterInfo>,
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
        let adapters: Vec<AdapterInfo> = devices
            .iter()
            .enumerate()
            .map(|(index, device)| adapter::adapter_info(index as u32, &device, driver.clone()))
            .collect();

        if adapters.is_empty() {
            log::warn!("crcbl-mtl: the system reports no Metal device");
            return None;
        }
        Some(Self { adapters })
    }

    /// The refusal this slice hands back, with `what` naming the slice the
    /// answer arrives in.
    ///
    /// One constructor rather than four literals so every entry point refuses
    /// in the same voice, and so the reader can see at a glance that all of
    /// them do.
    fn not_yet(what: &'static str) -> HalError {
        HalError::Unsupported {
            backend: BackendKind::Metal,
            what,
        }
    }
}

impl Instance for MetalInstance {
    fn backend(&self) -> BackendKind {
        BackendKind::Metal
    }

    fn adapters(&self) -> Vec<AdapterInfo> {
        self.adapters.clone()
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

    fn request_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn PendingDevice>, HalError> {
        // The adapter check is real and comes first: an unknown adapter is a
        // caller bug this slice can genuinely diagnose today, and the trait
        // makes `NoSuchAdapter` a distinct contract from "the backend cannot do
        // this". Reporting the slice's refusal for an out-of-range id would
        // hide a bug behind a "not yet".
        if !self.adapters.iter().any(|info| info.id == desc.adapter) {
            return Err(HalError::NoSuchAdapter(desc.adapter.0));
        }
        // `desc.required_features` is deliberately *not* checked against the
        // adapter's caps. The refusal below is unconditional, so answering
        // `UnsupportedFeatures` first would blame the adapter for a gap that is
        // this backend's: a caller asking for `Features::TIER_A` would be told
        // its GPU is inadequate when the truth is that no device opens yet.
        Err(Self::not_yet("device creation (the Metal device slice)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{Features, Limits};

    /// Opening the backend, with the failure loud.
    ///
    /// Every test below goes through this rather than tolerating `None`: a run
    /// on a machine with no Metal device must fail the suite, because a suite
    /// that skips itself into green is how "compile-verified only" backends
    /// happen — the trap `docs/plan/09-backends-metal-dx12.md` names in its
    /// risk list.
    fn open() -> MetalInstance {
        MetalInstance::open().expect("a Mac reports at least one Metal device")
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
        }
    }

    /// Opening a device is refused *by name*: the message says which backend
    /// and which slice, so the log line reads "not yet" and not "broken".
    #[test]
    fn opening_a_device_is_refused_and_the_refusal_names_metal() {
        let instance = open();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");

        let error = instance
            .request_device(&DeviceDesc::for_adapter(adapters[0].id))
            .expect_err("this slice creates no devices");
        assert!(
            matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Metal),
            "{error:?}"
        );
        let text = error.to_string();
        assert!(text.contains("metal"), "{text}");
        assert!(text.contains("device creation"), "{text}");
    }

    /// An out-of-range adapter is a distinct contract from "not yet", and it is
    /// one this slice can genuinely satisfy — so it must not be swallowed by
    /// the refusal above.
    #[test]
    fn an_unknown_adapter_is_refused_as_such_not_as_unimplemented() {
        let instance = open();
        let past_the_end = AdapterId(instance.adapters().len() as u32);

        let error = instance
            .request_device(&DeviceDesc::for_adapter(past_the_end))
            .expect_err("there is no adapter one past the last");
        assert!(
            matches!(error, HalError::NoSuchAdapter(id) if id == past_the_end.0),
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
}
