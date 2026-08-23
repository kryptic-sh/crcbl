//! The two central traits: [`Instance`] and [`Device`].
//!
//! ```text
//! Instance ──adapters()──▶ [AdapterInfo]      (caps known before a device exists)
//!    │
//!    ├──create_surface(SurfaceTarget)──▶ SurfaceHandle
//!    └──request_device(DeviceDesc)─────▶ Box<dyn PendingDevice>
//!                                            │
//!                                            └──poll()──▶ Pending | Ready(Box<dyn Device>)
//!                                                                        │
//!                                            ├─ resources: buffers, images, views, samplers
//!                                            ├─ pipelines, layouts, bind groups
//!                                            ├─ swapchains: create / reconfigure / acquire / present
//!                                            ├─ command encoders ──▶ Box<dyn CommandEncoder>
//!                                            └─ submit / wait
//! ```
//!
//! # Device creation is polled, not blocking
//!
//! Opening a device is [`Instance::request_device`] →
//! [`PendingDevice::poll`], the same request/poll pair
//! [`crate::readback`] already uses, and for the same reason: on WebGPU both
//! `requestAdapter` and `requestDevice` resolve on a **later turn of the event
//! loop**, and the browser main thread — where the rAF loop runs — cannot block
//! waiting for one. A synchronous-only `create_device` was a trait method that
//! returned [`HalError::Unsupported`] on the target
//! `docs/plan/10-wasm-webgpu.md` most wants to ship to, which is the exact
//! mistake [`crate::readback`] and [`crate::swapchain`] were shaped to avoid.
//!
//! | Step | Vulkan | WebGPU |
//! | --- | --- | --- |
//! | request | `vkCreateDevice`, which returns before this call does | call `requestDevice`, keep the promise |
//! | poll | hand over the already-open device | has the promise resolved? |
//! | ready | `Box<dyn Device>` | `Box<dyn Device>` around the resolved `GPUDevice` |
//!
//! Neither backend has to invent anything, and neither has to block. A backend
//! whose creation really is synchronous — `crcbl-vk` — does the work in
//! `request_device` and completes on its **first** poll; it does not fake
//! latency. [`crate::null`] can be asked to simulate a slow one with
//! [`Recorder::set_device_latency`](crate::null::Recorder::set_device_latency),
//! which is what makes a caller's poll *loop* testable with no GPU.
//!
//! ## Two deliberate differences from `poll_readback`
//!
//! 1. **The payload rides in the variant.** [`poll_readback`](Device::poll_readback)
//!    writes into a caller-supplied `&mut [u8]` and returns a `Copy` state, which
//!    a `Box<dyn Device>` has no equivalent of; an
//!    `&mut Option<Box<dyn Device>>` out-parameter would be strictly worse than
//!    [`DeviceRequestState::Ready`] carrying it. The *discipline* — "call once
//!    per frame, `Pending` means try again, failure is an `Err` and not a third
//!    state" — is identical.
//! 2. **`poll` takes `&mut self`.** A readback is polled through the shared
//!    `&self` device; a pending device is a single-owner, short-lived object, so
//!    the exclusive borrow is free and lets a backend hold a future without
//!    inventing interior mutability for it.
//!
//! ## `create_device` survives as a native-only convenience
//!
//! `Instance::create_device` is a **provided** method that requests and then
//! polls to completion, so every existing native caller — `crcbl screenshot`,
//! the golden-image tests, `crcbl-render`'s fixtures — is unchanged. It does not
//! exist on `wasm32` at all: it is `#[cfg(not(target_arch = "wasm32"))]`, so a
//! browser build that reaches for it fails to *compile* and is pointed at
//! `request_device`, rather than compiling and returning
//! [`HalError::Unsupported`] at run time. That is the one place this seam
//! departs from [`crate::readback`]'s "there is no blocking wrapper" rule, and
//! it is affordable precisely because the compiler enforces the rule where it
//! matters.
//!
//! # Queues are handles, not a trait
//!
//! `docs/plan/01-foundations.md` asks for "`Device` + `Queue`". A `Queue` here
//! is a [`QueueHandle`] and submission is [`Device::submit`], rather than a
//! third trait object. Queues carry no state a caller can usefully hold, and
//! `Device::submit(queue, …)` keeps the number of trait objects in the engine at
//! three. The plurality that matters — async compute and a dedicated transfer
//! queue, which `docs/plan/02-vulkan-backend.md` says must be *modelled* even
//! though the MVP uses one — is fully expressible: [`Device::queue`] returns a
//! handle per [`QueueKind`], and [`QueueTransfer`](crate::QueueTransfer)
//! already carries ownership transfers between them.
//!
//! # `&self`, not `&mut self`
//!
//! Every method takes `&self`, so a `Device` can be shared behind an `Arc`
//! across threads (the traits are `Send + Sync`). Backends provide their own
//! interior synchronisation, which they need regardless: `vkCreateBuffer` is
//! thread-safe but the underlying suballocator is not, and P8's job system will
//! upload from worker threads. A `&mut self` seam would have forced an
//! engine-wide lock on the device just to create a buffer.
//!
//! Command *recording* is the exception, and it is expressed in the type: a
//! [`CommandEncoder`] is [`HalThreadSafe`](crate::threading::HalThreadSafe)
//! (Send + Sync); two threads cannot record into one because every recording
//! method takes `&mut self`, which is how Vulkan's external-synchronisation
//! rule for command buffers is discharged.
//!
//! # Destruction is explicit and immediate-looking
//!
//! `destroy_*` takes the handle by value and returns nothing. It means "this
//! handle is dead now"; it does **not** promise the GPU is finished with the
//! object. Deferring the real free until N frames later — the deletion queue
//! from `docs/plan/02-vulkan-backend.md` §2.2 — is the backend's business.
//! Destroying a resource the GPU is still using is a caller bug that the graph's
//! lifetime tracking is responsible for preventing, exactly as it is in Vulkan.
//!
//! # Object lifetimes — implementation obligations on backends
//!
//! `Instance::create_device` returns `Box<dyn Device>`, which is `'static`, so
//! the type system permits a `Device` to outlive the `Instance` that made it.
//! In Vulkan that relationship is real and inverted: a `VkDevice` must not
//! outlive its `VkInstance`, and a `VkSurfaceKHR` is instance-owned while the
//! swapchain built from it is device-owned. Rather than leave the P1 backend
//! author to guess, the seam states the rules here. **These are obligations on
//! backends, not merely descriptions.**
//!
//! ## 1. A `Device` may outlive its `Instance`
//!
//! A backend **must** keep whatever instance-level state its device needs alive
//! internally — in practice an `Arc` to the instance's inner data, so dropping
//! the public `Instance` object does not destroy the underlying one while a
//! device still exists.
//!
//! The two alternatives were both worse:
//!
//! * `Box<dyn Device + 'a>` borrowed from the instance would express the truth,
//!   but any struct holding both an instance and a device becomes
//!   self-referential. `crcbl-render`'s renderer is exactly that struct, so the
//!   tax would be paid on every line of the layer this seam exists to serve.
//! * An unenforced "you must not drop the instance first" rule is the worst of
//!   both: no compile-time help *and* no runtime detection, with
//!   use-after-free as the failure mode.
//!
//! Keeping the instance alive internally costs one `Arc` per backend and makes
//! the `'static` that the type system already promises actually true.
//!
//! ## 2. Surfaces are instance-scoped; swapchains are device-scoped
//!
//! The legal teardown order is:
//!
//! ```text
//! 1. Device::destroy_swapchain(swapchain)
//! 2. Instance::destroy_surface(surface)
//! 3. drop the Device
//! 4. drop the Instance          (and only now may the window close)
//! ```
//!
//! Steps 3 and 4 may be reordered, per rule 1. Steps 1 and 2 may not, and
//! neither may be skipped while the window is still open — the
//! [`create_surface`](Instance::create_surface) safety contract requires the
//! native handles to outlive the surface.
//!
//! A caller that gets it wrong must get a **clean error, never undefined
//! behaviour**. Concretely, a backend must:
//!
//! * Invalidate the handle immediately on `destroy_surface`, so a later use
//!   fails the generational lookup and returns [`HalError::InvalidHandle`].
//! * **Defer** destroying the underlying driver object until the last swapchain
//!   configured on it is destroyed. Calling `vkDestroySurfaceKHR` with a live
//!   swapchain is undefined behaviour in the driver, so "pass it straight down"
//!   is not an option; the deletion queue that already exists for retiring
//!   resources N frames later is the same machinery.
//!
//! In short: the handle dies when the caller says so, the object dies when it is
//! safe. [`crate::null`] enforces the detectable half — a swapchain created from
//! a destroyed surface, or a present on a destroyed swapchain, is an error and
//! is covered by tests.
//!
//! ## 3. Handles never cross instances or devices
//!
//! A [`SurfaceHandle`] from instance A passed to a device created from instance
//! B is a caller bug, and so is a buffer from device X used on device Y. The
//! required behaviour is [`HalError::ForeignObject`] — detected, not undefined.
//!
//! This one needs a mechanism, because a [`Handle`] cannot carry it: its 32-bit
//! index and 32-bit generation are fully spoken for, and two instances' pools
//! genuinely do issue identical bits. Backends **must** therefore stamp an
//! owner identity into their own side table — one `u64` per tracked object,
//! compared on every lookup. That is one integer compare on a path that is
//! already doing a table lookup.
//!
//! [`crate::null`] implements exactly this: every object records the id of the
//! instance or device that created it, and every lookup checks it. Surfaces are
//! checked against the *instance* id (so any device from that instance may use
//! them) and everything else against the *device* id.

use std::time::Duration;

use crate::{
    AcquiredFrame, AdapterId, AdapterInfo, BackendKind, BindGroupDesc, BindGroupHandle,
    BindGroupLayoutDesc, BindGroupLayoutHandle, BufferDesc, BufferHandle, CommandBufferHandle,
    CommandEncoder, CommandEncoderDesc, ComputePipelineDesc, ComputePipelineHandle, DeviceCaps,
    DisplayTiming, Features, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageDesc,
    ImageHandle, ImageViewDesc, ImageViewHandle, PipelineLayoutDesc, PipelineLayoutHandle,
    PresentInfo, QuerySetDesc, QuerySetHandle, ReadbackDesc, ReadbackHandle, ReadbackState,
    SamplerDesc, SamplerHandle, SemaphoreDesc, SemaphoreHandle, ShaderModuleDesc,
    ShaderModuleHandle, SubmitInfo, SurfaceCaps, SurfaceError, SurfaceHandle, SwapchainDesc,
    SwapchainHandle,
};
use crcbl_core::{Handle, SurfaceTarget};

/// Marker type for queue handles. Uninhabited.
#[derive(Debug)]
pub enum Queue {}

/// A queue on a device.
pub type QueueHandle = Handle<Queue>;

/// Which class of work a queue accepts.
///
/// # Decision: `QueueKind`, not `QueueFamily`
///
/// "Queue family" is a Vulkan noun and nothing else's: Metal has one
/// `MTLCommandQueue` type and no families at all, and DX12 has *command list
/// types* (`D3D12_COMMAND_LIST_TYPE_DIRECT` / `_COMPUTE` / `_COPY`), which is
/// exactly this enum's three variants under a different name. Since
/// `docs/plan/01-foundations.md`'s P0 exit criterion is "no obviously vk-only
/// concept in the trait names", the noun that maps onto all three APIs is the
/// one the seam uses, and `Kind` is already this crate's word for it
/// ([`SemaphoreKind`](crate::SemaphoreKind), [`QueryKind`](crate::QueryKind),
/// [`BindingKind`](crate::BindingKind), [`BackendKind`]).
///
/// A Vulkan backend still maps each variant to a real queue family index; that
/// is a backend's business and not the seam's vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueueKind {
    /// Graphics, compute and transfer. Always present; the MVP uses only this
    /// one.
    Graphics,
    /// Compute and transfer, independent of graphics. Present only with
    /// [`Features::ASYNC_COMPUTE_QUEUE`].
    Compute,
    /// Transfer only. Present only with [`Features::TRANSFER_QUEUE`].
    Transfer,
}

/// Parameters for device creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceDesc<'a> {
    /// Debug name; see [`BufferDesc::label`](crate::BufferDesc::label).
    pub label: Option<&'a str>,
    /// Adapter to open, from [`Instance::adapters`].
    pub adapter: AdapterId,
    /// Features the caller cannot run without. Creation fails with
    /// [`HalError::UnsupportedFeatures`] naming the exact gap if the adapter
    /// lacks any of them.
    ///
    /// Keep this to what the caller genuinely cannot render without. Per topic
    /// 39 a missing feature degrades by default, and naming one here is how a
    /// caller opts out of that: a game whose whole look is ray traced puts
    /// [`Features::RAY_QUERY`] here and gets a named failure rather than a
    /// picture that is quietly a different game.
    pub required_features: Features,
    /// Features to enable if available. Absent ones are simply not enabled;
    /// check [`Device::caps`] afterwards to find out.
    pub optional_features: Features,
    /// A surface the device must be able to present to.
    ///
    /// Vulkan needs this at device-creation time to choose a present-capable
    /// queue family, and WebGPU's `requestAdapter` takes a `compatibleSurface`
    /// for the same reason. `None` for a headless device.
    pub compatible_surface: Option<SurfaceHandle>,
}

impl DeviceDesc<'_> {
    /// A headless device on `adapter`, asking for as little as possible.
    ///
    /// The shape a headless render test or `crcbl screenshot` uses. It requires
    /// only compute and a timeline semaphore — the two nothing in the engine
    /// can work without, since every pass culls in compute and every frame is
    /// paced on a monotonic counter — and asks for
    /// [`Features::GPU_DRIVEN`] on top as *optional*, so the device opens on
    /// whatever the adapter has and the selectors decide what to record. An
    /// earlier version required the whole bundle and refused a device over one
    /// absent flag while it had the rest; topic 39 exists to replace that.
    #[must_use]
    pub const fn for_adapter(adapter: AdapterId) -> Self {
        Self {
            label: None,
            adapter,
            required_features: Features::COMPUTE.union(Features::TIMELINE_SEMAPHORE),
            optional_features: Features::GPU_DRIVEN,
            compatible_surface: None,
        }
    }
}

/// A backend's entry point: enumerates adapters and opens devices and surfaces.
///
/// One instance exists per process per backend. More than one backend may be
/// instantiated in the same process — that is the whole point of the `dyn`
/// decision in the crate docs.
pub trait Instance: core::fmt::Debug + crate::threading::HalThreadSafe {
    /// Which backend this is. For logs and bug reports — never for behaviour.
    fn backend(&self) -> BackendKind;

    /// Every adapter this backend can see, with its capabilities already
    /// filled in so selection logic never has to open a device to reject one.
    fn adapters(&self) -> Vec<AdapterInfo>;

    /// Creates a presentation surface from a shell's native window handles.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that:
    ///
    /// 1. Every pointer and id in `target` names a **live** platform object of
    ///    exactly the kind the variant says (a `wl_surface*` really is a
    ///    `wl_surface*`, and so on). Nothing in `crcbl-core` validates this;
    ///    only the shell that created the window knows.
    /// 2. Those objects **outlive** the returned [`SurfaceHandle`] and every
    ///    swapchain configured on it. Destroying the window before calling
    ///    [`Device::destroy_swapchain`] and [`Instance::destroy_surface`] is
    ///    use-after-free inside the driver.
    /// 3. On platforms with thread-affine window handles (Win32, AppKit), this
    ///    is called from the thread that owns the window.
    ///
    /// The obligation lives here rather than at `SurfaceTarget` construction
    /// because this is the only place the handles are dereferenced; see
    /// [`crcbl_core::surface`] for the full argument.
    ///
    /// # This `unsafe` no longer lands in application code
    ///
    /// P0.7 drove the first shell→HAL join (`apps/sandbox`) and found the
    /// ergonomic cost: obligation 2 above — "the window outlives the surface" —
    /// can only be discharged by code holding *both* the shell and the device,
    /// which was every game's own startup path, so every sample carried an
    /// `unsafe` block in a crate that otherwise contained none.
    ///
    /// A safe wrapper *above* the seam is what the obligation permits, and it
    /// was built: `crcbl::engine::GpuContext` holds the shell's window and the
    /// device together and calls this from one place. No crate under `apps/`
    /// contains an `unsafe` block now. The seam did not move for it.
    ///
    /// What still hand-rolls the join is `crcbl new`'s generated `main.rs` —
    /// see `docs/backlog.md`.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] if the backend has no support for that
    /// platform, or [`HalError::Backend`] if the window system refused.
    unsafe fn create_surface(&self, target: &SurfaceTarget) -> Result<SurfaceHandle, HalError>;

    /// Destroys a surface. Every swapchain on it must already be destroyed.
    fn destroy_surface(&self, surface: SurfaceHandle);

    /// What `surface` supports on `adapter`.
    ///
    /// # This is also how adapter selection works
    ///
    /// [`adapters`](Instance::adapters) enumerates without reference to any
    /// surface, so **this call is the only way to find out whether a given
    /// adapter can present to a given window** — and the answer is routinely
    /// "no". A discrete GPU under an X server with no DRI3 cannot present to it
    /// while the software rasteriser beside it can; the same is true of the
    /// second GPU in a hybrid laptop. P1.1 hit exactly that, and taking
    /// `adapters()[0]` is therefore not a shortcut but a bug that only shows up
    /// on someone else's machine.
    ///
    /// The contract, in both directions:
    ///
    /// * **Backends** must report an adapter that cannot present to this
    ///   surface as [`HalError::Unsupported`]. Returning empty
    ///   [`formats`](SurfaceCaps::formats) is **not** an option — it collides
    ///   with [`SurfaceCaps::preferred_format`]'s documented meaning — and nor
    ///   is an empty [`present_modes`](SurfaceCaps::present_modes), which would
    ///   break the promise that [`PresentMode::Fifo`](crate::PresentMode::Fifo)
    ///   is always there.
    /// * **Callers** doing selection must treat an `Err` from this call as "try
    ///   the next adapter", not as fatal. Only running out of adapters is
    ///   fatal. `apps/sandbox` is the reference implementation.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] when the adapter cannot present to this
    /// surface, [`HalError::InvalidHandle`] for a stale surface,
    /// [`HalError::NoSuchAdapter`] for an unknown adapter, or a backend error if
    /// the query failed.
    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError>;

    /// Starts opening a device. The device itself arrives from
    /// [`PendingDevice::poll`].
    ///
    /// **There is no synchronous device creation on every target.** WebGPU's
    /// `requestDevice` resolves on a later turn of the event loop and cannot be
    /// blocked on from the browser main thread; see the [module
    /// docs](crate::device#device-creation-is-polled-not-blocking) for the full
    /// argument and the per-backend mapping.
    ///
    /// Everything a backend can decide *now* is decided now: an unknown adapter,
    /// a missing required feature, or a foreign surface is an `Err` from this
    /// call, not a deferred failure. Only what genuinely depends on the driver
    /// answering — device loss, a browser refusing the request — surfaces from
    /// [`PendingDevice::poll`].
    ///
    /// # Errors
    ///
    /// [`HalError::NoSuchAdapter`], [`HalError::UnsupportedFeatures`] naming
    /// exactly which of `required_features` the adapter lacks, or
    /// [`HalError::InvalidHandle`] / [`HalError::ForeignObject`] for
    /// [`DeviceDesc::compatible_surface`].
    fn request_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn PendingDevice>, HalError>;

    /// Opens a device, blocking until it is ready.
    ///
    /// The convenience wrapper over [`request_device`](Instance::request_device)
    /// for callers that have nothing else to do — start-up, `crcbl screenshot`,
    /// every headless test. **Absent on `wasm32`**, where blocking the main
    /// thread is not an option; browser code polls `request_device` from the
    /// rAF loop instead, and gets a compile error rather than a run-time
    /// [`HalError::Unsupported`] if it forgets.
    ///
    /// Backends do not normally override this: the default is exactly "request,
    /// then poll until ready".
    ///
    /// # Errors
    ///
    /// Anything [`request_device`](Instance::request_device) or
    /// [`PendingDevice::poll`] can report.
    #[cfg(not(target_arch = "wasm32"))]
    fn create_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn Device>, HalError> {
        let mut pending = self.request_device(desc)?;
        loop {
            match pending.poll()? {
                DeviceRequestState::Ready(device) => return Ok(device),
                // Yield rather than spin: a backend that really is asynchronous
                // is waiting on another thread or another task, and a hot loop
                // would starve it on a single-core CI runner.
                DeviceRequestState::Pending => std::thread::yield_now(),
            }
        }
    }
}

/// A device request that has been started and has not finished.
///
/// Returned by [`Instance::request_device`]; driven by [`poll`](Self::poll) once
/// per frame (or once per loop iteration at start-up) until it yields the
/// device. Dropping one abandons the request, which is legal — a browser
/// promise that later resolves simply has nobody listening.
///
/// The trait exists rather than an `async fn` because the seam's traits are
/// object-safe by design (see the crate docs): `async fn` in a trait is not
/// object-safe without boxing every call, and `async-trait` would put an
/// allocation and a `Pin<Box<dyn Future>>` in the seam's vocabulary to serve one
/// call made once per process.
pub trait PendingDevice: core::fmt::Debug + crate::threading::HalThreadSafe {
    /// Which backend this request will produce a device for. For logs and bug
    /// reports — never for behaviour.
    fn backend(&self) -> BackendKind;

    /// Advances the request. `Pending` means "ask again"; `Ready` hands over the
    /// device.
    ///
    /// This is a poll, not a wait: it must never block. Polling again after a
    /// `Ready` is a caller bug — the device was moved out — and reports
    /// [`HalError::InvalidDescriptor`] rather than producing a second device.
    ///
    /// # Errors
    ///
    /// [`HalError::DeviceLost`] if the driver or the browser refused the
    /// request, [`HalError::UnsupportedFeatures`] if the refusal named features,
    /// or [`HalError::InvalidDescriptor`] for a poll after completion.
    fn poll(&mut self) -> Result<DeviceRequestState, HalError>;
}

/// Where a device request has got to — the [`ReadbackState`] of device creation.
///
/// [`ReadbackState`]: crate::ReadbackState
///
/// Failure is an `Err` from [`PendingDevice::poll`] rather than a third variant,
/// for the same reason readback has no `Failed`: the reason is the only useful
/// part of a device-creation failure and a bare state would throw it away.
#[derive(Debug)]
pub enum DeviceRequestState {
    /// Not open yet. Poll again next frame.
    Pending,
    /// Open. The device is yours.
    Ready(Box<dyn Device>),
}

impl DeviceRequestState {
    /// Whether the device is available.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    /// The device, or `None` while the request is still pending.
    #[must_use]
    pub fn into_device(self) -> Option<Box<dyn Device>> {
        match self {
            Self::Ready(device) => Some(device),
            Self::Pending => None,
        }
    }
}

/// An open device: creates resources, records and submits work, presents.
///
/// See the module docs for why methods take `&self` and why destruction is
/// explicit.
pub trait Device: core::fmt::Debug + crate::threading::HalThreadSafe {
    /// Which backend this device belongs to.
    fn backend(&self) -> BackendKind;

    /// What this device can do. Drives the path selection; see
    /// [`DeviceCaps::geometry_path`] and its siblings.
    fn caps(&self) -> DeviceCaps;

    /// Whether this backend performs `capability` on this device, and why not
    /// when it does not.
    ///
    /// **The seam's anti-rot mechanism, and the reason it has no default.** A
    /// backend answers with an exhaustive `match` over
    /// [`Capability`](crate::Capability) and no wildcard arm, so a variant added
    /// to that enum fails to compile in every backend until each has said what
    /// it does. A default here — or a `_ =>` arm there — would restore exactly
    /// the silence the enum exists to remove, which is why neither is written.
    ///
    /// Costs one virtual call and a jump table; see [`crate::capability`] for
    /// why this is separate from [`Features`], what it deliberately does not
    /// model, and how [`DIVERGENCES`](crate::DIVERGENCES) turns the answers into
    /// a reviewed parity contract.
    ///
    /// ```
    /// use crcbl_hal::null::NullInstance;
    /// use crcbl_hal::{AdapterId, Capability, DeviceDesc, Instance};
    ///
    /// let instance = NullInstance::gpu_driven();
    /// let device = instance.create_device(&DeviceDesc::for_adapter(AdapterId(0)))?;
    ///
    /// // Every capability has an answer, and a refusal always carries a reason.
    /// for capability in Capability::ALL {
    ///     let support = device.supports(*capability);
    ///     assert_eq!(support.is_yes(), support.reason().is_none());
    /// }
    /// # Ok::<(), crcbl_hal::HalError>(())
    /// ```
    fn supports(&self, capability: crate::Capability) -> crate::Support;

    /// A queue of the given kind.
    ///
    /// Returns `None` when the device has no such queue — check
    /// [`Features::ASYNC_COMPUTE_QUEUE`] / [`Features::TRANSFER_QUEUE`] first,
    /// or just fall back to [`QueueKind::Graphics`], which always exists.
    fn queue(&self, kind: QueueKind) -> Option<QueueHandle>;

    /// Takes the error the driver reported **out of band**, if there is one.
    ///
    /// Some backends do not deliver every failure to the call that caused it.
    /// WebGPU is the case this exists for: a pipeline whose shader will not
    /// compile is still handed back as an object, and the reason arrives later
    /// on the device's error channel. A caller that only checks return values
    /// therefore sees a healthy device, submits command buffers the
    /// implementation silently discards, and draws nothing — which is exactly
    /// how the browser's first invalid shader presented, as a black canvas over
    /// a game that reported itself as playing.
    ///
    /// Call it once a frame and log or fail on what comes out; it is the only
    /// way to notice that class of failure at all. Each error is reported once
    /// — taking it clears it — and backends that report every failure through
    /// their return values keep the default and always answer `None`.
    fn take_error(&self) -> Option<String> {
        None
    }

    // --- resources ---

    /// Creates a buffer.
    ///
    /// # Errors
    ///
    /// [`HalError::OutOfDeviceMemory`], or [`HalError::InvalidDescriptor`] for a
    /// zero size or a usage/memory combination the backend cannot satisfy.
    fn create_buffer(&self, desc: &BufferDesc<'_>) -> Result<BufferHandle, HalError>;

    /// Destroys a buffer.
    fn destroy_buffer(&self, buffer: BufferHandle);

    /// Copies `data` into a mappable buffer at `offset`.
    ///
    /// Only valid for [`MemoryLocation::HostUpload`](crate::MemoryLocation::HostUpload).
    /// Deliberately a copy rather than a `map`/`unmap` pair returning a slice:
    /// a returned `&mut [u8]` cannot be made sound through a trait object
    /// without a lifetime the object cannot carry, and every real caller is
    /// writing a prepared block into a staging ring anyway. If a caller ever
    /// needs to build data in place, that is a persistent-mapping addition to
    /// this trait, not a reason to leak a pointer today.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::InvalidDescriptor`] if the
    /// range is out of bounds or the buffer is not host-visible.
    fn write_buffer(&self, buffer: BufferHandle, offset: u64, data: &[u8]) -> Result<(), HalError>;

    /// Starts a GPU→CPU readback of a range of a host-readable buffer.
    ///
    /// **There is no synchronous read.** WebGPU's `mapAsync` completes on a
    /// later turn of the event loop and cannot be blocked on from the browser
    /// main thread, so a blocking signature would be unimplementable on the
    /// target `docs/plan/10-wasm-webgpu.md` cares most about. See
    /// [`crate::readback`] for the full argument and the per-backend mapping.
    ///
    /// The result is observed with [`Device::poll_readback`] and released with
    /// [`Device::destroy_readback`]. A readback covers work already submitted
    /// when this is called, or the completion point named by
    /// [`ReadbackDesc::after`].
    ///
    /// This is still the *one* readback `docs/plan/03-gpu-driven-rendering.md`
    /// §3.5 permits in the frame loop — culling stats, N frames latent, debug
    /// builds only. Poll-shaped is what makes "N frames latent" expressible.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`] for the
    /// buffer, or [`HalError::InvalidDescriptor`] if the range is out of bounds
    /// or the buffer is not
    /// [`MemoryLocation::HostReadback`](crate::MemoryLocation::HostReadback).
    fn request_readback(&self, desc: &ReadbackDesc<'_>) -> Result<ReadbackHandle, HalError>;

    /// Checks whether a readback has completed, writing its bytes into `out` if
    /// so.
    ///
    /// `out` must be exactly [`ReadbackDesc::size`] bytes. It is left untouched
    /// while the state is [`ReadbackState::Pending`]. Polling again after
    /// [`ReadbackState::Ready`] is legal and yields the same bytes until the
    /// readback is destroyed.
    ///
    /// Call this at most once per frame per readback; it is a poll, not a wait.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::ForeignObject`] for the
    /// readback, [`HalError::InvalidDescriptor`] if `out` is the wrong length,
    /// or [`HalError::DeviceLost`] if the work behind it failed. Failure is an
    /// `Err` rather than a third [`ReadbackState`] so the reason survives.
    fn poll_readback(
        &self,
        readback: ReadbackHandle,
        out: &mut [u8],
    ) -> Result<ReadbackState, HalError>;

    /// Releases a readback request, whether or not it completed.
    ///
    /// On WebGPU this is the `unmap`; abandoning readbacks without destroying
    /// them leaks a mapped buffer, which is why this exists rather than being
    /// implicit.
    ///
    /// **Destroying one whose bytes never landed is legal and silent.** The
    /// request is cancelled, its bytes are never delivered, and the call reports
    /// nothing — not through a return value it does not have, and not through
    /// [`take_error`](Self::take_error) either. A caller that stops caring about
    /// a readback mid-flight is doing the thing this call is for.
    ///
    /// The obligation is on the backend, and only one of them has to do
    /// anything for it. On WebGPU the cancel is `unmap()`, which **rejects** an
    /// outstanding `mapAsync` with an `AbortError`; filing that rejection as a
    /// device error is how a cancelled readback came to report
    /// `could not be mapped: AbortError`, naming a call that did as it was told.
    /// The backend must tell that rejection apart from a real failure and stay
    /// quiet. Vulkan, D3D12 and Metal satisfy this by accident — there is no
    /// promise to reject, so dropping the tracking entry is the whole of it —
    /// which is exactly why it is written down here and asserted for every
    /// backend by `destroying_a_readback_before_its_bytes_land_reports_nothing`
    /// rather than left to three backends continuing to be incidentally right.
    fn destroy_readback(&self, readback: ReadbackHandle);

    /// Creates an image.
    ///
    /// The image is
    /// [`MemoryLocation::DeviceLocal`](crate::MemoryLocation::DeviceLocal);
    /// [`ImageDesc`] has no field to ask for anything else, and
    /// [`MemoryLocation`](crate::MemoryLocation) says why.
    ///
    /// # Errors
    ///
    /// [`HalError::OutOfDeviceMemory`], or [`HalError::InvalidDescriptor`] if
    /// the extent, mip count or sample count exceeds [`Device::caps`].
    fn create_image(&self, desc: &ImageDesc<'_>) -> Result<ImageHandle, HalError>;

    /// Destroys an image. Every view of it must already be destroyed.
    fn destroy_image(&self, image: ImageHandle);

    /// Creates an image view.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] for a stale image, or
    /// [`HalError::InvalidDescriptor`] for an incompatible format or an
    /// out-of-range subresource.
    fn create_image_view(&self, desc: &ImageViewDesc<'_>) -> Result<ImageViewHandle, HalError>;

    /// Destroys an image view.
    fn destroy_image_view(&self, view: ImageViewHandle);

    /// Creates a sampler.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] if anisotropy exceeds
    /// [`Limits::max_sampler_anisotropy`](crate::Limits::max_sampler_anisotropy).
    fn create_sampler(&self, desc: &SamplerDesc<'_>) -> Result<SamplerHandle, HalError>;

    /// Destroys a sampler.
    fn destroy_sampler(&self, sampler: SamplerHandle);

    // --- shaders and pipelines ---

    /// Creates a shader module from whichever artifact this backend consumes.
    ///
    /// The caller fills in every format it holds and does not choose between
    /// them; this implementation takes the one it can compile. See
    /// [`crate::shader`] for the contract in full.
    ///
    /// # Errors
    ///
    /// [`HalError::ShaderCompilation`] if the artifact is malformed, uses a
    /// capability the device lacks, or fails to translate. Also — via
    /// [`ShaderModuleDesc::unusable`] — when the descriptor carries no format
    /// this backend can compile, including a descriptor that carries none at
    /// all.
    fn create_shader_module(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError>;

    /// Destroys a shader module. Pipelines built from it stay valid.
    fn destroy_shader_module(&self, module: ShaderModuleHandle);

    /// Creates a bind-group layout.
    ///
    /// # Errors
    ///
    /// Whatever
    /// [`BindGroupLayoutDesc::check_entries`](crate::BindGroupLayoutDesc::check_entries)
    /// returns — every backend runs it here, and it is where the rules
    /// [`BindGroupLayoutDesc::entries`](crate::BindGroupLayoutDesc::entries)
    /// states are enforced. A backend may add [`HalError::InvalidDescriptor`]
    /// or [`HalError::Unsupported`] of its own for a layout only *it* cannot
    /// express.
    fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError>;

    /// Destroys a bind-group layout.
    fn destroy_bind_group_layout(&self, layout: BindGroupLayoutHandle);

    /// Creates a bind group.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] for a stale layout or resource, or
    /// [`HalError::InvalidDescriptor`] if an entry does not match the layout —
    /// including a host-visible buffer filling a writable storage binding,
    /// which every backend refuses because D3D12 cannot express it. See
    /// [`MemoryLocation`](crate::MemoryLocation).
    fn create_bind_group(&self, desc: &BindGroupDesc<'_>) -> Result<BindGroupHandle, HalError>;

    /// Updates entries of an existing bind group in place.
    ///
    /// The bindless write path: with
    /// [`BindingFlags::UPDATE_AFTER_BIND`](crate::BindingFlags::UPDATE_AFTER_BIND)
    /// this may be called while command buffers referencing the group are
    /// pending, which is what removes per-object descriptor updates from the
    /// frame loop (topic 03's "zero per-frame descriptor writes in steady
    /// state" is about the *steady* state; streaming a new texture in still
    /// writes one entry).
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::Unsupported`] if the group's
    /// layout was not created with `UPDATE_AFTER_BIND`.
    fn update_bind_group(
        &self,
        group: BindGroupHandle,
        entries: &[crate::BindGroupEntry],
    ) -> Result<(), HalError>;

    /// Destroys a bind group.
    fn destroy_bind_group(&self, group: BindGroupHandle);

    /// Creates a pipeline layout.
    ///
    /// # A layout inside every limit may still be refused
    ///
    /// [`Limits`](crate::Limits) bounds each quantity on its own, and D3D12 and
    /// Metal each spend more than one of them out of a single budget the seam
    /// has no field for — a root signature's DWORD cost and a per-stage buffer
    /// argument table respectively, both described on
    /// [`Limits`](crate::Limits). So
    /// [`max_bind_groups`](crate::Limits::max_bind_groups) sets *and*
    /// [`max_push_constant_size`](crate::Limits::max_push_constant_size) bytes
    /// is a legal request that a device may refuse, and the caller's answer is
    /// to attempt the creation and handle the refusal rather than to pre-check
    /// arithmetic that is not the seam's.
    ///
    /// **That refusal happens here**, with `InvalidDescriptor` and both sides of
    /// the arithmetic in the message, rather than at the pipeline that would
    /// have been built on it or at the draw that would have bound nothing.
    /// `crcbl-webgpu` is the one backend where it does not: its
    /// implementation encodes onto a command stream and returns before a browser
    /// has seen the layout, so it refuses nothing itself and every failure —
    /// WebGPU's own per-stage binding caps included — arrives later through
    /// [`take_error`](Device::take_error).
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::Unsupported`] if it declares
    /// push constants on a device without
    /// [`Features::PUSH_CONSTANTS`] — loudly, rather than dropping the writes
    /// later — or if
    /// [`PushConstantRange::stages`](crate::PushConstantRange::stages) names a
    /// mesh or task stage the device does not report, per
    /// [`ShaderStages::check_supported`](crate::ShaderStages::check_supported).
    ///
    /// [`HalError::InvalidDescriptor`] for more bind-group layouts than
    /// [`Limits::max_bind_groups`](crate::Limits::max_bind_groups), for a range
    /// ending past
    /// [`Limits::max_push_constant_size`](crate::Limits::max_push_constant_size),
    /// and for a layout inside both of those that the backend's own shared
    /// budget cannot hold.
    fn create_pipeline_layout(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError>;

    /// Destroys a pipeline layout.
    fn destroy_pipeline_layout(&self, layout: PipelineLayoutHandle);

    /// Creates a graphics pipeline.
    ///
    /// # Errors
    ///
    /// [`HalError::PipelineCreation`] with the driver's message, or
    /// [`HalError::InvalidHandle`] for a stale layout or shader module.
    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError>;

    /// Creates a **mesh** pipeline — the primary geometry path, per
    /// `docs/plan/03-gpu-driven-rendering.md` §3.5.
    ///
    /// It produces a [`GraphicsPipelineHandle`], and is bound and destroyed
    /// exactly like one; [`MeshPipelineDesc`](crate::MeshPipelineDesc) explains
    /// why a sibling descriptor shares the handle type rather than adding a
    /// second one.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] on a device that does not report
    /// [`Features::MESH_SHADER`], or whose
    /// [`MeshPipelineDesc::task`](crate::MeshPipelineDesc::task) is `Some` on a
    /// device without [`Features::TASK_SHADER`]. **Refusing here is the
    /// contract**, not an implementation detail: a backend that accepted the
    /// descriptor and failed at the draw would put the diagnosis a frame away
    /// from the mistake, and `docs/plan/39-capabilities.md` requires an absent
    /// capability to be a named, loud failure rather than a quiet one.
    ///
    /// Otherwise as [`Device::create_graphics_pipeline`].
    fn create_mesh_pipeline(
        &self,
        desc: &crate::MeshPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError>;

    /// Destroys a graphics pipeline. Also destroys a mesh pipeline — see
    /// [`Device::create_mesh_pipeline`].
    fn destroy_graphics_pipeline(&self, pipeline: GraphicsPipelineHandle);

    /// Creates a compute pipeline.
    ///
    /// # Errors
    ///
    /// As [`Device::create_graphics_pipeline`].
    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError>;

    /// Destroys a compute pipeline.
    fn destroy_compute_pipeline(&self, pipeline: ComputePipelineHandle);

    // --- queries ---

    /// Creates a query set.
    ///
    /// # A set of no queries is never created
    ///
    /// [`QuerySetDesc::count`](crate::QuerySetDesc::count) of zero is refused
    /// by every backend, and this is the contract rather than four
    /// coincidences: each API forbids it for its own reason — WebGPU's
    /// `GPUQuerySetDescriptor.count` has a minimum of one, Metal's
    /// `newBufferWithLength:options:` returns nil for a zero-length buffer —
    /// and a handle to such a set would be one whose every read is out of
    /// range, so the failure would surface at the read rather than at the
    /// mistake.
    ///
    /// Which refusal a caller gets when the device *also* lacks the feature is
    /// deliberately not specified: backends check the two in different orders,
    /// and both answers are refusals of a set that must not exist.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] if the device lacks the corresponding feature.
    /// [`HalError::InvalidDescriptor`] for a `count` of zero on a device that
    /// has the feature.
    fn create_query_set(&self, desc: &QuerySetDesc<'_>) -> Result<QuerySetHandle, HalError>;

    /// Destroys a query set.
    fn destroy_query_set(&self, set: QuerySetHandle);

    /// Reads query results directly, without a resolve-to-buffer round trip.
    ///
    /// `out.len()` results are read starting at `first_query`.
    ///
    /// # A [`QueryKind::Timestamp`](crate::QueryKind::Timestamp) result is **nanoseconds**
    ///
    /// Not a tick, and not a tick plus a scale factor the caller has to know
    /// about. The scale is a different thing in every API — Vulkan reports
    /// nanoseconds per tick (`VkPhysicalDeviceLimits::timestampPeriod`), D3D12
    /// reports its reciprocal (`ID3D12CommandQueue::GetTimestampFrequency`),
    /// WebGPU's `GPUQuerySet` results are already nanoseconds, and Metal has no
    /// fixed period at all because `MTLDevice`'s
    /// `sampleTimestamps:gpuTimestamp:` correlates the GPU clock to the host at
    /// the moment of sampling — so a seam that carried one backend's spelling
    /// of it would have two backends inventing a number to fill the field with.
    /// The unit is the seam's; each backend converts where the factor is
    /// something it actually knows.
    ///
    /// The value is **rounded to the nearest nanosecond, once, at the source**,
    /// so every caller reads the same number rather than each rounding a
    /// tick-times-float product its own way.
    ///
    /// **[`resolve_query_set`](crate::CommandEncoder::resolve_query_set) is the
    /// other read path and it does *not* convert** — it is a GPU-side copy, and
    /// there is nothing on that side to multiply with. Its destination holds
    /// whatever the device writes. See its own documentation.
    ///
    /// The other kinds count things — samples, invocations, primitives — and
    /// have no unit to convert: they come back as the device wrote them.
    ///
    /// Returns zeros on a device without
    /// [`Features::TIMESTAMP_QUERY`] rather than failing — the profiler HUD
    /// degrades, it does not break.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::InvalidDescriptor`] if the
    /// range exceeds the set.
    fn query_results(
        &self,
        set: QuerySetHandle,
        first_query: u32,
        out: &mut [u64],
    ) -> Result<(), HalError>;

    // --- synchronisation ---

    /// Creates a semaphore. See [`crate::sync`] on why timeline is the default.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] for a timeline semaphore on a device without
    /// [`Features::TIMELINE_SEMAPHORE`].
    fn create_semaphore(&self, desc: &SemaphoreDesc<'_>) -> Result<SemaphoreHandle, HalError>;

    /// Destroys a semaphore.
    fn destroy_semaphore(&self, semaphore: SemaphoreHandle);

    /// The current value of a timeline semaphore.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`], or [`HalError::Unsupported`] for a binary
    /// semaphore, which has no value to read.
    fn semaphore_value(&self, semaphore: SemaphoreHandle) -> Result<u64, HalError>;

    /// Signals a timeline semaphore to `value` from the **CPU**.
    ///
    /// The half [`semaphore_value`](Self::semaphore_value) had no counterpart
    /// for. It is what lets a submission wait on a value no earlier submission
    /// will produce: the caller opens the gate when it is ready, from whatever
    /// thread it likes. Every backend's API has it — `vkSignalSemaphore`,
    /// `ID3D12Fence::Signal`, `MTLSharedEvent`'s `setSignaledValue:` — and
    /// [`Capability::CpuTimelineSignal`](crate::Capability::CpuTimelineSignal)
    /// is the claim to perform it.
    ///
    /// # `value` must move the timeline forwards
    ///
    /// It must be **strictly greater** than the value the semaphore already
    /// holds. Vulkan refuses a lower one; D3D12 and Metal set a fence or an
    /// event backwards without any diagnostic at all, and then every waiter past
    /// the higher value stops waking on a queue that is otherwise healthy. So
    /// the rule is the seam's rather than one backend's, and it is the rule
    /// [`SemaphoreSignal::value`](crate::SemaphoreSignal::value) already carries
    /// for the submission-side signal.
    ///
    /// A backend that tracks the values submissions have already been *given* to
    /// signal holds `value` above those too, which is stricter than reading the
    /// counter: a signal sitting in a queue has not fired yet, so signalling
    /// past it would still make the timeline go backwards when it does. That is
    /// a caller error everywhere, including on Vulkan — there it is a validation
    /// error rather than a returned one.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`]; [`HalError::Unsupported`] for a binary
    /// semaphore, which has no value to signal, and on a backend with no
    /// timeline at all — see [`crate::sync`], where WebGPU is the case; and
    /// [`HalError::InvalidDescriptor`] when `value` does not move the timeline
    /// forwards, because a number the caller can correct is not a capability the
    /// backend lacks.
    fn signal_semaphore(&self, semaphore: SemaphoreHandle, value: u64) -> Result<(), HalError>;

    /// Blocks until every `(semaphore, value)` pair has been reached, or until
    /// `timeout_ns` elapses.
    ///
    /// Returns `Ok(true)` if the waits were satisfied and `Ok(false)` on
    /// timeout — a timeout is a normal outcome for a frame-pacing poll, not an
    /// error.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::DeviceLost`]; and
    /// [`HalError::Unsupported`] both for a **binary** semaphore, which carries
    /// no value for a host wait to compare against — the mirror of
    /// [`Self::signal_semaphore`]'s refusal, and what every backend here
    /// answers — and on a backend with no timeline to block on at all, see
    /// [`crate::sync`], where WebGPU is the case. A *timeout* is not an error:
    /// that is `Ok(false)`.
    fn wait_semaphores(
        &self,
        waits: &[crate::SemaphoreWait],
        timeout_ns: u64,
    ) -> Result<bool, HalError>;

    /// Blocks until the device has finished everything submitted so far.
    ///
    /// A shutdown and test primitive. Using it in a frame loop destroys
    /// pipelining; the frames-in-flight timeline is the tool for that.
    ///
    /// # Errors
    ///
    /// [`HalError::DeviceLost`].
    fn wait_idle(&self) -> Result<(), HalError>;

    // --- commands ---

    /// Creates a command encoder.
    fn create_command_encoder(&self, desc: &CommandEncoderDesc<'_>) -> Box<dyn CommandEncoder>;

    /// Releases a finished command buffer's resources.
    ///
    /// Must not be called until the submission that used it has completed.
    fn destroy_command_buffer(&self, buffer: CommandBufferHandle);

    /// Submits work to a queue.
    ///
    /// The queue must be the one named in the command buffer's
    /// [`CommandEncoderDesc::queue`]: a command buffer is created against a
    /// specific queue family, and submitting it to another is a validation
    /// error on Vulkan. The null backend does not check the match.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidHandle`] or [`HalError::DeviceLost`].
    fn submit(&self, queue: QueueHandle, submit: &SubmitInfo<'_>) -> Result<(), HalError>;

    // --- presentation ---

    /// Configures a swapchain on a surface.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::Lost`], or a [`HalError`] for an unsupported format or
    /// present mode.
    fn create_swapchain(&self, desc: &SwapchainDesc<'_>) -> Result<SwapchainHandle, SurfaceError>;

    /// Reconfigures an existing swapchain — resize, present-mode change, format
    /// change.
    ///
    /// The handle stays valid, so nothing above has to re-fetch it across a
    /// resize storm. See the [module docs](crate::swapchain) on why this exists
    /// instead of destroy-and-recreate.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::Lost`], or a [`HalError`] for an unsupported
    /// configuration.
    fn reconfigure_swapchain(
        &self,
        swapchain: SwapchainHandle,
        desc: &SwapchainDesc<'_>,
    ) -> Result<(), SurfaceError>;

    /// Destroys a swapchain.
    fn destroy_swapchain(&self, swapchain: SwapchainHandle);

    /// Acquires the next image to render into.
    ///
    /// The returned [`AcquiredFrame`] carries the synchronisation the backend
    /// needs, which is `None`/`None` on backends with an implicit acquire —
    /// see the [module docs](crate::swapchain) for why the seam is shaped this
    /// way rather than taking a caller-supplied semaphore.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::OutOfDate`] after a resize — **expected traffic**;
    /// reconfigure and retry next frame. [`SurfaceError::Lost`] if the window is
    /// gone, [`SurfaceError::Timeout`] if no image became available.
    fn acquire_next_frame(&self, swapchain: SwapchainHandle)
    -> Result<AcquiredFrame, SurfaceError>;

    /// Presents the acquired image.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::OutOfDate`] or [`SurfaceError::Lost`], handled as for
    /// [`Device::acquire_next_frame`].
    fn present(&self, queue: QueueHandle, present: &PresentInfo<'_>) -> Result<(), SurfaceError>;

    /// Blocks until the present numbered `present_id` has completed, so the
    /// caller can pace itself on the display rather than on a clock.
    ///
    /// `present_id` is one that was handed to [`present`](Self::present) as
    /// [`PresentInfo::present_id`]. **Ask for a frame or more back, never the
    /// one just submitted**: waiting on your own present drains the pipeline to
    /// a single frame and costs more than not waiting at all.
    ///
    /// # What "completed" means, and why it is not spelled more precisely
    ///
    /// The three platforms that can answer this answer slightly different
    /// questions — one knows a numbered image reached the display, one only
    /// knows that fewer than *n* presents are still outstanding, one calls back
    /// once a drawable has been shown. The guarantee the seam takes from all
    /// three is the weakest of them and the only one a frame loop needs: **when
    /// this returns, the numbered present is no longer waiting to happen**. A
    /// caller that needs a timestamp needs a different method, which does not
    /// exist yet.
    ///
    /// # A device without the capability is not an error
    ///
    /// Returns `Ok(())` immediately, without blocking, on a device that does
    /// not advertise [`Features::PRESENT_FEEDBACK`] — "there is nothing here to
    /// wait for", not "this call was wrong". The alternative, refusing, would
    /// put a branch on every frame of every caller for a condition that never
    /// changes after device creation, and a caller that skipped the branch
    /// would turn a missing capability into a failed frame. A caller that
    /// genuinely wants to know reads the flag once, at start-up.
    ///
    /// The same answer covers a `present_id` the backend has no record of —
    /// never presented, or presented before the last
    /// [`reconfigure_swapchain`](Self::reconfigure_swapchain), which restarts
    /// the numbering. That frame can no longer be waited for, and blocking
    /// until `timeout` for one that will never arrive is the worst of the
    /// available answers.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::Timeout`] if `timeout` passed with the frame still not
    /// up — **expected traffic** on a stalled compositor, and not a reason to
    /// abandon the frame; render it and let the next wait catch up.
    /// [`SurfaceError::OutOfDate`] or [`SurfaceError::Lost`], handled as for
    /// [`Device::acquire_next_frame`].
    fn wait_until_presented(
        &self,
        swapchain: SwapchainHandle,
        present_id: u64,
        timeout: Duration,
    ) -> Result<(), SurfaceError>;

    /// Asks what the display is **actually** doing with this swapchain's
    /// frames — fixed refresh, adaptive sync, or nothing it will say.
    ///
    /// [`PresentMode`](crate::PresentMode) is the request side of this: it picks a queueing
    /// discipline and tells you nothing about the panel. This is the answer,
    /// and it is the only thing in the seam that can tell a 60 Hz panel from a
    /// VRR one holding steady at 60 Hz. See [`DisplayTiming`] for what each arm
    /// promises.
    ///
    /// # This is a live query; caching it is a bug
    ///
    /// The answer changes underneath a swapchain that is running perfectly
    /// well, with no reconfigure and no error: the proposal's own example is a
    /// laptop entering power-saving mode and dropping the panel's rate, and
    /// dragging a window to a second monitor does the same thing. **Ask again**
    /// — every frame is fine, the query is a read of driver-side state — rather
    /// than reading it once at start-up as a caller may legitimately do with
    /// [`DeviceCaps::features`](crate::DeviceCaps::features).
    ///
    /// # A device without the capability is not an error
    ///
    /// Returns `Ok(DisplayTiming::Unknown)` on a device that does not advertise
    /// [`Features::PRESENT_TIMING`], exactly as
    /// [`wait_until_presented`](Self::wait_until_presented) returns `Ok(())`
    /// without [`Features::PRESENT_FEEDBACK`] — "there is nothing here to
    /// report", not "this call was wrong". [`DisplayTiming::Unknown`] is
    /// already an arm every caller must handle, so refusing would buy a second
    /// spelling of the same answer and a branch on every use.
    ///
    /// The same answer covers a swapchain that has never been presented to (the
    /// platform may not know yet), one that has no display behind it at all
    /// (an offscreen ring), and a platform that reports a cycle length without
    /// its dynamics.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::Hal`] wrapping [`HalError::ForeignObject`] for a
    /// `swapchain` created by a different device, per obligation 3, and
    /// [`HalError::InvalidHandle`] for one that has been destroyed —
    /// **validated before the capability is consulted**, so a backend that
    /// cannot answer still catches the caller bug. [`SurfaceError::Lost`] if
    /// the window is gone.
    fn display_timing(&self, swapchain: SwapchainHandle) -> Result<DisplayTiming, SurfaceError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The crate's central claim: both traits are usable as trait objects. If
    /// anyone adds a generic method, an `impl Trait` return, or a
    /// `where Self: Sized`, this stops compiling — which is the point.
    #[test]
    fn the_seam_is_object_safe() {
        const _INSTANCE: Option<&dyn Instance> = None;
        const _DEVICE: Option<&dyn Device> = None;
        const _ENCODER: Option<&dyn CommandEncoder> = None;
        const _PENDING: Option<&dyn PendingDevice> = None;
        // Boxing each one is the property that matters; naming all three in a
        // single function type only trips `clippy::type_complexity`.
        fn boxes_instance(_: Box<dyn Instance>) {}
        fn boxes_device(_: Box<dyn Device>) {}
        fn boxes_encoder(_: Box<dyn CommandEncoder>) {}
        fn boxes_pending(_: Box<dyn PendingDevice>) {}
        let _ = (
            boxes_instance as fn(_),
            boxes_device as fn(_),
            boxes_encoder as fn(_),
            boxes_pending as fn(_),
        );
    }

    /// `DeviceRequestState` answers two things, not three: the payload rides in
    /// `Ready` and failure leaves through `Err`, exactly as `ReadbackState` does.
    #[test]
    fn a_pending_request_yields_nothing_and_says_so() {
        let pending = DeviceRequestState::Pending;
        assert!(!pending.is_ready());
        assert!(pending.into_device().is_none());
    }

    #[test]
    fn default_device_desc_requires_only_the_unavoidable_and_no_surface() {
        let desc = DeviceDesc::for_adapter(AdapterId(0));
        assert_eq!(
            desc.required_features,
            Features::COMPUTE | Features::TIMELINE_SEMAPHORE
        );
        assert!(
            desc.compatible_surface.is_none(),
            "the headless shape must not require a window"
        );
        assert_eq!(desc.optional_features, Features::GPU_DRIVEN);
        // The point of the split: everything the GPU-driven path wants beyond
        // the two above degrades instead of refusing the device. Named one at a
        // time, because a bundle comparison would still pass if one flag leaked
        // back across the line.
        for optional in [
            Features::DESCRIPTOR_INDEXING,
            Features::BUFFER_DEVICE_ADDRESS,
            Features::DRAW_INDIRECT_COUNT,
            Features::MULTI_DRAW_INDIRECT,
        ] {
            assert!(
                !desc.required_features.contains(optional),
                "{optional:?} must be optional, not demanded"
            );
        }
    }
}
