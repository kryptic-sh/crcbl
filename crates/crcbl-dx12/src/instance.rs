//! The Direct3D 12 [`Instance`] implementation — adapter enumeration, the WARP
//! question, device creation, and the refusals that still name themselves.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, DeviceDesc, DeviceRequestState, HalError, Instance,
    PendingDevice, SurfaceCaps, SurfaceHandle, SurfaceTarget,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_ADAPTER_DESC1, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_NOT_FOUND,
    IDXGIAdapter1, IDXGIFactory4,
};

use crate::adapter::{self, RawCaps};
use crate::device::Dx12Device;

/// Process-wide source of owner ids.
///
/// `crcbl-hal`'s [`device`](crcbl_hal::device) obligation 3 obliges every
/// backend to stamp an owner identity into its own side table, because a
/// [`Handle`](crcbl_core::Handle) has no room for one and two devices genuinely
/// do issue identical bits. A counter is enough, and is cheaper to compare than
/// an interface pointer — which on D3D12 would additionally be the wrong key,
/// since two `D3D12CreateDevice` calls on one adapter may hand back the same
/// object.
static NEXT_OWNER_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_owner_id() -> u64 {
    NEXT_OWNER_ID.fetch_add(1, Ordering::Relaxed)
}

/// The refusal this backend hands back for a slice that has not arrived, with
/// `what` naming that slice.
///
/// One constructor rather than a literal per call site so every entry point
/// refuses in the same voice, and so the reader can see at a glance which ones
/// still do.
pub(crate) fn not_yet(what: &'static str) -> HalError {
    HalError::Unsupported {
        backend: BackendKind::Dx12,
        what,
    }
}

/// One enumerated adapter: what the seam was told about it, the raw D3D12
/// answers that were told from, and the DXGI interface it was read through.
///
/// DX1 kept only the first two and dropped the `IDXGIAdapter1`, because nothing
/// needed it. [`Instance::request_device`] needs it — `D3D12CreateDevice` takes
/// an `IDXGIAdapter1` and there is no way to name an adapter without one — so it
/// is kept now that there is a caller. `crcbl-mtl` made and reversed exactly
/// this call about its `MTLDevice` objects.
#[derive(Debug)]
pub(crate) struct AdapterRecord {
    pub(crate) info: AdapterInfo,
    pub(crate) raw: RawCaps,
    pub(crate) adapter: IDXGIAdapter1,
}

impl AdapterRecord {
    /// One line carrying everything this slice was written to measure.
    ///
    /// `docs/backlog.md`'s "Does WARP clear Tier A?" is answered by
    /// `sm66-dynamic-resources`, **not** by `tier`: the tier is B on every
    /// adapter in this slice because four Tier A flags wait on calls this crate
    /// does not make yet, so it says how much of the backend is written and
    /// nothing about the hardware. See the crate docs.
    ///
    /// Every test that can fail on an adapter carries this string in its
    /// message, so a red run publishes the measurement whether or not a green
    /// one was asked to print it.
    pub(crate) fn report(&self) -> String {
        format!(
            "crcbl-dx12 adapter {id} \"{name}\" luid={luid:#018x} type={kind:?} \
             vendor={vendor:#06x} device={device:#06x} ResourceBindingTier={tier} \
             HighestShaderModel={model} sm66-dynamic-resources={dynamic} \
             block-compression={bc} renderer-tier={renderer:?} features={features:?} \
             driver=\"{driver}\"",
            id = self.info.id.0,
            luid = self.raw.luid,
            name = self.info.name,
            kind = self.info.device_type,
            vendor = self.info.vendor_id,
            device = self.info.device_id,
            tier = self.raw.binding_tier.0,
            model = adapter::shader_model_name(self.raw.shader_model),
            dynamic = if self.raw.dynamic_resources() {
                "yes"
            } else {
                "no"
            },
            bc = if self.raw.block_compression {
                "yes"
            } else {
                "no"
            },
            renderer = self.info.caps.tier(),
            features = self.info.caps.features,
            driver = self.info.driver,
        )
    }
}

/// Everything a device needs to keep alive from the instance that made it.
///
/// `crcbl-hal`'s **obligation 1**: a `Device` may outlive its `Instance`, and
/// the backend *must* keep the instance-level state alive internally rather than
/// borrowing it. This is that state, and [`Dx12Device`] holds an [`Arc`] of it,
/// so dropping the public [`Dx12Instance`] while a device is open releases
/// nothing the device is still using.
///
/// On D3D12 that state is the DXGI factory and the adapters. The factory is
/// **not** dead weight: DXGI documents an adapter enumerated from a factory as
/// tied to it, and the swapchain slice needs the same factory to call
/// `CreateSwapChainForHwnd` — so keeping it here is what stops that slice
/// re-creating one and asking DXGI a question about an adapter the new factory
/// never enumerated.
///
/// There is no instance-level owner id here. Obligation 3 splits ownership two
/// ways — surfaces are checked against the *instance*, everything else against
/// the *device* — and this instance issues no surfaces, so an instance id would
/// be a field nothing compares. The swapchain slice adds it along with the
/// surfaces it has to check.
#[derive(Debug)]
pub(crate) struct InstanceInner {
    /// Held for the reasons above; nothing in this slice reads it back.
    _factory: IDXGIFactory4,
    pub(crate) adapters: Vec<AdapterRecord>,
}

/// Every D3D12 adapter on the machine, described before any of them is opened.
///
/// Holds owned [`AdapterInfo`], the raw D3D12 answers behind it, and the DXGI
/// factory and adapters — behind an [`Arc`] shared with every `Dx12Device`
/// opened from it. See `InstanceInner` for the lifetime obligation that shape
/// discharges, and the crate docs for why none of it needs an `unsafe` marker.
#[derive(Debug)]
pub struct Dx12Instance {
    inner: Arc<InstanceInner>,
}

impl Dx12Instance {
    /// Enumerates every D3D12 adapter, filling in each one's capabilities.
    ///
    /// `None` when DXGI will not start or when no adapter it lists can be opened
    /// as a D3D12 device at all — the case a backend registry falls through on,
    /// and the same shape `crcbl_mtl::MetalInstance::open` and
    /// `crcbl_wgpu::WgpuInstance::new_native` use for it. On a stock Windows
    /// this does not happen: WARP ships with the OS.
    ///
    /// # WARP is asked for by name, and the ordinary enumeration skips software
    ///
    /// `IDXGIFactory1::EnumAdapters1` already lists a software adapter on a
    /// stock Windows — "Microsoft Basic Render Driver", which *is* WARP — so
    /// enumerating it and then appending
    /// [`IDXGIFactory4::EnumWarpAdapter`](IDXGIFactory4) would list one
    /// rasteriser twice under two ids. The first pass therefore skips whatever
    /// `crcbl_dx12::adapter`'s `is_software` calls software — the one place that
    /// question is asked — and the second asks for WARP by name, which is also
    /// what gets it on machines where DXGI does *not* list it, the whole reason
    /// `EnumWarpAdapter` exists as a separate call.
    ///
    /// **`DXGI_ADAPTER_FLAG_SOFTWARE` alone is not enough, and CI is the
    /// evidence.** On a runner with no GPU, DXGI lists the Basic Render Driver
    /// with that flag *clear*, so a pass keyed on the flag admitted a software
    /// rasteriser as hardware and reported it as `Integrated`. `is_software`
    /// consults Microsoft's own vendor and device ids as well, which is what
    /// keeps such an entry out of the hardware pass and out of
    /// [`DeviceType::Integrated`](crcbl_hal::DeviceType::Integrated).
    ///
    /// The append separately compares `AdapterLuid`, the only identity DXGI
    /// guarantees — unique per adapter for the lifetime of the system, and equal
    /// across two interfaces onto one. It is what stops `EnumWarpAdapter`
    /// re-listing an adapter the first pass already kept. **It does not collapse
    /// the runner's two Basic Render Driver entries and cannot**: they carry
    /// different LUIDs, so DXGI considers them two adapters. Skipping both as
    /// software and appending WARP once by name is what leaves one entry there.
    /// Name and ids are no substitute for the LUID either way: two distinct
    /// cards of one model share all of them.
    ///
    /// The cost is that WARP is always last, so its [`AdapterId`] moves when a
    /// GPU is added or removed. That is true of every adapter id in every
    /// backend — the seam documents an id as a position in *this* enumeration —
    /// and a caller that wants the software rasteriser looks for
    /// [`DeviceType::Cpu`](crcbl_hal::DeviceType::Cpu), which is what this
    /// backend reports for it.
    #[must_use]
    pub fn open() -> Option<Self> {
        // `CreateDXGIFactory2` rather than `CreateDXGIFactory1`: they differ
        // only in the flags word, and `DXGI_CREATE_FACTORY_DEBUG` is where the
        // debug-layer slice will have to reach. Zero here, because turning the
        // debug layer on without also installing a message callback would put
        // driver output somewhere nobody in this workspace reads.
        //
        // SAFETY: the call takes no pointer of ours and writes only the
        // interface it returns; `IDXGIFactory4` is the IID it is asked for, so
        // the QI either succeeds or the call fails.
        let factory: IDXGIFactory4 =
            match unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS::default()) } {
                Ok(factory) => factory,
                Err(error) => {
                    log::warn!("crcbl-dx12: DXGI would not start: {error}");
                    return None;
                }
            };

        let mut adapters: Vec<AdapterRecord> = Vec::new();
        // The id is the position in *this* vector, so it advances only when an
        // adapter is actually kept. Counting the DXGI index instead would leave
        // a hole wherever `describe` answered `None`, and every id after it
        // would name the wrong adapter.
        let mut next_id = 0u32;
        for (raw_adapter, desc) in candidates(&factory) {
            let Some((info, raw)) = adapter::describe(AdapterId(next_id), &raw_adapter, &desc)
            else {
                continue;
            };
            let record = AdapterRecord {
                info,
                raw,
                adapter: raw_adapter,
            };
            // The measurement, emitted by the backend itself rather than only by
            // its tests: an application with a logger installed gets the line
            // that answers `docs/backlog.md`'s question without anyone running a
            // test suite, and `info` is the level adapter enumeration belongs at
            // — this runs once per process and produces one line per adapter.
            log::info!("{}", record.report());
            adapters.push(record);
            next_id += 1;
        }

        if adapters.is_empty() {
            log::warn!("crcbl-dx12: DXGI lists no adapter D3D12 will open, not even WARP");
            return None;
        }
        Some(Self {
            inner: Arc::new(InstanceInner {
                _factory: factory,
                adapters,
            }),
        })
    }

    /// The enumerated records, raw D3D12 answers included.
    ///
    /// [`Instance::adapters`] hands back [`AdapterInfo`] alone, which is all the
    /// seam has vocabulary for. This crate's tests need the numbers underneath
    /// it — the binding tier and the shader model — because checking a derived
    /// flag against other derived flags cannot tell a right derivation from a
    /// wrong one.
    ///
    /// `#[cfg(test)]` because that is the whole of its audience: nothing this
    /// backend ships reads a `RawCaps`, and a `pub(crate)` accessor with no
    /// caller outside the test module would be dead code the lint gate is right
    /// to object to.
    #[cfg(test)]
    pub(crate) fn records(&self) -> &[AdapterRecord] {
        &self.inner.adapters
    }

    /// Opens a device, returning this crate's own type.
    ///
    /// [`Instance::request_device`] wraps this in a [`PendingDevice`]; the
    /// crate's tests call it directly, because a `Box<dyn Device>` cannot be
    /// asked about the pools underneath it.
    ///
    /// The order of the three refusals is the seam's, and it is deliberate: the
    /// adapter first, then the features, then the surface. Answering
    /// `UnsupportedFeatures` for an adapter that does not exist would blame the
    /// hardware for a caller's index bug.
    pub(crate) fn open_device(&self, desc: &DeviceDesc<'_>) -> Result<Dx12Device, HalError> {
        let Some(record) = self
            .inner
            .adapters
            .iter()
            .find(|record| record.info.id == desc.adapter)
        else {
            return Err(HalError::NoSuchAdapter(desc.adapter.0));
        };
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
        Dx12Device::open(Arc::clone(&self.inner), record, desc)
    }
}

/// Every adapter worth describing: DXGI's hardware list, then WARP by name.
///
/// The descriptor is fetched here rather than inside
/// [`adapter::describe`] because both passes need it — the first to decide
/// whether an adapter is the software one, the second to name the adapter — and
/// asking DXGI twice for a struct it already filled in would be the same
/// question with two chances to disagree.
fn candidates(factory: &IDXGIFactory4) -> Vec<(IDXGIAdapter1, DXGI_ADAPTER_DESC1)> {
    let mut out: Vec<(IDXGIAdapter1, DXGI_ADAPTER_DESC1)> = Vec::new();
    let mut index = 0u32;
    loop {
        // SAFETY: `factory` is a live COM interface this crate owns a reference
        // to. `EnumAdapters1` writes only the interface it returns, and reports
        // the end of the list as an error code rather than by writing anything.
        let raw_adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(raw_adapter) => raw_adapter,
            // The documented terminator, and the only one that is not a
            // problem: every other failure is worth a log line.
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => {
                log::warn!("crcbl-dx12: EnumAdapters1({index}) failed: {error}");
                break;
            }
        };
        index += 1;
        let Some(desc) = desc_of(&raw_adapter) else {
            continue;
        };
        // Skipped here and appended by name below; see `Dx12Instance::open`.
        if adapter::is_software(&desc) {
            continue;
        }
        out.push((raw_adapter, desc));
    }

    // SAFETY: as above — `EnumWarpAdapter` writes only the interface it returns,
    // and `IDXGIAdapter1` is the IID it is asked for.
    match unsafe { factory.EnumWarpAdapter::<IDXGIAdapter1>() } {
        Ok(warp) => {
            if let Some(desc) = desc_of(&warp) {
                // `EnumWarpAdapter` may hand back an adapter the pass above
                // already kept — on a machine where DXGI flags WARP as software
                // it will not, but nothing guarantees the two agree.
                //
                // `AdapterLuid` is the identity DXGI actually guarantees:
                // locally unique for the lifetime of the system, and equal for
                // two interfaces onto one adapter. Comparing it is what makes
                // the de-duplication independent of a flag a driver may or may
                // not set. Name, vendor and device id would not do as a key:
                // two genuinely distinct cards of one model share all three,
                // and so — measured on `windows-latest` — do the two Basic
                // Render Driver entries DXGI lists there, which carry different
                // LUIDs and really are two adapters. **Those are kept out by
                // `is_software` above, not by this comparison**, which sees
                // nothing to collapse.
                let duplicate = out
                    .iter()
                    .any(|(_, seen)| seen.AdapterLuid == desc.AdapterLuid);
                if duplicate {
                    log::debug!(
                        "crcbl-dx12: WARP is already in the list as an unflagged adapter; \
                         keeping the one entry"
                    );
                } else {
                    out.push((warp, desc));
                }
            }
        }
        // Loud, because this is the interesting failure: WARP ships in Windows,
        // so its absence is the answer to `docs/backlog.md`'s question rather
        // than a detail.
        Err(error) => log::warn!("crcbl-dx12: this Windows reports no WARP adapter: {error}"),
    }
    out
}

/// One `GetDesc1`, with the failure logged rather than swallowed.
fn desc_of(raw_adapter: &IDXGIAdapter1) -> Option<DXGI_ADAPTER_DESC1> {
    // SAFETY: `raw_adapter` is a live COM interface this crate owns a reference
    // to, and `GetDesc1` fills in a `DXGI_ADAPTER_DESC1` the binding allocates
    // and returns by value.
    match unsafe { raw_adapter.GetDesc1() } {
        Ok(desc) => Some(desc),
        Err(error) => {
            log::warn!("crcbl-dx12: an adapter would not describe itself: {error}");
            None
        }
    }
}

impl Instance for Dx12Instance {
    fn backend(&self) -> BackendKind {
        BackendKind::Dx12
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
        // trivially here — this slice creates no DXGI swapchain and never reads
        // `target`. The swapchain slice is where obligation 2 (the window
        // outlives the surface) starts to matter, and where
        // `SurfaceTarget::Win32`'s `hwnd` finally gets read.
        Err(not_yet("surface creation (the DX12 swapchain slice)"))
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
        Err(not_yet(
            "surface capability queries (the DX12 swapchain slice)",
        ))
    }

    /// Opens the device *now* and hands it over on the first poll.
    ///
    /// D3D12 device creation is synchronous — `D3D12CreateDevice` and
    /// `CreateCommandQueue` both return before this call does — so there is
    /// nothing to wait for and this backend does not pretend otherwise. The seam
    /// is poll-shaped because WebGPU's `requestDevice` is a promise (see
    /// [`crcbl_hal::device`]); `crcbl-vk` and `crcbl-mtl` complete on their
    /// first poll for the same reason, and so does this.
    fn request_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn PendingDevice>, HalError> {
        let device = self.open_device(desc)?;
        Ok(Box::new(Dx12PendingDevice {
            device: Some(Box::new(device)),
        }))
    }
}

/// A D3D12 device request — already finished before it is returned.
///
/// See [`Instance::request_device`] above for why this exists at all on a
/// backend whose device creation is synchronous.
#[derive(Debug)]
struct Dx12PendingDevice {
    /// `None` once handed over, so a second poll is the caller bug it is rather
    /// than a second device.
    device: Option<Box<dyn crcbl_hal::Device>>,
}

impl PendingDevice for Dx12PendingDevice {
    fn backend(&self) -> BackendKind {
        BackendKind::Dx12
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
    use crcbl_hal::{DeviceType, Features, Limits, RendererTier};
    /// The two constants the bindless rule is written against, named here so the
    /// test compares with D3D12's own numbers rather than through the function
    /// it is checking.
    use windows::Win32::Graphics::Direct3D12::{
        D3D_SHADER_MODEL_6_6, D3D12_RESOURCE_BINDING_TIER_3,
    };

    /// Opening the backend, with the failure loud.
    ///
    /// Every test below goes through this rather than tolerating `None`: a run
    /// on a machine where D3D12 opens nothing must fail the suite, because a
    /// suite that skips itself into green is how "compile-verified only"
    /// backends happen — the trap `docs/plan/09-backends-metal-dx12.md` names in
    /// its risk list. WARP ships in Windows, so there is always something.
    pub(crate) fn open() -> Dx12Instance {
        Dx12Instance::open().expect("a Windows machine reports at least WARP")
    }

    /// A device request this backend can actually satisfy today.
    ///
    /// [`DeviceDesc::for_adapter`] asks for [`Features::TIER_A`], which this
    /// backend does not report until the pipeline and command slices land — see
    /// `the_default_device_desc_is_refused_for_the_tier_a_gap` below, which is
    /// the test that pins that.
    pub(crate) fn desc(adapter: AdapterId) -> DeviceDesc<'static> {
        DeviceDesc {
            label: Some("crcbl-dx12 test device"),
            adapter,
            required_features: Features::empty(),
            optional_features: Features::empty(),
            compatible_surface: None,
        }
    }

    /// Every adapter's measurement, on one line each.
    ///
    /// Used as the message of the assertions that can fail on a real adapter, so
    /// a red run carries the whole picture and not just the one adapter that
    /// tripped.
    fn report_all(instance: &Dx12Instance) -> String {
        instance
            .records()
            .iter()
            .map(AdapterRecord::report)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn enumeration_finds_at_least_one_d3d12_adapter() {
        let adapters = open().adapters();
        assert!(
            !adapters.is_empty(),
            "DXGI plus EnumWarpAdapter produced nothing on a Windows machine"
        );
    }

    /// Every adapter identifies itself. Asserted per adapter, after the list is
    /// known non-empty — a loop over an empty vector passes whatever the body
    /// says, so the emptiness check is its own assertion above rather than an
    /// assumption here.
    ///
    /// `vendor_id` is the one that would catch a `GetDesc1` whose result was
    /// never read: DXGI fills in a real PCI vendor for every adapter including
    /// WARP, so a zero here means the descriptor was defaulted rather than
    /// fetched. The seam documents `0` as "unknown", which is what `crcbl-mtl`
    /// reports because Metal has no such ids — DXGI does, so this backend has no
    /// excuse for one.
    #[test]
    fn every_adapter_names_itself_its_driver_its_vendor_and_its_backend() {
        let adapters = open().adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        for info in &adapters {
            assert_eq!(info.backend, BackendKind::Dx12, "{info:?}");
            assert!(
                !info.name.is_empty(),
                "DXGI_ADAPTER_DESC1::Description came back empty: {info:?}"
            );
            assert!(!info.driver.is_empty(), "no driver string: {info:?}");
            assert_ne!(
                info.vendor_id, 0,
                "a real DXGI descriptor always carries a vendor id: {info:?}"
            );
        }
    }

    /// [`AdapterId`] is documented as the position in the enumeration, and
    /// `request_device` resolves it by search — so a mismatch would make every
    /// id past the gap name the wrong adapter.
    ///
    /// This is the check that goes red if the id counter is ever advanced for an
    /// adapter `describe` rejected, which is the easy version of this bug: DXGI
    /// lists display-only and too-old adapters that D3D12 refuses to open, and
    /// on the machine that has one every id after it would be off by one.
    #[test]
    fn adapter_ids_are_their_enumeration_positions() {
        let adapters = open().adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        for (index, info) in adapters.iter().enumerate() {
            assert_eq!(info.id, AdapterId(index as u32), "{info:?}");
        }
    }

    /// The limits that come from D3D12 really came from it, and the ones keyed
    /// off a feature agree with the feature.
    ///
    /// The alignment check is the one doing the work. `>=` the floor would pass
    /// for [`Limits::minimum`] *and* [`Limits::desktop`], since both sit at or
    /// above it; **strictly greater than the floor's copy-offset alignment is
    /// something only this backend produces**, because D3D12's
    /// `D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT` is coarser than the floor's
    /// while `Limits::desktop` is far finer. So a preset leaking in fails here
    /// whichever preset it was.
    ///
    /// The three biconditionals at the end are the invariant `crcbl-wgpu` learnt
    /// the hard way: a bindless capacity without the bindless feature, a
    /// push-constant budget without push constants, or an anisotropy cap without
    /// anisotropic sampling is a promise no call can keep.
    #[test]
    fn reported_limits_come_from_d3d12_and_agree_with_the_features() {
        let instance = open();
        assert!(!instance.records().is_empty(), "nothing to check");
        let floor = Limits::minimum();
        for record in instance.records() {
            let limits = record.info.caps.limits;
            let features = record.info.caps.features;
            let line = record.report();
            assert!(
                limits.optimal_buffer_copy_offset_alignment
                    > floor.optimal_buffer_copy_offset_alignment,
                "a preset leaked in instead of D3D12's placement alignment: {line}"
            );
            assert!(
                limits.max_image_2d > floor.max_image_2d,
                "D3D12's 2D image ceiling did not clear the seam's floor: {line}"
            );
            assert!(
                limits.max_color_attachments > floor.max_color_attachments,
                "D3D12's render-target count did not clear the seam's floor: {line}"
            );
            assert!(
                limits.max_compute_workgroup_size[0] > floor.max_compute_workgroup_size[0],
                "D3D12's thread-group ceiling did not clear the seam's floor: {line}"
            );
            assert!(
                limits.max_sample_count.is_power_of_two() && limits.max_sample_count <= 64,
                "a sample count is a mask underneath: {line}"
            );

            assert_eq!(
                features.contains(Features::DESCRIPTOR_INDEXING),
                limits.max_bindless_descriptors > 0,
                "bindless capacity and the bindless feature disagree: {line}"
            );
            assert_eq!(
                features.contains(Features::PUSH_CONSTANTS),
                limits.max_push_constant_size > 0,
                "the push-constant budget and the push-constant feature disagree: {line}"
            );
            assert_eq!(
                features.contains(Features::SAMPLER_ANISOTROPY),
                limits.max_sampler_anisotropy > 1.0,
                "the anisotropy cap and the anisotropy feature disagree: {line}"
            );
        }
    }

    /// The bindless flag follows the two numbers D3D12 gave for it, on a real
    /// adapter.
    ///
    /// Deliberately compared against the raw `ResourceBindingTier` and
    /// `HighestShaderModel` rather than against
    /// [`RawCaps::dynamic_resources`](crate::adapter::RawCaps::dynamic_resources),
    /// which is the function under test here: going through it would make this a
    /// tautology that passes however the rule is written. The threshold *inside*
    /// that rule is pinned separately, on constructed inputs, by
    /// `crcbl_dx12::adapter`'s own tests — the combinations that matter (tier 3
    /// without SM6.6, and the reverse) do not both exist on one machine.
    ///
    /// The falsifying value here is a `features_of` that stopped consulting the
    /// rule at all — one that sets [`Features::DESCRIPTOR_INDEXING`]
    /// unconditionally goes red on any adapter below tier 3, and one that never
    /// sets it goes red on any adapter at or above it.
    #[test]
    fn the_bindless_flag_follows_the_two_answers_d3d12_gave_for_it() {
        let instance = open();
        assert!(!instance.records().is_empty(), "nothing to check");
        for record in instance.records() {
            let line = record.report();
            let claimed = record
                .info
                .caps
                .features
                .contains(Features::DESCRIPTOR_INDEXING);
            let queried = record.raw.binding_tier.0 >= D3D12_RESOURCE_BINDING_TIER_3.0
                && record.raw.shader_model.0 >= D3D_SHADER_MODEL_6_6.0;
            assert_eq!(
                claimed, queried,
                "the reported flag and the queried answers disagree: {line}"
            );
        }
    }

    /// **The question this slice exists to answer.**
    ///
    /// WARP is asked for by name, so its absence here means
    /// `IDXGIFactory4::EnumWarpAdapter` failed or D3D12 would not open what it
    /// returned — either of which is a finding about the runner rather than a
    /// flaky test, and both make this red with the reason attached.
    ///
    /// What it asserts is only what must be true whatever the answer turns out
    /// to be: WARP is enumerated exactly once, it is classified as the seam's
    /// software rasteriser, and it is where this enumeration puts it. **It
    /// deliberately does not assert that WARP supports SM6.6 dynamic
    /// resources.** That is the measurement, and a test that demanded one answer
    /// would turn a finding into a failure while a test that accepted either
    /// would prove nothing. Whether WARP's bindless flag agrees with the numbers
    /// behind it is checked for *every* adapter by
    /// `the_bindless_flag_follows_the_two_answers_d3d12_gave_for_it`, so it is
    /// not restated here.
    ///
    /// The measurement is printed. `.config/nextest.toml`'s `ci` profile does
    /// not set `success-output`, so a green run captures it — read it with
    /// `cargo nextest run -p crcbl-dx12 --success-output immediate`, or off the
    /// failure message of any assertion above if the run is red.
    #[test]
    fn warp_is_enumerated_exactly_once_and_publishes_its_dynamic_resource_answer() {
        let instance = open();
        let all = report_all(&instance);
        println!("{all}");

        // **No adapter is listed twice, keyed on the identity DXGI guarantees.**
        // Name, vendor and device id would not do as a key: two genuinely
        // distinct cards of one model share all three, and `windows-latest`
        // lists two Basic Render Driver entries that share all three and are
        // still two adapters. `AdapterLuid` is the only thing DXGI promises is
        // unique.
        //
        // **This cannot fail while the de-duplication keys on the same field**,
        // which is why it is not the whole of what this test does — it goes red
        // only if the key moves back to something DXGI does not guarantee. What
        // no adapter may be is *hardware while carrying the software ids*, and
        // that is
        // `a_microsoft_software_rasteriser_is_never_enumerated_as_hardware`.
        let mut luids: Vec<u64> = instance
            .records()
            .iter()
            .map(|record| record.raw.luid)
            .collect();
        let listed = luids.len();
        assert!(listed > 0, "nothing to check:\n{all}");
        luids.sort_unstable();
        luids.dedup();
        assert_eq!(
            luids.len(),
            listed,
            "one adapter is enumerated more than once — the WARP de-duplication \
             is keying on something DXGI does not guarantee:\n{all}"
        );

        let software: Vec<&AdapterRecord> = instance
            .records()
            .iter()
            .filter(|record| record.raw.software)
            .collect();
        assert_eq!(
            software.len(),
            1,
            "exactly one software adapter is expected — WARP, appended by name \
             after a hardware pass that skips software adapters:\n{all}"
        );

        let warp = software[0];
        let line = warp.report();
        assert_eq!(
            warp.info.device_type,
            DeviceType::Cpu,
            "the seam names WARP as its Cpu example: {line}"
        );
        // WARP is appended after the hardware pass, so it is last by
        // construction. Asserted rather than assumed because it is the half of
        // `open`'s ordering a reader cannot see from `adapters()`: a software
        // adapter that slipped through the hardware filter would land *before*
        // this one and take a lower id, and the count check above would still
        // pass if the appended one had then been dropped.
        assert_eq!(
            warp.info.id,
            AdapterId(
                u32::try_from(instance.records().len() - 1).expect("adapters fit in a u32 index")
            ),
            "WARP is appended last, after the hardware pass: {line}"
        );
    }

    /// **No enumerated adapter is hardware while carrying Microsoft's software
    /// ids.**
    ///
    /// This is the assertion the runner needed and did not have. It reported
    /// "Microsoft Basic Render Driver" as [`DeviceType::Integrated`] for as long
    /// as `is_software` consulted only `DXGI_ADAPTER_FLAG_SOFTWARE`, which that
    /// adapter arrives with *clear* — so a caller ranking
    /// `Discrete > Integrated > Cpu` to prefer real hardware picked the software
    /// rasteriser and believed it had a GPU. Nothing went red: the LUIDs are
    /// distinct, so the de-duplication had nothing to collapse, and the count of
    /// *flagged* software adapters was right the whole time.
    ///
    /// It is asserted over the **enumerated** adapters rather than over a
    /// constructed descriptor, because that is the half a constructed one cannot
    /// reach: whether the classification survives `describe`, the enumeration
    /// filter and the append. `crcbl_dx12::adapter`'s own
    /// `the_software_test_reads_the_known_ids_as_well_as_the_flag` pins the rule
    /// itself, on inputs no machine is obliged to produce.
    ///
    /// The loop is asserted to have run. On any Windows where
    /// `EnumWarpAdapter` succeeds there is at least one adapter carrying these
    /// ids, so an empty sweep means either that call failed — a finding, and one
    /// the software-count assertion above reports too — or that Microsoft's
    /// rasteriser now reports ids this backend does not recognise, which is the
    /// classification silently reverting to the bug.
    #[test]
    fn a_microsoft_software_rasteriser_is_never_enumerated_as_hardware() {
        let instance = open();
        let all = report_all(&instance);
        assert!(!instance.records().is_empty(), "nothing to check:\n{all}");

        let mut matched = 0usize;
        for record in instance.records() {
            if record.info.vendor_id != adapter::MICROSOFT_VENDOR_ID
                || record.info.device_id != adapter::BASIC_RENDER_DRIVER_DEVICE_ID
            {
                continue;
            }
            matched += 1;
            let line = record.report();
            assert!(
                record.raw.software,
                "an adapter with Microsoft's software rasteriser ids was described as \
                 hardware: {line}"
            );
            assert_eq!(
                record.info.device_type,
                DeviceType::Cpu,
                "a software rasteriser must not outrank itself into hardware: {line}"
            );
        }
        assert!(
            matched > 0,
            "no adapter carries vendor {:#06x} device {:#06x}, so this test checked nothing — \
             either EnumWarpAdapter failed or the ids the classification keys on have \
             moved:\n{all}",
            adapter::MICROSOFT_VENDOR_ID,
            adapter::BASIC_RENDER_DRIVER_DEVICE_ID
        );
    }

    /// Every adapter is Tier B in this slice, and it is this backend's gap
    /// rather than the adapter's.
    ///
    /// The four flags asserted missing are missing because no call in this crate
    /// makes them true — `CreateComputePipelineState`, `CreateFence` and
    /// `ExecuteIndirect` are all unwritten — so this test goes red on the day a
    /// slice earns one of them, which is exactly the day the crate docs' claim
    /// that "every adapter is Tier B" stops being true and has to be rewritten.
    /// [`Features::BUFFER_DEVICE_ADDRESS`] is asserted *present* in the same
    /// loop, so dropping it silently is red too.
    ///
    /// The falsifying value on the other side is an adapter that reports one of
    /// the four: that is a flag added ahead of the call behind it, which is the
    /// mistake `crcbl-mtl` had to reverse.
    #[test]
    fn every_adapter_is_tier_b_until_the_calls_behind_the_flags_land() {
        let instance = open();
        assert!(!instance.records().is_empty(), "nothing to check");
        let all = report_all(&instance);
        println!("{all}");

        for record in instance.records() {
            let line = record.report();
            let missing = record.info.caps.missing(Features::TIER_A);
            assert!(
                !missing.contains(Features::BUFFER_DEVICE_ADDRESS),
                "every D3D12 device has GPU virtual addresses: {line}"
            );
            for waiting in [
                Features::COMPUTE,
                Features::TIMELINE_SEMAPHORE,
                Features::MULTI_DRAW_INDIRECT,
                Features::DRAW_INDIRECT_COUNT,
            ] {
                assert!(
                    missing.contains(waiting),
                    "{waiting:?} is reported with no call behind it: {line}"
                );
            }
            assert_eq!(
                record.info.caps.tier(),
                RendererTier::B,
                "the derived tier moved; the crate docs say why it cannot yet: {line}"
            );
        }
    }

    /// A device now opens, and arrives through exactly the request/poll pair the
    /// seam specifies — including the second poll being a caller bug rather than
    /// a second device.
    #[test]
    fn a_device_opens_on_the_first_poll_and_only_once() {
        let instance = open();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");

        let mut pending = instance
            .request_device(&desc(adapters[0].id))
            .expect("a D3D12 device opens with no required features");
        assert_eq!(pending.backend(), BackendKind::Dx12);

        let device = pending
            .poll()
            .expect("D3D12 device creation is synchronous")
            .into_device()
            .expect("the first poll must complete a synchronous backend");
        assert_eq!(device.backend(), BackendKind::Dx12);

        let error = pending
            .poll()
            .expect_err("the device was already handed over");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// The seam's convenience constructor asks for Tier A, and this backend is
    /// Tier B until the pipeline and command slices land — so `for_adapter` is
    /// refused, by name, on real hardware.
    ///
    /// This is the visible consequence of the tier decision the crate docs argue
    /// for; if the backend ever starts reporting the four waiting features, this
    /// test says so rather than letting the change go unnoticed.
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
            missing.contains(Features::COMPUTE | Features::TIMELINE_SEMAPHORE),
            "the calls behind these are what keep this backend at Tier B: {missing:?}"
        );
    }

    /// A device asked to be compatible with a surface is refused on the surface,
    /// not on the device — no surface exists to be compatible with.
    ///
    /// The order matters and is what this pins: the surface is checked *after*
    /// the adapter and the features, so a valid adapter with a stale surface
    /// reaches this branch rather than an earlier one.
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

    /// Obligation 1, made observable: the device must survive its instance being
    /// dropped, and must still work afterwards.
    #[test]
    fn a_device_outlives_the_instance_that_made_it() {
        let device = {
            let instance = open();
            let adapters = instance.adapters();
            assert!(!adapters.is_empty(), "nothing to check");
            instance
                .create_device(&desc(adapters[0].id))
                .expect("a D3D12 device opens with no required features")
        };
        // The instance is gone; the `Arc` inside the device is what keeps its
        // factory and adapter alive. A buffer round trip is the cheapest proof
        // that the device is still usable rather than merely still addressable.
        assert_eq!(device.backend(), BackendKind::Dx12);
        let buffer = device
            .create_buffer(&crcbl_hal::BufferDesc {
                label: Some("outlives its instance"),
                size: 256,
                usage: crcbl_hal::BufferUsage::STORAGE,
                memory: crcbl_hal::MemoryLocation::HostUpload,
            })
            .expect("the D3D12 device is still live");
        device.destroy_buffer(buffer);
    }

    /// An out-of-range adapter is a distinct contract from a feature gap, and it
    /// must not be swallowed by one.
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
    /// that needs no window at all and the Win32 one this backend will
    /// eventually own.
    #[test]
    fn surfaces_are_refused_and_the_refusal_names_dx12() {
        let instance = open();
        let dangling = core::ptr::NonNull::dangling();
        for target in [
            SurfaceTarget::Offscreen,
            SurfaceTarget::Win32 {
                hinstance: dangling,
                hwnd: dangling,
            },
        ] {
            // SAFETY: `create_surface` refuses before it reads `target` at all —
            // it matches nothing and returns — so the dangling pointers are
            // never dereferenced. That is what makes it safe to name a target
            // this slice does not implement.
            let error = unsafe { instance.create_surface(&target) }
                .expect_err("this slice creates no surfaces");
            let text = error.to_string();
            assert!(text.contains("dx12"), "{target:?}: {text}");
            assert!(text.contains("surface creation"), "{target:?}: {text}");
        }
    }

    /// Surface capability queries refuse rather than answering emptily, which is
    /// the failure `Instance::surface_caps` calls out by name.
    #[test]
    fn surface_capability_queries_are_refused_and_name_dx12() {
        let instance = open();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        // No instance issued this handle, which is the point: the refusal must
        // arrive before the handle or the adapter is looked at.
        let stale = crcbl_core::Handle::from_bits(1 << 32).expect("generation 1 is non-zero");

        let error = instance
            .surface_caps(stale, adapters[0].id)
            .expect_err("this slice presents to nothing");
        let text = error.to_string();
        assert!(text.contains("dx12"), "{text}");
        assert!(text.contains("surface capability"), "{text}");
    }
}
