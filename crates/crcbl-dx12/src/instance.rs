//! The Direct3D 12 [`Instance`] implementation — adapter enumeration, the WARP
//! question, device creation, and the refusals that still name themselves.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crcbl_core::Pool;
use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, CompositeAlpha, DeviceDesc, DeviceRequestState, HalError,
    Instance, PendingDevice, SurfaceCaps, SurfaceHandle, SurfaceTarget,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_ADAPTER_DESC1, DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_NOT_FOUND,
    DXGI_FEATURE_PRESENT_ALLOW_TEARING, IDXGIAdapter1, IDXGIFactory4, IDXGIFactory5,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{BOOL, Interface};

use crate::adapter::{self, RawCaps};
use crate::debug;
use crate::device::Dx12Device;
use crate::handle::{self, Owned, Owner};
use crate::present;

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
    /// `docs/backlog.md`'s "Does WARP clear the GPU-driven set?" is answered by
    /// `sm66-dynamic-resources`, **not** by the selected paths: those sit at the
    /// floor on every adapter in this slice because four of the bundle's flags
    /// wait on calls this crate does not make yet, so they say how much of the
    /// backend is written and nothing about the hardware. See the crate docs.
    ///
    /// Every test that can fail on an adapter carries this string in its
    /// message, so a red run publishes the measurement whether or not a green
    /// one was asked to print it.
    pub(crate) fn report(&self) -> String {
        format!(
            "crcbl-dx12 adapter {id} \"{name}\" luid={luid:#018x} type={kind:?} \
             vendor={vendor:#06x} device={device:#06x} ResourceBindingTier={tier} \
             HighestShaderModel={model} sm66-dynamic-resources={dynamic} \
             block-compression={bc} geometry={geometry:?} binding={binding:?} \
             lighting={lighting:?} features={features:?} \
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
            geometry = self.info.caps.geometry_path(),
            binding = self.info.caps.binding_model(),
            lighting = self.info.caps.lighting_path(),
            features = self.info.caps.features,
            driver = self.info.driver,
        )
    }
}

/// One live surface: the window it presents to, and the instance that issued
/// it.
///
/// **There is no DXGI object here, and that is the whole shape of obligation 2
/// on this backend.** A `VkSurfaceKHR` is a driver object whose destruction has
/// to be deferred past the last swapchain on it — obligation 2b, and undefined
/// behaviour in the driver when it is not. DXGI has no such object: a swapchain
/// is created *from* an `HWND` and holds whatever it needs itself, so a
/// surface here is a record of an address the caller promised is a live window.
/// Destroying the handle therefore frees nothing and can strand nothing, and
/// the deferral obligation is discharged by having nothing to defer rather than
/// by a mechanism. What still binds is the caller's half: the **window** must
/// outlive every swapchain, which is
/// [`Instance::create_surface`](crcbl_hal::Instance::create_surface)'s safety
/// contract and not something this crate can check.
///
/// The `HWND` is kept as a plain address rather than as a
/// [`HWND`](windows::Win32::Foundation::HWND) because that type is a raw
/// pointer, which `windows-rs` declares neither `Send` nor `Sync` — and this
/// table lives behind a [`Mutex`] inside a struct the seam requires to be both.
/// Storing the integer is what keeps this crate's "no `unsafe` marker impls
/// anywhere" claim true; the pointer is rebuilt at each call site that needs
/// one.
#[derive(Debug)]
pub(crate) struct SurfaceEntry {
    /// Instance that created it. Surfaces are checked against the *instance*
    /// id, per obligation 3, so any device from that instance may use them.
    owner: u64,
    /// The window's `HWND`, as an address. Never dereferenced here.
    ///
    /// **Zero is [`SurfaceTarget::Offscreen`]**, and it is a sentinel rather
    /// than an `Option` for the reason `crcbl-vk` uses a null `VkSurfaceKHR`
    /// for the same case: an offscreen surface has no driver object at all, and
    /// a null window handle is what "there is no window" already spells on this
    /// platform. [`InstanceInner::surface_hwnd`] is the one place that reading
    /// happens, and it hands back an `Option` so no call site can pass a null
    /// `HWND` to DXGI by forgetting.
    hwnd: usize,
    /// Which [`SurfaceTarget`] variant it came from, for logs.
    platform: &'static str,
}

impl Owned for SurfaceEntry {
    fn owner(&self) -> u64 {
        self.owner
    }
}

/// What a surface or swapchain entry's window address holds when there is no
/// window: [`SurfaceTarget::Offscreen`].
///
/// Shared with `crate::swapchain`'s `SwapchainEntry::hwnd` rather than written
/// as a zero in each, because it is one fact — "this is the offscreen case" —
/// and `reconfigure_swapchain` compares the two against each other.
pub(crate) const OFFSCREEN_HWND: usize = 0;

/// Everything a device needs to keep alive from the instance that made it.
///
/// `crcbl-hal`'s **obligation 1**: a `Device` may outlive its `Instance`, and
/// the backend *must* keep the instance-level state alive internally rather than
/// borrowing it. This is that state, and [`Dx12Device`] holds an [`Arc`] of it,
/// so dropping the public [`Dx12Instance`] while a device is open releases
/// nothing the device is still using.
///
/// On D3D12 that state is the DXGI factory, the adapters, and the surface
/// table. The factory is **not** dead weight: DXGI documents an adapter
/// enumerated from a factory as tied to it, and `create_swapchain` calls
/// `CreateSwapChainForHwnd` on this one — so keeping it here is what stops a
/// device re-creating a factory and asking DXGI a question about an adapter the
/// new factory never enumerated.
///
/// The surface table is here rather than on the device because **obligation 2
/// makes surfaces instance-scoped**: a surface outlives the device that
/// presented through it, and any device from this instance may use it. That is
/// also why [`owner`](Self::owner) exists — obligation 3 splits ownership two
/// ways, surfaces against the *instance* and everything else against the
/// *device*, and two instances' pools genuinely do issue identical bits.
#[derive(Debug)]
pub(crate) struct InstanceInner {
    pub(crate) factory: IDXGIFactory4,
    pub(crate) adapters: Vec<AdapterRecord>,
    /// Which instance this is, and the tag it stamps into every surface handle
    /// it issues. The same scheme [`crate::handle`] uses for devices, from the
    /// same counter, so an instance id and a device id never collide.
    pub(crate) owner: Owner,
    /// What `IDXGIFactory5::CheckFeatureSupport(DXGI_FEATURE_PRESENT_ALLOW_TEARING)`
    /// answered, once, at start-up.
    ///
    /// Probed rather than assumed: the flag needs Windows 10 1703 and a DXGI
    /// 1.5 factory, and a swapchain created with
    /// `DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING` where it is unsupported fails to
    /// create at all. It decides both what [`Instance::surface_caps`] offers
    /// and what flags every swapchain carries.
    pub(crate) allow_tearing: bool,
    surfaces: Mutex<Pool<SurfaceEntry>>,
}

impl InstanceInner {
    fn surfaces(&self) -> MutexGuard<'_, Pool<SurfaceEntry>> {
        self.surfaces
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The window a surface handle names, checked against *this* instance.
    ///
    /// `None` for [`SurfaceTarget::Offscreen`], which names no window: the
    /// caller's swapchain is then a ring of plain images and nothing DXGI owns.
    /// Returning an `Option` rather than an `HWND(null)` is what makes that a
    /// case every call site has to answer — `CreateSwapChainForHwnd` on a null
    /// window is `E_INVALIDARG`, and `GetClientRect` on one fails too.
    ///
    /// The three outcomes [`crate::handle`] keeps apart are kept apart here
    /// too: this instance's live surface resolves, this instance's dead one is
    /// [`HalError::InvalidHandle`], and another instance's is
    /// [`HalError::ForeignObject`].
    ///
    /// # Errors
    ///
    /// As [`handle::lookup`].
    pub(crate) fn surface_hwnd(&self, surface: SurfaceHandle) -> Result<Option<HWND>, HalError> {
        let surfaces = self.surfaces();
        let entry = handle::lookup(&surfaces, "surface", surface, self.owner)?;
        if entry.hwnd == OFFSCREEN_HWND {
            return Ok(None);
        }
        Ok(Some(HWND(core::ptr::without_provenance_mut(entry.hwnd))))
    }
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
        // **Before anything else opens a device.** The D3D12 debug layer is
        // chosen per device at creation, and `candidates` below opens one per
        // adapter to describe it — so a layer turned on after this line would
        // validate nothing this instance enumerated. See [`crate::debug`],
        // which also says why its messages are pulled out of the info queue
        // rather than left for a debugger nobody has attached.
        debug::enable_debug_layer();

        // `CreateDXGIFactory2` rather than `CreateDXGIFactory1`: they differ
        // only in the flags word. Zero here even with the D3D12 debug layer on,
        // and that is a decision rather than an omission:
        // `DXGI_CREATE_FACTORY_DEBUG` turns on a *second*, separate layer whose
        // output is object-lifetime reporting rather than the API validation
        // this backend needs — and it fails factory creation outright with
        // `DXGI_ERROR_SDK_COMPONENT_MISSING` on a machine without the Graphics
        // Tools feature, which would cost enumeration itself for a diagnostic.
        //
        // SAFETY: the call takes no pointer of ours and writes only the
        // interface it returns; `IDXGIFactory4` is the IID it is asked for, so
        // the QI either succeeds or the call fails.
        let factory: IDXGIFactory4 =
            match unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS::default()) } {
                Ok(factory) => factory,
                Err(error) => {
                    crcbl_core::log::warn!("crcbl-dx12: DXGI would not start: {error}");
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
            crcbl_core::log::info!("{}", record.report());
            adapters.push(record);
            next_id += 1;
        }

        if adapters.is_empty() {
            crcbl_core::log::warn!(
                "crcbl-dx12: DXGI lists no adapter D3D12 will open, not even WARP"
            );
            return None;
        }
        let allow_tearing = probe_tearing(&factory);
        crcbl_core::log::info!("crcbl-dx12: DXGI_FEATURE_PRESENT_ALLOW_TEARING={allow_tearing}");
        Some(Self {
            inner: Arc::new(InstanceInner {
                factory,
                adapters,
                owner: Owner::new(next_owner_id()),
                allow_tearing,
                surfaces: Mutex::new(Pool::new()),
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
            // Resolved and then discarded, on purpose. `DeviceDesc::compatible_surface`
            // exists because Vulkan has to pick a present-capable queue family
            // at device-creation time; D3D12 has no such choice to make — every
            // adapter presents to every `HWND` — so the only thing this can
            // usefully do is check the handle really is one of this instance's,
            // which is obligation 3 and which no other call on this path would
            // catch. The three outcomes stay apart: a handle nothing issued is
            // `InvalidHandle`, another instance's is `ForeignObject`.
            self.inner.surface_hwnd(surface)?;
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
                crcbl_core::log::warn!("crcbl-dx12: EnumAdapters1({index}) failed: {error}");
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
                    crcbl_core::log::debug!(
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
        Err(error) => {
            crcbl_core::log::warn!("crcbl-dx12: this Windows reports no WARP adapter: {error}")
        }
    }
    out
}

/// The window's client area, or `None` when Win32 will not say.
///
/// `SurfaceCaps::current_extent` is obligation 4's cross-check and nothing
/// above the seam acts on it, so a failure is `None` rather than an error:
/// `GetClientRect` fails exactly when the `HWND` is already gone, which is the
/// caller's obligation 2 broken and not something to report through a
/// capability query.
///
/// A minimized window has a zero client rect, and that is reported as `None`
/// too — the seam's obligation 5 says a zero extent means "do not create a
/// swapchain yet", and a `Some((0, 0))` would read as a size somebody could
/// clamp to.
fn client_extent(hwnd: HWND) -> Option<(u32, u32)> {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is the address the caller promised names a live window in
    // `create_surface`'s safety contract, and `rect` is a live local the call
    // writes through and which outlives it.
    if let Err(error) = unsafe { GetClientRect(hwnd, &mut rect) } {
        crcbl_core::log::debug!(
            "crcbl-dx12: GetClientRect failed, so the surface reports no extent: {error}"
        );
        return None;
    }
    // A client rect's origin is always (0, 0), so the far corner *is* the size;
    // `saturating_sub` is there because a driver-supplied rectangle is not
    // something to trust into a subtraction underflow.
    let width = u32::try_from(rect.right.saturating_sub(rect.left)).ok()?;
    let height = u32::try_from(rect.bottom.saturating_sub(rect.top)).ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

/// Whether this machine will let a present tear.
///
/// **Asked, not assumed, and the two halves of the answer are separate
/// questions.** `DXGI_PRESENT_ALLOW_TEARING` on a present and
/// `DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING` on the swapchain that receives it are
/// both required, and both are refused outright on a machine where this query
/// says no — so a swapchain created with the flag unqueried does not tear, it
/// fails to be created. That is why the answer is taken once here, at the
/// factory, rather than at each swapchain.
///
/// `false` on any failure, including the `QueryInterface` for `IDXGIFactory5`:
/// the interface arrived with DXGI 1.5 and the feature with Windows 10 1703, so
/// an older machine simply has no tearing and reporting none is the honest
/// answer rather than an error to propagate out of enumeration.
fn probe_tearing(factory: &IDXGIFactory4) -> bool {
    let factory5 = match factory.cast::<IDXGIFactory5>() {
        Ok(factory5) => factory5,
        Err(error) => {
            crcbl_core::log::debug!("crcbl-dx12: no IDXGIFactory5, so no tearing: {error}");
            return false;
        }
    };
    let mut allowed = BOOL(0);
    let size =
        u32::try_from(size_of::<BOOL>()).unwrap_or_else(|_| unreachable!("a BOOL is four bytes"));
    // SAFETY: `factory5` is a live COM interface this crate owns a reference
    // to. The pointer names a live local of exactly the type
    // `DXGI_FEATURE_PRESENT_ALLOW_TEARING` is documented to write — a `BOOL` —
    // and `size` is that type's own size, so the call cannot write past it. The
    // local outlives the call.
    let queried = unsafe {
        factory5.CheckFeatureSupport(
            DXGI_FEATURE_PRESENT_ALLOW_TEARING,
            core::ptr::from_mut(&mut allowed).cast(),
            size,
        )
    };
    if let Err(error) = queried {
        crcbl_core::log::debug!("crcbl-dx12: the tearing query failed, so no tearing: {error}");
        return false;
    }
    allowed.as_bool()
}

/// One `GetDesc1`, with the failure logged rather than swallowed.
fn desc_of(raw_adapter: &IDXGIAdapter1) -> Option<DXGI_ADAPTER_DESC1> {
    // SAFETY: `raw_adapter` is a live COM interface this crate owns a reference
    // to, and `GetDesc1` fills in a `DXGI_ADAPTER_DESC1` the binding allocates
    // and returns by value.
    match unsafe { raw_adapter.GetDesc1() } {
        Ok(desc) => Some(desc),
        Err(error) => {
            crcbl_core::log::warn!("crcbl-dx12: an adapter would not describe itself: {error}");
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

    /// Records the window a swapchain will be created on.
    ///
    /// # What this call actually does with the handles
    ///
    /// It reads [`SurfaceTarget::Win32`]'s `hwnd` — as an **address**, which it
    /// stores — and dereferences nothing. The window is first touched by
    /// `CreateSwapChainForHwnd` in
    /// [`Device::create_swapchain`](crcbl_hal::Device::create_swapchain), and
    /// by `GetClientRect` in [`Instance::surface_caps`]. So the trait's safety
    /// contract is not discharged trivially any more, and each of its three
    /// obligations lands somewhere real:
    ///
    /// 1. **`hwnd` really is an `HWND`.** Nothing here can check it — an
    ///    arbitrary address may name a live window belonging to another process
    ///    — and DXGI is what finds out. `hinstance` is ignored: DXGI takes the
    ///    window, not the module that registered its class.
    /// 2. **The window outlives the surface and every swapchain on it.** This
    ///    is the obligation with teeth on Windows: `DestroyWindow` while a
    ///    swapchain is configured leaves DXGI presenting to a dead `HWND`.
    ///    `crcbl-hal`'s obligation 2 orders the teardown, and see
    ///    `SurfaceEntry` for why the *backend* half of 2b is vacuous here.
    /// 3. **Called from the thread that owns the window.** An `HWND` is
    ///    thread-affine and `CreateSwapChainForHwnd` installs a message hook on
    ///    it.
    ///
    /// # Offscreen names no window, and touches nothing
    ///
    /// [`SurfaceTarget::Offscreen`] carries no pointer and no handle, so all
    /// three obligations above are discharged vacuously for it and the entry
    /// records `OFFSCREEN_HWND` instead of an address. What it *becomes* is
    /// decided one call later:
    /// [`Device::create_swapchain`](crcbl_hal::Device::create_swapchain) builds
    /// a ring of plain `ID3D12Resource` textures rather than asking DXGI for
    /// anything, which is what makes `crcbl screenshot` and every headless
    /// harness run through the same acquire/present path a window does.
    ///
    /// # The window-system targets are refused permanently
    ///
    /// The four are [`HalError::Unsupported`] rather than `not_yet`: D3D12's
    /// only presentation target is an `HWND` (a `CoreWindow` and a composition
    /// surface are the other two DXGI accepts, and neither is a variant of this
    /// enum), so no slice will ever make them work.
    unsafe fn create_surface(&self, target: &SurfaceTarget) -> Result<SurfaceHandle, HalError> {
        let never = |what| {
            Err(HalError::Unsupported {
                backend: BackendKind::Dx12,
                what,
            })
        };
        let hwnd = match *target {
            // The one variant this backend presents to. `hinstance` is
            // deliberately unread: `CreateSwapChainForHwnd` takes the window
            // alone.
            SurfaceTarget::Win32 { hwnd, .. } => hwnd.as_ptr() as usize,
            SurfaceTarget::Offscreen => OFFSCREEN_HWND,
            SurfaceTarget::Wayland { .. } | SurfaceTarget::Xcb { .. } => {
                return never("a Linux window system is crcbl-vk's target, not D3D12's");
            }
            SurfaceTarget::AppKit { .. } => {
                return never("a CAMetalLayer is crcbl-mtl's target, not D3D12's");
            }
            SurfaceTarget::Web { .. } => {
                return never("a canvas is crcbl-wgpu's target, not D3D12's");
            }
        };

        let handle: SurfaceHandle = {
            let mut surfaces = self.inner.surfaces();
            let slot = surfaces.insert(SurfaceEntry {
                owner: self.inner.owner.id,
                hwnd,
                platform: target.platform_name(),
            });
            handle::stamp(self.inner.owner, slot)
        };
        crcbl_core::log::debug!(
            "crcbl-dx12: created a {} surface {handle:?} on HWND {hwnd:#x}",
            target.platform_name()
        );
        Ok(handle)
    }

    /// Invalidates the handle. There is nothing underneath it to destroy.
    ///
    /// Obligation 2's two halves come apart on this backend: the handle dies
    /// here and now, which is what makes a later use fail the generational
    /// lookup, and the *object* half has nothing to defer because DXGI never
    /// made one — see `SurfaceEntry`. A swapchain still configured on this
    /// surface keeps working, because it holds its own `IDXGISwapChain3` and
    /// its own copy of the `HWND` and never consults this table again.
    fn destroy_surface(&self, surface: SurfaceHandle) {
        let mut surfaces = self.inner.surfaces();
        if let Some(entry) = handle::take_owned(&mut surfaces, surface, self.inner.owner) {
            crcbl_core::log::debug!(
                "crcbl-dx12: destroyed a {} surface handle; the window is the caller's",
                entry.platform
            );
        }
    }

    /// What a flip-model swapchain on this window will accept.
    ///
    /// The adapter is checked first and the surface second, which is the order
    /// `Dx12Instance::open_device` uses and for the same reason: answering
    /// about a surface for an adapter that does not exist would blame the
    /// window for a caller's index bug.
    ///
    /// **The adapter otherwise does not change the answer, and that is a real
    /// difference from Vulkan.** `crcbl-vk` has to refuse the pairing when no
    /// queue family on the adapter can present to the surface — a common case
    /// on Linux. DXGI has no equivalent: any D3D12 adapter can present to any
    /// `HWND`, with the runtime copying across adapters where it has to. So
    /// there is no "this pairing does not work" answer to give, and none is
    /// invented.
    ///
    /// What the trait calls out by name — reporting a surface as empty
    /// `formats` or empty `present_modes` — cannot happen here: both lists are
    /// non-empty constants, `present::FLIP_MODEL_FORMATS` and
    /// `present::present_modes`, and the second always contains
    /// [`PresentMode::Fifo`](crcbl_hal::PresentMode::Fifo).
    ///
    /// **An offscreen surface answers from `present::offscreen_surface_caps`
    /// instead**, and the answer is a different set rather than the same one
    /// with a null window: a ring of plain images is not a flip-model
    /// swapchain, so flip-discard's format refusals and its two-buffer floor do
    /// not apply to it. The adapter is still checked first, for the reason
    /// below.
    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        if !self
            .inner
            .adapters
            .iter()
            .any(|record| record.info.id == adapter)
        {
            return Err(HalError::NoSuchAdapter(adapter.0));
        }
        let Some(hwnd) = self.inner.surface_hwnd(surface)? else {
            return Ok(present::offscreen_surface_caps());
        };
        Ok(SurfaceCaps {
            formats: present::FLIP_MODEL_FORMATS.to_vec(),
            present_modes: present::present_modes(self.inner.allow_tearing),
            // Flip-model on an `HWND` composites as opaque:
            // `DXGI_ALPHA_MODE_PREMULTIPLIED` is a composition-swapchain mode
            // and `CreateSwapChainForHwnd` rejects anything but `IGNORE` and
            // `UNSPECIFIED`. Offering one this backend cannot configure would
            // be the same mistake as offering a format it cannot create.
            composite_alpha: vec![CompositeAlpha::Opaque],
            min_image_count: present::MIN_BUFFERS,
            max_image_count: present::MAX_BUFFERS,
            current_extent: client_extent(hwnd),
        })
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
    use crcbl_hal::{
        Device as _, DeviceType, Features, GeometryPath, Limits, SemaphoreDesc, SemaphoreKind,
    };
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

    /// The adapter every device this suite opens is opened on.
    ///
    /// **Every call site that used to write `adapters()[0]` goes through this**,
    /// because "the first enumerated adapter" is a different machine's answer on
    /// every machine: WARP on a runner with no GPU, the discrete card on a
    /// workstation. `tests/run-dx12-e2e.sh` sets [`crate::pin::PIN_VAR`] so the
    /// harness run is the one it says it is, and [`crate::pin::resolve`] refuses
    /// rather than falling back — see that module for why a fallback is the
    /// failure and not the recovery.
    ///
    /// Panics with the resolver's own explanation, which carries the whole
    /// enumeration: a pin that missed is diagnosed by what was there instead.
    pub(crate) fn pinned_adapter(instance: &Dx12Instance) -> AdapterId {
        let adapters = instance.adapters();
        let pin = std::env::var(crate::pin::PIN_VAR).ok();
        crate::pin::resolve(pin.as_deref(), &adapters).unwrap_or_else(|why| panic!("{why}"))
    }

    /// A device request this backend can actually satisfy today.
    ///
    /// [`DeviceDesc::for_adapter`] requires compute and a timeline semaphore,
    /// and this backend reports the first but not the second until the command
    /// slice lands — see `the_default_device_desc_is_refused_for_the_gap` below,
    /// which is the test that pins that.
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
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
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
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn every_dxgi_adapter_names_itself_its_driver_its_vendor_and_its_backend() {
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
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn adapter_ids_are_their_positions_in_the_dxgi_enumeration() {
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
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
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
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
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
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
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
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
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

    /// Every flag this backend reports has a call behind it, and the derived
    /// path follows from them.
    ///
    /// Each flag below is asserted *present*, and each is here because a call in
    /// this crate makes it true — dropping one silently is red, which is the
    /// half that rots once a flag stops being newsworthy.
    ///
    /// [`Features::COMPUTE`] joined the list with
    /// `CreateComputePipelineState` and `Dispatch`,
    /// [`Features::MULTI_DRAW_INDIRECT`] and [`Features::DRAW_INDIRECT_COUNT`]
    /// with `ExecuteIndirect` for a *draw* — which is what moved the derived
    /// path off the per-batch floor and onto
    /// [`GeometryPath::IndirectCount`], asserted here so the move cannot happen
    /// unnoticed in either direction — and
    /// [`Features::TIMELINE_SEMAPHORE`] with `CreateFence` handed out as a seam
    /// semaphore and consumed by `ID3D12CommandQueue::Wait` and `Signal`.
    ///
    /// That last one was the only member of [`Features::GPU_DRIVEN`] still
    /// waiting on a call, so what this test now says is that **the whole bundle
    /// is earned** on any adapter reporting
    /// [`Features::DESCRIPTOR_INDEXING`] — and that flag is the WARP question
    /// itself, which is why it is not asserted here.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn every_reported_flag_has_a_call_behind_it_and_the_path_follows() {
        let instance = open();
        assert!(!instance.records().is_empty(), "nothing to check");
        let all = report_all(&instance);
        println!("{all}");

        for record in instance.records() {
            let line = record.report();
            let missing = record.info.caps.missing(Features::GPU_DRIVEN);
            for earned in [
                Features::BUFFER_DEVICE_ADDRESS,
                Features::COMPUTE,
                Features::MULTI_DRAW_INDIRECT,
                Features::DRAW_INDIRECT_COUNT,
                Features::TIMELINE_SEMAPHORE,
            ] {
                assert!(
                    !missing.contains(earned),
                    "{earned:?} has a call behind it and is not reported: {line}"
                );
            }
            // What is left of the bundle is the WARP question and nothing else,
            // so a missing flag that is not `DESCRIPTOR_INDEXING` is a flag this
            // crate dropped rather than an adapter that lacks one.
            assert!(
                missing.difference(Features::DESCRIPTOR_INDEXING).is_empty(),
                "a GPU-driven flag with a call behind it went missing: {missing:?} on {line}"
            );
            assert_eq!(
                record.info.caps.geometry_path(),
                GeometryPath::IndirectCount,
                "the derived geometry path moved; the crate docs say what selects it: {line}"
            );
            // The ceiling has to move with the flag, or a caller reading it
            // would cap every indirect-count call at one draw while the backend
            // executes as many as `ExecuteIndirect` is given.
            assert!(
                record.info.caps.limits.max_draw_indirect_count > 1,
                "DRAW_INDIRECT_COUNT is reported and the limit is still the floor: {line}"
            );
        }
    }

    /// **The line `tests/run-dx12-e2e.sh` reads its verdict off.**
    ///
    /// A pin is worth having only if something checks it landed, and that check
    /// cannot be the pin's own code. [`crate::pin::PIN_VAR`] reaching the
    /// harness's shell and not this process — a stale binary, a wrapper that
    /// resets the environment, a runner that does not pass it through — resolves
    /// to the first adapter with nothing anywhere saying so, which is the
    /// silent-fallback failure the pin exists to prevent wearing a different
    /// hat. So the adapter this suite actually opened is printed, in the format
    /// [`AdapterRecord::report`] already defines, and the harness fails when the
    /// line is absent or names something other than what it pinned.
    /// `crates/crcbl-vk/tests/run-vk-e2e.sh` makes exactly this check on the
    /// driver its ICD pin asked for, and for exactly this reason.
    ///
    /// nextest captures a passing test's stdout, so read it with
    /// `--success-output immediate` — which is what the harness passes.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn the_pinned_adapter_opens_a_device_and_names_itself() {
        let instance = open();
        let adapter = pinned_adapter(&instance);
        let record = instance
            .records()
            .iter()
            .find(|record| record.info.id == adapter)
            .expect("the pin resolved against this same enumeration");
        println!("crcbl-dx12 e2e: device on {}", record.report());
        instance
            .create_device(&desc(adapter))
            .expect("a D3D12 device opens on the pinned adapter with no required features");
    }

    /// A device now opens, and arrives through exactly the request/poll pair the
    /// seam specifies — including the second poll being a caller bug rather than
    /// a second device.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_d3d12_device_opens_on_the_first_poll_and_only_once() {
        let instance = open();
        let adapter = pinned_adapter(&instance);

        let mut pending = instance
            .request_device(&desc(adapter))
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

    /// The seam's convenience constructor requires compute **and** a timeline
    /// semaphore, and this backend now has both — so `for_adapter` opens a
    /// device where it used to be refused for the gap.
    ///
    /// The required half is asserted through the *call*, and the reported half
    /// separately on the caps, because they are different claims: a backend that
    /// reported the flags and could not create a fence would pass a test that
    /// only read the caps, and `create_semaphore` is what closes that.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn the_default_device_desc_opens_a_device_now_that_the_bundle_is_earned() {
        let instance = open();
        let adapter = pinned_adapter(&instance);

        let device = instance
            .open_device(&DeviceDesc::for_adapter(adapter))
            .expect("compute and a timeline semaphore are both reported now");
        let features = device.caps().features;
        for required in [Features::COMPUTE, Features::TIMELINE_SEMAPHORE] {
            assert!(
                features.contains(required),
                "for_adapter requires {required:?} and the device it opened lacks it: {features:?}"
            );
        }
        device
            .create_semaphore(&SemaphoreDesc {
                label: Some("for_adapter"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })
            .map(|semaphore| device.destroy_semaphore(semaphore))
            .expect("the flag the constructor required has the call behind it");
    }

    /// A device asked to be compatible with a surface handle nothing issued is
    /// refused on the surface, not on the device.
    ///
    /// The order matters and is what this pins: the surface is checked *after*
    /// the adapter and the features, so a valid adapter with a stale surface
    /// reaches this branch rather than an earlier one.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_compatible_surface_is_refused_by_d3d12_as_an_unresolvable_handle() {
        let instance = open();
        let adapter = pinned_adapter(&instance);

        let mut with_surface = desc(adapter);
        // Carries no instance tag, so no instance ever issued it — which is a
        // different answer from a *real* surface's, and the reason
        // `open_device` resolves the handle rather than refusing every one.
        // `Handle::from_bits` is the only way to build one.
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

    /// An out-of-range adapter is a distinct contract from a feature gap, and it
    /// must not be swallowed by one.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn an_unknown_dxgi_adapter_is_refused_as_such_not_as_unimplemented() {
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

    /// **Every target D3D12 cannot present to is refused by name, and an
    /// offscreen one is not among them.**
    ///
    /// The four window-system targets are not D3D12's at all, so each names the
    /// backend that owns it and none of them reads as an unwritten slice — a
    /// caller has to be able to tell "this backend will never do that" from
    /// "that has not been written yet", because only the second is worth
    /// waiting for.
    ///
    /// [`SurfaceTarget::Offscreen`] used to be the second kind and is now
    /// neither: it succeeds, and its capabilities are the ring's own rather
    /// than a window's. That is asserted here because this is the test the
    /// refusal used to live in, so a regression to `not_yet` lands on an
    /// assertion instead of on a deleted one.
    ///
    /// [`SurfaceTarget::Win32`] is deliberately **not** here. It succeeds too,
    /// and a surface made from a dangling pointer would be one `surface_caps`
    /// goes on to hand to `GetClientRect` — see
    /// `crcbl_dx12::swapchain`'s tests, which make a real window instead.
    ///
    /// Red when a refusal loses the backend's name, when a permanent refusal
    /// starts naming a slice, and when `Offscreen` stops resolving.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn every_target_d3d12_cannot_present_to_is_refused_by_name() {
        let instance = open();
        let adapter = pinned_adapter(&instance);
        let dangling = core::ptr::NonNull::dangling();

        // SAFETY: `Offscreen` names no platform object, so there is nothing
        // this call could dereference and nothing that has to outlive it.
        let offscreen = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
            .expect("an offscreen surface needs no window");
        let caps = instance
            .surface_caps(offscreen, adapter)
            .expect("a live offscreen surface on a real adapter");
        assert_eq!(
            caps,
            crate::present::offscreen_surface_caps(),
            "an offscreen surface must answer with the ring's caps, not a window's"
        );
        instance.destroy_surface(offscreen);

        let never = [
            SurfaceTarget::Wayland {
                display: dangling,
                surface: dangling,
            },
            SurfaceTarget::Xcb {
                connection: dangling,
                window: 1,
                visual_id: 1,
            },
            SurfaceTarget::AppKit { layer: dangling },
            SurfaceTarget::Web { canvas_id: 0 },
        ];
        assert!(!never.is_empty(), "nothing to check");
        for target in never {
            // SAFETY: as above — the arm that matches each of these returns
            // without reading a pointer, which is what makes a dangling one
            // safe to name here.
            let error = unsafe { instance.create_surface(&target) }
                .expect_err("D3D12 presents to an HWND and nothing else");
            let text = error.to_string();
            assert!(text.contains("dx12"), "{target:?}: {text}");
            assert!(
                !text.contains("slice"),
                "a permanent refusal must not read as an unwritten slice: {target:?}: {text}"
            );
            assert!(
                matches!(error, HalError::Unsupported { backend, .. } if backend == BackendKind::Dx12),
                "{target:?}: {error:?}"
            );
        }
    }

    /// **A capability query refuses the two arguments it can genuinely
    /// diagnose, and keeps them apart.**
    ///
    /// The failure `Instance::surface_caps` calls out by name — answering with
    /// empty `formats` or empty `present_modes` — is now structurally
    /// impossible: both come from non-empty constants. What is left to guard is
    /// the pair of refusals, and their **order**: the adapter is checked first,
    /// so an out-of-range adapter beside a stale surface is reported as the
    /// adapter, which is the same order `open_device` uses.
    ///
    /// Red when the adapter check moves below the surface lookup (the first
    /// assertion then finds `InvalidHandle`), and red when either check is
    /// dropped.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn surface_capability_queries_refuse_an_unknown_adapter_before_a_stale_handle() {
        let instance = open();
        let adapters = instance.adapters();
        assert!(!adapters.is_empty(), "nothing to check");
        // No instance issued this handle. `Handle::from_bits` is the only way
        // to build one.
        let stale: SurfaceHandle =
            crcbl_core::Handle::from_bits(1 << 32).expect("generation 1 is non-zero");
        let past_the_end = AdapterId(adapters.len() as u32);

        let error = instance
            .surface_caps(stale, past_the_end)
            .expect_err("there is no adapter one past the last");
        assert!(
            matches!(error, HalError::NoSuchAdapter(id) if id == past_the_end.0),
            "the adapter must be blamed before the surface: {error:?}"
        );

        let error = instance
            .surface_caps(stale, pinned_adapter(&instance))
            .expect_err("no instance issued that surface handle");
        assert!(
            matches!(error, HalError::InvalidHandle { kind, .. } if kind == "surface"),
            "{error:?}"
        );
    }

    /// Destroying a handle nothing issued is a no-op, not a panic and not
    /// somebody else's surface.
    ///
    /// `destroy_surface` returns `()`, so the only way it can report a
    /// mistake is by not making one. The handle here carries no instance tag at
    /// all, which is the case `crate::handle`'s `take_owned` exists to keep
    /// away from a live row that shares its bits.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn destroying_a_surface_handle_nothing_issued_does_nothing() {
        let instance = open();
        let stale: SurfaceHandle =
            crcbl_core::Handle::from_bits(1 << 32).expect("generation 1 is non-zero");
        instance.destroy_surface(stale);
        // And again, because a double destroy is a caller bug this must absorb
        // rather than a second free.
        instance.destroy_surface(stale);
    }
}
