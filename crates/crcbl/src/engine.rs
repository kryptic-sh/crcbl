//! The shell↔HAL join, once, so a sample does not carry its own copy.
//!
//! # Why this is here
//!
//! `apps/sandbox/src/gpu.rs` and `apps/breakout/src/gpu.rs` were the same file
//! twice: `open()` was 87% identical and the
//! `frame`/`record_and_submit`/`retire_to`/`resize`/`reconfigure`/`destroy`
//! block ran ~90% the same over roughly 170 lines each, with `SwapchainConfig`,
//! `FrameOutcome`, a `GpuError` enum and all four of its `From` impls duplicated
//! verbatim. Their `app.rs` files were 69% identical, down to `Clock`,
//! `Pending`, `wait_for_configure`, `CONFIGURE_TIMEOUT`, `WINDOWED_IDLE`,
//! `HEADLESS_FRAME_STEP` and `frame_budget()`.
//!
//! Only one of the two copies carried the ninety lines of design rationale that
//! explain *why* the join is shaped this way, so the pair could only ever drift
//! — and the sandbox's own module docs already said where this belongs:
//!
//! > The right home is an engine-setup helper in the `crcbl` umbrella, which is
//! > where both seams already meet.
//!
//! This is that helper. `crcbl/Cargo.toml` sanctions it: "it must never grow
//! logic of its own beyond the engine-setup helpers
//! `docs/plan/01-foundations.md`'s workspace layout allows it".
//!
//! # What it is not
//!
//! It is **not** `crcbl::run(game)`. The crate docs' "no engine loop" rule
//! stands: the outer loop stays in the app, because on wasm that outer loop is
//! `requestAnimationFrame`, which calls the engine and cannot be called by it.
//! What is shared here is the *plumbing between two frames* — acquire, submit,
//! present, retire, reconfigure — and the event/clock bookkeeping around it.
//! Every app still writes its own `frame()`, its own render graph and its own
//! `tick`.
//!
//! # There are no barriers in this file
//!
//! There used to be two, hand-written, around the render pass:
//!
//! ```text
//! barrier Undefined → ColorAttachment
//! render pass: clear + triangle
//! barrier ColorAttachment → Present
//! ```
//!
//! `docs/plan/02-vulkan-backend.md` §2.4 says "**no manual barriers outside the
//! graph, ever**", and both are gone. What replaced them is a *declaration* —
//! the swapchain image is imported into the graph saying "it arrives
//! [`Undefined`](crcbl_hal::ResourceState::Undefined) and must leave
//! [`Present`](crcbl_hal::ResourceState::Present)" — and the graph computes the
//! rest. [`ForwardRenderer::present_target`](crcbl_render::ForwardRenderer::present_target)
//! is the helper that writes that declaration.
//!
//! The frame is:
//!
//! ```text
//! acquire → build the graph → compile → execute (barriers computed)
//!         → submit(wait acquire, signal present + timeline)
//!         → present(wait present) → retire the command buffer
//! ```
//!
//! # Frames in flight, not `wait_idle`
//!
//! [`Device::destroy_command_buffer`] may not be called until the submission
//! that used it has completed, and the seam offers exactly two ways to know
//! that: a timeline semaphore, or [`Device::wait_idle`] — which the seam itself
//! documents as "a shutdown and test primitive" that "destroys pipelining". So
//! [`GpuContext`] keeps a ring keyed on a timeline semaphore value, and falls
//! back to `wait_idle` only on a Tier B device that has no timeline semaphores.
//!
//! # What the join revealed
//!
//! P0.7 was the first time anything drove both seams at once, and
//! `docs/plan/01-foundations.md` freezes neither at P0. The findings are kept
//! here because this is where they were found.
//!
//! 1. **Two sources of truth for the swapchain extent, with no stated
//!    precedence** — *fixed in the seam.*
//!    [`WindowState::size`](crcbl_shell::WindowState::size) is one;
//!    [`SurfaceCaps::current_extent`](crcbl_hal::SurfaceCaps::current_extent)
//!    is the other, and on Vulkan it is a real size on X11 and deliberately
//!    `0xFFFFFFFF` ("you choose") on Wayland. `crcbl-hal`'s
//!    [`swapchain`](crcbl_hal::swapchain) module states the rule as four
//!    numbered backend obligations, and [`GpuContext::open`] is the reference
//!    implementation of the caller's half.
//! 2. **[`SurfaceTarget::Offscreen`](crcbl_core::SurfaceTarget) embedded a size,
//!    so a headless target went stale on resize** — *fixed by deleting the
//!    size.* [`GpuContext::resize`] therefore reconfigures the swapchain and
//!    nothing else, on every backend.
//! 3. **`unsafe` at the join is unavoidable.** [`Instance::create_surface`] is
//!    `unsafe` because it dereferences platform handles, and the safety
//!    obligation ("these outlive the surface") is one only the code holding
//!    *both* the shell and the device can discharge. That code is now here,
//!    once, instead of in each app.
//! 4. **Teardown order is stated in three places and enforced in none.** The
//!    swapchain must die before the surface, the surface before the window, and
//!    the device may outlive its instance. [`GpuContext::destroy`] does it by
//!    hand; at P1.1 a real driver with validation on agreed.
//! 5. **The swapchain's configured extent was unobservable** — *fixed in the
//!    seam*, [`AcquiredFrame::extent`].
//! 6. **A render pass needed a view the seam would not give it** — *fixed in
//!    the seam*, [`AcquiredFrame::view`].

use std::collections::VecDeque;
use std::time::Duration;

use crcbl_core::time::{ManualTime, MonotonicTime, TimeSource};
use crcbl_hal::{
    AcquiredFrame, CommandBufferHandle, DeviceDesc, Features, Format, HalError, PresentInfo,
    PresentMode, QueueHandle, QueueKind, SemaphoreDesc, SemaphoreHandle, SemaphoreKind,
    SemaphoreSignal, SemaphoreWait, SubmitInfo, SurfaceError, SurfaceHandle, SwapchainDesc,
    SwapchainHandle,
};
use crcbl_hal::{Device, Instance};
use crcbl_shell::{CloseReply, PhysicalSize, Shell, ShellError, ShellEvent, WindowId};

use crate::backend::GpuBackend;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How long to wait for the window to configure before giving up.
pub const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(10);

/// Advisory idle for windowed frames, handed to [`Shell::wait_events`].
pub const WINDOWED_IDLE: Duration = Duration::from_millis(4);

/// The simulated step a headless frame advances by: a 60 Hz wall clock.
pub const HEADLESS_FRAME_STEP: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// How many consecutive frames may fail to present before a loop gives up.
///
/// A frame budget counts *presented* frames, so a swapchain that is permanently
/// suboptimal — or permanently out of date — would reconfigure forever and
/// never reach the budget, and `--frames N` would never terminate. Four seconds
/// of 60 Hz reconfiguring is far past "a resize storm" and squarely in "this
/// surface will never present".
pub const MAX_CONSECUTIVE_RECONFIGURES: u32 = 240;

/// How many frames may be in flight before the loop waits for the oldest.
///
/// Two is the classic double-buffered default: one frame being recorded while
/// one is executing. It is `crcbl-render`'s constant because the uniform ring
/// has to be the same depth — one buffer per frame in flight, or a spinning
/// camera is a read-after-write hazard across submissions.
pub const FRAMES_IN_FLIGHT: usize = crcbl_render::forward::FRAMES_IN_FLIGHT;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// What can go wrong between the window and the device.
#[derive(Debug)]
pub enum GpuError {
    /// No GPU backend could be opened at all.
    NoBackend(crate::backend::GpuError),
    /// The backend has no adapter, no graphics queue, or no usable format.
    Unusable(&'static str),
    /// A HAL call failed.
    Hal(HalError),
    /// A surface or swapchain call failed.
    Surface(SurfaceError),
    /// The render graph refused the frame.
    Graph(crcbl_render::GraphError),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBackend(error) => write!(
                f,
                "{error}\n\
                 hint: `--backend null` runs the same loop against the recording \
                 no-op backend, which needs no driver."
            ),
            Self::Unusable(what) => write!(f, "the backend is unusable: {what}"),
            Self::Hal(error) => write!(f, "{error}"),
            Self::Surface(error) => write!(f, "{error}"),
            Self::Graph(error) => write!(f, "render graph: {error}"),
        }
    }
}

impl std::error::Error for GpuError {}

impl From<crate::backend::GpuError> for GpuError {
    fn from(error: crate::backend::GpuError) -> Self {
        Self::NoBackend(error)
    }
}

impl From<HalError> for GpuError {
    fn from(error: HalError) -> Self {
        Self::Hal(error)
    }
}

impl From<SurfaceError> for GpuError {
    fn from(error: SurfaceError) -> Self {
        Self::Surface(error)
    }
}

impl From<crcbl_render::GraphError> for GpuError {
    fn from(error: crcbl_render::GraphError) -> Self {
        Self::Graph(error)
    }
}

// ---------------------------------------------------------------------------
// Frame outcome
// ---------------------------------------------------------------------------

/// What one presented frame did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    /// The frame was recorded, submitted and presented.
    Presented,
    /// The swapchain no longer matched the surface, so it was reconfigured and
    /// this frame was skipped. Expected traffic during a resize, not an error.
    Reconfigured,
}

// ---------------------------------------------------------------------------
// Swapchain config
// ---------------------------------------------------------------------------

/// The swapchain parameters, kept so a reconfigure changes exactly one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwapchainConfig {
    label: &'static str,
    format: Format,
    extent: (u32, u32),
    image_count: u32,
    present_mode: PresentMode,
}

impl SwapchainConfig {
    fn desc(self, surface: SurfaceHandle) -> SwapchainDesc<'static> {
        SwapchainDesc {
            label: Some(self.label),
            surface,
            format: self.format,
            extent: self.extent,
            image_count: self.image_count,
            present_mode: self.present_mode,
            composite_alpha: crcbl_hal::CompositeAlpha::Opaque,
        }
    }
}

// ---------------------------------------------------------------------------
// GpuContext
// ---------------------------------------------------------------------------

/// What [`GpuContext::open`] should ask the device for.
#[derive(Clone, Copy, Debug)]
pub struct GpuContextDesc<'a> {
    /// Debug label for the device and the swapchain.
    pub label: &'a str,
    /// Which backend to open, or `None` to let [`crate::backend::open`] choose.
    pub backend: Option<GpuBackend>,
    /// Features the device must have. A device without them is an error.
    pub required_features: Features,
    /// Features to enable if present. Absent ones are simply not enabled — ask
    /// the returned [`GpuContext::device`] what it got.
    pub optional_features: Features,
}

impl Default for GpuContextDesc<'_> {
    fn default() -> Self {
        Self {
            label: "crcbl",
            backend: None,
            // Nothing here needs a feature, and demanding `TIER_A` would refuse
            // to run on the Tier B devices `docs/plan/02-vulkan-backend.md`
            // requires the engine to support. Ask for everything optionally and
            // branch on what came back. `TIMESTAMP_QUERY` is deliberately not
            // part of `TIER_A` — topic 10's browsers may lack it — so the
            // per-pass timers have to be asked for by name.
            required_features: Features::empty(),
            optional_features: Features::TIER_A
                | Features::TIMESTAMP_QUERY
                | Features::DEBUG_MARKERS,
        }
    }
}

/// The engine's GPU side, driven entirely through the `crcbl-hal` seam.
///
/// Nothing in this struct names a backend. [`GpuContext::open`] asks
/// [`crate::backend::open_backend`] for one **by value** and everything after
/// it is `dyn Instance` / `dyn Device` — which is what made P1.1's swap from
/// the null backend to `crcbl-vk` a change to one argument.
#[derive(Debug)]
pub struct GpuContext {
    instance: Box<dyn Instance>,
    device: Box<dyn Device>,
    queue: QueueHandle,
    surface: SurfaceHandle,
    swapchain: SwapchainHandle,
    /// Everything `create_swapchain` was last called with, so a resize
    /// reconfigures with one field changed rather than a fresh guess.
    config: SwapchainConfig,
    /// `None` on a device without timeline semaphores; see the module docs.
    timeline: Option<SemaphoreHandle>,
    /// Submissions issued so far, and therefore the value the next one signals.
    submitted: u64,
    in_flight: VecDeque<(u64, CommandBufferHandle)>,
    /// The extent the swapchain was last *configured* at, from
    /// [`AcquiredFrame::extent`]. Distinct from what the shell asked for.
    configured_extent: (u32, u32),
    /// Scratch, reused every frame so a steady-state frame allocates nothing.
    waits: Vec<SemaphoreWait>,
    signals: Vec<SemaphoreSignal>,
}

/// How far [`PendingGpuContext`] has got.
///
/// Exactly the two steps that can take longer than an instant, in order: the
/// backend instance (adapter enumeration is a promise on the web) and then the
/// device (`requestDevice` is a promise everywhere). Everything between them —
/// surface creation, surface-aware adapter selection, format and present-mode
/// choice — is synchronous on every backend and happens in one step when the
/// instance lands.
#[derive(Debug)]
enum OpenStage {
    Instance(crate::backend::PendingInstance),
    Device {
        instance: Box<dyn Instance>,
        surface: SurfaceHandle,
        config: SwapchainConfig,
        pending: Box<dyn crcbl_hal::PendingDevice>,
    },
    /// The context has been handed over, or a step failed.
    Done,
}

/// A [`GpuContext`] being opened, one poll at a time.
///
/// From [`GpuContext::request_open`]. Poll it until it yields; see
/// [`crcbl_hal::device`] for why start-up is shaped this way rather than
/// blocking, and [`crate::backend`] for the instance half of it.
///
/// Dropping one mid-flight abandons the open. The surface, if one was already
/// created, is destroyed with the instance — the same teardown obligation
/// [`GpuContext::destroy`] discharges, except that no swapchain exists yet.
#[derive(Debug)]
pub struct PendingGpuContext {
    stage: OpenStage,
    target: crcbl_hal::SurfaceTarget,
    extent: (u32, u32),
    label: String,
    required_features: Features,
    optional_features: Features,
}

impl PendingGpuContext {
    /// Advances the open. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened, if the backend exposes no adapter, no
    /// graphics queue or no surface format, or if any HAL call failed. Polling
    /// after the context was handed over is a caller bug and reports
    /// [`GpuError::Unusable`].
    pub fn poll(&mut self) -> Result<Option<GpuContext>, GpuError> {
        loop {
            match core::mem::replace(&mut self.stage, OpenStage::Done) {
                OpenStage::Instance(mut pending) => {
                    let Some(instance) = pending.poll()? else {
                        self.stage = OpenStage::Instance(pending);
                        return Ok(None);
                    };
                    // The stage is left `Done` if this fails, which is what
                    // makes a failed open stay failed rather than retrying a
                    // half-built context on the next frame.
                    self.stage = GpuContext::start_device(
                        instance,
                        &self.target,
                        self.extent,
                        &self.label,
                        self.required_features,
                        self.optional_features,
                    )?;
                }
                OpenStage::Device {
                    instance,
                    surface,
                    config,
                    mut pending,
                } => match pending.poll()? {
                    crcbl_hal::DeviceRequestState::Pending => {
                        self.stage = OpenStage::Device {
                            instance,
                            surface,
                            config,
                            pending,
                        };
                        return Ok(None);
                    }
                    crcbl_hal::DeviceRequestState::Ready(device) => {
                        return GpuContext::finish(instance, surface, config, device, self.extent)
                            .map(Some);
                    }
                },
                OpenStage::Done => {
                    return Err(GpuError::Unusable("this GPU context was already opened"));
                }
            }
        }
    }
}

impl GpuContext {
    /// Creates an instance, a surface for `window`, a device and a swapchain.
    ///
    /// `extent` must come from the window system — call this only after the
    /// first configure, because a swapchain needs a size and an unconfigured
    /// window does not have one.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened, if the backend exposes no adapter, no
    /// graphics queue or no surface format, or if any HAL call fails.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        desc: &GpuContextDesc<'_>,
    ) -> Result<Self, GpuError> {
        let mut pending = Self::request_open(shell, window, extent, desc)?;
        loop {
            if let Some(context) = pending.poll()? {
                return Ok(context);
            }
            std::thread::yield_now();
        }
    }

    /// Starts opening the instance, surface, device and swapchain, without
    /// blocking.
    ///
    /// The non-blocking form of [`open`](Self::open), and the only one a browser
    /// can use: both halves of start-up that a browser defers — adapter
    /// enumeration and `requestDevice` — are promises there. Poll the returned
    /// [`PendingGpuContext`] once per rAF frame until it yields.
    ///
    /// The window must stay alive from this call until the context is destroyed,
    /// which is the same obligation [`open`](Self::open) already carries; the
    /// surface is created part-way through, once an instance exists.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the registry has no such backend or the window went away
    /// before its surface could be described. Everything else is reported from
    /// [`PendingGpuContext::poll`].
    pub fn request_open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        desc: &GpuContextDesc<'_>,
    ) -> Result<PendingGpuContext, GpuError> {
        // The line that used to name `NullInstance`. It now names a *value*
        // from a registry, which is the whole difference between "the sample
        // knows about Vulkan" and "the sample knows there are backends".
        let instance = match desc.backend {
            Some(backend) => crate::backend::request_open_backend(backend)?,
            None => crate::backend::request_open()?,
        };

        // The join. `shell` produced this; only a HAL backend looks inside it.
        // Taken now rather than at poll time so the shell is not borrowed for
        // the whole of start-up.
        let target = shell
            .surface_target(window)
            .map_err(|_| GpuError::Unusable("the window went away before its surface was made"))?;
        log::debug!(
            "hal: creating a surface for a {} target",
            target.platform_name()
        );

        Ok(PendingGpuContext {
            stage: OpenStage::Instance(instance),
            target,
            extent,
            label: desc.label.to_string(),
            required_features: desc.required_features,
            optional_features: desc.optional_features,
        })
    }

    /// Creates the surface, picks an adapter and starts the device request.
    fn start_device(
        instance: Box<dyn Instance>,
        target: &crcbl_hal::SurfaceTarget,
        extent: (u32, u32),
        label: &str,
        required_features: Features,
        optional_features: Features,
    ) -> Result<OpenStage, GpuError> {
        let adapters = instance.adapters();
        if adapters.is_empty() {
            return Err(GpuError::Unusable("no adapter"));
        }

        // SAFETY: `target` was produced by the caller's shell for a window that
        // must still be live — `request_open` documents that obligation and it
        // is the same one `open` has always carried — so every handle in it
        // names an object of the stated kind. The caller keeps the shell, and
        // therefore the window, alive until after `destroy`, which tears the
        // swapchain and surface down first. This is the whole
        // `Instance::create_surface` contract.
        let surface = unsafe { instance.create_surface(target) }?;

        // **Adapter selection is surface-aware, and has to be.** P1.1 found a
        // discrete radv GPU that enumerates first, is Tier A, and *cannot
        // present to an Xvfb window* — while the software rasteriser behind it
        // can. An `Err` here means "not this one", not "give up".
        let mut chosen = None;
        let mut last_error = None;
        for adapter in &adapters {
            match instance.surface_caps(surface, adapter.id) {
                Ok(caps) if caps.preferred_format().is_some() => {
                    chosen = Some((adapter.clone(), caps));
                    break;
                }
                Ok(_) => log::debug!(
                    "hal: adapter {:?} offers no usable surface format; trying the next",
                    adapter.name
                ),
                Err(error) => {
                    log::debug!(
                        "hal: adapter {:?} cannot serve this surface ({error}); trying the next",
                        adapter.name
                    );
                    last_error = Some(error);
                }
            }
        }
        let Some((adapter, caps)) = chosen else {
            instance.destroy_surface(surface);
            return Err(match last_error {
                Some(error) => error.into(),
                None => GpuError::Unusable("no adapter can present to this window"),
            });
        };
        log::info!(
            "hal: {} adapter {:?} ({:?}), tier {:?}",
            instance.backend(),
            adapter.name,
            adapter.device_type,
            adapter.caps.tier()
        );

        if let Some(reported) = caps.current_extent
            && reported != extent
        {
            log::info!(
                "hal: surface reports {reported:?} but the shell configured {extent:?}; \
                 using the shell's size"
            );
        }
        let format = caps
            .preferred_format()
            .ok_or(GpuError::Unusable("the surface offers no format"))?;
        let present_mode = caps.choose_present_mode(&[PresentMode::Mailbox, PresentMode::Fifo]);
        let image_count = caps
            .min_image_count
            .saturating_add(1)
            .min(caps.max_image_count);

        let config = SwapchainConfig {
            label: "crcbl swapchain",
            format,
            extent,
            image_count,
            present_mode,
        };

        // The one call that may take more than an instant. On the web it is a
        // promise; here it is a `PendingDevice` the caller polls.
        let pending = instance.request_device(&DeviceDesc {
            label: Some(label),
            adapter: adapter.id,
            required_features,
            optional_features,
            compatible_surface: Some(surface),
        })?;

        Ok(OpenStage::Device {
            instance,
            surface,
            config,
            pending,
        })
    }

    /// Builds the context once the device has arrived.
    fn finish(
        instance: Box<dyn Instance>,
        surface: SurfaceHandle,
        config: SwapchainConfig,
        device: Box<dyn Device>,
        extent: (u32, u32),
    ) -> Result<Self, GpuError> {
        let queue = device
            .queue(QueueKind::Graphics)
            .ok_or(GpuError::Unusable("no graphics queue"))?;

        let swapchain = device.create_swapchain(&config.desc(surface))?;
        let (format, present_mode, image_count) =
            (config.format, config.present_mode, config.image_count);
        log::info!(
            "hal: swapchain {}x{} {format:?} {present_mode:?} ({image_count} images)",
            extent.0,
            extent.1
        );

        let timeline = if device
            .caps()
            .features
            .contains(Features::TIMELINE_SEMAPHORE)
        {
            Some(device.create_semaphore(&SemaphoreDesc {
                label: Some("crcbl frames in flight"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })?)
        } else {
            log::debug!("hal: no timeline semaphores; retiring command buffers with wait_idle");
            None
        };

        Ok(Self {
            instance,
            device,
            queue,
            surface,
            swapchain,
            config,
            timeline,
            submitted: 0,
            in_flight: VecDeque::with_capacity(FRAMES_IN_FLIGHT + 1),
            configured_extent: extent,
            waits: Vec::with_capacity(1),
            signals: Vec::with_capacity(2),
        })
    }

    /// The device, for building renderers and recording work.
    #[must_use]
    pub fn device(&self) -> &dyn Device {
        self.device.as_ref()
    }

    /// The graphics queue everything is submitted to.
    #[must_use]
    pub const fn queue(&self) -> QueueHandle {
        self.queue
    }

    /// The format the swapchain was created with — what a pipeline rendering
    /// into it has to name.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.config.format
    }

    /// The swapchain's current size — the one it was **configured** at.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.configured_extent
    }

    /// Acquires the next swapchain image, or reconfigures and says so.
    ///
    /// `Ok(None)` means the swapchain was out of date and has been
    /// reconfigured: skip this frame and try the next one.
    ///
    /// # Errors
    ///
    /// [`GpuError`] for anything except a swapchain that has merely gone out of
    /// date, which is handled here.
    pub fn acquire(&mut self) -> Result<Option<AcquiredFrame>, GpuError> {
        let acquired = match self.device.acquire_next_frame(self.swapchain) {
            Ok(frame) => frame,
            // Expected traffic after a resize, per the seam's docs: reconfigure
            // and let the next frame have the image.
            Err(SurfaceError::OutOfDate) => {
                self.reconfigure()?;
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };

        // Obligation 3: the answer, not the request.
        if acquired.extent != self.configured_extent {
            log::debug!(
                "hal: swapchain configured at {:?} (the shell asked for {:?})",
                acquired.extent,
                self.config.extent
            );
            self.configured_extent = acquired.extent;
            // Track the answer in `config` too. `resize` early-returns when the
            // requested extent already matches `config.extent`, so leaving
            // `config` holding the size the shell asked for made a resize back
            // to the compositor's chosen size look like a change and triggered
            // a reconfigure that changed nothing.
            self.config.extent = acquired.extent;
        }
        Ok(Some(acquired))
    }

    /// Submits `command_buffer` for `acquired`, presents it, and retires the
    /// oldest command buffer once its submission has completed.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the submit, the present or the retire failed. A present
    /// that reports the swapchain out of date is not an error: it reconfigures
    /// and reports [`FrameOutcome::Reconfigured`].
    pub fn submit_and_present(
        &mut self,
        acquired: &AcquiredFrame,
        command_buffer: CommandBufferHandle,
    ) -> Result<FrameOutcome, GpuError> {
        self.submitted += 1;
        let value = self.submitted;

        self.waits.clear();
        self.waits
            .extend(acquired.acquire_semaphore.map(|semaphore| SemaphoreWait {
                semaphore,
                value: 0,
            }));
        self.signals.clear();
        self.signals
            .extend(acquired.present_semaphore.map(|semaphore| SemaphoreSignal {
                semaphore,
                value: 0,
            }));
        // A timeline value is monotonic, so the frame number *is* the value and
        // no rotation is needed.
        self.signals.extend(
            self.timeline
                .map(|semaphore| SemaphoreSignal { semaphore, value }),
        );

        self.device.submit(
            self.queue,
            &SubmitInfo {
                command_buffers: &[command_buffer],
                waits: &self.waits,
                signals: &self.signals,
            },
        )?;
        self.in_flight.push_back((value, command_buffer));
        self.retire_to(FRAMES_IN_FLIGHT)?;

        match self.device.present(
            self.queue,
            &PresentInfo {
                swapchain: self.swapchain,
                waits: acquired.present_semaphore.as_slice(),
            },
        ) {
            Ok(()) => {}
            // **Present is the usual place a resize is noticed**, not acquire.
            Err(SurfaceError::OutOfDate) => {
                self.reconfigure()?;
                return Ok(FrameOutcome::Reconfigured);
            }
            Err(error) => return Err(error.into()),
        }

        if acquired.suboptimal {
            log::debug!("hal: swapchain suboptimal; reconfiguring after present");
            self.reconfigure()?;
        }
        Ok(FrameOutcome::Presented)
    }

    /// Waits for and destroys command buffers until at most `keep` are in
    /// flight.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if waiting failed.
    pub fn retire_to(&mut self, keep: usize) -> Result<(), GpuError> {
        while self.in_flight.len() > keep {
            let (value, command_buffer) = self
                .in_flight
                .pop_front()
                .unwrap_or_else(|| unreachable!("the queue is non-empty above"));
            match self.timeline {
                Some(semaphore) => {
                    self.device
                        .wait_semaphores(&[SemaphoreWait { semaphore, value }], u64::MAX)?;
                }
                // The Tier B fallback. Correct, and coarse enough that the log
                // line in `open` exists to explain the frame rate.
                None => self.device.wait_idle()?,
            }
            self.device.destroy_command_buffer(command_buffer);
        }
        Ok(())
    }

    /// Resizes the swapchain to `extent`.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the reconfigure failed. A zero extent is *not* an error
    /// — a minimized window reports one, and the swapchain is left alone.
    pub fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        if extent == self.config.extent {
            return Ok(());
        }
        if extent.0 == 0 || extent.1 == 0 {
            log::debug!("hal: window has an empty extent {extent:?}; keeping the swapchain");
            return Ok(());
        }
        self.config.extent = extent;
        self.reconfigure()
    }

    fn reconfigure(&mut self) -> Result<(), GpuError> {
        log::debug!(
            "hal: reconfiguring the swapchain to {}x{}",
            self.config.extent.0,
            self.config.extent.1
        );
        // Reconfigure, never destroy-and-recreate: the handle stays valid
        // across a resize storm, which is what the seam promises callers.
        self.device
            .reconfigure_swapchain(self.swapchain, &self.config.desc(self.surface))?;
        Ok(())
    }

    /// Waits for the device to go idle and retires everything still in flight.
    ///
    /// Call this before destroying anything the app owns — renderers, pools,
    /// timers — and [`GpuContext::destroy`] afterwards.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if waiting failed.
    pub fn drain(&mut self) -> Result<(), GpuError> {
        self.device.wait_idle()?;
        self.retire_to(0)
    }

    /// Tears the swapchain and the surface down, in the order the seam
    /// requires. The device must already be idle — call [`GpuContext::drain`]
    /// and release the app's own objects first.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if a final wait failed.
    pub fn destroy(mut self) -> Result<(), GpuError> {
        self.drain()?;
        if let Some(semaphore) = self.timeline.take() {
            self.device.destroy_semaphore(semaphore);
        }
        // Swapchain before surface, surface before the window the caller owns;
        // the device is allowed to outlive its instance, so their drop order is
        // the one the struct declares.
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

/// A time source a loop can drive, whichever kind it is.
///
/// The variants exist so the *loop* stays free of `if headless`: it calls
/// [`Clock::advance`] once per frame and gets a timestamp, and the difference
/// between "read the real clock" and "step the fake one" lives here. A headless
/// run therefore produces the same frame and tick counts on every machine,
/// which is the whole reason CI can assert them.
#[derive(Debug)]
pub enum Clock {
    /// The real monotonic clock.
    Real(MonotonicTime),
    /// A fake clock stepped by a fixed amount each frame.
    Manual {
        /// The current reading.
        time: ManualTime,
        /// How far one frame advances it.
        step: Duration,
    },
}

impl Clock {
    /// A real clock, or a manual one stepping [`HEADLESS_FRAME_STEP`].
    #[must_use]
    pub fn new(headless: bool) -> Self {
        if headless {
            Self::manual(HEADLESS_FRAME_STEP)
        } else {
            Self::Real(MonotonicTime::new())
        }
    }

    /// A manual clock with an explicit per-frame step, for a test that wants to
    /// drive the loop at a frame rate other than 60.
    #[must_use]
    pub fn manual(step: Duration) -> Self {
        Self::Manual {
            time: ManualTime::new(),
            step,
        }
    }

    /// The current reading, without advancing anything.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        match self {
            Self::Real(time) => time.elapsed(),
            Self::Manual { time, .. } => time.elapsed(),
        }
    }

    /// Moves to the next frame's timestamp and returns it.
    pub fn advance(&mut self) -> Duration {
        match self {
            Self::Real(time) => time.elapsed(),
            Self::Manual { time, step } => {
                time.advance(*step);
                time.elapsed()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Event sink
// ---------------------------------------------------------------------------

/// What one [`Shell::pump`] batch asked the loop to do.
///
/// Recorded rather than acted on inline because the sink borrows this mutably
/// while `shell` is borrowed mutably too — which is exactly why replies
/// (`reply_close_request`, `resize`) happen after the pump returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    /// Events observed, of every kind.
    pub count: u64,
    /// The most recent size the window reported.
    pub resized: Option<PhysicalSize>,
    /// The window system asked the window to close.
    pub close_requested: bool,
    /// The window went away without asking.
    pub destroyed: bool,
}

impl Pending {
    /// Folds one event in.
    pub fn observe(&mut self, event: &ShellEvent) {
        self.count += 1;
        match event {
            ShellEvent::Resized { size, .. } | ShellEvent::ScaleFactorChanged { size, .. } => {
                self.resized = Some(*size);
            }
            ShellEvent::CloseRequested { .. } => self.close_requested = true,
            ShellEvent::WindowDestroyed { .. } => self.destroyed = true,
            _ => {}
        }
        log::debug!("shell event: {event:?}");
    }
}

/// Why a loop stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    /// The frame budget ran out. The only way a headless run ends.
    FrameBudget,
    /// The window system asked the window to close and it was allowed to.
    CloseRequested,
    /// The window went away without asking.
    WindowDestroyed,
    /// A frame failed. The loop stopped, and teardown still ran.
    Failed,
}

impl ExitReason {
    /// Whether the window still exists and therefore still needs destroying.
    #[must_use]
    pub const fn window_survives(self) -> bool {
        !matches!(self, Self::CloseRequested | Self::WindowDestroyed)
    }
}

/// What one turn of a loop decided about the next one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Keep going.
    Continue,
    /// Stop, for this reason.
    Stop(ExitReason),
}

// ---------------------------------------------------------------------------
// Window bring-up
// ---------------------------------------------------------------------------

/// Why [`wait_for_configure`] gave up.
#[derive(Debug)]
pub enum ConfigureError {
    /// The shell refused a call.
    Shell(ShellError),
    /// No configure arrived within [`CONFIGURE_TIMEOUT`].
    TimedOut,
}

impl std::fmt::Display for ConfigureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell(error) => write!(f, "shell error: {error}"),
            Self::TimedOut => write!(
                f,
                "the window system never configured the window within {}s, \
                 so it never had a size",
                CONFIGURE_TIMEOUT.as_secs()
            ),
        }
    }
}

impl std::error::Error for ConfigureError {}

impl From<ShellError> for ConfigureError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

/// Pumps until `window` reports a size, and returns it.
///
/// A swapchain needs a size and an unconfigured window does not have one, so
/// this is the gate between [`Shell::create_window`] and [`GpuContext::open`].
/// Events observed while waiting are added to `events`.
///
/// # Errors
///
/// [`ConfigureError`] if the shell failed, or if no configure arrived within
/// [`CONFIGURE_TIMEOUT`].
pub fn wait_for_configure<S: Shell + ?Sized>(
    shell: &mut S,
    window: WindowId,
    events: &mut u64,
) -> Result<(u32, u32), ConfigureError> {
    let started = MonotonicTime::new();
    loop {
        shell.pump(&mut |event| {
            *events += 1;
            log::debug!("shell event: {event:?}");
        });
        if let Some(size) = shell.window_state(window)?.size() {
            return Ok((size.width, size.height));
        }
        if started.elapsed() >= CONFIGURE_TIMEOUT {
            return Err(ConfigureError::TimedOut);
        }
        shell.wait_events(Some(Duration::from_millis(8)));
    }
}

/// Answers a close request with "yes".
///
/// A close request is a question. A game asks the player about unsaved progress
/// here; a sample has none.
///
/// # Errors
///
/// [`ShellError`] if the shell refused the reply.
pub fn accept_close<S: Shell + ?Sized>(shell: &mut S, window: WindowId) -> Result<(), ShellError> {
    shell.reply_close_request(window, CloseReply::Close)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_headless_clock_advances_by_exactly_one_frame_step() {
        let mut clock = Clock::new(true);
        assert_eq!(clock.elapsed(), Duration::ZERO);
        assert_eq!(clock.advance(), HEADLESS_FRAME_STEP);
        assert_eq!(clock.advance(), HEADLESS_FRAME_STEP * 2);
    }

    #[test]
    fn a_manual_clock_takes_any_frame_rate() {
        let step = Duration::from_nanos(1_000_000_000 / 240);
        let mut clock = Clock::manual(step);
        for i in 1..=240u32 {
            assert_eq!(clock.advance(), step * i);
        }
    }

    #[test]
    fn a_real_clock_only_moves_forward() {
        let mut clock = Clock::new(false);
        let first = clock.advance();
        let second = clock.advance();
        assert!(second >= first);
    }

    /// The browser's start-up shape, driven on a headless shell with the null
    /// backend: `request_open` never blocks, and polling it produces exactly
    /// the context `open` would have produced.
    ///
    /// `open` itself is this loop with a `yield_now` in it, so a break in the
    /// state machine breaks both — which is the point of having only one.
    #[test]
    fn a_gpu_context_can_be_opened_by_polling_instead_of_blocking() {
        use crcbl_shell::{HeadlessShell, WindowDesc};

        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("headless always creates a window");

        let mut pending = GpuContext::request_open(
            &shell,
            window,
            (320, 240),
            &GpuContextDesc {
                label: "polled open",
                backend: Some(GpuBackend::Null),
                ..GpuContextDesc::default()
            },
        )
        .expect("the null backend is always registered");

        let mut polls = 0;
        let mut gpu = loop {
            polls += 1;
            assert!(polls < 64, "the null backend must not poll forever");
            if let Some(context) = pending.poll().expect("nothing here can fail") {
                break context;
            }
        };
        assert_eq!(gpu.extent(), (320, 240));
        assert_eq!(
            gpu.device().backend(),
            crcbl_hal::BackendKind::Null,
            "the backend asked for by value is the one that opened"
        );

        // Polling a spent request is a caller bug, not a second context.
        let error = pending.poll().expect_err("the context was already taken");
        assert!(matches!(error, GpuError::Unusable(_)), "{error}");

        gpu.drain().expect("nothing was submitted");
        gpu.destroy()
            .expect("teardown is in the seam's stated order");
    }

    #[test]
    fn pending_records_the_last_size_and_every_flag() {
        use crcbl_shell::{HeadlessShell, WindowDesc};

        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("headless always creates a window");
        let mut pending = Pending::default();
        pending.observe(&ShellEvent::Resized {
            window,
            size: PhysicalSize::new(100, 50),
            scale_factor: 1.0,
        });
        pending.observe(&ShellEvent::Resized {
            window,
            size: PhysicalSize::new(200, 100),
            scale_factor: 1.0,
        });
        pending.observe(&ShellEvent::CloseRequested { window });
        assert_eq!(pending.count, 3);
        assert_eq!(pending.resized, Some(PhysicalSize::new(200, 100)));
        assert!(pending.close_requested);
        assert!(!pending.destroyed);
    }

    /// A window destroyed in answer to a close request is already gone; every
    /// other exit still owns one.
    #[test]
    fn only_a_live_window_needs_destroying() {
        assert!(ExitReason::FrameBudget.window_survives());
        assert!(ExitReason::Failed.window_survives());
        assert!(!ExitReason::CloseRequested.window_survives());
        assert!(!ExitReason::WindowDestroyed.window_survives());
    }

    /// The whole join, against the null backend: shell → surface → device →
    /// swapchain → acquire → submit → present → teardown.
    #[test]
    fn the_join_runs_end_to_end_on_the_null_backend() {
        use crcbl_hal::CommandEncoderDesc;
        use crcbl_shell::{HeadlessShell, WindowDesc};

        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("headless always creates a window");
        let mut events = 0;
        let extent = wait_for_configure(&mut shell, window, &mut events).expect("configured");

        let mut gpu = GpuContext::open(
            &shell,
            window,
            extent,
            &GpuContextDesc {
                label: "engine test",
                backend: Some(GpuBackend::Null),
                ..GpuContextDesc::default()
            },
        )
        .expect("the null backend opens everywhere");
        assert_eq!(gpu.extent(), extent);

        for _ in 0..4 {
            let Some(acquired) = gpu.acquire().expect("acquire") else {
                continue;
            };
            let encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
                label: Some("engine test"),
                queue: gpu.queue(),
            });
            let command_buffer = encoder.finish().expect("an empty command buffer");
            assert_eq!(
                gpu.submit_and_present(&acquired, command_buffer)
                    .expect("present"),
                FrameOutcome::Presented,
            );
        }

        gpu.resize((640, 480)).expect("resize");
        assert_eq!(
            gpu.acquire().expect("acquire").map(|f| f.extent),
            Some((640, 480))
        );
        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
    }
}
