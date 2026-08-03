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
use crcbl_shell::{CloseReply, DisplayMode, PhysicalSize, Shell, ShellError, ShellEvent, WindowId};

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

/// The largest step a driven clock will accept for one frame.
///
/// A tab that was backgrounded for a minute reports a one-minute
/// `requestAnimationFrame` delta on the frame it comes back. Handing that to a
/// fixed-timestep accumulator asks for thousands of ticks in one frame, which is
/// a freeze the user reads as a crash. Four frames' worth of catch-up is the
/// budget; anything beyond it is time the simulation simply did not experience.
///
/// The engine's rather than each game's: the browser behaviour it defends
/// against is the shell's, and a sample that picked a different number would be
/// deciding how the *platform* behaves.
pub const MAX_FRAME_STEP: Duration = Duration::from_millis(64);

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

/// Anything that can stop a loop, plus whatever the game itself refuses.
///
/// Every sample had its own copy of this enum with the same five loop variants
/// and the same `Display` arms, differing only in the name and in one variant
/// carrying that game's error type. That is duplicated *knowledge*, not
/// duplicated shape: "the swapchain never presented" reads the same however the
/// game above it is spelled, and five copies is five places for the hint text
/// to drift.
///
/// `G` is the game's own error. A game that cannot fail on its own leaves it at
/// the default [`Infallible`](core::convert::Infallible), which makes
/// [`Self::Game`] uninhabited and costs the enum nothing.
///
/// # Why there is no `From<G>`
///
/// The three engine error types convert with `?`, so the bring-up path reads as
/// it did. A blanket `impl<G> From<G> for LoopError<G>` cannot join them —
/// `G` may itself be [`ShellError`], so the compiler rejects the overlap — and
/// dropping the concrete impls to buy it would cost every `?` in the loop to
/// save one at the game's constructor. Game errors are wrapped by name:
/// `.map_err(LoopError::Game)`.
#[derive(Debug)]
pub enum LoopError<G = core::convert::Infallible> {
    /// No shell backend could be opened: nothing is listening, or this platform
    /// has none yet.
    NoWindowSystem(ShellError),
    /// The window system refused something.
    Shell(ShellError),
    /// The window was never configured, so there was never a size to create a
    /// swapchain at.
    Configure(ConfigureError),
    /// The swapchain reconfigured over and over and never presented a frame.
    /// See [`MAX_CONSECUTIVE_RECONFIGURES`].
    NeverPresented,
    /// The GPU seam failed.
    Gpu(GpuError),
    /// The game refused to start or to step.
    Game(G),
}

impl<G: std::fmt::Display> std::fmt::Display for LoopError<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindowSystem(error) => write!(
                f,
                "no window system: {error}\n\
                 hint: either nothing is listening (no compositor, no DISPLAY), \
                 or this platform has no shell backend yet. `--headless` runs \
                 the same loop with no window and works everywhere."
            ),
            Self::Shell(error) => write!(f, "shell error: {error}"),
            Self::Configure(error) => write!(f, "{error}"),
            Self::NeverPresented => write!(
                f,
                "the swapchain reconfigured {MAX_CONSECUTIVE_RECONFIGURES} times \
                 in a row without presenting a frame"
            ),
            Self::Gpu(error) => write!(f, "gpu error: {error}"),
            Self::Game(error) => write!(f, "game error: {error}"),
        }
    }
}

impl<G: std::error::Error> std::error::Error for LoopError<G> {}

impl<G> From<ShellError> for LoopError<G> {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

impl<G> From<ConfigureError> for LoopError<G> {
    fn from(error: ConfigureError) -> Self {
        Self::Configure(error)
    }
}

impl<G> From<GpuError> for LoopError<G> {
    fn from(error: GpuError) -> Self {
        Self::Gpu(error)
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

/// How presented frames are paced against the display.
///
/// # One value, so the exclusive cases cannot both be asked for
///
/// Vsync and adaptive sync are alternatives, not switches: a display is either
/// following a fixed refresh or following the application's presents, and it
/// cannot do both. Two booleans would make "vsync on, VRR on" a state a caller
/// can write down and the engine has to reject at runtime; one value makes it a
/// state that cannot be spelled.
///
/// # What the engine can and cannot do about VRR
///
/// **Nothing here turns adaptive sync on.** VRR is negotiated between the
/// display, the driver and the compositor, and an application never enables it
/// — what changes is what presenting *means*: on a VRR panel the present does
/// not wait for a fixed vblank, the panel follows the presents. So the choice
/// this enum makes is which present mode to ask for, and the job left to the
/// frame limiter is staying inside the panel's range.
///
/// Which mode is actually running is a separate question the engine cannot
/// answer yet — it needs `VK_EXT_present_timing`, which has no Rust bindings in
/// the pinned `ash` and is still a provisional extension. Until then
/// [`Adaptive`](Self::Adaptive) is a request, not an observation, and the
/// default is [`Vsync`](Self::Vsync) because it is the one every surface
/// supports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pacing {
    /// Wait for the display. [`PresentMode::Fifo`], which is the only mode
    /// guaranteed to exist — and the only one WebGPU has.
    #[default]
    Vsync,
    /// Follow the display when it can follow us: [`PresentMode::FifoRelaxed`]
    /// where the surface offers it, otherwise [`PresentMode::Mailbox`].
    ///
    /// `FifoRelaxed` is the closer fit — it waits for vblank when the frame is
    /// on time and tears rather than stalling when it is late, which is what
    /// keeps a VRR panel inside its range instead of dropping to a duplicated
    /// frame.
    Adaptive,
    /// Do not wait: [`PresentMode::Mailbox`] where offered, otherwise
    /// [`PresentMode::Immediate`]. Tears on `Immediate`. For latency work and
    /// for benchmarks, where the [`FrameLimit`] is the only thing pacing the
    /// loop.
    Off,
}

impl Pacing {
    /// The present modes to try, best first.
    ///
    /// Every list ends in a mode the surface must support, so
    /// [`SurfaceCaps::choose_present_mode`](crcbl_hal::SurfaceCaps::choose_present_mode)
    /// cannot fall through to a mode that is not there.
    #[must_use]
    pub const fn preferences(self) -> &'static [PresentMode] {
        match self {
            Self::Vsync => &[PresentMode::Fifo],
            Self::Adaptive => &[
                PresentMode::FifoRelaxed,
                PresentMode::Mailbox,
                PresentMode::Fifo,
            ],
            Self::Off => &[
                PresentMode::Mailbox,
                PresentMode::Immediate,
                PresentMode::Fifo,
            ],
        }
    }
}

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
    /// How presented frames are paced against the display.
    ///
    /// The swapchain's present mode comes from this. A game that wants to
    /// change it after start-up reconfigures the context; the mode is a
    /// swapchain property and cannot be edited in place.
    pub pacing: Pacing,
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
            pacing: Pacing::default(),
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
    /// Carried from the desc because the swapchain is built at the end of the
    /// open, several polls after the caller handed it over.
    pacing: Pacing,
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
                        self.pacing,
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
    /// # This one blocks, and on `wasm32` that is a hang
    ///
    /// It spins on [`request_open`](Self::request_open) until the device
    /// arrives. In a browser the promise behind that device is resolved by the
    /// event loop this call is *inside*, so it would never complete — which is
    /// why [`crate::backend::open`] and
    /// [`Instance::create_device`] do not
    /// exist on `wasm32` at all.
    ///
    /// This one still does, deliberately and temporarily: `apps/breakout`'s
    /// start-up is written on it, and the slice that gives the sample a polled
    /// loop (P5.7, the rAF entry point) is the one that can take it away
    /// without deleting the sample's wasm build in the same edit. **Browser
    /// code must call [`request_open`](Self::request_open).** See this crate's
    /// `backend` module docs for the compile-error-over-run-time-error rule
    /// this is the single exception to.
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
            pacing: desc.pacing,
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
        pacing: Pacing,
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
        let present_mode = caps.choose_present_mode(pacing.preferences());
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
        // Before anything else, because an error the device reported out of
        // band is a reason not to record another frame. On a backend that
        // reports every failure through its return values this is `None` every
        // time; on WebGPU it is the *only* way a failed pipeline is ever heard
        // from, and until this call existed the answer was a black canvas over
        // a game that reported itself as playing. Start of the frame rather
        // than end of the last one so that failures during start-up — where
        // every pipeline is built — are caught by the first frame.
        if let Some(message) = self.device.take_error() {
            return Err(GpuError::Hal(HalError::Backend(message)));
        }

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

/// The most frames a second a real-time loop will run.
///
/// # Why a limit at all, and why this one
///
/// A loop with nothing to wait for runs as fast as the machine can draw, which
/// on a menu or a paused game means a pegged GPU and audible fans for frames
/// nobody sees. The default here is deliberately *high* — high enough that no
/// display or hand can tell, so it is a runaway guard rather than a pacing
/// policy — and a game that wants a real cap says so.
///
/// # This is a floor on the frame period, not a promise about it
///
/// [`Clock::advance`] sleeps until the period has passed, and a sleep may
/// overrun. At the default the period is a millisecond, which is the same order
/// as a scheduler's granularity, so the *observed* rate can sit under the limit
/// on a loaded machine. That is the honest behaviour for a limiter: it can slow
/// a loop down and can never speed one up.
///
/// # Under vsync it usually does nothing
///
/// With [`PresentMode::Fifo`] the present itself blocks on vblank, so the frame
/// period is already the display's and this never fires. It earns its keep on
/// [`Pacing::Adaptive`] and [`Pacing::Off`], where nothing else is pacing the
/// loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLimit {
    /// The least time one frame may take, or `None` for no limit.
    period: Option<Duration>,
}

impl FrameLimit {
    /// The default ceiling: a thousand frames a second.
    pub const DEFAULT_FPS: u32 = 1000;

    /// A limit of `fps` frames a second.
    ///
    /// Zero is [`unlimited`](Self::unlimited) rather than an error or a divide
    /// by zero: "no frames per second" is not a rate anyone means, and the only
    /// other reading of it — a loop that never runs — is not something a caller
    /// would ask for by accident.
    #[must_use]
    pub fn fps(fps: u32) -> Self {
        if fps == 0 {
            return Self::unlimited();
        }
        Self {
            period: Some(Duration::from_secs(1) / fps),
        }
    }

    /// No limit: run as fast as the loop can.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self { period: None }
    }

    /// The least time one frame may take, if there is a limit.
    #[must_use]
    pub const fn period(self) -> Option<Duration> {
        self.period
    }

    /// How long to wait before starting a frame, given when the last one
    /// started and what the clock reads now.
    ///
    /// Separated from the sleep so the arithmetic is testable without spending
    /// the time: a test can ask what a limiter *would* wait and get an answer
    /// in nanoseconds rather than in seconds of test runtime.
    ///
    /// `None` when there is no limit, when no frame has started yet, or when
    /// the deadline has already passed — a late frame is never "caught up" by
    /// running the next one early, because that would turn one slow frame into
    /// a burst.
    #[must_use]
    pub fn wait_from(self, last_start: Option<Duration>, now: Duration) -> Option<Duration> {
        let period = self.period?;
        let deadline = last_start?.checked_add(period)?;
        deadline.checked_sub(now).filter(|wait| !wait.is_zero())
    }
}

impl Default for FrameLimit {
    fn default() -> Self {
        Self::fps(Self::DEFAULT_FPS)
    }
}

/// Blocks the calling thread for `wait`.
///
/// Split out for the browser, where it does **nothing**. A wasm module runs on
/// the page's only thread, so sleeping there does not pace a frame — it freezes
/// the tab, input and all, until the sleep ends. The browser paces frames with
/// `requestAnimationFrame` and the shim drives the loop from it, which is why
/// every wasm entry point builds a [`Clock::Manual`] and never reaches this.
/// The no-op is a backstop for a caller that constructs a real clock anyway.
#[cfg(not(target_arch = "wasm32"))]
fn sleep(wait: Duration) {
    std::thread::sleep(wait);
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::needless_pass_by_value)]
fn sleep(_wait: Duration) {}

/// The real clock, plus the frame limiter that paces it.
///
/// A struct behind [`Clock::Real`] rather than more fields on the variant, so
/// the `Clock::Real(_)` patterns the samples already match on keep compiling.
#[derive(Debug)]
pub struct RealClock {
    time: MonotonicTime,
    limit: FrameLimit,
    /// When the last frame started, so the next one can be held off until a
    /// whole period has passed. `None` before the first frame.
    last_start: Option<Duration>,
}

impl RealClock {
    /// A real clock limited to [`FrameLimit::DEFAULT_FPS`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            time: MonotonicTime::new(),
            limit: FrameLimit::default(),
            last_start: None,
        }
    }

    /// The limit in force.
    #[must_use]
    pub const fn limit(&self) -> FrameLimit {
        self.limit
    }

    /// Changes the limit. Takes effect on the next frame.
    pub const fn set_limit(&mut self, limit: FrameLimit) {
        self.limit = limit;
    }
}

impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}

/// A time source a loop can drive, whichever kind it is.
///
/// The variants exist so the *loop* stays free of `if headless`: it calls
/// [`Clock::advance`] once per frame and gets a timestamp, and the difference
/// between "read the real clock" and "step the fake one" lives here. A headless
/// run therefore produces the same frame and tick counts on every machine,
/// which is the whole reason CI can assert them.
///
/// The frame limiter lives on [`Real`](Self::Real) alone, which is what makes a
/// headless run unpaced **by construction** rather than by a check somebody has
/// to remember: there is no wall clock to sleep against, and a manual clock's
/// frames are supposed to be as fast as the machine can produce them.
#[derive(Debug)]
pub enum Clock {
    /// The real monotonic clock, paced by a [`FrameLimit`].
    Real(RealClock),
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
            Self::Real(RealClock::new())
        }
    }

    /// The frame limit in force, if this is a real clock.
    ///
    /// `None` for a manual clock — not "unlimited", because the question does
    /// not apply: a manual clock is stepped by its caller and never waits.
    #[must_use]
    pub const fn limit(&self) -> Option<FrameLimit> {
        match self {
            Self::Real(real) => Some(real.limit()),
            Self::Manual { .. } => None,
        }
    }

    /// Sets the frame limit, if this is a real clock.
    ///
    /// A no-op on a manual clock rather than an error: a game that sets a limit
    /// during setup should not have to ask whether it is running headless, and
    /// a headless run that silently obeyed one would stop being deterministic.
    pub const fn set_limit(&mut self, limit: FrameLimit) {
        if let Self::Real(real) = self {
            real.set_limit(limit);
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
            Self::Real(real) => real.time.elapsed(),
            Self::Manual { time, .. } => time.elapsed(),
        }
    }

    /// Moves to the next frame's timestamp and returns it.
    ///
    /// **On a real clock this may sleep**, for as long as the [`FrameLimit`]
    /// says the frame is early. It is the one call every loop already makes
    /// once per frame, which is why the limiter lives here rather than in five
    /// copies of a loop — and why a game gets it without asking.
    ///
    /// A manual clock never waits: there is no wall clock to wait against, and
    /// a headless run's frames are meant to arrive as fast as they can.
    pub fn advance(&mut self) -> Duration {
        match self {
            Self::Real(real) => {
                if let Some(wait) = real.limit.wait_from(real.last_start, real.time.elapsed()) {
                    sleep(wait);
                }
                let now = real.time.elapsed();
                real.last_start = Some(now);
                now
            }
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
/// # What it folds, and what it leaves
///
/// Everything the *loop* acts on rather than the game: the window's own
/// business, the pointer, focus loss, and the three keys below that are the
/// engine's. Whatever is left — the game's keys — the caller matches on itself,
/// which is why [`observe`](Self::observe) reports whether it took the event.
///
/// The pointer half was byte-for-byte identical in all four samples before it
/// moved here, and it is not trivial code: it carries the last position across
/// frames because motion and buttons arrive as separate events and a click
/// carries a position only on some backends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pending {
    /// Events observed, of every kind.
    pub count: u64,
    /// The most recent size the window reported.
    pub resized: Option<PhysicalSize>,
    /// The window system asked the window to close.
    pub close_requested: bool,
    /// The window went away without asking.
    pub destroyed: bool,
    /// The window lost focus during this batch.
    ///
    /// Not a key event and never will be: the releases for whatever was held
    /// are exactly what no platform sends, which is the obligation
    /// [`ShellEvent::Focus`] documents and a loop discharges by releasing every
    /// key it forwarded.
    pub focus_lost: bool,
    /// Where the pointer is, in framebuffer pixels, or `None` if it is outside
    /// the window.
    ///
    /// Starts at whatever [`Pending::carrying`] was given, because a batch with
    /// no motion event in it has not moved the pointer.
    pub pointer: Option<glam::Vec2>,
    /// The primary pointer button went down during this batch.
    pub pointer_pressed: bool,
    /// …and came up.
    ///
    /// Both can be true for one batch — a click that begins and ends inside a
    /// single pump — which is why they are two flags and not one state.
    pub pointer_released: bool,
    /// [`DEBUG_OVERLAY_KEY`] was pressed, and it was a real press.
    pub toggle_debug_overlay: bool,
    /// [`PAUSE_KEY`] was pressed.
    pub toggle_pause: bool,
    /// [`FULLSCREEN_KEY`] was pressed.
    pub toggle_fullscreen: bool,
}

/// Whether [`Pending::observe`] took an event, or left it for the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handled {
    /// The loop's own. The game must not see it.
    ///
    /// A reserved key recorded into a tick's input would change what a seeded,
    /// scripted run replays.
    Loop,
    /// Not the loop's. The caller decides what it means.
    Game,
}

impl Pending {
    /// An empty batch that remembers where the pointer was left.
    ///
    /// A frame whose pump delivers no pointer event must not forget the cursor:
    /// starting from [`Default`] would put every menu's hover state at "no
    /// pointer" on any frame the mouse did not move.
    #[must_use]
    pub fn carrying(pointer: Option<glam::Vec2>) -> Self {
        Self {
            pointer,
            ..Self::default()
        }
    }

    /// Folds one event in, and says whether the loop claimed it.
    pub fn observe(&mut self, event: &ShellEvent) -> Handled {
        self.count += 1;
        log::debug!("shell event: {event:?}");

        let position = |point: Option<crcbl_shell::PhysicalPoint>| {
            point.map(|point| glam::Vec2::new(point.x as f32, point.y as f32))
        };

        match event {
            ShellEvent::Resized { size, .. } | ShellEvent::ScaleFactorChanged { size, .. } => {
                self.resized = Some(*size);
            }
            ShellEvent::CloseRequested { .. } => self.close_requested = true,
            ShellEvent::WindowDestroyed { .. } => self.destroyed = true,
            ShellEvent::Focus { focused: false, .. } => self.focus_lost = true,
            ShellEvent::PointerMotion {
                abs: Some(point), ..
            } => {
                self.pointer = position(Some(*point));
            }
            // A pointer that left the window is not hovering anything, and must
            // not leave the last button it crossed lit up.
            ShellEvent::PointerFocus {
                entered,
                position: at,
                ..
            } => {
                self.pointer = if *entered { position(*at) } else { None };
            }
            ShellEvent::Button {
                button: crcbl_core::input::PointerButton::Left,
                state,
                position: at,
                ..
            } => {
                if let Some(point) = position(*at) {
                    self.pointer = Some(point);
                }
                if matches!(state, crcbl_shell::ButtonState::Pressed) {
                    self.pointer_pressed = true;
                } else {
                    self.pointer_released = true;
                }
            }
            ShellEvent::Key {
                key_code: Some(code),
                state,
                repeat,
                ..
            } => {
                // `!repeat` because holding F11 down would otherwise toggle the
                // display mode at the keyboard's repeat rate.
                let edge = matches!(state, crcbl_shell::ButtonState::Pressed) && !repeat;
                match *code {
                    DEBUG_OVERLAY_KEY => self.toggle_debug_overlay |= edge,
                    PAUSE_KEY => self.toggle_pause |= edge,
                    FULLSCREEN_KEY => self.toggle_fullscreen |= edge,
                    _ => return Handled::Game,
                }
            }
            _ => return Handled::Game,
        }
        Handled::Loop
    }
}

/// Shows and hides the engine's debug overlay.
///
/// One of three keys the loop keeps for itself, and they are the engine's
/// rather than each game's because the thing F3 opens is the engine's. All five
/// samples had spelled out the same three constants.
pub const DEBUG_OVERLAY_KEY: crcbl_core::input::KeyCode = crcbl_core::input::KeyCode::F3;

/// Stops and resumes the simulation.
pub const PAUSE_KEY: crcbl_core::input::KeyCode = crcbl_core::input::KeyCode::Escape;

/// Toggles fullscreen.
pub const FULLSCREEN_KEY: crcbl_core::input::KeyCode = crcbl_core::input::KeyCode::F11;

/// Moves the selection up, while a menu is showing.
///
/// The three menu keys, like the three reserved ones above, are the engine's
/// because the widget they drive is: [`crcbl_ui::menu::MenuSet`] owns the
/// selection, and a sample that bound a different key would be describing the
/// engine's list box. All five samples had spelled out the same three.
///
/// They are consumed **only while a menu is showing**, so a frame with no menu
/// on it forwards them to the game like any other key.
pub const MENU_UP_KEY: crcbl_core::input::KeyCode = crcbl_core::input::KeyCode::ArrowUp;

/// Moves the selection down. See [`MENU_UP_KEY`].
pub const MENU_DOWN_KEY: crcbl_core::input::KeyCode = crcbl_core::input::KeyCode::ArrowDown;

/// Commits the selection. See [`MENU_UP_KEY`].
pub const MENU_ACTIVATE_KEY: crcbl_core::input::KeyCode = crcbl_core::input::KeyCode::Enter;

/// A loop that can be stepped and torn down.
///
/// The two things a driver needs and the only two both drivers agree on: the
/// native [`drive`] below, and the browser's `crcbl::web::App`, which cannot
/// use `drive` because a browser main thread may not sit in a `loop {}`.
/// Declaring them once is what keeps the two paths stepping the same loop.
pub trait GameLoop: Sized {
    /// The game's error.
    type Error: core::fmt::Display;
    /// What a finished run reports.
    type Summary;

    /// One frame.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] if the frame failed.
    fn frame(&mut self) -> Result<Flow, Self::Error>;

    /// Tears the loop down: the GPU, then the window.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] if teardown failed.
    fn finish(self, exit: ExitReason) -> Result<Self::Summary, Self::Error>;
}

/// Steps `engine` until it stops, then tears it down.
///
/// The native driver. Every sample's `run` was this, and the part worth having
/// once is the error path: **a frame error is the one worth reporting**, so a
/// teardown failure on top of it is logged rather than allowed to replace it.
/// Teardown still runs — a loop that failed mid-frame still holds a device and a
/// window.
///
/// # Errors
///
/// The game's error, from the frame that failed or from teardown.
pub fn drive<L: GameLoop>(mut engine: L) -> Result<L::Summary, L::Error> {
    let outcome = loop {
        match engine.frame() {
            Ok(Flow::Continue) => {}
            Ok(Flow::Stop(reason)) => break Ok(reason),
            Err(error) => break Err(error),
        }
    };
    match outcome {
        Ok(reason) => engine.finish(reason),
        Err(error) => {
            if let Err(teardown) = engine.finish(ExitReason::Failed) {
                log::error!("teardown after a failed frame also failed: {teardown}");
            }
            Err(error)
        }
    }
}

/// Presented frames, the budget they are counted against, and the guard that
/// makes the budget reachable.
///
/// The three belong together because the third exists only for the first two. A
/// budget counts **presented** frames, so a swapchain that is permanently
/// suboptimal — or permanently out of date — would reconfigure forever, never
/// present, and `--frames N` would never terminate. See
/// [`MAX_CONSECUTIVE_RECONFIGURES`].
#[derive(Clone, Copy, Debug)]
pub struct FrameBudget {
    presented: u64,
    budget: Option<u64>,
    reconfigures_in_a_row: u32,
}

impl FrameBudget {
    /// A budget of `Some(n)` frames, or `None` to run until something else
    /// stops the loop.
    #[must_use]
    pub const fn new(budget: Option<u64>) -> Self {
        Self {
            presented: 0,
            budget,
            reconfigures_in_a_row: 0,
        }
    }

    /// Whether the loop should stop before doing any more work this frame.
    #[must_use]
    pub const fn is_spent(&self) -> bool {
        match self.budget {
            Some(budget) => self.presented >= budget,
            None => false,
        }
    }

    /// Records what one frame did.
    ///
    /// # Errors
    ///
    /// [`LoopError::NeverPresented`] once [`MAX_CONSECUTIVE_RECONFIGURES`]
    /// frames in a row have failed to present. Four seconds of 60 Hz
    /// reconfiguring is far past "a resize storm" and squarely in "this surface
    /// will never present".
    pub fn record<G>(&mut self, outcome: FrameOutcome) -> Result<(), LoopError<G>> {
        match outcome {
            FrameOutcome::Presented => {
                self.presented += 1;
                self.reconfigures_in_a_row = 0;
            }
            FrameOutcome::Reconfigured => {
                self.reconfigures_in_a_row += 1;
                if self.reconfigures_in_a_row >= MAX_CONSECUTIVE_RECONFIGURES {
                    return Err(LoopError::NeverPresented);
                }
            }
        }
        Ok(())
    }

    /// How many frames have actually reached the screen.
    #[must_use]
    pub const fn presented(&self) -> u64 {
        self.presented
    }
}

/// Releases every held key, then pauses.
///
/// **The release is the load-bearing half.** A window that loses focus mid-input
/// leaves the game holding whatever was down — and a game that resumes still
/// believing the key is held flies into the wall until the player taps it again.
/// The keys go out as real release events, through the same path a player's
/// would take, so the action map's edges resolve exactly as they always do.
///
/// Idempotent on the pause: a batch carrying two focus losses logs once.
pub fn lose_focus(
    held: &mut Vec<crcbl_core::input::KeyCode>,
    paused: &mut bool,
    mut release: impl FnMut(crcbl_core::input::KeyCode),
) {
    for key in held.drain(..) {
        release(key);
    }
    if !*paused {
        *paused = true;
        log::info!("paused: the window lost focus");
    }
}

/// Drains the fixed-step accumulator, and returns how many ticks ran.
///
/// The loop body every sample wrote out, with the game's own tick as the
/// closure. Zero when `paused`, because a paused frame **keeps the clock and
/// throws the ticks away**.
///
/// # Why a paused frame still drains
///
/// The three candidates differ only after a long pause, and only one resumes
/// without a lurch:
///
/// * *Stop calling `update`.* [`crcbl_core::FrameClock::update`] measures
///   `now - last_update`, so the first update after the pause covers the whole
///   of it. The catch-up cap discards the rest, so resuming spends one frame
///   running the cap's worth of simulation and the dropped-tick count climbs by
///   however long the player was away.
/// * *Update but do not drain.* The accumulator saturates at the same cap, so
///   resuming runs the same burst in one frame. No better.
/// * *Update and drain.* The accumulator holds only the sub-tick remainder when
///   the game resumes, so the first live frame runs the one tick it is owed.
///   This one.
///
/// Draining also keeps `render_dt` real while paused, which is what the debug
/// overlay records — a pause that froze the clock would show the frame graph
/// flatlining at whatever it read when Escape was pressed.
pub fn run_ticks(clock: &mut crcbl_core::FrameClock, paused: bool, mut tick: impl FnMut()) -> u64 {
    if paused {
        while clock.consume_tick() {}
        return 0;
    }
    let mut ran = 0;
    while clock.consume_tick() {
        ran += 1;
        tick();
    }
    ran
}

/// The loop's fullscreen request, and whether the window system agreed.
///
/// All five samples carried a `mode_honoured` bool and the same three methods
/// beside it. Every one of them talks only to the shell, which is what makes
/// this the last piece of the loop that extracts without a game type anywhere
/// near it.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModeRequest {
    honoured: bool,
}

impl ModeRequest {
    /// A loop that has asked for nothing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { honoured: false }
    }

    /// The mode the window system actually has the window in.
    ///
    /// **Not the one that was last asked for.** A summary that reported the
    /// request would say "borderless" for every compositor that refused, which
    /// is the difference this whole type exists to keep visible.
    pub fn mode<S: Shell + ?Sized>(shell: &S, window: WindowId) -> DisplayMode {
        shell
            .window_state(window)
            .map_or(DisplayMode::Windowed, |state| {
                state.effective_mode().unwrap_or(state.requested_mode)
            })
    }

    /// Asks for the opposite of whatever the window is in now.
    ///
    /// Read back rather than remembered: a loop that tracked its own idea of
    /// fullscreen would invert a state the compositor never entered, and the
    /// key would then do nothing every other press.
    ///
    /// # Errors
    ///
    /// [`ShellError`] if the shell refused the request outright. A shell that
    /// accepts it and then does not honour it is not an error — see
    /// [`Self::check`].
    pub fn toggle<S: Shell + ?Sized>(shell: &mut S, window: WindowId) -> Result<(), ShellError> {
        let target = if Self::mode(shell, window).is_borderless() {
            DisplayMode::Windowed
        } else {
            DisplayMode::Borderless { monitor: None }
        };
        shell.set_mode(window, target)?;
        log::info!("shell: asked for {target}");
        Ok(())
    }

    /// Whether the window system was last seen honouring what was asked for.
    ///
    /// Updated by [`Self::check`], so it is a *report* rather than a request —
    /// which is the distinction each sample's
    /// `fullscreen_is_requested_and_the_refusal_is_reported` asserts.
    #[must_use]
    pub const fn honoured(self) -> bool {
        self.honoured
    }

    /// Logs the moment the window system stops agreeing with the request.
    ///
    /// Once per transition, not once per frame: a backend that cannot do
    /// fullscreen at all — the browser without a shim that calls
    /// `requestFullscreen`, a tiling window manager — would otherwise print a
    /// line every frame forever.
    pub fn check<S: Shell + ?Sized>(&mut self, shell: &S, window: WindowId) {
        let Ok(state) = shell.window_state(window) else {
            return;
        };
        // Defensive, and currently untested because it makes no difference:
        // an unconfigured window has no effective mode, so
        // `mode_request_honoured` is already false and matches the initial
        // state. It is here for a backend that unconfigures a window that was
        // configured, which none of ours does.
        if !state.is_configured() {
            return;
        }
        let honoured = state.mode_request_honoured();
        if honoured == self.honoured {
            return;
        }
        self.honoured = honoured;
        if honoured {
            log::info!("shell: the window is {}", state.requested_mode);
        } else {
            log::warn!(
                "shell: asked for {} and got {}",
                state.requested_mode,
                Self::mode(shell, window),
            );
        }
    }
}

/// Everything the loop remembers about the pointer between frames.
///
/// Two fields that every sample carried separately and resolved by hand: where
/// the cursor was left, and whether its button is still down. Both are needed
/// *across* frames, because pointer motion and button events arrive separately
/// and a click carries a position only on some backends.
///
/// [`Self::pending`] starts a batch from the remembered position and
/// [`Self::resolve`] folds the batch back in, so the rule below lives in one
/// place rather than five.
#[derive(Clone, Copy, Debug, Default)]
pub struct PointerCapture {
    at: Option<glam::Vec2>,
    held: bool,
}

impl PointerCapture {
    /// A pointer that has never been in the window.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            at: None,
            held: false,
        }
    }

    /// Starts a pump batch from where the last frame left the cursor.
    ///
    /// A batch with no pointer event in it has not moved the cursor, and a menu
    /// whose hover state reset every still frame would flicker.
    #[must_use]
    pub fn pending(self) -> Pending {
        Pending::carrying(self.at)
    }

    /// Folds `pending` in and answers what the UI should be asked this frame.
    ///
    /// **`down` must be false on the frame the button came up**, or
    /// `UiState::interact` latches the capture and fires it in the same call —
    /// and a press that started in the corner of the screen would be credited to
    /// whatever the cursor was over at release, which is the exact bug press
    /// capture exists to prevent.
    ///
    /// **Except when the press also arrived this frame:** a click faster than a
    /// frame is one event pair, and it must latch and fire together or a quick
    /// tap does nothing.
    pub fn resolve(&mut self, pending: &Pending) -> crcbl_ui::PointerInput {
        self.at = pending.pointer;
        if pending.pointer_pressed {
            self.held = true;
        }
        let down = pending.pointer_pressed || (self.held && !pending.pointer_released);
        if pending.pointer_released {
            self.held = false;
        }
        crcbl_ui::PointerInput {
            // A pointer that has never been in the window is nowhere, not at the
            // origin — which is a real pixel, inside the HUD.
            pos: self.at.unwrap_or(glam::Vec2::splat(f32::NEG_INFINITY)),
            down,
            released: pending.pointer_released,
        }
    }

    /// Where the cursor was last seen, if it has ever been in the window.
    #[must_use]
    pub const fn at(self) -> Option<glam::Vec2> {
        self.at
    }
}

/// What firing a menu button asks the loop to do.
///
/// An action rather than a key: a button that "pressed Space" would be a menu
/// re-entering its own input path, and the loop would have to tell a synthesised
/// key from a real one.
///
/// Three of them are the **loop's** and mean the same thing in every game —
/// un-pause, toggle fullscreen, toggle the debug panel, which are the three
/// reserved keys' menu equivalents. `G` is whatever else this game's menus
/// offer, and is where "serve the ball", "flap" or "take the second upgrade"
/// live. Same shape as [`LoopError`], for the same reason: the shared part is
/// genuinely shared and the rest genuinely is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction<G> {
    /// Un-pause. See [`PAUSE_KEY`].
    Resume,
    /// Toggle borderless fullscreen. See [`FULLSCREEN_KEY`].
    Fullscreen,
    /// Toggle the debug panel. See [`DEBUG_OVERLAY_KEY`].
    DebugOverlay,
    /// Something only this game's menus do.
    Game(G),
}

/// The [`WidgetId`](crcbl_ui::WidgetId) carrying [`MenuAction::Resume`].
///
/// The loop's three actions have **fixed** ids, because the loop has to
/// recognise them in a layout it did not build. Written out rather than derived
/// from a discriminant, so that inserting a variant cannot silently re-point
/// every button in every sample.
pub const RESUME_ID: crcbl_ui::WidgetId = 1;

/// The id carrying [`MenuAction::Fullscreen`]. See [`RESUME_ID`].
pub const FULLSCREEN_ID: crcbl_ui::WidgetId = 2;

/// The id carrying [`MenuAction::DebugOverlay`]. See [`RESUME_ID`].
pub const DEBUG_OVERLAY_ID: crcbl_ui::WidgetId = 3;

/// The first id a game may use for an action of its own.
///
/// Everything below it is reserved. A game that numbered its own buttons from
/// one would collide with [`RESUME_ID`], and the symptom would be a button that
/// un-pauses instead of doing what its label says — which is why
/// [`MenuAction::from_id`] refuses a reserved id it was not given by the engine
/// rather than trusting the caller.
pub const FIRST_GAME_ID: crcbl_ui::WidgetId = 16;

impl<G> MenuAction<G> {
    /// The id this action is carried by, given the game's own numbering.
    ///
    /// # Panics
    ///
    /// If `game_id` returns a reserved id — below [`FIRST_GAME_ID`] — because
    /// that is a numbering bug that would otherwise show up as the wrong button
    /// firing.
    pub fn id(&self, game_id: impl FnOnce(&G) -> crcbl_ui::WidgetId) -> crcbl_ui::WidgetId {
        match self {
            Self::Resume => RESUME_ID,
            Self::Fullscreen => FULLSCREEN_ID,
            Self::DebugOverlay => DEBUG_OVERLAY_ID,
            Self::Game(game) => {
                let id = game_id(game);
                assert!(
                    id >= FIRST_GAME_ID,
                    "a game action claimed the reserved id {id}; \
                     game ids start at {FIRST_GAME_ID}",
                );
                id
            }
        }
    }

    /// The action an id names, asking `game` only about ids the loop does not
    /// own.
    ///
    /// `None` for an id from another menu system entirely, which is what makes
    /// it safe to point at a layout the loop did not build.
    pub fn from_id(
        id: crcbl_ui::WidgetId,
        game: impl FnOnce(crcbl_ui::WidgetId) -> Option<G>,
    ) -> Option<Self> {
        match id {
            RESUME_ID => Some(Self::Resume),
            FULLSCREEN_ID => Some(Self::Fullscreen),
            DEBUG_OVERLAY_ID => Some(Self::DebugOverlay),
            _ if id < FIRST_GAME_ID => None,
            _ => game(id).map(Self::Game),
        }
    }
}

/// The menu's half of a pump batch, and the held-key bookkeeping beside it.
///
/// Built for one pump and read after it. What [`Pending`] is to the window, this
/// is to the menu: the sample's closure asks it about each event, and what comes
/// back is the key the *game* should see — `None` for one the menu took.
///
/// # Why the widget id and not the game's action
///
/// [`Self::activated`] is a [`crcbl_ui::WidgetId`], because mapping one to a game's own
/// `MenuAction` is the game's business and is the only part of this that ever
/// differed. The engine does not learn what `RESUME` means.
#[derive(Debug)]
pub struct MenuPump<'a, K> {
    menus: &'a mut crcbl_ui::menu::MenuSet<K>,
    held: &'a mut Vec<crcbl_core::input::KeyCode>,
    showing: bool,
    /// The widget the commit key released over, if any.
    ///
    /// Set on **release**, not press, so the pressed frame of the skin is on
    /// screen for as long as the key is held.
    pub activated: Option<crcbl_ui::WidgetId>,
}

impl<'a, K: Copy + Eq> MenuPump<'a, K> {
    /// Starts a batch.
    ///
    /// `showing` is whether a menu was on screen **before** this pump — last
    /// frame's, deliberately. The pump runs before this frame's state is known,
    /// and the menu the player is pressing keys at is the one that was on screen
    /// when they pressed them.
    pub fn new(
        menus: &'a mut crcbl_ui::menu::MenuSet<K>,
        held: &'a mut Vec<crcbl_core::input::KeyCode>,
        showing: bool,
    ) -> Self {
        Self {
            menus,
            held,
            showing,
            activated: None,
        }
    }

    /// Offers one event, and returns the key the game should be told about.
    ///
    /// `None` for anything that was not a key, or that the menu claimed. Held
    /// keys are tracked either way, because a key released while a menu ate it
    /// still has to come off the list.
    pub fn observe(
        &mut self,
        event: &crcbl_shell::ShellEvent,
    ) -> Option<(crcbl_core::input::KeyCode, bool)> {
        let crcbl_shell::ShellEvent::Key {
            key_code: Some(code),
            state,
            ..
        } = event
        else {
            return None;
        };
        let code = *code;
        let pressed = matches!(state, crcbl_shell::ButtonState::Pressed);

        if self.showing {
            match code {
                MENU_UP_KEY => {
                    // Repeats move the selection, because holding Down to walk a
                    // list is what a player expects.
                    if pressed {
                        self.menus.select_previous();
                    }
                    return None;
                }
                MENU_DOWN_KEY => {
                    if pressed {
                        self.menus.select_next();
                    }
                    return None;
                }
                MENU_ACTIVATE_KEY => {
                    if pressed {
                        self.menus.press(true);
                    } else {
                        self.activated = self.menus.activate();
                    }
                    return None;
                }
                _ => {}
            }
        }

        if pressed {
            if !self.held.contains(&code) {
                self.held.push(code);
            }
        } else {
            self.held.retain(|key| *key != code);
        }
        Some((code, pressed))
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

/// Logs the backend, puts the shell's event clock on the engine's, and creates
/// the window.
///
/// The three lines every sample runs before the native and browser paths
/// diverge over *how they wait* for the first configure. `desc` stays the
/// caller's, because a title, an app id and a preferred size are the game's to
/// choose and nothing here could guess them.
///
/// **The clock alignment is the load-bearing line**, and it is the one a game
/// writing this by hand would omit: shell events carry their own timestamps, and
/// a shell whose clock never met the engine's reports every event at an origin
/// the loop cannot compare against its own.
///
/// # Errors
///
/// [`ShellError`] if the shell refused the window.
pub fn open_window<S: Shell + ?Sized>(
    shell: &mut S,
    clock_source: &Clock,
    desc: &crcbl_shell::WindowDesc<'_>,
) -> Result<WindowId, ShellError> {
    log::info!(
        "shell: {} backend, caps {:?}",
        shell.backend(),
        shell.caps()
    );
    shell.align_event_clock(clock_source.elapsed());
    shell.create_window(desc)
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

// ---------------------------------------------------------------------------
// Polled bring-up
// ---------------------------------------------------------------------------

/// A game's GPU bundle, opened without blocking on the device.
///
/// [`PolledBoot`] drives start-up for a caller that may not block —
/// a browser main thread resolves both of the things a blocking bring-up waits
/// for from inside the very event loop the wait would be sitting in — and this
/// is the four methods it needs from whatever a game calls its `Gpu`.
///
/// Deliberately **not** implemented for [`GpuContext`]. A sample's `Gpu` is its
/// renderers and its atlas as well as the context, and the thing that has to
/// arrive before a loop can be assembled is the whole bundle.
pub trait PolledGpu: Sized {
    /// The in-flight device request.
    type Pending;

    /// Asks for a device and returns immediately.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend could be opened at the requested extent.
    fn request<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        backend: Option<GpuBackend>,
    ) -> Result<Self::Pending, GpuError>;

    /// `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the device request failed.
    fn poll_pending(pending: &mut Self::Pending) -> Result<Option<Self>, GpuError>;

    /// The extent the swapchain was actually created at.
    fn extent(&self) -> (u32, u32);

    /// Rebuilds the swapchain at a new size.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the swapchain could not be recreated.
    fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError>;
}

/// How far a [`PolledBoot`] has got.
enum BootStage<G: PolledGpu> {
    /// The window has no size yet. On every platform this is the compositor's
    /// answer to `create_window`; in a browser it is the shim's first resize,
    /// from initial layout.
    Configure,
    /// A device has been requested and has not arrived.
    Device { pending: G::Pending },
    /// The parts have been handed over, or a step failed.
    Done,
}

/// Everything a loop needs, once start-up has finished.
///
/// The engine stops here rather than building the loop itself: assembling one is
/// the game's, and a `Loop` type parameter would drag its `Options` and its
/// error type in behind it for no gain.
#[derive(Debug)]
pub struct Booted<S: Shell + ?Sized, G> {
    /// The shell the window belongs to.
    pub shell: Box<S>,
    /// The one window.
    pub window: WindowId,
    /// The opened GPU bundle.
    pub gpu: G,
    /// The clock the caller handed to [`PolledBoot::request`].
    pub clock_source: Clock,
    /// Shell events observed during start-up, to be added to the loop's count.
    pub events: u64,
}

/// Start-up with the waits turned inside out, one poll per frame.
///
/// A blocking bring-up waits twice — once in [`wait_for_configure`] and once
/// inside the device request — and a browser main thread may do neither. This is
/// the same sequence driven by an outer loop that can be
/// `requestAnimationFrame`.
///
/// It is deliberately **not** the native path's implementation. A native loop
/// keeps its blocking waits because there they are the honest shape and their
/// timeouts are real diagnostics; what the two share is everything after the
/// waiting, which is the game's `assemble`.
pub struct PolledBoot<S: Shell + ?Sized, G: PolledGpu> {
    shell: Option<Box<S>>,
    window: WindowId,
    clock_source: Option<Clock>,
    backend: Option<GpuBackend>,
    stage: BootStage<G>,
    /// The most recent size the shell reported, which is not necessarily the one
    /// the swapchain was requested at: the canvas can be resized while the
    /// device request is still in flight.
    extent: Option<(u32, u32)>,
    events: u64,
}

impl<S: Shell + ?Sized, G: PolledGpu> std::fmt::Debug for PolledBoot<S, G> {
    /// Hand-written because `G::Pending` is the game's and need not be `Debug`;
    /// what a reader wants from this is which stage it is in.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolledBoot")
            .field("window", &self.window)
            .field(
                "stage",
                &match self.stage {
                    BootStage::Configure => "Configure",
                    BootStage::Device { .. } => "Device",
                    BootStage::Done => "Done",
                },
            )
            .field("extent", &self.extent)
            .field("events", &self.events)
            .finish()
    }
}

impl<S: Shell + ?Sized, G: PolledGpu> PolledBoot<S, G> {
    /// Starts the wait on an already-created window.
    ///
    /// The window is the caller's because its title and size are the game's;
    /// [`open_window`] is what makes one.
    #[must_use]
    pub fn request(
        shell: Box<S>,
        window: WindowId,
        clock_source: Clock,
        backend: Option<GpuBackend>,
    ) -> Self {
        Self {
            shell: Some(shell),
            window,
            clock_source: Some(clock_source),
            backend,
            stage: BootStage::Configure,
            extent: None,
            events: 0,
        }
    }

    /// Advances start-up. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// Events are pumped on every poll, including while the device request is
    /// outstanding — a queue nobody drains is how a resize during start-up
    /// becomes a swapchain at the wrong size, and how a shim's own diagnostic
    /// ("the canvas never gets input") goes unexplained.
    ///
    /// `E` is the caller's error type, so a game's own error is what comes back
    /// out; both conversions it needs are on [`LoopError`].
    ///
    /// # Errors
    ///
    /// `E` if the window went away before it had a size, or if the device
    /// request failed. Polling after the parts were handed over is a caller bug
    /// and reports [`GpuError::Unusable`].
    pub fn poll<E>(&mut self) -> Result<Option<Booted<S, G>>, E>
    where
        E: From<ShellError> + From<GpuError>,
    {
        let Some(shell) = self.shell.as_mut() else {
            return Err(E::from(GpuError::Unusable("this loop was already started")));
        };

        let mut pending = Pending::default();
        shell.pump(&mut |event| {
            pending.observe(&event);
        });
        self.events += pending.count;
        if pending.destroyed {
            return Err(E::from(ShellError::invalid_window(self.window)));
        }
        if let Some(size) = pending.resized {
            self.extent = Some((size.width, size.height));
        }

        match core::mem::replace(&mut self.stage, BootStage::Done) {
            BootStage::Configure => {
                let Some(extent) = self.extent else {
                    self.stage = BootStage::Configure;
                    return Ok(None);
                };
                log::info!("shell: first configure at {}x{}", extent.0, extent.1);
                // Left `Done` if this fails, so a failed start-up stays failed
                // rather than requesting a second device next frame.
                self.stage = BootStage::Device {
                    pending: G::request(shell.as_ref(), self.window, extent, self.backend)?,
                };
                Ok(None)
            }
            BootStage::Device { mut pending } => {
                let Some(mut gpu) = G::poll_pending(&mut pending)? else {
                    self.stage = BootStage::Device { pending };
                    return Ok(None);
                };
                // The canvas may have been resized while the request was in
                // flight; the swapchain was created at the older size.
                if let Some(extent) = self.extent
                    && extent != gpu.extent()
                {
                    gpu.resize(extent)?;
                }
                Ok(Some(Booted {
                    shell: self.shell.take().expect("checked at the top"),
                    window: self.window,
                    gpu,
                    clock_source: self.clock_source.take().expect("taken with the shell"),
                    events: self.events,
                }))
            }
            BootStage::Done => Err(E::from(GpuError::Unusable("this loop was already started"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A headless shell with one window, so the tests below drive `observe`
    /// through the **real** event path rather than through hand-built structs.
    ///
    /// `ShellEvent::Key` carries a device id, an event time, a scancode and a
    /// keysym that only a backend can supply; a test that filled them in by
    /// hand would be asserting against its own idea of an event.
    fn shell() -> (crcbl_shell::HeadlessShell, WindowId) {
        let mut shell = crcbl_shell::HeadlessShell::new();
        let window = shell
            .create_window(&crcbl_shell::WindowDesc::default())
            .expect("headless always creates a window");
        (shell, window)
    }

    /// Stands in for a game's own error type.
    #[derive(Debug)]
    struct GameError;

    impl std::fmt::Display for GameError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "the server would not bind")
        }
    }

    impl std::error::Error for GameError {}

    /// **A game's error reaches the report, it is not swallowed by the wrapper.**
    ///
    /// The whole point of the type parameter: five samples each had a `Game`
    /// variant, and a `Display` that printed only "game error" — with no
    /// `{error}` — would look right in every one of them and tell an operator
    /// nothing about why the run died.
    #[test]
    fn a_wrapped_game_error_still_says_what_the_game_refused() {
        let error: LoopError<GameError> = LoopError::Game(GameError);
        let text = error.to_string();
        assert!(text.contains("game error"), "{text}");
        assert!(
            text.contains("the server would not bind"),
            "the game's own message must survive the wrapper: {text}"
        );
    }

    /// **The reconfigure cap is quoted from the constant, not written out.**
    ///
    /// A message naming a different number than the loop actually enforces
    /// sends whoever reads it looking for a bug at the wrong count.
    #[test]
    fn the_never_presented_message_names_the_cap_the_loop_enforces() {
        let error: LoopError = LoopError::NeverPresented;
        assert!(
            error
                .to_string()
                .contains(&MAX_CONSECUTIVE_RECONFIGURES.to_string()),
            "{error}"
        );
    }

    /// **The three engine error types convert with `?`; the game's does not.**
    ///
    /// Asserted here rather than left to the samples' call sites, because a
    /// missing `From` is a compile error *there* and this is the crate that
    /// owes it. The absent `From<G>` is the deliberate half — see
    /// [`LoopError`]'s docs — and is not testable, only compilable.
    #[test]
    fn the_engines_own_errors_convert_into_the_wrapper() {
        let (mut shell, window) = shell();
        shell.destroy_window(window).expect("the window goes away");
        // A window that is gone: a real ShellError, not a hand-built one.
        let shell_error = shell
            .destroy_window(window)
            .expect_err("destroying it twice is refused");

        let wrapped: LoopError<GameError> = shell_error.into();
        assert!(matches!(wrapped, LoopError::Shell(_)));

        let wrapped: LoopError<GameError> = ConfigureError::TimedOut.into();
        assert!(matches!(wrapped, LoopError::Configure(_)));

        let wrapped: LoopError<GameError> = GpuError::Unusable("no queue").into();
        assert!(matches!(wrapped, LoopError::Gpu(_)));
    }

    /// **`open_window` puts the shell's event clock on the engine's.**
    ///
    /// The line a game writing this by hand omits, because the window still
    /// opens without it and nothing fails until something compares an event's
    /// timestamp against the loop's own clock. Asserted through an event's
    /// `time` rather than by reading the shell's clock back: the timestamp is
    /// what every consumer actually sees.
    #[test]
    fn opening_the_window_aligns_the_shells_event_clock_with_the_engines() {
        let mut shell = crcbl_shell::HeadlessShell::new();
        let mut clock = Clock::new(true);
        // Somewhere other than zero, or an unaligned shell would pass by
        // agreeing with a clock that had never moved.
        let elapsed = clock.advance();
        assert!(elapsed > Duration::ZERO, "the clock must have moved");

        let window = open_window(
            &mut shell,
            &clock,
            &crcbl_shell::WindowDesc {
                title: "alignment",
                ..crcbl_shell::WindowDesc::default()
            },
        )
        .expect("headless always creates a window");
        shell
            .key_press(window, crcbl_core::input::KeyCode::Space)
            .expect("the window is live");

        let mut times = Vec::new();
        shell.pump(&mut |event| {
            if let ShellEvent::Key { time, .. } = event {
                times.push(time);
            }
        });
        let time = *times.first().expect("the key press is reported");
        assert_eq!(
            time,
            crcbl_core::EventTime::from_duration(elapsed),
            "the shell timestamped its event on an epoch the loop cannot compare against",
        );
    }

    thread_local! {
        /// How many device requests [`FakeGpu`] has been asked for.
        static REQUESTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        /// Whether the next [`FakeGpu`] request should fail.
        static REQUESTS_FAIL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// A [`PolledGpu`] that arrives after a set number of polls.
    ///
    /// A real bundle needs a driver; what these tests are about is the order the
    /// engine does things in, which a fake observes better than a device can —
    /// it can count the requests.
    #[derive(Debug)]
    struct FakeGpu {
        extent: (u32, u32),
    }

    struct FakePending {
        polls_left: u32,
        extent: (u32, u32),
    }

    impl PolledGpu for FakeGpu {
        type Pending = FakePending;

        fn request<S: Shell + ?Sized>(
            _shell: &S,
            _window: WindowId,
            extent: (u32, u32),
            _backend: Option<GpuBackend>,
        ) -> Result<Self::Pending, GpuError> {
            REQUESTS.with(|n| n.set(n.get() + 1));
            if REQUESTS_FAIL.with(std::cell::Cell::get) {
                return Err(GpuError::Unusable("the fixture refused"));
            }
            Ok(FakePending {
                polls_left: 1,
                extent,
            })
        }

        fn poll_pending(pending: &mut Self::Pending) -> Result<Option<Self>, GpuError> {
            if pending.polls_left > 0 {
                pending.polls_left -= 1;
                return Ok(None);
            }
            Ok(Some(FakeGpu {
                extent: pending.extent,
            }))
        }

        fn extent(&self) -> (u32, u32) {
            self.extent
        }

        fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
            self.extent = extent;
            Ok(())
        }
    }

    /// **The device is asked for once, on the poll that first learns the size.**
    ///
    /// The browser's configure handshake: a `<canvas>` has no size until the
    /// document lays it out, and a swapchain cannot be created without one, so a
    /// shell that has not reported a size must leave start-up parked rather than
    /// request one at a guessed extent. Counted rather than inferred from the
    /// stage, because "parked" and "asked twice" are different bugs and only the
    /// count tells them apart.
    #[test]
    fn start_up_parks_until_the_window_reports_a_size_then_asks_once() {
        REQUESTS.with(|n| n.set(0));
        let mut shell = crcbl_shell::HeadlessShell::new();
        let window = shell
            .create_window(&crcbl_shell::WindowDesc::default())
            .expect("headless always creates a window");

        let mut boot: PolledBoot<crcbl_shell::HeadlessShell, FakeGpu> =
            PolledBoot::request(Box::new(shell), window, Clock::new(true), None);

        // `HeadlessShell` delays the first configure by a pump or two, exactly
        // as a compositor does.
        let mut polls = 0;
        let booted = loop {
            polls += 1;
            assert!(polls < 64, "the configure never arrived");
            if let Some(booted) = boot.poll::<LoopError>().expect("no failure") {
                break booted;
            }
        };

        assert!(polls > 1, "the fixture handed over a size immediately");
        assert_eq!(
            REQUESTS.with(std::cell::Cell::get),
            1,
            "the device must be asked for exactly once",
        );
        assert_eq!(
            booted.gpu.extent(),
            booted
                .shell
                .window_state(window)
                .expect("the window is live")
                .size()
                .map(|size| (size.width, size.height))
                .expect("the window has a size by now"),
            "the swapchain was left at a size the window no longer has",
        );
    }

    /// **Polling after the parts were handed over is refused, not repeated.**
    ///
    /// A caller that keeps polling would otherwise request a second device and
    /// leak the first.
    #[test]
    fn a_boot_that_already_finished_refuses_to_start_again() {
        REQUESTS.with(|n| n.set(0));
        let mut shell = crcbl_shell::HeadlessShell::new();
        let window = shell
            .create_window(&crcbl_shell::WindowDesc::default())
            .expect("headless always creates a window");
        let mut boot: PolledBoot<crcbl_shell::HeadlessShell, FakeGpu> =
            PolledBoot::request(Box::new(shell), window, Clock::new(true), None);

        let mut polls = 0;
        while boot.poll::<LoopError>().expect("no failure").is_none() {
            polls += 1;
            assert!(polls < 64, "start-up never finished");
        }

        let error = boot
            .poll::<LoopError>()
            .expect_err("a second hand-over is a caller bug");
        assert!(
            matches!(error, LoopError::Gpu(GpuError::Unusable(_))),
            "{error}"
        );
        assert_eq!(
            REQUESTS.with(std::cell::Cell::get),
            1,
            "polling again asked for another device",
        );
    }

    /// **A start-up that failed stays failed.**
    ///
    /// The subtle half of the state machine, and the one the success path cannot
    /// reach: when the device request errors, the stage was already moved to
    /// `Done` by the `mem::replace` above it, and the shell is still here. A
    /// machine that put itself back would request a second device on the very
    /// next frame — once per frame, forever, against a backend that has already
    /// said no.
    #[test]
    fn a_start_up_that_failed_does_not_ask_for_another_device_next_frame() {
        REQUESTS.with(|n| n.set(0));
        REQUESTS_FAIL.with(|f| f.set(true));
        let mut shell = crcbl_shell::HeadlessShell::new();
        let window = shell
            .create_window(&crcbl_shell::WindowDesc::default())
            .expect("headless always creates a window");
        let mut boot: PolledBoot<crcbl_shell::HeadlessShell, FakeGpu> =
            PolledBoot::request(Box::new(shell), window, Clock::new(true), None);

        let mut polls = 0;
        let failure = loop {
            polls += 1;
            assert!(polls < 64, "the configure never arrived");
            if let Err(error) = boot.poll::<LoopError>() {
                break error;
            }
        };
        assert!(matches!(failure, LoopError::Gpu(_)), "{failure}");
        assert_eq!(REQUESTS.with(std::cell::Cell::get), 1);

        // Three more frames of an outer loop that did not notice.
        REQUESTS_FAIL.with(|f| f.set(false));
        for _ in 0..3 {
            let error = boot
                .poll::<LoopError>()
                .expect_err("a failed start-up must stay failed");
            assert!(
                matches!(error, LoopError::Gpu(GpuError::Unusable(_))),
                "{error}"
            );
        }
        assert_eq!(
            REQUESTS.with(std::cell::Cell::get),
            1,
            "a failed start-up asked for another device",
        );
    }

    /// A loop that stops or fails after a set number of frames, and can also
    /// fail its teardown.
    struct DrivenLoop {
        frames_left: u32,
        fail_frame: bool,
        fail_teardown: bool,
        torn_down: std::rc::Rc<std::cell::Cell<Option<ExitReason>>>,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum DrivenError {
        Frame,
        Teardown,
    }

    impl core::fmt::Display for DrivenError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "{self:?}")
        }
    }

    impl GameLoop for DrivenLoop {
        type Error = DrivenError;
        type Summary = u32;

        fn frame(&mut self) -> Result<Flow, Self::Error> {
            if self.frames_left == 0 {
                return if self.fail_frame {
                    Err(DrivenError::Frame)
                } else {
                    Ok(Flow::Stop(ExitReason::CloseRequested))
                };
            }
            self.frames_left -= 1;
            Ok(Flow::Continue)
        }

        fn finish(self, exit: ExitReason) -> Result<Self::Summary, Self::Error> {
            self.torn_down.set(Some(exit));
            if self.fail_teardown {
                return Err(DrivenError::Teardown);
            }
            Ok(0)
        }
    }

    /// **A frame error is what comes back, and teardown still runs.**
    ///
    /// Both halves fail silently on their own. A driver that returned the
    /// teardown error would replace the real cause with a consequence; one that
    /// skipped teardown would leak a device and a window on every failed run.
    #[test]
    fn a_failed_frame_is_reported_and_the_loop_is_still_torn_down() {
        let torn_down = std::rc::Rc::new(std::cell::Cell::new(None));
        let error = drive(DrivenLoop {
            frames_left: 2,
            fail_frame: true,
            fail_teardown: true,
            torn_down: std::rc::Rc::clone(&torn_down),
        })
        .expect_err("the frame failed");

        assert_eq!(
            error,
            DrivenError::Frame,
            "the teardown error replaced the cause",
        );
        assert_eq!(
            torn_down.get(),
            Some(ExitReason::Failed),
            "a failed run must still release its device and window",
        );
    }

    /// **A clean stop tears down with the reason it stopped for.**
    #[test]
    fn a_loop_that_stops_cleanly_tears_down_with_its_own_reason() {
        let torn_down = std::rc::Rc::new(std::cell::Cell::new(None));
        drive(DrivenLoop {
            frames_left: 3,
            fail_frame: false,
            fail_teardown: false,
            torn_down: std::rc::Rc::clone(&torn_down),
        })
        .expect("a clean stop");
        assert_eq!(torn_down.get(), Some(ExitReason::CloseRequested));
    }

    /// **A swapchain that never presents fails the run instead of hanging it.**
    ///
    /// The reason the reconfigure cap exists: `--frames N` counts *presented*
    /// frames, so a surface that reconfigures forever would leave the budget
    /// unreachable and the loop spinning with no error and no exit.
    #[test]
    fn a_surface_that_never_presents_gives_up_rather_than_spinning() {
        let mut budget = FrameBudget::new(Some(1));
        for i in 1..MAX_CONSECUTIVE_RECONFIGURES {
            budget
                .record::<core::convert::Infallible>(FrameOutcome::Reconfigured)
                .unwrap_or_else(|_| panic!("gave up after {i}, before the cap"));
        }
        let error = budget
            .record::<core::convert::Infallible>(FrameOutcome::Reconfigured)
            .expect_err("the cap was never reached");
        assert!(matches!(error, LoopError::NeverPresented), "{error}");
        assert!(
            !budget.is_spent(),
            "no frame ever presented, so none counted"
        );
    }

    /// **One presented frame clears the run**, so a resize storm that recovers
    /// is not a failure.
    #[test]
    fn a_reconfigure_run_broken_by_a_present_starts_over() {
        let mut budget = FrameBudget::new(None);
        for _ in 0..MAX_CONSECUTIVE_RECONFIGURES - 1 {
            budget
                .record::<core::convert::Infallible>(FrameOutcome::Reconfigured)
                .expect("under the cap");
        }
        budget
            .record::<core::convert::Infallible>(FrameOutcome::Presented)
            .expect("a present is never a failure");
        assert_eq!(budget.presented(), 1);

        // The counter reset, so another near-full run is still fine.
        for _ in 0..MAX_CONSECUTIVE_RECONFIGURES - 1 {
            budget
                .record::<core::convert::Infallible>(FrameOutcome::Reconfigured)
                .expect("the run did not reset");
        }
    }

    /// **The budget counts presented frames, not attempts.**
    #[test]
    fn the_budget_is_spent_by_presented_frames_only() {
        let mut budget = FrameBudget::new(Some(2));
        assert!(!budget.is_spent());
        budget
            .record::<core::convert::Infallible>(FrameOutcome::Reconfigured)
            .expect("under the cap");
        assert!(!budget.is_spent(), "a reconfigure spent budget");
        for _ in 0..2 {
            budget
                .record::<core::convert::Infallible>(FrameOutcome::Presented)
                .expect("presenting never fails");
        }
        assert!(budget.is_spent());
    }

    /// **A focus loss releases every held key before it pauses.**
    ///
    /// The half that matters: a game resuming while it still believes a key is
    /// held flies into the wall until the player taps it again. Pausing without
    /// releasing looks identical until someone actually holds a key over a
    /// window switch.
    #[test]
    fn losing_focus_releases_what_was_held_before_pausing() {
        use crcbl_core::input::KeyCode;
        let mut held = vec![KeyCode::KeyA, KeyCode::KeyD];
        let mut paused = false;
        let mut released = Vec::new();

        lose_focus(&mut held, &mut paused, |key| released.push(key));

        assert_eq!(released, vec![KeyCode::KeyA, KeyCode::KeyD]);
        assert!(held.is_empty(), "the held list survived the focus loss");
        assert!(paused);

        // A second focus loss with nothing held changes nothing.
        lose_focus(&mut held, &mut paused, |_| panic!("nothing was held"));
        assert!(paused);
    }

    /// **A pause drains the accumulator instead of letting it fill.**
    ///
    /// The whole reason `run_ticks` takes `paused` rather than the caller
    /// skipping the call: a paused frame that stopped draining would bank the
    /// pause and spend it in one burst on the frame the player resumes. Measured
    /// as "the frame after a long pause runs the one tick it is owed, not a
    /// catch-up storm".
    #[test]
    fn a_long_pause_costs_one_tick_on_resume_rather_than_a_burst() {
        let mut clock = crcbl_core::FrameClock::new(60);
        let step = Duration::from_nanos(1_000_000_000 / 60);
        let mut elapsed = Duration::ZERO;
        clock.update(elapsed);

        // Ten seconds of paused frames.
        for _ in 0..600 {
            elapsed += step;
            clock.update(elapsed);
            assert_eq!(run_ticks(&mut clock, true, || unreachable!()), 0);
        }

        // The first live frame.
        elapsed += step;
        clock.update(elapsed);
        let mut ran = 0;
        let count = run_ticks(&mut clock, false, || ran += 1);
        assert_eq!(count, 1, "resuming ran a catch-up burst of {count} ticks");
        assert_eq!(ran, 1, "the closure and the count disagree");
    }

    /// **The count is what actually ran, and the closure runs exactly that many
    /// times.**
    ///
    /// A frame long enough for several ticks must run several: anything stepped
    /// once per frame instead has a speed proportional to the frame rate, which
    /// a headless run — where a frame is pinned to exactly one tick — cannot
    /// see.
    #[test]
    fn a_slow_frame_runs_every_tick_it_owes() {
        let mut clock = crcbl_core::FrameClock::new(60);
        clock.update(Duration::ZERO);
        // Four ticks' worth of wall time in one frame.
        clock.update(Duration::from_nanos(4 * 1_000_000_000 / 60));

        let mut ran = 0;
        let count = run_ticks(&mut clock, false, || ran += 1);
        assert_eq!(count, 4, "a four-tick frame ran {count}");
        assert_eq!(ran, 4);
    }

    /// **Toggling reads the window's mode back rather than remembering it.**
    ///
    /// A loop that tracked its own idea of fullscreen would invert a state the
    /// compositor never entered, and the key would then work every other press.
    /// Asserted by toggling twice and requiring the request to come back to
    /// where it started — which a remembered flag also does, so the third
    /// toggle is the one that matters: it must ask for borderless again.
    #[test]
    fn toggling_the_mode_asks_for_the_opposite_of_what_the_window_is_in() {
        let (mut shell, window) = shell();
        assert_eq!(ModeRequest::mode(&shell, window), DisplayMode::Windowed);

        ModeRequest::toggle(&mut shell, window).expect("the window is live");
        assert!(
            ModeRequest::mode(&shell, window).is_borderless(),
            "the first toggle must ask for borderless",
        );

        ModeRequest::toggle(&mut shell, window).expect("the window is live");
        assert_eq!(
            ModeRequest::mode(&shell, window),
            DisplayMode::Windowed,
            "the second must come back",
        );

        ModeRequest::toggle(&mut shell, window).expect("the window is live");
        assert!(
            ModeRequest::mode(&shell, window).is_borderless(),
            "the third must ask for borderless again",
        );
    }

    /// **A refused request is reported, and `mode` keeps saying what the window
    /// system actually did.**
    ///
    /// The property the whole type exists for. `HeadlessShell` accepts
    /// `set_mode(Borderless)` and then stays `Windowed`, which is exactly what a
    /// tiling window manager or a browser with no `requestFullscreen` shim does
    /// — so a loop that reported its *request* would tell the player it went
    /// fullscreen when nothing happened.
    #[test]
    fn a_refused_mode_request_is_noticed_and_the_reported_mode_stays_honest() {
        let (mut shell, window) = shell();
        shell
            .resize(window, crcbl_shell::PhysicalSize::new(320, 240))
            .expect("the window is live");
        shell.pump(&mut |_| {});

        let mut request = ModeRequest::new();
        request.check(&shell, window);
        assert!(request.honoured, "a configured window starts out agreeing");

        ModeRequest::toggle(&mut shell, window).expect("the shell accepts the request");
        shell.pump(&mut |_| {});
        request.check(&shell, window);
        assert!(
            !request.honoured,
            "the refusal went unnoticed, so it would never be logged",
        );
        assert_eq!(
            ModeRequest::mode(&shell, window),
            DisplayMode::Windowed,
            "mode() reported the request rather than what the window actually is",
        );

        // Latched: checking again must not flip it back, or the loop logs the
        // same refusal on every frame forever.
        request.check(&shell, window);
        assert!(!request.honoured);
    }

    /// **A click faster than one frame still latches and fires.**
    ///
    /// The exception in [`PointerCapture::resolve`], and the one a naive "down
    /// is false on the release frame" rule gets wrong: a tap that arrives as one
    /// event pair must be `down` *and* `released` in the same batch, or
    /// `UiState::interact` never sees a press and the button does nothing.
    #[test]
    fn a_press_and_release_in_one_batch_is_still_a_click() {
        let mut capture = PointerCapture::new();
        let mut pending = capture.pending();
        pending.pointer_pressed = true;
        pending.pointer_released = true;

        let input = capture.resolve(&pending);
        assert!(input.down, "a tap inside one frame must latch");
        assert!(input.released, "and fire in the same call");
    }

    /// **A press held across frames stays down until the release, and the
    /// release frame is not down.**
    ///
    /// The other half of the same rule. A release frame that still reported
    /// `down` would credit the press to whatever the cursor was over at release,
    /// which is the bug press capture exists to prevent.
    #[test]
    fn a_press_held_across_frames_goes_down_then_up_exactly_once() {
        let mut capture = PointerCapture::new();

        let mut press = capture.pending();
        press.pointer_pressed = true;
        let input = capture.resolve(&press);
        assert!(input.down && !input.released, "the press frame");

        // A still frame in between: no pointer events at all.
        let idle = capture.pending();
        let input = capture.resolve(&idle);
        assert!(input.down && !input.released, "the button is still held");

        let mut release = capture.pending();
        release.pointer_released = true;
        let input = capture.resolve(&release);
        assert!(
            !input.down && input.released,
            "the release frame must not also be down",
        );

        // …and the capture is clear afterwards, or the next still frame would
        // report a button nobody is pressing.
        let idle = capture.pending();
        let input = capture.resolve(&idle);
        assert!(!input.down && !input.released, "the capture did not clear");
    }

    /// **A cursor that has never been in the window is nowhere, not at the
    /// origin** — which is a real pixel, and in every sample sits inside the HUD.
    #[test]
    fn a_pointer_that_has_never_arrived_is_not_at_the_origin() {
        let mut capture = PointerCapture::new();
        let pending = capture.pending();
        let input = capture.resolve(&pending);
        assert!(input.pos.x.is_infinite() && input.pos.x.is_sign_negative());
        assert_eq!(capture.at(), None);
    }

    /// **The position carries across a batch that had no pointer event.**
    #[test]
    fn a_still_frame_does_not_forget_where_the_cursor_is() {
        let mut capture = PointerCapture::new();
        let mut moved = capture.pending();
        moved.pointer = Some(glam::Vec2::new(12.0, 34.0));
        capture.resolve(&moved);

        let idle = capture.pending();
        let input = capture.resolve(&idle);
        assert_eq!(input.pos, glam::Vec2::new(12.0, 34.0));
    }

    /// A game action, for the menu-action tests.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Serve {
        Launch,
    }

    /// **The loop's three actions round-trip, and a game's own do too.**
    ///
    /// `from_id` has to work on a layout the loop did not build, so the ids it
    /// owns are fixed and everything else is asked of the game.
    #[test]
    fn a_menu_action_round_trips_through_its_widget_id() {
        let game_id = |_: &Serve| FIRST_GAME_ID;
        let from_game = |id| (id == FIRST_GAME_ID).then_some(Serve::Launch);

        for action in [
            MenuAction::Resume,
            MenuAction::Fullscreen,
            MenuAction::DebugOverlay,
            MenuAction::Game(Serve::Launch),
        ] {
            let id = action.id(game_id);
            assert_eq!(
                MenuAction::from_id(id, from_game),
                Some(action),
                "id {id} did not come back as the action that produced it",
            );
        }

        // The loop's three ids are distinct from each other and from the game's.
        let ids: Vec<_> = [
            MenuAction::Resume,
            MenuAction::Fullscreen,
            MenuAction::DebugOverlay,
            MenuAction::Game(Serve::Launch),
        ]
        .iter()
        .map(|action| action.id(game_id))
        .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "two actions share an id: {ids:?}");
    }

    /// **An id from another menu system is not an action.**
    ///
    /// The property that lets a loop point at a layout it did not build without
    /// firing something at random.
    #[test]
    fn an_unknown_id_names_no_action() {
        let from_game = |_| None::<Serve>;
        assert_eq!(MenuAction::from_id(9_999, from_game), None);
        // …and a reserved id the loop does not own is refused without the game
        // ever being asked, so a game cannot claim one by accident.
        assert_eq!(
            MenuAction::from_id(FIRST_GAME_ID - 1, |_| Some(Serve::Launch)),
            None,
            "a reserved id reached the game's mapping",
        );
    }

    /// **A game that numbers a button into the reserved range is caught.**
    ///
    /// Silently, this is a button that un-pauses instead of doing what its label
    /// says — the symptom is in the wrong place entirely, so it is worth a
    /// panic at the point the id is produced.
    #[test]
    #[should_panic(expected = "claimed the reserved id")]
    fn a_game_action_numbered_into_the_reserved_range_panics() {
        let _ = MenuAction::Game(Serve::Launch).id(|_: &Serve| RESUME_ID);
    }

    /// The two states a `MenuPump` is tested in.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Panel {
        None,
        Shown,
    }

    /// A set with the panel **shown**, which is what makes `activate` and the
    /// selection moves real — a `MenuSet` with nothing shown answers `None` to
    /// everything, and a test built on one passes without the menu doing a thing.
    fn menus() -> crcbl_ui::menu::MenuSet<Panel> {
        let mut set = crcbl_ui::menu::MenuSet::new(
            Panel::None,
            vec![(
                Panel::Shown,
                crcbl_ui::menu::Menu::new(
                    "PAUSED",
                    vec![
                        crcbl_ui::menu::MenuItem::new(1, "RESUME", "ESC"),
                        crcbl_ui::menu::MenuItem::new(2, "QUIT", "Q"),
                    ],
                ),
            )],
        );
        set.show(Panel::Shown);
        set
    }

    /// **A menu key reaches the menu and NOT the game; every other key reaches
    /// the game.**
    ///
    /// The half that matters is the second: the menu takes Up, Down and Enter
    /// only while it is showing, so a game that binds Up to something keeps it
    /// on every frame with no menu on screen. A pump that swallowed them
    /// unconditionally would be a control that silently stopped working.
    #[test]
    fn the_menu_keys_are_taken_only_while_a_menu_is_showing() {
        let (mut shell, window) = shell();
        let mut set = menus();
        let mut held = Vec::new();

        for showing in [true, false] {
            shell.key_press(window, MENU_DOWN_KEY).expect("live");
            shell.key_release(window, MENU_DOWN_KEY).expect("live");
            shell
                .key_press(window, crcbl_core::input::KeyCode::KeyW)
                .expect("live");
            shell
                .key_release(window, crcbl_core::input::KeyCode::KeyW)
                .expect("live");

            let selected = |set: &crcbl_ui::menu::MenuSet<Panel>| {
                set.current()
                    .and_then(crcbl_ui::menu::Menu::selected_item)
                    .map(|item| item.id)
            };
            let before = selected(&set);

            let mut forwarded = Vec::new();
            let mut pump = MenuPump::new(&mut set, &mut held, showing);
            shell.pump(&mut |event| {
                if let Some(key) = pump.observe(&event) {
                    forwarded.push(key);
                }
            });

            let seen: Vec<_> = forwarded.iter().map(|(code, _)| *code).collect();
            assert!(
                seen.contains(&crcbl_core::input::KeyCode::KeyW),
                "a game key must always reach the game (showing={showing})",
            );
            assert_eq!(
                seen.contains(&MENU_DOWN_KEY),
                !showing,
                "the menu key must reach the game only when no menu is up",
            );
            // …and it must have *done* something, or a pump that dropped the key
            // on the floor would pass the check above. Compared against the
            // selection before this pass, because it persists between them.
            assert_eq!(
                selected(&set) != before,
                showing,
                "Down moved the selection only when the menu had the key",
            );
        }
    }

    /// **The commit key fires on release, not on press.**
    ///
    /// So the pressed frame of the skin is on screen for as long as the key is
    /// held. A pump that activated on press would light the button and act in
    /// the same frame, which reads as a missed press.
    #[test]
    fn the_commit_key_activates_on_release() {
        let (mut shell, window) = shell();
        let mut set = menus();
        let mut held = Vec::new();

        shell.key_press(window, MENU_ACTIVATE_KEY).expect("live");
        let mut pump = MenuPump::new(&mut set, &mut held, true);
        shell.pump(&mut |event| {
            pump.observe(&event);
        });
        assert_eq!(pump.activated, None, "pressing must not commit");

        shell.key_release(window, MENU_ACTIVATE_KEY).expect("live");
        let mut pump = MenuPump::new(&mut set, &mut held, true);
        shell.pump(&mut |event| {
            pump.observe(&event);
        });
        assert_eq!(
            pump.activated,
            Some(1),
            "releasing must commit the selected item",
        );
    }

    /// **A key released while a menu ate it still comes off the held list.**
    ///
    /// The bookkeeping runs for menu keys too, or a key held as a menu opened
    /// would stay "held" forever and the game would read it as stuck down.
    #[test]
    fn a_key_is_tracked_as_held_between_its_press_and_its_release() {
        let (mut shell, window) = shell();
        let mut set = menus();
        let mut held = Vec::new();

        shell
            .key_press(window, crcbl_core::input::KeyCode::KeyW)
            .expect("live");
        let mut pump = MenuPump::new(&mut set, &mut held, false);
        shell.pump(&mut |event| {
            pump.observe(&event);
        });
        assert_eq!(held, vec![crcbl_core::input::KeyCode::KeyW]);

        shell
            .key_release(window, crcbl_core::input::KeyCode::KeyW)
            .expect("live");
        let mut pump = MenuPump::new(&mut set, &mut held, false);
        shell.pump(&mut |event| {
            pump.observe(&event);
        });
        assert!(held.is_empty(), "the release did not clear it: {held:?}");
    }

    /// Drains the shell into a `Pending`, and reports what each event was
    /// judged to be.
    fn drain(shell: &mut crcbl_shell::HeadlessShell, pending: &mut Pending) -> Vec<Handled> {
        let mut verdicts = Vec::new();
        shell.pump(&mut |event| verdicts.push(pending.observe(&event)));
        verdicts
    }

    /// **A batch with no pointer event has not moved the pointer.**
    ///
    /// The reason [`Pending::carrying`] exists at all, and the reason it is not
    /// `Default::default()`: a menu's hover state is resolved from
    /// `pending.pointer` every frame, so a loop that forgot the cursor between
    /// frames would light a button under the mouse and drop it again on the
    /// first frame the mouse held still.
    #[test]
    fn a_batch_with_no_pointer_event_keeps_the_pointer_it_started_with() {
        let (mut shell, window) = shell();
        let last = Some(glam::Vec2::new(120.0, 40.0));

        shell
            .key(
                window,
                crcbl_core::input::KeyCode::KeyA,
                crcbl_shell::ButtonState::Pressed,
            )
            .expect("the window is live");
        let mut quiet = Pending::carrying(last);
        assert!(drain(&mut shell, &mut quiet).contains(&Handled::Game));
        assert_eq!(
            quiet.pointer, last,
            "a frame of keyboard events forgot where the cursor was"
        );

        // …and a batch that *does* carry motion takes the new position.
        shell
            .move_pointer(
                window,
                crcbl_shell::PhysicalPoint { x: 7.0, y: 9.0 },
                (0.0, 0.0),
            )
            .expect("the window is live");
        let mut moved = Pending::carrying(last);
        drain(&mut shell, &mut moved);
        assert_eq!(moved.pointer, Some(glam::Vec2::new(7.0, 9.0)));
    }

    /// The three reserved keys are the loop's and must not reach the game: one
    /// recorded into a tick's input would change what a seeded, scripted run
    /// replays.
    #[test]
    fn the_reserved_keys_are_claimed_and_every_other_key_is_not() {
        let (mut shell, window) = shell();
        for code in [DEBUG_OVERLAY_KEY, PAUSE_KEY, FULLSCREEN_KEY] {
            shell
                .key(window, code, crcbl_shell::ButtonState::Pressed)
                .expect("the window is live");
        }
        let mut pending = Pending::default();
        assert!(
            drain(&mut shell, &mut pending)
                .iter()
                .all(|verdict| *verdict == Handled::Loop),
            "a reserved key was handed to the game"
        );
        assert!(pending.toggle_debug_overlay);
        assert!(pending.toggle_pause);
        assert!(pending.toggle_fullscreen);

        for code in [
            crcbl_core::input::KeyCode::Space,
            crcbl_core::input::KeyCode::KeyW,
            crcbl_core::input::KeyCode::Enter,
        ] {
            shell
                .key(window, code, crcbl_shell::ButtonState::Pressed)
                .expect("the window is live");
        }
        // Only the key events are the claim here: a pump batch can carry other
        // events the loop legitimately owns, and asserting over all of them
        // would be asserting about the shell rather than about `observe`.
        let mut game_keys = Pending::default();
        let mut verdicts = Vec::new();
        shell.pump(&mut |event| {
            if matches!(event, ShellEvent::Key { .. }) {
                verdicts.push(game_keys.observe(&event));
            } else {
                game_keys.observe(&event);
            }
        });
        assert_eq!(verdicts.len(), 3, "the three game keys did not arrive");
        assert!(
            verdicts.iter().all(|verdict| *verdict == Handled::Game),
            "the loop swallowed a game key: {verdicts:?}"
        );
        assert!(!game_keys.toggle_pause, "a game key moved a reserved flag");
    }

    /// A held key repeats, and a display-mode toggle driven at the keyboard's
    /// repeat rate is a window flickering between modes. A release must not
    /// toggle either, or every press would fire twice.
    #[test]
    fn only_a_real_press_of_a_reserved_key_toggles() {
        let (mut shell, window) = shell();

        shell
            .key_repeat(window, FULLSCREEN_KEY)
            .expect("the window is live");
        shell
            .key(window, FULLSCREEN_KEY, crcbl_shell::ButtonState::Released)
            .expect("the window is live");
        let mut pending = Pending::default();
        drain(&mut shell, &mut pending);
        assert!(
            !pending.toggle_fullscreen,
            "a repeat or a release toggled the display mode"
        );

        shell
            .key(window, FULLSCREEN_KEY, crcbl_shell::ButtonState::Pressed)
            .expect("the window is live");
        drain(&mut shell, &mut pending);
        assert!(pending.toggle_fullscreen, "a real press did not toggle");
    }

    /// A thousand a second is a millisecond a frame.
    #[test]
    fn the_default_limit_is_a_millisecond_a_frame() {
        assert_eq!(FrameLimit::DEFAULT_FPS, 1000);
        assert_eq!(
            FrameLimit::default().period(),
            Some(Duration::from_millis(1))
        );
        assert_eq!(
            FrameLimit::fps(60).period(),
            Some(Duration::from_nanos(16_666_666))
        );
        assert_eq!(FrameLimit::unlimited().period(), None);
    }

    /// Zero frames a second is "no limit", not a divide by zero.
    #[test]
    fn a_limit_of_zero_is_no_limit() {
        assert_eq!(FrameLimit::fps(0), FrameLimit::unlimited());
        assert_eq!(FrameLimit::fps(0).period(), None);
        assert_eq!(
            FrameLimit::fps(0).wait_from(Some(Duration::ZERO), Duration::ZERO),
            None,
            "and it waits for nothing"
        );
    }

    /// An early frame waits exactly the remainder of its period.
    #[test]
    fn an_early_frame_waits_out_the_rest_of_its_period() {
        let limit = FrameLimit::fps(100); // 10ms
        let started = Duration::from_millis(50);

        assert_eq!(
            limit.wait_from(Some(started), started + Duration::from_millis(4)),
            Some(Duration::from_millis(6)),
            "4ms into a 10ms period leaves 6"
        );
        assert_eq!(
            limit.wait_from(Some(started), started),
            Some(Duration::from_millis(10)),
            "no time spent yet leaves the whole period"
        );
    }

    /// A late frame does not make the next one early.
    ///
    /// The failure this guards is a burst: if a limiter tried to average out to
    /// the target rate, one slow frame would be repaid by running the following
    /// frames back to back, which is the opposite of what a limiter is for.
    #[test]
    fn a_late_frame_is_never_caught_up() {
        let limit = FrameLimit::fps(100);
        let started = Duration::from_millis(50);

        assert_eq!(
            limit.wait_from(Some(started), started + Duration::from_millis(10)),
            None,
            "exactly on the deadline is not early"
        );
        assert_eq!(
            limit.wait_from(Some(started), started + Duration::from_millis(500)),
            None,
            "fifty periods late, and the answer is still 'do not wait', not a \
             negative wait and not a credit against the next frame"
        );
    }

    /// The first frame of a run does not wait.
    #[test]
    fn the_first_frame_never_waits() {
        assert_eq!(
            FrameLimit::fps(100).wait_from(None, Duration::from_secs(9)),
            None
        );
    }

    /// A headless clock has no limit, and cannot be given one.
    ///
    /// Not a policy the loop has to remember — a manual clock has no wall clock
    /// to wait against, so the limiter is absent by construction. A headless
    /// run that quietly obeyed a limit would stop being deterministic, and CI
    /// would take a thousand times longer to say so.
    #[test]
    fn a_headless_clock_cannot_be_paced() {
        let mut clock = Clock::new(true);
        assert_eq!(clock.limit(), None);

        clock.set_limit(FrameLimit::fps(1));
        assert_eq!(clock.limit(), None, "still none: the call did nothing");

        let first = clock.advance();
        let second = clock.advance();
        assert_eq!(
            second - first,
            HEADLESS_FRAME_STEP,
            "and it still steps by exactly one frame, at once"
        );
    }

    /// A real clock starts at the default limit and takes a new one.
    #[test]
    fn a_real_clock_starts_limited_and_can_be_changed() {
        let mut clock = Clock::new(false);
        assert_eq!(clock.limit(), Some(FrameLimit::default()));

        clock.set_limit(FrameLimit::unlimited());
        assert_eq!(clock.limit(), Some(FrameLimit::unlimited()));

        clock.set_limit(FrameLimit::fps(30));
        assert_eq!(clock.limit(), Some(FrameLimit::fps(30)));
    }

    /// The limiter actually holds a real clock back.
    ///
    /// The only test here that spends wall time, and the only one that observes
    /// the *mechanism* rather than the arithmetic: every other limiter test
    /// asks `wait_from` what it would do, which passes identically whether
    /// [`Clock::advance`] consults it or ignores it.
    ///
    /// Asserts a lower bound only. A limiter can slow a loop and can never
    /// speed one up, so "at least the period" is the whole promise — an upper
    /// bound would be a test of this machine's scheduler.
    #[test]
    fn a_limited_real_clock_holds_the_next_frame_back() {
        /// Long enough to be unmistakable against scheduler noise, short
        /// enough that the suite does not notice: two frames of it.
        const PERIOD: Duration = Duration::from_millis(20);

        let mut clock = Clock::new(false);
        clock.set_limit(FrameLimit::fps(50)); // 20ms

        let first = clock.advance();
        let second = clock.advance();
        assert!(
            second - first >= PERIOD,
            "the second frame started {:?} after the first, which is less than \
             the {PERIOD:?} the limit asks for — advance() is not waiting",
            second - first
        );

        let mut unlimited = Clock::new(false);
        unlimited.set_limit(FrameLimit::unlimited());
        let first = unlimited.advance();
        let second = unlimited.advance();
        assert!(
            second - first < PERIOD,
            "an unlimited clock waited {:?}, so something is pacing it that \
             should not be",
            second - first
        );
    }

    /// Every pacing choice ends in a mode the surface must support.
    ///
    /// `choose_present_mode` walks the list and falls back to `Fifo` if nothing
    /// matches, so a list that omitted it would still work — but only by
    /// accident, and a caller reading the list would not know the last entry
    /// was the guaranteed one. Fifo is the only mode Vulkan requires and the
    /// only one WebGPU has.
    #[test]
    fn every_pacing_ends_in_the_mode_that_always_exists() {
        for pacing in [Pacing::Vsync, Pacing::Adaptive, Pacing::Off] {
            let modes = pacing.preferences();
            assert!(!modes.is_empty(), "{pacing:?} offers no mode at all");
            assert_eq!(
                modes.last(),
                Some(&PresentMode::Fifo),
                "{pacing:?} does not end in Fifo, so a surface offering only \
                 Fifo would be matched by luck rather than by the list"
            );
        }
    }

    /// The three pacings are three different requests.
    ///
    /// Vsync asks for exactly one mode — it is the one case where a fallback
    /// would silently give the caller the opposite of what they asked for.
    #[test]
    fn vsync_asks_for_vsync_and_nothing_else() {
        assert_eq!(Pacing::Vsync.preferences(), &[PresentMode::Fifo]);
        assert_eq!(Pacing::default(), Pacing::Vsync);

        assert_eq!(
            Pacing::Adaptive.preferences().first(),
            Some(&PresentMode::FifoRelaxed),
            "adaptive prefers the mode that tears only when late"
        );
        assert_eq!(
            Pacing::Off.preferences().first(),
            Some(&PresentMode::Mailbox),
            "off prefers the untorn uncapped mode before the torn one"
        );
    }

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

    /// An error the device reported **outside any call** stops the frame.
    ///
    /// This is the failure mode P5.13 found in a browser and could not see from
    /// the code: WebGPU hands back a pipeline object whose shader did not
    /// compile and delivers the reason to the device's error channel later, so
    /// every subsequent submit is silently discarded and the page draws nothing
    /// while reporting itself healthy. `acquire` asks the device before it
    /// records anything, which is the only point in a frame where that answer
    /// can still change what happens.
    #[test]
    fn a_device_error_raised_between_calls_fails_the_next_frame() {
        use crcbl_hal::null::{NullInstance, Recorder};
        use crcbl_shell::{HeadlessShell, WindowDesc};

        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("headless always creates a window");
        let mut events = 0;
        let extent = wait_for_configure(&mut shell, window, &mut events).expect("configured");

        // Built by hand rather than through the registry, because the point of
        // the test is to hold the recorder that decides when the device fails.
        let recorder = Recorder::new();
        let instance: Box<dyn Instance> =
            Box::new(NullInstance::tier_a().with_recorder(recorder.clone()));
        let target = shell
            .surface_target(window)
            .expect("the window is still alive");
        let stage = GpuContext::start_device(
            instance,
            &target,
            extent,
            "device error test",
            Features::empty(),
            Features::empty(),
            Pacing::default(),
        )
        .expect("the null backend opens everywhere");
        let mut pending = PendingGpuContext {
            stage,
            target,
            extent,
            label: "device error test".to_string(),
            required_features: Features::empty(),
            optional_features: Features::empty(),
            pacing: Pacing::default(),
        };
        let mut gpu = loop {
            if let Some(context) = pending.poll().expect("the null backend cannot fail here") {
                break context;
            }
        };

        // The control: a device with nothing to report does not fail a frame.
        assert!(
            gpu.acquire().expect("a healthy device acquires").is_some(),
            "an unprovoked drain must not break every frame"
        );

        recorder.report_device_error("shader module 3 failed to compile");
        let error = gpu
            .acquire()
            .expect_err("the device reported an error before this frame");
        assert!(
            error.to_string().contains("shader module 3"),
            "the reason must survive to the caller, not just the fact: {error}"
        );

        // Reported once. A latched error would turn one bad pipeline into a
        // frame loop that can never run again.
        assert!(
            gpu.acquire()
                .expect("the error was already reported")
                .is_some(),
            "taking an error must clear it"
        );

        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
    }
}
