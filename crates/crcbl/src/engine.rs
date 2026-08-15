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
//! back to `wait_idle` only on a device that has no timeline semaphores.
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
    AcquiredFrame, CommandBufferHandle, DeviceDesc, DisplayTiming, Features, Format, HalError,
    PresentInfo, PresentMode, QueueHandle, QueueKind, SemaphoreDesc, SemaphoreHandle,
    SemaphoreKind, SemaphoreSignal, SemaphoreWait, SubmitInfo, SurfaceError, SurfaceHandle,
    SwapchainDesc, SwapchainHandle,
};
use crcbl_hal::{Device, Instance};
use crcbl_shell::{CloseReply, DisplayMode, PhysicalSize, Shell, ShellError, ShellEvent, WindowId};

use crate::backend::GpuBackend;

pub mod pause;

pub use pause::PauseControl;

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

/// How long a frame will wait for an older present to reach the display.
///
/// A bound rather than a pacing choice: the wait it guards normally returns
/// within a display period, and this only fires when something underneath has
/// stopped answering — a compositor that stopped drawing, a monitor being
/// re-plugged. Waiting forever there would hang the loop with no way out,
/// including its input handling and its close button, so the loop renders the
/// frame instead and asks again next time. Far longer than any panel's period
/// and far shorter than a user's patience.
pub const PRESENT_WAIT_TIMEOUT: Duration = Duration::from_millis(100);

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
/// Which mode is actually running is a separate question, and it is one the
/// engine answers **once**: [`GpuContext`] asks [`Device::display_timing`]
/// after its first present, settles [`Auto`](Self::Auto) against the answer,
/// and never asks again. See [`GpuContext::effective_pacing`] for what that
/// resolution can and cannot see.
///
/// A caller that names [`Vsync`](Self::Vsync), [`Adaptive`](Self::Adaptive) or
/// [`Off`](Self::Off) is never overridden by that observation — it refines
/// [`Auto`](Self::Auto) and nothing else — and any of the four can be switched
/// mid-run with [`GpuContext::set_pacing`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pacing {
    /// Follow the display: adaptive sync where the display is running it,
    /// vsync where it is not. The default.
    ///
    /// **This is not a synonym for [`Vsync`](Self::Vsync), even though the
    /// swapchain opens on the same present mode.**
    /// [`preferences`](Self::preferences) is the vsync list here because the
    /// present mode is chosen when the swapchain is created, which is before
    /// any present exists and therefore before the display can be asked
    /// anything — `VK_EXT_present_timing` is specified to report nothing until
    /// an image has been presented. The difference is what happens *after* that
    /// first present: the display is read once, and an answer of
    /// [`DisplayTiming::Variable`] or [`DisplayTiming::Stepped`] rebuilds the
    /// swapchain on [`Adaptive`](Self::Adaptive)'s present mode. Asking for
    /// [`Vsync`](Self::Vsync) outright is never rebuilt.
    #[default]
    Auto,
    /// Wait for the display. [`PresentMode::Fifo`], which is the only mode
    /// guaranteed to exist — and the only one WebGPU has.
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
    /// The value a command line spells `name`, or `None` for a word this is not
    /// one of.
    ///
    /// Trimmed and case-folded like
    /// [`GpuBackend::from_name`](crate::backend::GpuBackend::from_name), so a
    /// wrapper script that quoted a value with a stray space still works.
    ///
    /// **`vrr` is deliberately not a spelling of [`Adaptive`](Self::Adaptive).**
    /// The names here are the variants' own, and a caller who typed the hardware
    /// word is better served by a rejection that says which word this engine
    /// uses than by a synonym that quietly works in one of the two places the
    /// value is written down.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "vsync" => Some(Self::Vsync),
            "adaptive" => Some(Self::Adaptive),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    /// The present modes to try, best first.
    ///
    /// Every list ends in a mode the surface must support, so
    /// [`SurfaceCaps::choose_present_mode`](crcbl_hal::SurfaceCaps::choose_present_mode)
    /// cannot fall through to a mode that is not there.
    #[must_use]
    pub const fn preferences(self) -> &'static [PresentMode] {
        match self {
            // `Auto` shares the vsync list because that is genuinely what the
            // swapchain opens with: the mode is picked before the first
            // present, and the observation that could say otherwise does not
            // exist yet. The adaptive list is reached from here only through
            // `resolve`, on the rebuild after that present.
            Self::Auto | Self::Vsync => &[PresentMode::Fifo],
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

    /// What is actually in force, given what the display turned out to be
    /// doing. Never [`Auto`](Self::Auto).
    ///
    /// The whole policy, in one place, with no device in the signature — which
    /// is what makes every pair of it checkable on a machine that has no
    /// display, and that is the only way three of the four [`DisplayTiming`]
    /// arms are reachable at all (see [`GpuContext::effective_pacing`]).
    ///
    /// A concrete request comes back unchanged. Someone who asked for
    /// [`Vsync`](Self::Vsync) on a VRR panel meant it, and someone who asked
    /// for [`Off`](Self::Off) is measuring something the display's opinion must
    /// not disturb.
    ///
    /// [`Stepped`](DisplayTiming::Stepped) resolves the same way as
    /// [`Variable`](DisplayTiming::Variable), because the question the pacing
    /// asks is whether the cycle is *fixed*, and a quantised cycle is not one:
    /// a panel that moves between 120, 60 and 40 Hz breaks a fixed-vblank
    /// assumption in the same way a free-running one does, and
    /// [`PresentMode::FifoRelaxed`] — wait when the frame is on time, tear
    /// rather than stall when it is late — is the answer to both. It is the
    /// narrower case of the two, so if a stepped panel ever turns out to want
    /// something else, this is the arm to split, not the enum.
    /// [`Fixed`](DisplayTiming::Fixed) and [`Unknown`](DisplayTiming::Unknown)
    /// are both vsync: the first because the display really is on a fixed
    /// cycle, the second because a display that will not say is the fallback
    /// case rather than a guess.
    #[must_use]
    const fn resolve(self, observed: DisplayTiming) -> Self {
        match (self, observed) {
            (Self::Auto, DisplayTiming::Variable { .. } | DisplayTiming::Stepped { .. }) => {
                Self::Adaptive
            }
            (Self::Auto, DisplayTiming::Fixed { .. } | DisplayTiming::Unknown) => Self::Vsync,
            // Every concrete request, unchanged. Written as a binding rather
            // than a wildcard over `observed` alone so that a new `Pacing`
            // variant is a decision made here rather than one that silently
            // lands on "whatever the caller said".
            (Self::Vsync, _) => Self::Vsync,
            (Self::Adaptive, _) => Self::Adaptive,
            (Self::Off, _) => Self::Off,
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

/// The half of [`GpuContextDesc`] that comes from the command line rather than
/// from the game.
///
/// A game's own `desc` fills in its label and the features its passes need,
/// which are properties of the *game*; which backend to open and how to pace
/// are properties of the **run**, and they arrive together, from the same place,
/// through every bring-up path a sample has. Carried as one value so that the
/// next run-level knob is a field here rather than a fifth parameter threaded
/// through five `Gpu::open`s, five `request_open`s and five
/// [`PolledGpu::request`]s — which is what a second one would have cost.
///
/// [`Common::gpu`](crate::args::Common::gpu) is where a sample gets one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuOptions {
    /// Which backend to open, or `None` to let `crcbl::backend`'s table choose.
    pub backend: Option<GpuBackend>,
    /// How presented frames should be paced against the display.
    pub pacing: Pacing,
}

/// What [`GpuContext::open`] should ask the device for.
#[derive(Clone, Copy, Debug)]
pub struct GpuContextDesc<'a> {
    /// Debug label for the device and the swapchain.
    pub label: &'a str,
    /// Which backend to open, or `None` to let `crcbl::backend`'s own table
    /// choose.
    pub backend: Option<GpuBackend>,
    /// Features the device must have. A device without them is an error.
    pub required_features: Features,
    /// Features to enable if present. Absent ones are simply not enabled — ask
    /// the returned [`GpuContext::device`] what it got.
    pub optional_features: Features,
    /// How presented frames are paced against the display.
    ///
    /// The swapchain's present mode comes from this. A game that wants to
    /// change it after start-up calls [`GpuContext::set_pacing`], which rebuilds
    /// the swapchain: the mode is a swapchain property and cannot be edited in
    /// place.
    pub pacing: Pacing,
}

impl Default for GpuContextDesc<'_> {
    fn default() -> Self {
        Self {
            label: "crcbl",
            backend: None,
            // Nothing here needs a feature, and demanding `GPU_DRIVEN` would
            // refuse to run on the lesser devices `docs/plan/39-capabilities.md`
            // requires the engine to degrade onto. Ask for everything optionally
            // and branch on what came back. `TIMESTAMP_QUERY` is deliberately
            // not part of `GPU_DRIVEN` — topic 10's browsers may lack it — so
            // the per-pass timers have to be asked for by name.
            required_features: Features::empty(),
            // `PRESENT_FEEDBACK` is asked for here because `acquire` already
            // calls `wait_until_presented` every frame: a device that has the
            // capability and was never asked for it answers that call
            // immediately forever, and the closed loop is dead code nothing can
            // reach. `PRESENT_TIMING` is the same argument for the same reason:
            // `submit_and_present` calls `display_timing` every frame, and
            // unasked-for it answers `Unknown` forever, so the engine would
            // report every panel as unreadable and never negotiate the
            // extension chain that could have told it otherwise. Optional like
            // the rest — neither is in `GPU_DRIVEN`, and a device without them just
            // keeps the open-loop frame limiter and the `Unknown` cadence.
            // `MESH_SHADER` is named beside the bundle rather than inside it.
            // `docs/plan/03-gpu-driven-rendering.md` §3.5 makes it the primary
            // geometry path, so a capable device has to be *asked* — but it is a
            // selector axis of its own and `GPU_DRIVEN` is the data-layout one,
            // and a device that folded them together would refuse mesh-less
            // hardware over a flag it does not need. Optional, so a device
            // without it degrades to an indirect tail and draws the same frame:
            // `crcbl/tests/render_e2e.rs` compares the two paths per scene, and
            // `crcbl-vk`'s `every_geometry_path_draws_the_same_frame` compares
            // all three.
            optional_features: Features::GPU_DRIVEN
                | Features::MESH_SHADER
                | Features::TIMESTAMP_QUERY
                | Features::DEBUG_MARKERS
                | Features::PRESENT_FEEDBACK
                | Features::PRESENT_TIMING,
            pacing: Pacing::default(),
        }
    }
}

impl From<GpuOptions> for GpuContextDesc<'_> {
    /// The defaults, with the run's own two fields filled in.
    ///
    /// Written for `..GpuContextDesc::from(gpu)` at the end of a game's struct
    /// literal: the label and the feature set above it are the game's, and
    /// everything the command line had a say in comes from here.
    fn from(gpu: GpuOptions) -> Self {
        Self {
            backend: gpu.backend,
            pacing: gpu.pacing,
            ..Self::default()
        }
    }
}

/// The engine's GPU side, driven entirely through the `crcbl-hal` seam.
///
/// Nothing in this struct names a backend. [`GpuContext::open`] asks
/// `crcbl::backend::open_backend` for one **by value** and everything after
/// it is `dyn Instance` / `dyn Device` — which is what made P1.1's swap from
/// the null backend to `crcbl-vk` a change to one argument.
#[derive(Debug)]
pub struct GpuContext {
    instance: Box<dyn Instance>,
    device: Box<dyn Device>,
    queue: QueueHandle,
    surface: SurfaceHandle,
    /// The adapter the surface was matched against, kept so a pacing change can
    /// ask *this* surface which present modes it offers rather than choosing
    /// from a list cached at start-up.
    adapter: crcbl_hal::AdapterId,
    swapchain: SwapchainHandle,
    /// Everything `create_swapchain` was last called with, so a resize
    /// reconfigures with one field changed rather than a fresh guess.
    config: SwapchainConfig,
    /// `None` on a device without timeline semaphores; see the module docs.
    timeline: Option<SemaphoreHandle>,
    /// Submissions issued so far, and therefore the value the next one signals.
    ///
    /// Doubles as the present id: it is already monotonic and already one per
    /// frame, so the frame the display is being asked about and the frame the
    /// timeline is being asked about are the same number.
    submitted: u64,
    /// What the caller asked for, [`Pacing::Auto`] included. Kept apart from
    /// `effective_pacing` because "asked for `Auto` and the display turned out
    /// fixed" and "asked for `Vsync`" are different facts about a run, and
    /// because a later [`set_pacing`](Self::set_pacing) back to `Auto` resolves
    /// from here.
    pacing: Pacing,
    /// What is actually in force: `pacing` resolved against the one display
    /// observation, and therefore never [`Pacing::Auto`].
    ///
    /// This is the value the loop reads — the resolved [`PresentMode`] in
    /// `config` cannot answer whether waiting for the display is wanted at all,
    /// which [`Pacing::Off`] says it is not.
    effective_pacing: Pacing,
    in_flight: VecDeque<(u64, CommandBufferHandle)>,
    /// The extent the swapchain was last *configured* at, from
    /// [`AcquiredFrame::extent`]. Distinct from what the shell asked for.
    configured_extent: (u32, u32),
    /// The single cadence [`Device::display_timing`] reported, and the latch
    /// that stops it being asked twice.
    ///
    /// `None` is "not asked yet", which is a different thing from
    /// [`DisplayTiming::Unknown`] — "asked, and the display would not say".
    /// Collapsing them would make the query run every frame forever on every
    /// driver that answers `Unknown`, which today is every driver this repo can
    /// reach.
    ///
    /// Kept after the resolution rather than dropped, so that a later
    /// [`set_pacing`](Self::set_pacing) to [`Pacing::Auto`] can settle against
    /// the sample already taken instead of taking a second one.
    observed_timing: Option<DisplayTiming>,
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
        adapter: crcbl_hal::AdapterId,
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
                    adapter,
                    config,
                    mut pending,
                } => match pending.poll()? {
                    crcbl_hal::DeviceRequestState::Pending => {
                        self.stage = OpenStage::Device {
                            instance,
                            surface,
                            adapter,
                            config,
                            pending,
                        };
                        return Ok(None);
                    }
                    crcbl_hal::DeviceRequestState::Ready(device) => {
                        // Topic 39's one downgrade line, said here rather than
                        // at adapter selection because this is the first point
                        // that knows what the device actually *granted* — an
                        // adapter's report is only what it could have given.
                        // Nothing is logged when it granted the lot, and that
                        // silence is the useful half: it tells a reader that
                        // `IndirectPerBatch` was the device's ceiling rather
                        // than something the descriptor never asked for.
                        let absent = crcbl_hal::downgrades(self.optional_features, &device.caps());
                        if !absent.is_empty() {
                            log::info!("hal: this device does not have {absent}");
                        }
                        return GpuContext::finish(
                            instance,
                            surface,
                            adapter,
                            config,
                            device,
                            self.extent,
                            self.pacing,
                        )
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
    /// why `crcbl::backend::open` and
    /// `Instance::create_device` do not
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
            "hal: {} adapter {:?} ({:?}), geometry {:?}, binding {:?}, lighting {:?}",
            instance.backend(),
            adapter.name,
            adapter.device_type,
            adapter.caps.geometry_path(),
            adapter.caps.binding_model(),
            adapter.caps.lighting_path()
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
            adapter: adapter.id,
            config,
            pending,
        })
    }

    /// Builds the context once the device has arrived.
    fn finish(
        instance: Box<dyn Instance>,
        surface: SurfaceHandle,
        adapter: crcbl_hal::AdapterId,
        config: SwapchainConfig,
        device: Box<dyn Device>,
        extent: (u32, u32),
        pacing: Pacing,
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

        // Which of the two pacing stories this run gets, said once at start-up
        // rather than branched on per frame: `acquire` calls the wait either
        // way, and a device without the capability answers it immediately.
        if device.caps().features.contains(Features::PRESENT_FEEDBACK) {
            log::info!("hal: pacing on presents, {FRAMES_IN_FLIGHT} frames deep");
        } else {
            log::debug!("hal: no present feedback; the frame limiter is the only pacing");
        }

        Ok(Self {
            instance,
            device,
            queue,
            surface,
            adapter,
            swapchain,
            config,
            timeline,
            submitted: 0,
            pacing,
            // Nothing has been presented, so nothing has been observed, and
            // `Unknown` is exactly what "the display has not said" resolves
            // through — which for `Auto` is the vsync the swapchain was just
            // created on. The first present may move it; see `settle_pacing`.
            effective_pacing: pacing.resolve(DisplayTiming::Unknown),
            in_flight: VecDeque::with_capacity(FRAMES_IN_FLIGHT + 1),
            configured_extent: extent,
            // Not asked yet, and the query is only meaningful after a present —
            // see `settle_pacing`.
            observed_timing: None,
            waits: Vec::with_capacity(1),
            signals: Vec::with_capacity(2),
        })
    }

    /// The device, for building renderers and recording work.
    #[must_use]
    pub fn device(&self) -> &dyn Device {
        self.device.as_ref()
    }

    /// The swapchain everything renders into, for a caller that wants the
    /// seam's present-feedback calls directly — `Device::wait_until_presented`
    /// is how a frame is held for the display.
    #[must_use]
    pub const fn swapchain(&self) -> SwapchainHandle {
        self.swapchain
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

    /// What the caller asked for, [`Pacing::Auto`] included.
    ///
    /// Pair with [`effective_pacing`](Self::effective_pacing): this one is the
    /// request, that one is the answer.
    #[must_use]
    pub const fn pacing(&self) -> Pacing {
        self.pacing
    }

    /// What is actually pacing the frames. Never [`Pacing::Auto`].
    ///
    /// Equal to [`pacing`](Self::pacing) unless [`Pacing::Auto`] was asked for,
    /// in which case it is [`Pacing::Vsync`] or [`Pacing::Adaptive`] depending
    /// on what the display reported.
    ///
    /// # The observation behind it happens once
    ///
    /// After the first present — never before it, because
    /// `VK_EXT_present_timing` is specified to report nothing until an image
    /// has been presented — the display is asked exactly once, and the answer
    /// settles this for the life of the context. It is not re-read on a resize,
    /// a display-mode change, or a window dragged to another monitor, and
    /// [`set_pacing`](Self::set_pacing) does not re-read it either.
    ///
    /// **Once, rather than until it answers**, because a driver that only ever
    /// reports [`DisplayTiming::Unknown`] would otherwise be re-queried every
    /// frame for the life of the process — and that is every driver this repo
    /// has been able to test against. The known cost: a platform that needs
    /// more than one present before it will answer reads `Unknown` here and
    /// stays on [`Pacing::Vsync`], which is the documented fallback. A caller
    /// on such a platform asks for [`Pacing::Adaptive`] by name.
    ///
    /// # What has actually been observed
    ///
    /// Only [`DisplayTiming::Unknown`] has ever come back from a real driver in
    /// this repo — see `docs/backlog.md`. The
    /// [`Variable`](DisplayTiming::Variable), [`Stepped`](DisplayTiming::Stepped)
    /// and [`Fixed`](DisplayTiming::Fixed) paths through the resolution are
    /// exercised by unit tests and by nothing else on any machine.
    #[must_use]
    pub const fn effective_pacing(&self) -> Pacing {
        self.effective_pacing
    }

    /// Changes how presented frames are paced, mid-run.
    ///
    /// The present mode is a swapchain property, so this rebuilds the swapchain
    /// — but **only when the mode it resolves to differs from the one
    /// presenting**. Setting the pacing already in force costs nothing, which
    /// is what a settings screen that re-applies every value on every apply
    /// needs.
    ///
    /// # `Auto` here does not re-detect
    ///
    /// Detection happens once per context, and this call is not it: `Auto`
    /// resolves against the sample the first present already took. Before that
    /// present there is no sample, so `Auto` set here is vsync until the first
    /// present takes one — the same start-up path a context opened on `Auto`
    /// follows, and the same single query.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the swapchain could not be rebuilt. The old swapchain is
    /// still configured when that happens — `reconfigure_swapchain` replaces
    /// nothing until the new one is built — so the context stays usable, and
    /// [`pacing`](Self::pacing), [`effective_pacing`](Self::effective_pacing)
    /// and the swapchain's own mode are rolled back together rather than left
    /// describing a change that did not take.
    pub fn set_pacing(&mut self, pacing: Pacing) -> Result<(), GpuError> {
        let previous = (self.pacing, self.effective_pacing);
        self.pacing = pacing;
        self.effective_pacing = pacing.resolve(self.observed_timing.unwrap_or_default());
        let result = self.apply_present_mode();
        if result.is_err() {
            (self.pacing, self.effective_pacing) = previous;
        }
        result
    }

    /// Which present the frame about to start should wait for, or `None` if
    /// there is nothing worth waiting on yet.
    ///
    /// `submitted` is the id of the most recent present, so the frame about to
    /// start will be `submitted + 1` and the one to wait for is
    /// **`FRAMES_IN_FLIGHT` behind it**. The two ends of that range are both
    /// wrong: waiting for `submitted + 1` is waiting for something that has not
    /// been submitted, and waiting for `submitted` — the frame just sent —
    /// empties the pipeline every frame, so the CPU sits out a whole display
    /// period doing nothing and the result is worse than not waiting at all.
    /// `pacing` is the **effective** one — [`Pacing::Auto`] has already been
    /// resolved by the time a frame is paced, and the `Off` test below would
    /// otherwise have to guess what `Auto` meant.
    ///
    /// One `FRAMES_IN_FLIGHT` back is the id whose display leaves exactly
    /// `FRAMES_IN_FLIGHT` presents outstanding, which is the depth
    /// [`retire_to`](Self::retire_to) already holds command buffers to; the
    /// wait therefore paces the loop without changing that depth.
    ///
    /// `None` in two cases. The first `FRAMES_IN_FLIGHT` frames have nothing
    /// that far behind them — ids start at 1, so the subtraction lands on 0 or
    /// underflows — and the loop is still filling anyway. And [`Pacing::Off`]
    /// asked *not* to be paced by the display: blocking until a frame is on
    /// screen would pin the loop to the refresh rate, which is the one thing
    /// that mode exists to avoid.
    fn present_to_wait_for(submitted: u64, pacing: Pacing) -> Option<u64> {
        if matches!(pacing, Pacing::Off) {
            return None;
        }
        submitted
            .saturating_add(1)
            .checked_sub(FRAMES_IN_FLIGHT as u64)
            .filter(|id| *id > 0)
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

        // The two blocking calls, and only those, under one span: this is where
        // a frame waits for the display rather than for the CPU, and
        // `crate::perf::frame_cpu_time` subtracts it. The reconfigure the match
        // below can run is deliberately outside — it is work, not waiting.
        let next = {
            let _waiting = crcbl_core::trace::span(crate::perf::PRESENT_WAIT_SPAN);

            // Closed-loop pacing, before an image is taken and before any work is
            // recorded: this is the point in a frame where the CPU can be held back
            // without holding anything else up. A device without present feedback
            // answers immediately, which is why there is no branch here on which
            // backend is underneath.
            if let Some(present_id) =
                Self::present_to_wait_for(self.submitted, self.effective_pacing)
            {
                match self.device.wait_until_presented(
                    self.swapchain,
                    present_id,
                    PRESENT_WAIT_TIMEOUT,
                ) {
                    Ok(()) => {}
                    // The display did not get to it in time. Render anyway: a
                    // frame skipped because the *last* one was slow is two frames
                    // lost instead of one, and the next wait catches up.
                    Err(SurfaceError::Timeout) => {
                        log::debug!(
                            "hal: present {present_id} was still not up after a whole timeout"
                        );
                    }
                    // Not reported here: the acquire below reports it too, through
                    // the arm that already reconfigures and skips the frame.
                    Err(SurfaceError::OutOfDate) => {}
                    Err(error) => return Err(error.into()),
                }
            }

            self.device.acquire_next_frame(self.swapchain)
        };

        let acquired = match next {
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
                // The submission counter, not a second one: it is monotonic
                // already, and a frame's present and its timeline value being
                // the same number is what lets `acquire` ask about a frame by
                // the only id it has.
                present_id: Some(value),
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

        // After the present, never before it — and after *this* present rather
        // than at the top of the next frame, because the whole question is what
        // the display did with a frame it has been given.
        self.settle_pacing()?;

        if acquired.suboptimal {
            log::debug!("hal: swapchain suboptimal; reconfiguring after present");
            self.reconfigure()?;
        }
        Ok(FrameOutcome::Presented)
    }

    /// Reads what the display is doing with the frame just presented, settles
    /// [`Pacing::Auto`] against it, and never asks again.
    ///
    /// A no-op after the first call, which is the point: see
    /// [`effective_pacing`](Self::effective_pacing) for why the query is
    /// one-shot and what that gives up.
    ///
    /// # Why here, and why after the present
    ///
    /// `VK_EXT_present_timing`'s proposal is explicit that
    /// `vkGetSwapchainTimingPropertiesEXT` may answer `VK_NOT_READY` until at
    /// least one image has been presented to the swapchain. So a query at
    /// acquire time — or anywhere before the first present — is expected to
    /// report [`Unknown`](DisplayTiming::Unknown), and that is the platform
    /// working as specified rather than a fault to chase. The present mode, on
    /// the other hand, is chosen when the swapchain is created, which is before
    /// any present exists: the answer is not available at the moment it is
    /// needed, and opening on vsync and rebuilding once is the only order that
    /// can use it at all.
    ///
    /// # Why an error is not a failed frame
    ///
    /// A failed *query* degrades to [`Unknown`](DisplayTiming::Unknown) and a
    /// `debug!` line, and the pacing falls back to vsync exactly as it would
    /// for a display that stayed quiet. The frame has already been presented by
    /// the time this runs, so there is nothing left to abandon, and turning a
    /// diagnostic into a `GpuError` would let it kill a loop that is working.
    /// A failed *rebuild* is another matter and is reported: the caller asked
    /// for a pacing the engine then could not deliver, and
    /// [`apply_present_mode`](Self::apply_present_mode) leaves the old
    /// swapchain in place for it.
    ///
    /// # The log line
    ///
    /// One line, once, saying all three of what was asked, what the display
    /// reported and what is in force — "asked for `Auto`, display said
    /// `Variable`, running adaptive" and "asked for `Adaptive`" are different
    /// runs and a line naming only the result cannot tell them apart. It leads
    /// with `hal: display timing `, which is what
    /// `crates/crcbl-shell/tests/run-wayland-e2e.sh` greps for to prove the
    /// engine really asked the driver; keep that prefix first if the line is
    /// ever reworded.
    fn settle_pacing(&mut self) -> Result<(), GpuError> {
        if self.observed_timing.is_some() {
            return Ok(());
        }
        let observed = match self.device.display_timing(self.swapchain) {
            Ok(timing) => timing,
            Err(error) => {
                log::debug!("hal: could not read the display timing: {error}");
                DisplayTiming::Unknown
            }
        };
        self.observed_timing = Some(observed);
        let previous = self.effective_pacing;
        self.effective_pacing = self.pacing.resolve(observed);
        log::info!(
            "hal: display timing {observed:?}; asked for {:?}, pacing {:?}",
            self.pacing,
            self.effective_pacing
        );
        let result = self.apply_present_mode();
        if result.is_err() {
            // The same rollback [`set_pacing`](Self::set_pacing) does, and for
            // the same reason: the swapchain is still the one that was already
            // configured, so leaving `effective_pacing` on the mode that did
            // not take would make the accessor describe a swapchain that does
            // not exist. `observed_timing` is deliberately *not* rolled back —
            // the display was asked and it answered, and the query is one-shot
            // whether or not acting on the answer worked.
            self.effective_pacing = previous;
        }
        result
    }

    /// Puts [`effective_pacing`](Self::effective_pacing)'s present mode on the
    /// swapchain, rebuilding only if the mode actually changes.
    ///
    /// The mode comes from the surface's own list rather than from the pacing
    /// alone, because a preference the surface does not offer falls back — a
    /// surface with no `FifoRelaxed` and no `Mailbox` runs
    /// [`Pacing::Adaptive`] on `Fifo`, and rebuilding a swapchain to the mode
    /// it already has is the no-op this comparison exists to prevent.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the surface could not be queried or the swapchain could
    /// not be rebuilt. On a failed rebuild the old swapchain is still the
    /// configured one, so `config` is rolled back to describe it rather than
    /// the mode that did not take.
    fn apply_present_mode(&mut self) -> Result<(), GpuError> {
        let caps = self.instance.surface_caps(self.surface, self.adapter)?;
        let mode = caps.choose_present_mode(self.effective_pacing.preferences());
        if mode == self.config.present_mode {
            return Ok(());
        }
        log::info!(
            "hal: pacing {:?} wants {mode:?}, not {:?}; rebuilding the swapchain",
            self.effective_pacing,
            self.config.present_mode
        );
        let previous = self.config.present_mode;
        self.config.present_mode = mode;
        let result = self.reconfigure();
        if result.is_err() {
            self.config.present_mode = previous;
        }
        result
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
        // `observed_timing` is deliberately *not* cleared. A rebuilt swapchain
        // has had nothing presented to it and would answer `Unknown` again, so
        // clearing the latch would re-run the whole resolution on every resize
        // — and on a driver that never says anything but `Unknown`, that is the
        // per-frame query this design exists to avoid. Detection happens once
        // per context; a window dragged to a VRR monitor keeps the pacing it
        // started with until the game asks for another with `set_pacing`.
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
///
/// # It stores the rate, and derives the period
///
/// The other way round loses the only number anybody typed: a period recovers
/// its rate by a division that is exact for the rates a display has and off by
/// one for the rates it does not, so a run reporting what it was asked for would
/// be reporting an approximation of it. The period is a division away and is
/// wanted once a frame; the rate is wanted whenever a human reads a log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLimit {
    /// Frames a second, or zero for no limit — see [`fps`](Self::fps).
    fps: u32,
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
    pub const fn fps(fps: u32) -> Self {
        Self { fps }
    }

    /// No limit: run as fast as the loop can.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self { fps: 0 }
    }

    /// The rate this was asked for, or zero when there is no limit.
    #[must_use]
    pub const fn rate(self) -> u32 {
        self.fps
    }

    /// The least time one frame may take, if there is a limit.
    ///
    /// Truncated to whole nanoseconds, which is the resolution a [`Duration`]
    /// has: at 60 the period is 16.666666 ms rather than a recurring decimal, so
    /// the limiter's ceiling is a hair *above* the rate rather than below it.
    #[must_use]
    pub fn period(self) -> Option<Duration> {
        (self.fps != 0).then(|| Duration::from_secs(1) / self.fps)
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
        let period = self.period()?;
        let deadline = last_start?.checked_add(period)?;
        deadline.checked_sub(now).filter(|wait| !wait.is_zero())
    }
}

impl Default for FrameLimit {
    fn default() -> Self {
        Self::fps(Self::DEFAULT_FPS)
    }
}

impl std::fmt::Display for FrameLimit {
    /// How a log says what is capping a loop: `1000 fps`, or `unlimited`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.fps {
            0 => f.write_str("unlimited"),
            fps => write!(f, "{fps} fps"),
        }
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
    ///
    /// # The log line
    ///
    /// One line per call, here rather than at the call sites, because there are
    /// three kinds of caller — [`Loop::new`] applying
    /// [`LoopConfig::limit`], a hand-written loop like `apps/bare`, and a game
    /// changing it mid-run from a settings screen — and a run that does not say
    /// what is capping it leaves "the frame rate is wrong" with nothing to read.
    /// It leads with `engine: the frame limit is `, which
    /// `crates/crcbl-shell/tests/run-wayland-e2e.sh` greps for; keep that prefix
    /// first if the line is ever reworded.
    ///
    /// [`RealClock::new`] does *not* log: the default is not news, and a run
    /// that never touches this reports the default through [`Loop::new`]
    /// anyway.
    pub fn set_limit(&mut self, limit: FrameLimit) {
        self.limit = limit;
        log::info!("engine: the frame limit is {limit}");
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
    /// Nothing is logged in that case either, for the same reason: a headless
    /// run has no frame limit to report.
    pub fn set_limit(&mut self, limit: FrameLimit) {
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
///
/// # Not `Copy`, because of [`touches`](Self::touches)
///
/// The pointer collapses to a position and two flags — a batch with five moves
/// in it has moved the pointer once. Contacts do not: two fingers are two
/// independent gestures, and a batch that lands one finger and lifts another has
/// two facts in it that no fixed set of fields can hold. So the batch keeps the
/// list, and the type is `Clone` rather than `Copy`.
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// Every contact this batch carried, in the order the shell reported them.
    ///
    /// **Not collapsed per contact**, which is the difference between this and
    /// the pointer fields above: a finger that lands and lifts inside one pump
    /// is a tap, and a batch that kept only each contact's latest state would
    /// report a finger that was never down. On a phone that is every tap, since
    /// the press and the release of a real one arrive in the same frame.
    ///
    /// Positions are framebuffer pixels, like [`pointer`](Self::pointer). The
    /// game is handed them normalised — see [`TouchUpdate`].
    ///
    /// Empty on every backend but the web one today: contacts arrive only where
    /// [`ShellCaps::TOUCH`](crcbl_shell::ShellCaps::TOUCH) is set.
    pub touches: Vec<TouchContact>,
    /// [`DEBUG_OVERLAY_KEY`] was pressed, and it was a real press.
    pub toggle_debug_overlay: bool,
    /// [`PAUSE_KEY`] was pressed.
    pub toggle_pause: bool,
    /// [`FULLSCREEN_KEY`] was pressed.
    pub toggle_fullscreen: bool,
}

/// One contact event, as a batch saw it.
///
/// The shell's [`ShellEvent::Touch`] with the
/// window and the device dropped and the position in framebuffer pixels — the
/// same reduction [`Pending::pointer`] is. [`TouchUpdate`] is the same fact once
/// the loop has normalised it for the game; the two are separate types so that a
/// position cannot cross between the spaces without being converted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchContact {
    /// Which finger. Unique among the contacts that are down together, and
    /// reused after one ends — see
    /// [`ContactId`](crcbl_core::input::ContactId).
    pub contact: crcbl_core::input::ContactId,
    /// What it just did.
    pub phase: crcbl_core::input::TouchPhase,
    /// Where it is, in framebuffer pixels.
    pub at: glam::Vec2,
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
    ///
    /// Nothing is carried for touch: a contact that did not move this batch has
    /// no event in it, and a finger's position between events is the game's to
    /// remember — it knows which of its own controls that finger grabbed, and
    /// the loop does not.
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
            // Appended, never merged: see `touches`. A contact's own id is what
            // tells two fingers apart, so nothing here has to guess which
            // finger a position belongs to — which is the whole point of the
            // seam carrying the id.
            ShellEvent::Touch {
                contact,
                phase,
                position,
                ..
            } => self.touches.push(TouchContact {
                contact: *contact,
                phase: *phase,
                at: glam::Vec2::new(position.x as f32, position.y as f32),
            }),
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
    /// The last mode [`check`](Self::check) saw the window actually in.
    ///
    /// Kept for one reason: [`mode_at_exit`](Self::mode_at_exit), because by
    /// the time a run that ended in a close request builds its summary the
    /// window is gone and there is nothing left to read.
    seen: DisplayMode,
}

impl ModeRequest {
    /// A loop that has asked for nothing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            honoured: false,
            seen: DisplayMode::Windowed,
        }
    }

    /// The mode the window system actually has the window in, or `None` when
    /// there is no window to read.
    ///
    /// **Not the one that was last asked for.** A summary that reported the
    /// request would say "borderless" for every compositor that refused, which
    /// is the difference this whole type exists to keep visible.
    ///
    /// `None` is the honest answer for a window that is gone: a caller with
    /// nothing to read must not be handed an invented `Windowed`, because it
    /// reads exactly like the truth. [`mode_at_exit`](Self::mode_at_exit) is the
    /// caller that wants a *mode* for a run that ended, and it keeps the last
    /// mode the window was seen in rather than inventing one.
    pub fn mode<S: Shell + ?Sized>(shell: &S, window: WindowId) -> Option<DisplayMode> {
        shell
            .window_state(window)
            .ok()
            .map(|state| state.effective_mode().unwrap_or(state.requested_mode))
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
        let target = if Self::mode(shell, window)
            .expect("toggle is only ever called on a window the loop owns")
            .is_borderless()
        {
            DisplayMode::Windowed
        } else {
            DisplayMode::Borderless { monitor: None }
        };
        shell.set_mode(window, target)?;
        log::info!("shell: asked for {target}");
        Ok(())
    }

    /// The mode to report for a run that has **ended**.
    ///
    /// [`mode`](Self::mode) reads the window and answers `None` when there is no
    /// window to read — which is every run a player ended by closing the window,
    /// because accepting a close destroys it before teardown gets to ask. A
    /// summary that unwrapped that `None` into a `Windowed` would say "windowed"
    /// for a session that spent all of itself fullscreen, in the same words a
    /// genuinely windowed run uses, so nothing downstream could tell the two
    /// apart.
    ///
    /// So this prefers the live answer and falls back to the last one
    /// [`check`](Self::check) saw. A loop that never called `check` reports
    /// `Windowed`, which is what it started in and all it ever knew.
    #[must_use]
    pub fn mode_at_exit<S: Shell + ?Sized>(&self, shell: &S, window: WindowId) -> DisplayMode {
        shell.window_state(window).map_or(self.seen, |state| {
            state.effective_mode().unwrap_or(state.requested_mode)
        })
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
        // Recorded on every call, not only on a transition: the mode can change
        // while `honoured` stays true — borderless back to windowed is two
        // honoured states — and the early return below would skip it.
        self.seen = state.effective_mode().unwrap_or(state.requested_mode);
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
                Self::mode(shell, window).expect("check just read this window's state"),
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

/// One frame's worth of pointer, as the *game* sees it.
///
/// The pointer's half of [`HostedGame::key_event`], and it is not a `ShellEvent`
/// for the same reason that one is not: what reaches the game is what the menu
/// and the loop did not claim, in the units a game can bind to.
///
/// **The position is normalised to the surface**, −1 at one edge and +1 at the
/// other, +X right and **+Y up**. Framebuffer pixels are the loop's business: a
/// game handed them would redo the DPI arithmetic the windowing layer already
/// did, once per sample, and get it wrong on the displays nobody develops on. A
/// game still owns the step from the surface to its own world — a camera's half
/// width is not the loop's to know.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerUpdate {
    /// Where the pointer moved to this frame, or `None` on a frame it did not
    /// move.
    ///
    /// **A pointer that leaves the window reports `None` too**, rather than a
    /// last position or a recentring: leaving is not a command, and a game that
    /// was told "the pointer is now nowhere" has no better answer than the one
    /// it already had. [`crcbl_input::Binding::PointerPosition`] holds the last
    /// position for exactly this reason.
    pub at: Option<glam::Vec2>,
    /// The primary button went down this frame, over no menu.
    pub pressed: bool,
    /// …and came up.
    pub released: bool,
}

/// One contact, as the *game* sees it.
///
/// [`PointerUpdate`]'s counterpart for a finger, in the same normalised surface
/// coordinates and for the same reason: −1 at one edge and +1 at the other, +X
/// right and +Y up.
///
/// # What a consumer may rely on
///
/// * A [`Began`](crcbl_core::input::TouchPhase::Began) arrives before anything
///   else for a [`contact`](Self::contact), and an
///   [`Ended`](crcbl_core::input::TouchPhase::Ended) or
///   [`Cancelled`](crcbl_core::input::TouchPhase::Cancelled) is the last. After
///   that the id may name a different finger, so state keyed on it must go.
/// * Two contacts that are down at the same time never share an id.
/// * **A held contact is cancelled if the window loses focus**, exactly as held
///   keys are released and a held pointer button comes up. No platform sends
///   that one, and without it a finger on a stick keeps holding it after the
///   player has alt-tabbed away.
///
/// And the one it must not: there is **no** guarantee that a contact still down
/// produces an event on every frame. A finger resting on the glass moves
/// nothing, so a game that reads a stick's value only on the frames a contact
/// reports must hold the value between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchUpdate {
    /// Which finger.
    pub contact: crcbl_core::input::ContactId,
    /// What it just did.
    ///
    /// [`Cancelled`](crcbl_core::input::TouchPhase::Cancelled) is not a quiet
    /// [`Ended`](crcbl_core::input::TouchPhase::Ended): the system took the
    /// gesture away rather than the player finishing it, so a tap must not fire
    /// and a drag must go back where it started.
    pub phase: crcbl_core::input::TouchPhase,
    /// Where it is, normalised to the surface.
    ///
    /// Present on every phase, including the two that end a contact — the
    /// platform reports where the finger was when it went, and a control that
    /// needs to know where a drag finished cannot ask afterwards.
    pub at: glam::Vec2,
}

impl TouchUpdate {
    /// Where this contact is in **framebuffer pixels**, Y down from the
    /// top-left.
    ///
    /// The one thing a normalised position cannot do is hit-test a widget: an
    /// on-screen control is laid out in the same pixels the [`DrawList`] it
    /// draws into uses, and a circle in normalised coordinates is an ellipse on
    /// a surface that is not square. So the conversion back lives here, beside
    /// the one that got us here, rather than in each game that grows a control —
    /// two halves of one convention, and
    /// `a_contact_survives_the_round_trip_through_the_surface` holds them
    /// together.
    ///
    /// [`DrawList`]: crcbl_ui::draw_list::DrawList
    #[must_use]
    pub fn pixels(&self, extent: (u32, u32)) -> glam::Vec2 {
        surface_pixels(self.at, extent)
    }
}

/// The −1…1 a game binds against back to framebuffer pixels, Y down from the
/// top-left — [`normalised`] undone.
///
/// Shared by [`TouchUpdate::pixels`] and by [`pause`], which hit-tests the
/// *pointer* against a rectangle laid out in the same pixels a
/// [`DrawList`](crcbl_ui::draw_list::DrawList) uses.
fn surface_pixels(at: glam::Vec2, extent: (u32, u32)) -> glam::Vec2 {
    let width = extent.0.max(1) as f32;
    let height = extent.1.max(1) as f32;
    glam::Vec2::new(
        (at.x + 1.0) * 0.5 * width,
        // The Y flip `normalised` applied, undone.
        (1.0 - at.y) * 0.5 * height,
    )
}

/// Framebuffer pixels to the −1…1 the game binds against, +Y up.
fn normalised(point: glam::Vec2, extent: (u32, u32)) -> glam::Vec2 {
    let width = extent.0.max(1) as f32;
    let height = extent.1.max(1) as f32;
    glam::Vec2::new(
        point.x / width * 2.0 - 1.0,
        // Window pixels count down from the top and every game's world counts
        // up, so the flip belongs here rather than in each game's sign that
        // nobody can justify from the call site.
        1.0 - point.y / height * 2.0,
    )
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
    /// `None` for anything that was not a key, or that the menu claimed.
    ///
    /// # A release the menu claims still reaches the game when the press did
    ///
    /// The held list is the keys the *game* has been told are down, so a menu
    /// key pressed before the menu opened is on it — and its release is
    /// forwarded even though the menu is claiming that key. Swallowing it
    /// leaves the game holding the key for good: level up with Down held, pick
    /// an upgrade, and the wizard keeps walking south with nothing pressed.
    ///
    /// The other direction has no such repair. A press the menu claimed never
    /// reaches the game, so it does not go on the list either, and its release
    /// is dropped like the press was — the game only ever sees matched pairs.
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

        let claimed =
            self.showing && matches!(code, MENU_UP_KEY | MENU_DOWN_KEY | MENU_ACTIVATE_KEY);
        if claimed {
            match (code, pressed) {
                // Repeats move the selection, because holding Down to walk a
                // list is what a player expects.
                (MENU_UP_KEY, true) => self.menus.select_previous(),
                (MENU_DOWN_KEY, true) => self.menus.select_next(),
                (MENU_ACTIVATE_KEY, true) => self.menus.press(true),
                (MENU_ACTIVATE_KEY, false) => self.activated = self.menus.activate(),
                _ => {}
            }
        }

        if pressed {
            if claimed {
                return None;
            }
            if !self.held.contains(&code) {
                self.held.push(code);
            }
            return Some((code, true));
        }

        let was_held = self.held.contains(&code);
        self.held.retain(|key| *key != code);
        (!claimed || was_held).then_some((code, false))
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

/// The two questions anything holding a swapchain has to answer.
///
/// Split out because both halves of the engine ask them and neither is about
/// the other: [`PolledBoot`] resizes a swapchain that arrived at a stale extent,
/// and [`Loop`] resizes one the compositor just moved. Declaring them twice
/// would be two copies of the same contract on one type, which is where the two
/// drift apart.
pub trait GpuSurface {
    /// The extent the swapchain was actually created at.
    fn extent(&self) -> (u32, u32);

    /// Rebuilds the swapchain at a new size.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the swapchain could not be recreated.
    fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError>;
}

/// A game's GPU bundle, opened without blocking on the device.
///
/// [`PolledBoot`] drives start-up for a caller that may not block —
/// a browser main thread resolves both of the things a blocking bring-up waits
/// for from inside the very event loop the wait would be sitting in — and this
/// plus [`GpuSurface`] is what it needs from whatever a game calls its `Gpu`.
///
/// Deliberately **not** implemented for [`GpuContext`]. A sample's `Gpu` is its
/// renderers and its atlas as well as the context, and the thing that has to
/// arrive before a loop can be assembled is the whole bundle.
pub trait PolledGpu: GpuSurface + Sized {
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
        gpu: GpuOptions,
    ) -> Result<Self::Pending, GpuError>;

    /// `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the device request failed.
    fn poll_pending(pending: &mut Self::Pending) -> Result<Option<Self>, GpuError>;
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
    gpu: GpuOptions,
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
    pub fn request(shell: Box<S>, window: WindowId, clock_source: Clock, gpu: GpuOptions) -> Self {
        Self {
            shell: Some(shell),
            window,
            clock_source: Some(clock_source),
            gpu,
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
                    pending: G::request(shell.as_ref(), self.window, extent, self.gpu)?,
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

// ---------------------------------------------------------------------------
// The engine-owned loop
// ---------------------------------------------------------------------------

/// The rest of what a hosted game's GPU bundle does, once per frame.
///
/// [`GpuSurface`] is the half [`PolledBoot`] shares; this is the half only a
/// running [`Loop`] needs. Every sample's `Gpu` already had all six of these as
/// inherent methods with these exact signatures — the trait is what lets the
/// loop above call them.
pub trait GameGpu: GpuSurface + Sized {
    /// The glyph atlas the UI pass renders text from.
    ///
    /// The menu lays itself out with it and the debug overlay measures its own
    /// panel with it, and both must use the *same* atlas the pass draws with or
    /// the background rect is the wrong size for the text inside it.
    fn atlas(&self) -> &crcbl_ui::FontAtlas;

    /// Takes this frame's menu, or `None` on a frame that shows none.
    fn set_menu(&mut self, menu: Option<(&crcbl_ui::menu::Menu, &crcbl_ui::menu::MenuLayout)>);

    /// Takes this frame's UI geometry, handing the previous frame's allocation
    /// back so the loop can refill it instead of building a new one.
    fn take_draw_list(&mut self, list: &mut crcbl_ui::draw_list::DrawList);

    /// The most recent pass timings, or `None` on a device without timestamp
    /// queries.
    fn timings(&self) -> Option<&crcbl_render::FrameTimings>;

    /// What the last [`frame`](Self::frame) recorded: draws, instances and
    /// triangles, summed over this bundle's renderers.
    ///
    /// **No default**, deliberately. A default returning
    /// [`FrameCounters::default`](crcbl_render::FrameCounters::default) would
    /// put `draws: 0` on the panel for every bundle that forgot to implement it
    /// — "not counted" arriving as "nothing was drawn", which is the one failure
    /// `docs/plan/40-profiling.md` names for counters. Every renderer in
    /// `crcbl-render` answers this, so an implementation is
    /// [`plus`](crcbl_render::FrameCounters::plus)ing the ones this bundle
    /// holds.
    ///
    /// The loop reads it *after* the frame, and shows it on the *next* one — the
    /// panel is gathered before [`frame`](Self::frame) runs. So every row of that
    /// section is one frame behind, uniformly; see
    /// [`crcbl_render::counters`].
    fn counters(&self) -> crcbl_render::FrameCounters;

    /// Records, submits and presents one frame.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the frame could not be recorded or presented.
    fn frame(&mut self) -> Result<FrameOutcome, GpuError>;

    /// Releases everything, in dependency order.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if waiting for outstanding work failed.
    fn destroy(self) -> Result<(), GpuError>;
}

/// What one frame tells the game about the frame around it.
///
/// Passed rather than queried because all four are the loop's own bookkeeping:
/// a game that read them back would be reading its host's fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameInfo {
    /// Whether the simulation is stopped — see [`Loop::is_paused`].
    pub paused: bool,
    /// How many ticks **this** frame ran, which is zero while paused and can be
    /// more than one after a long frame. An animation stepped on the simulation
    /// clock advances by this, not by one.
    pub ticks: u64,
    /// How far the render sits between the last tick and the next, in `0..1`.
    pub alpha: f32,
    /// One tick's duration, in seconds.
    ///
    /// `f64` because that is what [`crcbl_core::FrameClock`] reports and
    /// narrowing it here would be the engine deciding a precision on the game's
    /// behalf.
    pub tick_dt: f64,
}

/// The shared half of what a run reports.
///
/// Every sample's `Summary` carried these eight fields with the same meaning,
/// and then its own two or three. [`HostedGame::summary`] receives this and
/// returns the whole thing, so the game keeps a summary type of its own rather
/// than the engine growing a `score: Option<u32>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSummary {
    /// Which shell backend the run actually opened.
    pub backend: crcbl_shell::ShellBackend,
    /// Presented frames.
    pub frames: u64,
    /// Simulation ticks.
    pub ticks: u64,
    /// Shell events observed, start-up included.
    pub events: u64,
    /// The swapchain's extent when the run ended.
    pub extent: (u32, u32),
    /// Why the loop stopped.
    pub exit: ExitReason,
    /// Whether the simulation was stopped when the run ended.
    pub paused: bool,
    /// The mode the window system actually had the window in, **not** the one
    /// the run last asked for. A summary that reported the request would say
    /// "borderless" for every compositor that refused.
    pub mode: DisplayMode,
}

/// The game a [`Loop`] hosts.
///
/// The engine owns the frame; this is everything in one that was ever a
/// particular game's. It is deliberately small — a tick, a key, a menu, a draw
/// — because everything a sample's `frame()` did *around* those turned out to
/// be the same in all five.
///
/// # Not [`crcbl_ecs::GameModule`]
///
/// That one is the **simulation** the server hosts: systems on a `World`,
/// stepped on the tick, and the thing a wasm binding will one day have to
/// reproduce bit for bit. This one is the **presentation** the loop hosts:
/// which menu is on screen, what a button does, what goes in the draw list. A
/// game implements both, and they are hosted by different things for different
/// reasons.
///
/// # A hosted game never owns engine resources
///
/// Every method that needs the GPU is handed `&mut Self::Gpu` for the call.
/// There is no field here holding a device, a window or a shell, so "who tears
/// this down" has one answer: the loop, in [`Loop::finish`].
pub trait HostedGame: Sized {
    /// This game's own failures. The loop's are [`LoopError`]'s other variants.
    type Error: core::fmt::Display;
    /// This game's GPU bundle.
    type Gpu: GameGpu;
    /// The key naming which menu a frame shows.
    type MenuKind: Copy + Eq;
    /// The `G` in [`MenuAction`] — what only this game's menus do.
    type MenuAction;
    /// What a finished run reports, built from [`RunSummary`].
    type Summary;

    /// This game's name, for the engine's own log lines.
    const NAME: &'static str;

    /// This game's menus, with nothing shown.
    ///
    /// Their widget ids must start at [`FIRST_GAME_ID`] for anything the loop
    /// does not own; [`MenuAction::id`] is what enforces that.
    fn menus() -> crcbl_ui::menu::MenuSet<Self::MenuKind>;

    /// One fixed-timestep step. Called zero or more times per frame, never
    /// while paused.
    fn tick(&mut self, gpu: &mut Self::Gpu, tick_dt: f64);

    /// A key the menu did not claim.
    fn key_event(&mut self, key: crcbl_core::input::KeyCode, pressed: bool);

    /// The pointer, once the menu has had its turn — see [`PointerUpdate`].
    ///
    /// Called only on a frame where the pointer did something — and once more
    /// on a frame that lost focus while the button was down, which is a release
    /// no platform sends and the loop owes. A game that binds no pointer input
    /// never overrides this, exactly as it never overrides
    /// [`debug_sections`](Self::debug_sections): the empty body is what "this
    /// game is played with the keyboard" looks like, and it is a statement
    /// about the game rather than a check that passes by doing nothing.
    ///
    /// **A menu on screen owns the button.** A press over one fires the widget
    /// under it and is not delivered here, or a tap on `TRY AGAIN` would both
    /// restart the run and flap. The *position* is delivered either way — a
    /// place is not a command, and a paddle that stopped following the finger
    /// while a panel was up would be a paddle the player cannot line up before
    /// serving.
    fn pointer_event(&mut self, pointer: PointerUpdate) {
        let _ = pointer;
    }

    /// One finger, once per event the shell reported — see [`TouchUpdate`].
    ///
    /// Called for **every** contact, including the primary one that also arrives
    /// through [`pointer_event`](Self::pointer_event): a game plays with one
    /// finger through the pointer without overriding this, and a game that wants
    /// two fingers overrides it and ignores the pointer.
    /// [`ShellEvent::Touch`] says why the two
    /// streams overlap.
    ///
    /// **A menu on screen does not claim contacts**, which is the one place this
    /// differs from the pointer. A menu is hit-tested against a position the
    /// loop knows about; an on-screen stick is the *game's* widget, laid out by
    /// the game, and the loop cannot tell a contact that landed on one from a
    /// contact that landed on a panel. Handing the game fewer contacts than the
    /// screen has would take the decision away from the only code that can make
    /// it — so the button is claimed on the pointer stream, where the loop
    /// knows what it is claiming, and contacts are delivered whole.
    ///
    /// The empty default is the honest answer for a game played with a
    /// keyboard and one finger, the same way
    /// [`debug_sections`](Self::debug_sections)'s is: nothing is verified by
    /// this method, and a game that never overrides it is a game with no
    /// on-screen controls.
    fn touch_event(&mut self, touch: TouchUpdate) {
        let _ = touch;
    }

    /// The game action a widget id names, or `None` for an id this game's menus
    /// do not use. Never asked about a reserved id — see [`FIRST_GAME_ID`].
    fn menu_action(id: crcbl_ui::WidgetId) -> Option<Self::MenuAction>;

    /// Does what a fired menu button of this game's asks for.
    ///
    /// Infallible on purpose: the three actions that can fail — resume,
    /// fullscreen, the debug panel — are the loop's, and a game that reported
    /// an error here would be reporting one for a button press.
    fn apply(&mut self, action: Self::MenuAction);

    /// Which menu this frame shows — and a chance to rebuild one first.
    ///
    /// Called after [`Self::draw`], so it may read whatever that refreshed.
    ///
    /// `menus` is the set [`Self::menus`] built. A game whose panel depends on
    /// live state — horde's level-up offer names three upgrades the simulation
    /// picked this level — replaces that entry here, before the kind it returns
    /// is shown. A game whose menus are fixed ignores the argument.
    fn menu_kind(
        &mut self,
        menus: &mut crcbl_ui::menu::MenuSet<Self::MenuKind>,
        paused: bool,
    ) -> Self::MenuKind;

    /// Hands this frame's state to the GPU and appends this game's UI geometry.
    ///
    /// Called with a cleared `draw_list`, before the menu and the debug overlay
    /// are appended to the same list: the scrim dims the game *including its
    /// HUD*, and the overlay is a developer tool that stays legible on top of
    /// everything.
    fn draw(
        &mut self,
        gpu: &mut Self::Gpu,
        draw_list: &mut crcbl_ui::draw_list::DrawList,
        frame: FrameInfo,
    );

    /// Adds this game's own sections to the debug panel.
    ///
    /// Called once a frame between the loop's own section — frame timing, and
    /// the GPU's pass timings where the device has timestamp queries — and the
    /// panel being drawn, so a game's numbers appear below the engine's.
    ///
    /// **The empty default is not the "opt-in hook that reports success by
    /// doing nothing" shape**: nothing is being verified here, and a game that
    /// adds no section renders a panel with no section of its own, which is a
    /// complete answer rather than a silent skip.
    ///
    /// In practice nearly every sample overrides it — `apps/sandbox` is the one
    /// that does not — because a sample's own numbers are the reason it exists:
    /// `apps/horde` reports how much of its field survived the cull, which is
    /// the number its whole design argument rests on. So the default is what a
    /// game with nothing to say gets, not what most of them use.
    fn debug_sections(&self, panel: &mut crcbl_ui::DebugPanel) {
        let _ = panel;
    }

    /// A frame limit this game's settings screen asked for since the loop
    /// last read it, or `None` on a frame that asked for nothing.
    ///
    /// The loop applies it to its own clock — [`Clock::set_limit`] — on the
    /// frame it is returned, which is what lets a game's pause-menu row
    /// change the fps cap mid-run without holding the loop. `Some` at most
    /// once per change: the loop takes the value, so a row that re-applies
    /// its state every frame does not re-apply the limit every frame.
    ///
    /// The empty default is the honest answer for the games with no settings
    /// screen, in the same way [`debug_sections`](Self::debug_sections) is:
    /// nothing is verified by this method, and a game that forgets to
    /// override it has a row that changes its own state and nothing else.
    fn take_pending_frame_limit(&mut self) -> Option<FrameLimit> {
        None
    }

    /// Whether an on-screen control asked the loop to toggle the pause since it
    /// last looked.
    ///
    /// [`PAUSE_KEY`] is the loop's and never reaches the game, which leaves the
    /// pause unreachable on a device with no keyboard: a phone can start a run
    /// and then cannot stop it, and the pause menu — the only place fullscreen
    /// and the debug panel are tappable — cannot be opened at all. This is the
    /// way back, and it is a *request* rather than a state so that the loop
    /// stays the only thing that knows whether the simulation is running.
    ///
    /// Taken like [`take_pending_frame_limit`](Self::take_pending_frame_limit),
    /// and for the same reason: a control that re-reports its state every frame
    /// would toggle the pause every frame. A frame carrying both this and the
    /// key is **one** toggle, not two — the player asked for the pause once.
    ///
    /// The `false` default is the honest answer for a game with no on-screen
    /// controls, and it is not an opt-in hook reporting success by doing
    /// nothing: a game that never overrides it is a game whose pause is the key,
    /// which is every game that came before touch.
    fn take_pending_pause(&mut self) -> bool {
        false
    }

    /// Adds this game's own fields to the run's shared ones.
    fn summary(&self, run: RunSummary) -> Self::Summary;

    /// Logs the one line a finished run is worth.
    ///
    /// Which numbers those are is the only thing that ever differed between the
    /// samples' browser entry points: breakout has a score, horde a time
    /// survived and a kill count, and no shared shape covers both without
    /// inventing a summary type neither wanted.
    fn log_summary(summary: &Self::Summary);
}

/// The parts of a loop that come from the command line rather than the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopConfig {
    /// The fixed timestep, in hertz.
    pub tick_hz: u32,
    /// Stop after this many **presented** frames — see [`FrameBudget`].
    pub frames: Option<u64>,
    /// Whether the debug panel starts visible.
    pub debug_overlay: bool,
    /// Whether to idle between frames. False for a headless run, which has no
    /// compositor to wait on and would otherwise sleep through its budget.
    pub windowed: bool,
    /// The most frames a second the loop will run.
    ///
    /// Applied to [`Booted::clock_source`] by [`Loop::new`], and a no-op on the
    /// manual clock a headless run gets — see [`Clock::set_limit`]. A game that
    /// exposes this as a user setting calls that method on
    /// [`Loop::clock_source_mut`] when the setting changes; this is only the
    /// value the run starts at.
    pub limit: FrameLimit,
}

/// The frame, owned by the engine.
///
/// What five samples' `app.rs` each spelled out: pump the shell, route the
/// input, run the ticks the clock owes, draw, present. The parts that genuinely
/// differed are [`HostedGame`] and [`GameGpu`].
///
/// # This is not the only way in
///
/// `apps/bare` drives the same public pieces — [`GpuContext`], [`Pending`],
/// [`FrameBudget`] — with a loop it writes itself, and
/// `crates/crcbl/tests/seam_from_outside.rs` guards that path. A game that wants the
/// frame back takes it; this type is the default, not the toll gate.
pub struct Loop<S: Shell + ?Sized, G: HostedGame> {
    shell: Box<S>,
    window: WindowId,
    gpu: G::Gpu,
    game: G,
    clock_source: Clock,
    frame_clock: crcbl_core::FrameClock,
    /// Reused every frame, so a steady-state frame does not allocate a fresh
    /// draw list.
    draw_list: crcbl_ui::draw_list::DrawList,
    menus: crcbl_ui::menu::MenuSet<G::MenuKind>,
    /// Where the pointer was last seen and whether its button is down — both
    /// are needed across frames, see [`PointerCapture`].
    pointer: PointerCapture,
    debug: crcbl_ui::DebugOverlay,
    /// Whether the simulation is stopped. **The loop owns this, not the game.**
    /// Pause is not something a simulation does — it is the loop declining to
    /// advance it — and a `Paused` state inside the game would put a value in
    /// the summary that a headless scripted run could reach.
    paused: bool,
    /// Keys forwarded to the game as pressed and not yet released, so focus
    /// loss can release them — see [`lose_focus`].
    held_keys: Vec<crcbl_core::input::KeyCode>,
    /// Whether the game was told the pointer button is down and not yet told it
    /// came up.
    ///
    /// The pointer's half of [`Self::held_keys`], and it exists for the same two
    /// repairs: a release must reach a game that was told about the press even
    /// if a menu opened in between, and focus loss must deliver one that no
    /// platform will send. A game left holding the button sees no *edge* on the
    /// next tap, so the bug is not a stuck paddle — it is a button that stops
    /// working.
    pointer_in_game: bool,
    /// Whether the press the pointer is holding was made **on a panel**, and so
    /// whether a menu may act on it.
    ///
    /// [`Self::pointer_in_game`]'s opposite number, and the two are exclusive by
    /// construction: a press goes to the game or to the menu depending on
    /// whether one was on screen when it landed, and it stays there until it is
    /// released. Without this a menu opened under a held pointer — which on a
    /// phone is any menu opened while a finger holds an on-screen control —
    /// fires the button that happens to appear beneath it.
    menu_owns_press: bool,
    /// Contacts the game was told began and not yet told ended, with where each
    /// was last seen.
    ///
    /// The contact's half of [`Self::held_keys`], and it exists for the same
    /// repair: focus loss must deliver an end no platform sends. A cancel and
    /// not a release, because the player did not lift the finger — see
    /// [`TouchUpdate::phase`].
    live_contacts: Vec<(crcbl_core::input::ContactId, glam::Vec2)>,
    /// Which contact the platform is **also** reporting as the emulated
    /// pointer, while it is down.
    ///
    /// The seam carries no flag for it, so it is re-derived from the rule every
    /// backend that sets [`ShellCaps::TOUCH`](crcbl_shell::ShellCaps::TOUCH)
    /// owes — the browser's, which the others must match: the emulated pointer
    /// is the **first contact of a gesture**, the one down while no other is,
    /// and no later finger inherits it when that one lifts.
    ///
    /// It is not a pointer the engine synthesizes and nothing reads a position
    /// off it: it exists so [`Self::menu_contact`] can skip the one finger the
    /// menu already hears about through the pointer. Without it a one-finger tap
    /// on a button would arrive twice and fire twice — which for `FULLSCREEN` is
    /// a toggle that does nothing at all.
    pointer_contact: Option<crcbl_core::input::ContactId>,
    /// The contact holding a press **on the panel on screen**, if any.
    ///
    /// [`Self::menu_owns_press`] for fingers, and it is what makes a menu
    /// reachable while another contact holds an on-screen control: only the
    /// primary contact drives the pointer, so a thumb on a stick leaves every
    /// other finger with no way to press a button at all. A contact is the
    /// menu's only if it *landed on a button* while a panel was up — a finger
    /// that came down on the field before the panel did stays the game's, and
    /// one that lands on the panel's background is nobody's.
    ///
    /// One at a time, like the pointer's single capture: first come, first
    /// served, which is the rule every other control here follows.
    menu_contact: Option<crcbl_core::input::ContactId>,
    mode: ModeRequest,
    budget: FrameBudget,
    ticks: u64,
    events: u64,
    windowed: bool,
}

impl<S: Shell + ?Sized, G: HostedGame> std::fmt::Debug for Loop<S, G> {
    /// Hand-written because the game's associated types are the game's and need
    /// not be `Debug`; what a reader wants from this is where the run has got
    /// to.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loop")
            .field("window", &self.window)
            .field("paused", &self.paused)
            .field("ticks", &self.ticks)
            .field("events", &self.events)
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl<S: Shell + ?Sized, G: HostedGame> Loop<S, G> {
    /// Assembles a loop around a game, on parts start-up already produced.
    ///
    /// [`Booted`] is what both bring-up paths hand over — the blocking one
    /// builds it from [`wait_for_configure`] and its own `Gpu::open`, the
    /// browser's from [`PolledBoot::poll`] — so there is one struct literal for
    /// the loop rather than one per path per game.
    pub fn new(booted: Booted<S, G::Gpu>, game: G, config: LoopConfig) -> Self {
        // Here rather than at every bring-up path's `Clock::new`, because this
        // is the one place both of them pass through and the one place the
        // command line's value is already in hand. A manual clock ignores it.
        let mut clock_source = booted.clock_source;
        clock_source.set_limit(config.limit);
        Self {
            shell: booted.shell,
            window: booted.window,
            gpu: booted.gpu,
            game,
            clock_source,
            frame_clock: crcbl_core::FrameClock::new(config.tick_hz),
            draw_list: crcbl_ui::draw_list::DrawList::new(),
            menus: G::menus(),
            pointer: PointerCapture::new(),
            debug: crcbl_ui::DebugOverlay::with_visible(config.debug_overlay),
            paused: false,
            held_keys: Vec::new(),
            pointer_in_game: false,
            menu_owns_press: false,
            live_contacts: Vec::new(),
            pointer_contact: None,
            menu_contact: None,
            mode: ModeRequest::new(),
            budget: FrameBudget::new(config.frames),
            ticks: 0,
            events: booted.events,
            windowed: config.windowed,
        }
    }

    /// One frame: pump, route, tick the simulation to catch up with the clock,
    /// draw, present.
    ///
    /// # The trace's frame boundary is here
    ///
    /// [`crcbl_core::trace::drain`] is called once per call to this, after the
    /// body — which owns the outermost span — has returned and its guard has
    /// dropped. A drain does not disturb a span that is open across
    /// it, it *splits* it, so a drain inside the frame span would put every
    /// frame's begin in one snapshot and its end in the next and leave
    /// [`crate::perf::frame_cpu_time`] with no whole frame to read. Here is also
    /// the one place both drivers pass through: the native [`drive`] and the
    /// browser's `crcbl::web::App` both step this method and neither has to know
    /// the trace exists.
    ///
    /// # Errors
    ///
    /// [`LoopError`] if the shell or the GPU failed.
    pub fn frame(&mut self) -> Result<Flow, LoopError<G::Error>> {
        if self.budget.is_spent() {
            return Ok(Flow::Stop(ExitReason::FrameBudget));
        }

        // **Deliberately outside the frame span.** This is the loop idling with
        // nothing to do, and a CPU frame time that counted the compositor's idle
        // timeout would be the timeout on every still frame — larger than any
        // GPU total, and "CPU-bound" as the answer to a question it never looked
        // at. `crate::perf` says which spans are work and which are waiting.
        if self.windowed {
            self.shell.wait_events(Some(WINDOWED_IDLE));
        }

        let flow = self.frame_body();
        self.record_frame_cost();
        flow
    }

    /// The frame proper, with the outermost span open across all of it.
    fn frame_body(&mut self) -> Result<Flow, LoopError<G::Error>> {
        let _frame = crcbl_core::trace::span(crate::perf::FRAME_SPAN);
        let input = crcbl_core::trace::span(crate::perf::INPUT_SPAN);

        // **Carrying the pointer, not defaulting it.** A batch with no pointer
        // event in it has not moved the cursor, and a menu whose hover state
        // reset every still frame would flicker.
        let mut pending = self.pointer.pending();
        let game = &mut self.game;
        // **Last frame's menu, deliberately.** The pump runs before this
        // frame's state is known, and the menu the player is pressing keys at
        // is the one that was on screen when they pressed them.
        let showing = self.menus.current().is_some();
        let mut menu = MenuPump::new(&mut self.menus, &mut self.held_keys, showing);
        self.shell.pump(&mut |event| {
            // The window's business, the pointer, focus loss and the loop's
            // three reserved keys are all folded by `Pending::observe`; the
            // menu's three and the held-key list are `MenuPump`'s. What comes
            // back from that is the key the *game* should see.
            if pending.observe(&event) == Handled::Loop {
                return;
            }
            if let Some((code, pressed)) = menu.observe(&event) {
                game.key_event(code, pressed);
            }
        });
        let from_keyboard = menu.activated;
        self.events += pending.count;
        // Hit-tested against **this** frame's layout, which is why the pointer
        // is resolved here and not inside the pump: the rectangles depend on the
        // framebuffer's size, and a click checked against last frame's would
        // miss on the frame a resize lands.
        // Before `resolve`, which is what folds this batch's position in: a
        // frame whose pump carried no pointer event has not moved the pointer,
        // and a game told it moved to where it already was would treat a resting
        // cursor as the player asking for something every frame.
        let pointer_moved = pending.pointer != self.pointer.at();
        let pointer_input = self.pointer.resolve(&pending);
        // **A press that began before the panel did is not the panel's.**
        //
        // `UiState` latches a button while the pointer is *down* over it, so a
        // pointer that was already held when a menu opened latches whatever
        // button appeared underneath and fires it on release — a click the
        // player never made on a panel that was not there when they pressed.
        // Rare with a mouse, and the ordinary case on a phone: the thumb
        // holding an on-screen stick **is** the emulated pointer, so every menu
        // opened with the other hand opens under a held press.
        //
        // The rule is the mirror of the one three lines down, where a press
        // over a panel does not reach the game: whoever the press was made on
        // keeps it until it is released.
        if pending.pointer_pressed && showing {
            self.menu_owns_press = true;
        }
        let from_pointer = self.menus.point(
            self.gpu.extent(),
            self.gpu.atlas(),
            crcbl_ui::PointerInput {
                down: pointer_input.down && self.menu_owns_press,
                released: pointer_input.released && self.menu_owns_press,
                ..pointer_input
            },
        );
        if pending.pointer_released {
            self.menu_owns_press = false;
        }
        for id in [from_keyboard, from_pointer].into_iter().flatten() {
            if let Some(action) = MenuAction::from_id(id, G::menu_action) {
                self.apply(action)?;
            }
        }

        // **Contacts before the pointer**, which is the order they exist in:
        // the finger is the event, and the emulated pointer is derived from it
        // by the platform. A game whose on-screen control took the contact has
        // to be able to say that the pointer press the *same* finger raised is
        // that control's and not a flap — see
        // [`PauseControl::takes_pointer`](pause::PauseControl::takes_pointer) —
        // and it can only say so once it has heard about the finger.
        //
        // One call per event and none merged, because a tap is a `Began` and an
        // `Ended` in one batch and a game that only heard the last of them
        // would never see a finger land.
        for touch in std::mem::take(&mut pending.touches) {
            let at = normalised(touch.at, self.gpu.extent());
            // Before the bookkeeping below moves it: the first contact of a
            // gesture is the one down while `live_contacts` is empty, which is
            // exactly the rule the platform used to pick the finger it emulates
            // the pointer with. See `Self::pointer_contact`.
            if matches!(touch.phase, crcbl_core::input::TouchPhase::Began)
                && self.live_contacts.is_empty()
            {
                self.pointer_contact = Some(touch.contact);
            }
            self.live_contacts.retain(|(id, _)| *id != touch.contact);
            if touch.phase.ends_contact() {
                if self.pointer_contact == Some(touch.contact) {
                    self.pointer_contact = None;
                }
            } else {
                self.live_contacts.push((touch.contact, at));
            }
            // The menu's turn at this finger, before the game's — the same
            // order the pointer's half above takes, and for the same reason: a
            // button fires on the batch the player pressed it in.
            if let Some(id) = self.route_contact_to_menu(&touch)
                && let Some(action) = MenuAction::from_id(id, G::menu_action)
            {
                self.apply(action)?;
            }
            self.game.touch_event(TouchUpdate {
                contact: touch.contact,
                phase: touch.phase,
                at,
            });
        }

        // What is left of the pointer once the menu and the game's own controls
        // have had it. `showing` is
        // last frame's menu for the same reason the keyboard's claim is: the
        // panel the player tapped is the one that was on screen when they did.
        let pressed = pending.pointer_pressed && !showing;
        // `|| pressed` because a tap faster than a frame is one batch, and its
        // release has to go out with the press it answers — which on a phone is
        // every tap. Without it the game keeps the button down, and the *next*
        // tap raises no edge: the first one works and the second does nothing,
        // which is the shape a single-tap test cannot see.
        let released = pending.pointer_released && (self.pointer_in_game || pressed);
        let at = pointer_moved
            .then_some(pending.pointer)
            .flatten()
            .map(|point| normalised(point, self.gpu.extent()));
        if pressed || released || at.is_some() {
            self.pointer_in_game = (self.pointer_in_game || pressed) && !released;
            self.game.pointer_event(PointerUpdate {
                at,
                pressed,
                released,
            });
        }

        // A settings row fired this frame: hand the limit it asked for to the
        // clock before it is advanced, so the frame about to run is the first
        // one the new cap paces.
        if let Some(limit) = self.game.take_pending_frame_limit() {
            self.clock_source.set_limit(limit);
        }

        if pending.toggle_debug_overlay {
            self.debug.toggle();
        }
        // Before the pause toggle, so a batch carrying both a focus loss and an
        // Escape resolves as "paused, then the player unpaused" rather than the
        // reverse.
        if pending.focus_lost {
            let game = &mut self.game;
            lose_focus(&mut self.held_keys, &mut self.paused, |key| {
                game.key_event(key, false);
            });
            // The same obligation for the button: no platform sends the release
            // for a pointer that was down when focus left, and a game still
            // holding it sees no edge on the next tap.
            if self.pointer_in_game {
                self.pointer_in_game = false;
                self.game.pointer_event(PointerUpdate {
                    at: None,
                    pressed: false,
                    released: true,
                });
            }
            // A finger that was pressing a panel is holding a press nobody will
            // finish, and the menu's is the same obligation: dropped rather than
            // fired, because the window going away is not a tap.
            self.pointer_contact = None;
            if self.menu_contact.take().is_some() {
                self.menus.cancel_press();
            }
            // And the same for every finger that was down. **Cancelled, not
            // ended**: the player did not lift it, so a stick centres and a
            // charge-up does not fire. Each keeps its last position, which is
            // where the finger was when the window went away.
            for (contact, at) in std::mem::take(&mut self.live_contacts) {
                self.game.touch_event(TouchUpdate {
                    contact,
                    phase: crcbl_core::input::TouchPhase::Cancelled,
                    at,
                });
            }
        }
        // Taken every frame rather than only when the key did not fire, so a
        // press left pending behind an Escape is not still waiting on the next
        // frame to toggle the pause a second time. Both in one frame is one
        // toggle: the player asked once, with two fingers or with two hands.
        let pause_control = self.game.take_pending_pause();
        if pending.toggle_pause || pause_control {
            self.paused = !self.paused;
            log::info!("game {}", if self.paused { "paused" } else { "resumed" });
        }
        if pending.toggle_fullscreen {
            ModeRequest::toggle(self.shell.as_mut(), self.window)?;
        }
        // Once per transition, not once per frame: a backend that cannot do
        // fullscreen at all would otherwise print a line every frame forever.
        self.mode.check(self.shell.as_ref(), self.window);

        if pending.destroyed {
            return Ok(Flow::Stop(ExitReason::WindowDestroyed));
        }
        if pending.close_requested {
            accept_close(self.shell.as_mut(), self.window)?;
            return Ok(Flow::Stop(ExitReason::CloseRequested));
        }
        if let Some(size) = pending.resized {
            self.gpu.resize((size.width, size.height))?;
        }
        drop(input);

        // The one call in the frame that sleeps, and the reason it has a span of
        // its own: the limiter holds the loop back, and time spent held back is
        // not time the CPU spent on the frame.
        let pace = crcbl_core::trace::span(crate::perf::PACE_SPAN);
        let now = self.clock_source.advance();
        drop(pace);
        self.frame_clock.update(now);
        // Recorded whether or not the panel is visible — a window that only
        // fills while you are looking at it shows two seconds of nothing every
        // time you press F3.
        self.debug.record(self.frame_clock.render_dt());
        // A paused frame keeps the clock and throws the ticks away, which is
        // `run_ticks`'s whole job; its docs carry the argument for why.
        let tick = crcbl_core::trace::span(crate::perf::TICK_SPAN);
        let tick_dt = self.frame_clock.tick_dt_secs();
        let game = &mut self.game;
        let gpu = &mut self.gpu;
        let ran = run_ticks(&mut self.frame_clock, self.paused, || {
            game.tick(gpu, tick_dt)
        });
        drop(tick);
        self.ticks += ran;

        let draw = crcbl_core::trace::span(crate::perf::DRAW_SPAN);
        // `alpha` is read after the tick loop, never before: before, the
        // accumulator may still hold whole ticks.
        let info = FrameInfo {
            paused: self.paused,
            ticks: ran,
            alpha: self.frame_clock.alpha(),
            tick_dt,
        };
        self.draw_list.clear();
        self.game.draw(&mut self.gpu, &mut self.draw_list, info);
        self.draw_menu();
        self.draw_debug_overlay();
        self.gpu.take_draw_list(&mut self.draw_list);
        drop(draw);

        let present = crcbl_core::trace::span(crate::perf::PRESENT_SPAN);
        let outcome = self.gpu.frame()?;
        drop(present);

        // After the frame, because that is when the renderers have recorded
        // anything to count — and still inside the frame span, so the drain that
        // closes the frame carries its counters and its spans in one snapshot.
        crate::perf::sample_counters(self.gpu.counters());

        self.budget.record(outcome)?;
        Ok(Flow::Continue)
    }

    /// Closes the trace's frame and feeds the debug panel's budget row.
    ///
    /// Both halves are recorded whether or not the panel is visible, for the
    /// reason [`DebugOverlay::record`](crcbl_ui::DebugOverlay::record) is: a
    /// window that only starts filling when you look at it shows nothing for the
    /// first two seconds every time.
    ///
    /// The GPU half is *not* this frame's — the timers are frames latent by
    /// design and `docs/plan/40-profiling.md` refuses to stall them — which is
    /// why it goes in by frame number and why the row shows two distributions
    /// rather than a pair. See [`crcbl_ui::budget`].
    fn record_frame_cost(&mut self) {
        // The whole cost of a run that never turned the trace on: one relaxed
        // atomic load. `drain` takes locks, so it is not on the other side of
        // this branch by accident.
        if crcbl_core::trace::is_enabled() {
            let snapshot = crcbl_core::trace::drain();
            if let Some(cpu) = crate::perf::frame_cpu_time(&snapshot) {
                self.debug.budget.record_cpu(cpu);
            }
        }
        if let Some(timings) = self.gpu.timings()
            && !timings.is_empty()
        {
            self.debug
                .budget
                .record_gpu(timings.frame, Duration::from_nanos(timings.total_nanos()));
        }
    }

    /// Offers one contact to the menu on screen, and reports a button it fired.
    ///
    /// # Why a menu hears fingers at all
    ///
    /// Because the pointer cannot carry them. Only the **primary** contact is
    /// reported as the emulated pointer, so a game whose on-screen control is
    /// held — a thumb on horde's stick — owns that pointer for as long as the
    /// thumb is down, and every other finger raises no pointer event anywhere.
    /// The menu was therefore unreachable at exactly the moment a player most
    /// wants it: pause with one thumb still down and `RESUME` could not be
    /// tapped by the other hand until the first was lifted.
    ///
    /// Contacts are a **second device** driving the same widgets, the way
    /// [`MENU_ACTIVATE_KEY`] is. Nothing here synthesizes a pointer: the
    /// position comes from the contact, no [`PointerCapture`] state is touched,
    /// and a game bound to [`Binding::MouseButton`](crcbl_input::Binding) sees
    /// exactly what it saw before.
    ///
    /// # Which fingers are the menu's
    ///
    /// * **Not the one the pointer already carries** — see
    ///   [`Self::pointer_contact`]. A one-finger tap arrives on both streams,
    ///   and a button that fired on each would fire twice.
    /// * **Only one at a time**, and only one whose landing **latched a
    ///   button**. That is the whole of the rule — a press the menu is not
    ///   holding is never followed, so a finger that came down on the field
    ///   before the panel existed cannot fire what appears under it (the menu
    ///   was not on screen for its `Began`, so nothing latched), and a finger
    ///   resting on the panel's background cannot occupy the slot the other hand
    ///   needs. [`Self::menu_owns_press`] is the same rule for the pointer,
    ///   which needs a flag of its own because its press is a *level* the loop
    ///   re-reads every frame rather than an event that arrives once.
    /// * A contact the system **cancelled** drops the press without firing,
    ///   which is the whole reason [`TouchPhase::Cancelled`] is not
    ///   [`TouchPhase::Ended`].
    ///
    /// [`TouchPhase::Cancelled`]: crcbl_core::input::TouchPhase::Cancelled
    /// [`TouchPhase::Ended`]: crcbl_core::input::TouchPhase::Ended
    fn route_contact_to_menu(&mut self, touch: &TouchContact) -> Option<crcbl_ui::WidgetId> {
        use crcbl_core::input::TouchPhase;

        if self.pointer_contact == Some(touch.contact) {
            return None;
        }
        match touch.phase {
            TouchPhase::Began => {
                // `press_captured` as well as `menu_contact`: the press already
                // latched may be the pointer's, and a second finger arriving on
                // top of one is not the menu's either. First come, first served,
                // which is the rule every control here follows.
                if self.menu_contact.is_some() || self.menus.press_captured() {
                    return None;
                }
                let fired = self.point_contact(touch.at, true, false);
                // A press latches a button or it lands on nothing. Only the
                // first is a press the menu is holding, and only that one is
                // worth remembering — the rest of this finger's gesture is the
                // game's business.
                if self.menus.press_captured() {
                    self.menu_contact = Some(touch.contact);
                }
                fired
            }
            TouchPhase::Moved => {
                if self.menu_contact != Some(touch.contact) {
                    return None;
                }
                self.point_contact(touch.at, true, false)
            }
            TouchPhase::Ended => {
                if self.menu_contact != Some(touch.contact) {
                    return None;
                }
                self.menu_contact = None;
                self.point_contact(touch.at, false, true)
            }
            TouchPhase::Cancelled => {
                if self.menu_contact != Some(touch.contact) {
                    return None;
                }
                self.menu_contact = None;
                self.menus.cancel_press();
                None
            }
        }
    }

    /// One contact's worth of input against this frame's menu, in the pixels it
    /// is laid out in.
    ///
    /// The layout is the menu's own, recomputed per call for the reason
    /// [`MenuSet::point`](crcbl_ui::menu::MenuSet::point) recomputes it: it
    /// depends on the framebuffer's size, and a hit test against last frame's
    /// rectangles misses on the frame a rotation lands.
    fn point_contact(
        &mut self,
        at: glam::Vec2,
        down: bool,
        released: bool,
    ) -> Option<crcbl_ui::WidgetId> {
        self.menus.point(
            self.gpu.extent(),
            self.gpu.atlas(),
            crcbl_ui::PointerInput {
                pos: at,
                down,
                released,
            },
        )
    }

    /// What a fired menu button does.
    ///
    /// The one place a button becomes an effect. Both input devices arrive
    /// here: [`MENU_ACTIVATE_KEY`] and a click produce the same [`MenuAction`]
    /// and this cannot tell them apart, which is what makes "the keyboard still
    /// works" and "the mouse works too" the same sentence.
    fn apply(&mut self, action: MenuAction<G::MenuAction>) -> Result<(), LoopError<G::Error>> {
        match action {
            MenuAction::Resume => {
                if self.paused {
                    self.paused = false;
                    log::info!("game resumed");
                }
            }
            MenuAction::Fullscreen => ModeRequest::toggle(self.shell.as_mut(), self.window)?,
            MenuAction::DebugOverlay => self.debug.toggle(),
            MenuAction::Game(action) => self.game.apply(action),
        }
        Ok(())
    }

    /// Picks this frame's menu, lays it out, and emits both halves of it.
    ///
    /// **Two halves, two passes.** The window frame and the buttons are
    /// nine-sliced sprites and go to the menu pass through
    /// [`GameGpu::set_menu`]; the title and the labels are text and go to the
    /// UI pass through the draw list.
    fn draw_menu(&mut self) {
        let kind = self.game.menu_kind(&mut self.menus, self.paused);
        // A panel that has been replaced takes the press with it, the same way
        // [`MenuSet::show`] drops the capture: the button the press landed on is
        // not on screen any more, and the next panel's button in the same place
        // is not the one anybody pressed. Cleared here rather than left to
        // `show`, because the *loop's* half of the ownership is what decides
        // whether the pointer reaches the panel at all.
        if kind != self.menus.kind() {
            self.menu_owns_press = false;
            // The finger's half of the same rule: the button it was holding is
            // not on screen any more, so its lift belongs to nothing. `show`
            // below drops the capture it was latched onto.
            self.menu_contact = None;
        }
        self.menus.show(kind);
        let layout = self
            .menus
            .current()
            .map(|menu| menu.layout(self.gpu.extent(), self.gpu.atlas()));
        match &layout {
            Some(layout) => {
                let menu = self.menus.current().expect("a layout implies a menu");
                menu.render(&mut self.draw_list, layout);
                self.gpu.set_menu(Some((menu, layout)));
            }
            None => self.gpu.set_menu(None),
        }
    }

    /// Gathers this frame's debug sections and draws the panel.
    ///
    /// The GPU timings are a `Some` check because a device without timestamp
    /// queries has no timers at all.
    ///
    /// The counters section has no such condition — every bundle answers
    /// [`GameGpu::counters`] — but it is the **previous** frame's, because this
    /// runs before [`GameGpu::frame`] has recorded anything. Uniformly so: the
    /// whole section lags by one frame rather than one row of it lagging beside
    /// a live neighbour. See [`crcbl_render::counters`].
    fn draw_debug_overlay(&mut self) {
        self.debug.begin_frame();
        if let Some(timings) = self.gpu.timings() {
            self.debug.panel.add(timings);
        }
        let counters = self.gpu.counters();
        self.debug.panel.add(&counters);
        self.game.debug_sections(&mut self.debug.panel);
        let (width, height) = self.gpu.extent();
        #[allow(clippy::cast_precision_loss)]
        self.debug.render(
            &mut self.draw_list,
            glam::Vec2::new(width as f32, height as f32),
            self.gpu.atlas(),
        );
    }

    /// Tears the frame down and reports what the run did.
    ///
    /// # Errors
    ///
    /// [`LoopError`] if the GPU or the shell failed to release something. Both
    /// are attempted regardless: the window is destroyed even when the GPU
    /// teardown failed, because leaving it mapped is strictly worse.
    pub fn finish(mut self, exit: ExitReason) -> Result<G::Summary, LoopError<G::Error>> {
        if let Some(timings) = self.gpu.timings()
            && !timings.is_empty()
        {
            log::info!("{}", timings.report().trim_end());
        }
        // **The CPU half of the same report.** `FrameTimings` above is GPU
        // timestamps; this is the monotonic clock the loop was actually driven
        // from, which on a headless run is the fixed step and therefore says
        // nothing about the machine. Printed either way, **with the clock
        // named**, because a number whose conditions are not stated is not a
        // measurement — `apps/horde`'s `--wall-clock` exists to make this line
        // mean something and every figure in `docs/plan/sample/03-horde.md` was
        // taken through it.
        let frame = &self.debug.frame;
        if let (Some(best), Some(worst)) = (frame.best(), frame.worst()) {
            log::info!(
                "frame cpu ({} clock, last {} frames): mean {:.3} ms ({:.1} fps), \
                 best {:.3} ms, worst {:.3} ms",
                if matches!(self.clock_source, Clock::Real(_)) {
                    "real"
                } else {
                    "fixed-step"
                },
                frame.len(),
                frame.mean().as_secs_f64() * 1e3,
                frame.fps(),
                best.as_secs_f64() * 1e3,
                worst.as_secs_f64() * 1e3,
            );
        }
        let summary = self.game.summary(RunSummary {
            backend: self.shell.backend(),
            frames: self.budget.presented(),
            ticks: self.ticks,
            events: self.events,
            extent: self.gpu.extent(),
            exit,
            paused: self.paused,
            // Not `ModeRequest::mode`: a close request has already destroyed
            // the window by the time this runs.
            mode: self.mode.mode_at_exit(self.shell.as_ref(), self.window),
        });

        let gpu_result = self.gpu.destroy();
        let shell_result = if exit.window_survives() {
            self.shell.destroy_window(self.window)
        } else {
            Ok(())
        };
        gpu_result?;
        shell_result?;
        Ok(summary)
    }

    /// Sets how far one [`frame`](Self::frame) advances a manual clock.
    ///
    /// **The browser's clock is the browser's.** `Clock::Real` reads
    /// [`std::time::Instant`], which on `wasm32-unknown-unknown` has no
    /// implementation at all and panics on the first `now()`. A web entry point
    /// therefore builds the loop on [`Clock::manual`] and calls this once per
    /// `requestAnimationFrame` with the delta the browser reported. A real
    /// clock ignores this — a loop that *can* read the time must not be steered
    /// by its caller.
    ///
    /// `dt` is clamped to [`MAX_FRAME_STEP`]: a backgrounded tab resumes with a
    /// multi-second gap, and feeding that to the accumulator spends the next
    /// frame running thousands of ticks.
    pub fn set_frame_step(&mut self, dt: Duration) {
        if let Clock::Manual { step, .. } = &mut self.clock_source {
            *step = dt.min(MAX_FRAME_STEP);
        }
    }

    /// Whether the simulation is stopped.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Stops or resumes the simulation, without a key event.
    ///
    /// For an embedder with a reason of its own — a tab going invisible, a
    /// modal opening over the canvas — and for a test that wants a paused loop
    /// without scripting the keystroke that gets there. [`PAUSE_KEY`] is the
    /// player's way in and goes through the same field.
    pub const fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// The swapchain's current extent, in pixels.
    #[must_use]
    pub fn extent(&self) -> (u32, u32) {
        self.gpu.extent()
    }

    /// The mode the window system actually has this window in.
    ///
    /// Read back rather than remembered. There is deliberately no
    /// `self.fullscreen` field to disagree with the compositor.
    #[must_use]
    pub fn display_mode(&self) -> DisplayMode {
        ModeRequest::mode(self.shell.as_ref(), self.window)
            .expect("the loop's window is live while the loop runs")
    }

    /// The window this loop is driving.
    #[must_use]
    pub const fn window(&self) -> WindowId {
        self.window
    }

    /// The hosted game.
    #[must_use]
    pub const fn game(&self) -> &G {
        &self.game
    }

    /// The hosted game, for a test or an embedder that drives it directly.
    pub const fn game_mut(&mut self) -> &mut G {
        &mut self.game
    }

    /// The GPU bundle, for a test that reads back what a frame handed over.
    #[must_use]
    pub const fn gpu(&self) -> &G::Gpu {
        &self.gpu
    }

    /// The GPU bundle, mutably — for a game whose read-back builds something,
    /// and for an embedder that has its own use for the device between frames.
    pub const fn gpu_mut(&mut self) -> &mut G::Gpu {
        &mut self.gpu
    }

    /// The debug panel, for a test that asks which sections a frame gathered.
    #[must_use]
    pub const fn debug(&self) -> &crcbl_ui::DebugOverlay {
        &self.debug
    }

    /// Simulation ticks run so far.
    #[must_use]
    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Shell events observed so far, start-up's included.
    #[must_use]
    pub const fn events(&self) -> u64 {
        self.events
    }

    /// Whether the window system agreed with the last display-mode request.
    ///
    /// Distinct from [`display_mode`](Self::display_mode), which says what the
    /// window *is*: a tiling window manager leaves this `false` while the mode
    /// reads `Windowed`, and the pair is what "asked and was refused" looks
    /// like.
    #[must_use]
    pub const fn mode_honoured(&self) -> bool {
        self.mode.honoured()
    }

    /// The clock this loop advances, so a caller can see whether it is the
    /// steerable one — see [`set_frame_step`](Self::set_frame_step).
    #[must_use]
    pub const fn clock_source(&self) -> &Clock {
        &self.clock_source
    }

    /// The clock, mutably, so a run can change its [`FrameLimit`] after it has
    /// started.
    ///
    /// The frame limit's counterpart to
    /// [`GpuContext::set_pacing`](GpuContext::set_pacing): a settings screen
    /// that offers "vsync / VRR / off" and an fps cap changes the first through
    /// the game's own `Gpu` — which [`HostedGame::tick`] is handed every frame —
    /// and the second through here. [`LoopConfig::limit`] is only the value the
    /// run starts at.
    pub const fn clock_source_mut(&mut self) -> &mut Clock {
        &mut self.clock_source
    }

    /// Keys forwarded to the game as pressed and not yet released.
    ///
    /// Public because the obligation it discharges is testable and worth
    /// testing: focus loss must release every one of them — see [`lose_focus`].
    #[must_use]
    pub fn held_keys(&self) -> &[crcbl_core::input::KeyCode] {
        &self.held_keys
    }

    /// The shell, at whatever type this loop was built with — so a test can
    /// inject the events a compositor would deliver.
    pub fn shell_mut(&mut self) -> &mut S {
        self.shell.as_mut()
    }

    /// Which menu this frame is showing.
    #[must_use]
    pub fn menu_kind(&self) -> G::MenuKind {
        self.menus.kind()
    }

    /// Where this frame's menu was laid out, so a scripted click lands on the
    /// button the player would have seen.
    #[must_use]
    pub fn menu_layout(&self) -> Option<crcbl_ui::menu::MenuLayout> {
        self.menus
            .current()
            .map(|menu| menu.layout(self.gpu.extent(), self.gpu.atlas()))
    }
}

/// Lets [`drive`] step an engine-owned loop, and the browser's `App` step the
/// same one.
impl<S: Shell + ?Sized, G: HostedGame> GameLoop for Loop<S, G> {
    type Error = LoopError<G::Error>;
    type Summary = G::Summary;

    fn frame(&mut self) -> Result<Flow, Self::Error> {
        Self::frame(self)
    }

    fn finish(self, exit: ExitReason) -> Result<Self::Summary, Self::Error> {
        Self::finish(self, exit)
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
        /// What the last [`FakeGpu`] request was asked to open.
        static REQUESTED: std::cell::Cell<GpuOptions> =
            const { std::cell::Cell::new(GpuOptions { backend: None, pacing: Pacing::Auto }) };
    }

    /// A [`PolledGpu`] that arrives after a set number of polls.
    ///
    /// A real bundle needs a driver; what these tests are about is the order the
    /// engine does things in, which a fake observes better than a device can —
    /// it can count the requests.
    #[derive(Debug)]
    struct FakeGpu {
        extent: (u32, u32),
        atlas: crcbl_ui::FontAtlas,
        /// The UI geometry the last frame handed over.
        draw_list: crcbl_ui::draw_list::DrawList,
        /// Whether the last frame handed over a menu.
        had_menu: bool,
        /// Frames recorded and presented.
        frames: u32,
        /// What [`GameGpu::timings`] answers. `None` is a device with no
        /// timestamp queries, which is what a fake is unless a test says
        /// otherwise.
        timings: Option<crcbl_render::FrameTimings>,
        /// What the last [`GameGpu::frame`] recorded.
        ///
        /// Derived from the draw list that frame was handed rather than set by a
        /// test, so it is a number that **moves when the frame's content moves**
        /// — which is what an assertion about a counter has to be written
        /// against. A fake whose counters were a constant would pass every test
        /// a counter wired to a constant also passes.
        counters: crcbl_render::FrameCounters,
    }

    impl FakeGpu {
        fn at(extent: (u32, u32)) -> Self {
            Self {
                extent,
                atlas: crcbl_ui::FontAtlas::built_in(),
                draw_list: crcbl_ui::draw_list::DrawList::new(),
                had_menu: false,
                frames: 0,
                timings: None,
                counters: crcbl_render::FrameCounters::default(),
            }
        }
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
            gpu: GpuOptions,
        ) -> Result<Self::Pending, GpuError> {
            REQUESTS.with(|n| n.set(n.get() + 1));
            REQUESTED.with(|options| options.set(gpu));
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
            Ok(Some(FakeGpu::at(pending.extent)))
        }
    }

    impl GpuSurface for FakeGpu {
        fn extent(&self) -> (u32, u32) {
            self.extent
        }

        fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
            self.extent = extent;
            Ok(())
        }
    }

    /// The frame half, so the same fake can be booted *and* hosted.
    ///
    /// A device would answer none of what the loop's tests ask — whether a menu
    /// was handed over, what geometry the frame ended with — and would refuse to
    /// open at all on a machine with no GPU, which is every CI runner the
    /// headless jobs use.
    impl GameGpu for FakeGpu {
        fn atlas(&self) -> &crcbl_ui::FontAtlas {
            &self.atlas
        }

        fn set_menu(&mut self, menu: Option<(&crcbl_ui::menu::Menu, &crcbl_ui::menu::MenuLayout)>) {
            self.had_menu = menu.is_some();
        }

        fn take_draw_list(&mut self, list: &mut crcbl_ui::draw_list::DrawList) {
            std::mem::swap(&mut self.draw_list, list);
        }

        fn timings(&self) -> Option<&crcbl_render::FrameTimings> {
            self.timings.as_ref()
        }

        fn counters(&self) -> crcbl_render::FrameCounters {
            self.counters
        }

        fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
            self.frames += 1;
            // One draw for the whole list, one instance per command in it, and
            // two triangles a command — a stand-in for the real renderers'
            // arithmetic whose only job is to be a function of what the frame
            // was actually handed.
            let commands = self.draw_list.len() as u64;
            self.counters = crcbl_render::FrameCounters {
                draws: u64::from(!self.draw_list.is_empty()),
                instances: commands,
                drawn: Some(commands),
                triangles: Some(commands * 2),
                clusters: Some(0),
                cull_frame: None,
            };
            Ok(FrameOutcome::Presented)
        }

        fn destroy(self) -> Result<(), GpuError> {
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

        let asked_for = GpuOptions {
            backend: Some(GpuBackend::Null),
            pacing: Pacing::Off,
        };
        let mut boot: PolledBoot<crcbl_shell::HeadlessShell, FakeGpu> =
            PolledBoot::request(Box::new(shell), window, Clock::new(true), asked_for);

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
        // And asked for what the caller said. The polled path is the browser's,
        // and it is the one where a dropped field would never be noticed: no CLI
        // reaches it, so `Auto` is what it would carry either way.
        assert_eq!(
            REQUESTED.with(std::cell::Cell::get),
            asked_for,
            "start-up requested a device the caller did not ask for",
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
        let mut boot: PolledBoot<crcbl_shell::HeadlessShell, FakeGpu> = PolledBoot::request(
            Box::new(shell),
            window,
            Clock::new(true),
            GpuOptions::default(),
        );

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
        let mut boot: PolledBoot<crcbl_shell::HeadlessShell, FakeGpu> = PolledBoot::request(
            Box::new(shell),
            window,
            Clock::new(true),
            GpuOptions::default(),
        );

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
        assert_eq!(
            ModeRequest::mode(&shell, window),
            Some(DisplayMode::Windowed)
        );

        ModeRequest::toggle(&mut shell, window).expect("the window is live");
        assert!(
            ModeRequest::mode(&shell, window)
                .expect("the window is live")
                .is_borderless(),
            "the first toggle must ask for borderless",
        );

        ModeRequest::toggle(&mut shell, window).expect("the window is live");
        assert_eq!(
            ModeRequest::mode(&shell, window),
            Some(DisplayMode::Windowed),
            "the second must come back",
        );

        ModeRequest::toggle(&mut shell, window).expect("the window is live");
        assert!(
            ModeRequest::mode(&shell, window)
                .expect("the window is live")
                .is_borderless(),
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
            Some(DisplayMode::Windowed),
            "mode() reported the request rather than what the window actually is",
        );

        // Latched: checking again must not flip it back, or the loop logs the
        // same refusal on every frame forever.
        request.check(&shell, window);
        assert!(!request.honoured);
    }

    /// **A run that ended fullscreen says so, even though closing the window is
    /// what ended it.**
    ///
    /// Accepting a close request destroys the window, and the summary is built
    /// afterwards — so [`ModeRequest::mode`] has nothing left to read and
    /// answers `None`, which is the honest shape and not a summary: a caller
    /// that unwrapped it into `Windowed` would say the same words a genuinely
    /// windowed run uses. That is every session a player ended the ordinary way:
    /// the whole run borderless, the summary line saying windowed, and nothing
    /// downstream able to tell it from the truth.
    #[test]
    fn the_mode_a_finished_run_reports_survives_the_window_it_was_read_from() {
        let (mut shell, window) = shell();
        shell
            .resize(window, crcbl_shell::PhysicalSize::new(320, 240))
            .expect("the window is live");

        let mut request = ModeRequest::new();
        ModeRequest::toggle(&mut shell, window).expect("the shell accepts the request");
        // `HeadlessShell` delivers the fullscreen configure a few pumps later,
        // exactly as a compositor does.
        for _ in 0..8 {
            shell.pump(&mut |_| {});
            request.check(&shell, window);
        }
        assert!(
            request.honoured(),
            "the window never went borderless, so the rest of this proves nothing"
        );
        assert!(
            ModeRequest::mode(&shell, window)
                .expect("the window is live")
                .is_borderless()
        );

        // What accepting a close request does, one layer down.
        shell.destroy_window(window).expect("the window is live");
        assert!(
            ModeRequest::mode(&shell, window).is_none(),
            "a dead window must not read as an invented Windowed"
        );
        assert!(
            request.mode_at_exit(&shell, window).is_borderless(),
            "the summary would have called a fullscreen session windowed"
        );
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

    /// **A menu key pressed before the menu opened is released to the game,
    /// not only from the held list.**
    ///
    /// Clearing the list alone is not the fix it looks like: the list is only
    /// read on focus loss, so a game told about the press and never about the
    /// release goes on steering. Holding Down through a level-up and picking an
    /// upgrade left the horde's wizard walking south with nothing pressed —
    /// the menu's early return dropped the release on the floor.
    #[test]
    fn a_menu_key_held_when_the_menu_opens_is_released_to_the_game() {
        let (mut shell, window) = shell();
        let mut set = menus();
        let mut held = Vec::new();

        // Pressed while no menu is up: forwarded, and tracked as held.
        shell.key_press(window, MENU_UP_KEY).expect("live");
        let mut pump = MenuPump::new(&mut set, &mut held, false);
        let mut forwarded = Vec::new();
        shell.pump(&mut |event| {
            if let Some(key) = pump.observe(&event) {
                forwarded.push(key);
            }
        });
        assert_eq!(forwarded, vec![(MENU_UP_KEY, true)]);
        assert_eq!(held, vec![MENU_UP_KEY]);

        // Released while the menu is up: the menu is claiming that key, and the
        // release reaches the game anyway.
        shell.key_release(window, MENU_UP_KEY).expect("live");
        let mut pump = MenuPump::new(&mut set, &mut held, true);
        let mut forwarded = Vec::new();
        shell.pump(&mut |event| {
            if let Some(key) = pump.observe(&event) {
                forwarded.push(key);
            }
        });
        assert_eq!(
            forwarded,
            vec![(MENU_UP_KEY, false)],
            "the game was never told the key came up",
        );
        assert!(
            !held.contains(&MENU_UP_KEY),
            "the menu key is still held: {held:?}",
        );
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

    /// **Two contacts arrive as two, and moving one moves only that one.**
    ///
    /// The claim the whole seam exists for, made where it can be checked without
    /// a device: a batch carrying two fingers keeps them apart by id, a move
    /// updates the finger it names, and the finger that did not move produces
    /// nothing.
    #[test]
    fn a_batch_carries_every_contact_and_a_move_names_the_one_that_moved() {
        use crcbl_core::input::{ContactId, TouchPhase};
        let (mut shell, window) = shell();
        let point = |x: f64, y: f64| crcbl_shell::PhysicalPoint { x, y };

        for (contact, phase, at) in [
            (1, TouchPhase::Began, point(100.0, 200.0)),
            (2, TouchPhase::Began, point(500.0, 40.0)),
            (2, TouchPhase::Moved, point(520.0, 60.0)),
        ] {
            shell
                .touch(window, ContactId(contact), phase, at)
                .expect("the window is live");
        }

        let mut pending = Pending::default();
        let verdicts = drain(&mut shell, &mut pending);
        assert!(
            verdicts.iter().all(|verdict| *verdict == Handled::Loop),
            "a contact was handed to the game raw: {verdicts:?}",
        );
        assert_eq!(
            pending.touches,
            vec![
                TouchContact {
                    contact: ContactId(1),
                    phase: TouchPhase::Began,
                    at: glam::Vec2::new(100.0, 200.0),
                },
                TouchContact {
                    contact: ContactId(2),
                    phase: TouchPhase::Began,
                    at: glam::Vec2::new(500.0, 40.0),
                },
                TouchContact {
                    contact: ContactId(2),
                    phase: TouchPhase::Moved,
                    at: glam::Vec2::new(520.0, 60.0),
                },
            ],
        );
        // The finger that held still reported once and is still where it landed
        // — a batch that credited the move to the wrong contact would put it at
        // the other's position.
        let first: Vec<glam::Vec2> = pending
            .touches
            .iter()
            .filter(|touch| touch.contact == ContactId(1))
            .map(|touch| touch.at)
            .collect();
        assert_eq!(first, vec![glam::Vec2::new(100.0, 200.0)]);

        // And a contact is not a pointer: the menu's hover state and the
        // button's edges are the pointer stream's, and a finger must not move
        // either of them on a backend where both arrive.
        assert_eq!(pending.pointer, None);
        assert!(!pending.pointer_pressed && !pending.pointer_released);
    }

    /// **A tap is a press and a release in one batch, and both survive.**
    ///
    /// The case a per-contact fold would lose: on a phone a finger is on the
    /// glass for a fraction of a frame, so `Began` and `Ended` reach the engine
    /// in the same pump. Keeping only each contact's latest state would report a
    /// finger that was never down.
    #[test]
    fn a_tap_inside_one_batch_keeps_both_of_its_phases() {
        use crcbl_core::input::{ContactId, TouchPhase};
        let (mut shell, window) = shell();
        let at = crcbl_shell::PhysicalPoint { x: 8.0, y: 9.0 };
        shell
            .touch(window, ContactId(4), TouchPhase::Began, at)
            .expect("the window is live");
        shell
            .touch(window, ContactId(4), TouchPhase::Ended, at)
            .expect("the window is live");

        let mut pending = Pending::default();
        drain(&mut shell, &mut pending);
        assert_eq!(
            pending
                .touches
                .iter()
                .map(|touch| touch.phase)
                .collect::<Vec<_>>(),
            vec![TouchPhase::Began, TouchPhase::Ended],
        );
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

    /// The rate survives the round trip, and says itself out loud.
    ///
    /// [`FrameLimit`] stores the rate and derives the period precisely so that
    /// the number a log prints is the number that was asked for: recovering 30
    /// from a 33.333333 ms period is a division that rounds, and a run reporting
    /// `29 fps` for a `--fps 30` would be a diagnostic that lies.
    #[test]
    fn a_frame_limit_reports_the_rate_it_was_asked_for() {
        for fps in [1, 30, 60, 144, 1000, FrameLimit::DEFAULT_FPS, u32::MAX] {
            let limit = FrameLimit::fps(fps);
            assert_eq!(limit.rate(), fps);
            assert_eq!(limit.to_string(), format!("{fps} fps"));
        }
        assert_eq!(FrameLimit::unlimited().rate(), 0);
        assert_eq!(FrameLimit::unlimited().to_string(), "unlimited");
        assert_eq!(FrameLimit::fps(0).to_string(), "unlimited");
        assert_eq!(FrameLimit::default().to_string(), "1000 fps");
    }

    /// **The loop applies [`LoopConfig::limit`] to the clock it was handed.**
    ///
    /// The mechanism `--fps` arrives through, and one nothing else can observe:
    /// a [`Loop::new`] that ignored the field would build an identical loop, run
    /// identically in every headless test — where the clock is manual and takes
    /// no limit at all — and only ever be wrong on a machine with a window.
    #[test]
    fn a_hosted_loop_takes_its_frame_limit_from_the_config_and_can_be_changed() {
        let mut shell = crcbl_shell::HeadlessShell::new();
        let window = shell
            .create_window(&crcbl_shell::WindowDesc::default())
            .expect("headless always creates a window");
        let mut engine: Loop<_, FakeGame> = Loop::new(
            Booted {
                shell: Box::new(shell),
                window,
                gpu: FakeGpu::at((640, 480)),
                // A *real* clock, because the limiter lives on that one: a
                // manual clock would report `None` however this went.
                clock_source: Clock::new(false),
                events: 0,
            },
            FakeGame::default(),
            LoopConfig {
                limit: FrameLimit::fps(30),
                ..hosted_config(None)
            },
        );
        assert_eq!(engine.clock_source().limit(), Some(FrameLimit::fps(30)));

        // And the settings-screen half: a game that offers an fps cap changes it
        // while the loop is running, not only when it is built.
        engine.clock_source_mut().set_limit(FrameLimit::unlimited());
        assert_eq!(
            engine.clock_source().limit(),
            Some(FrameLimit::unlimited()),
            "a mid-run change did not reach the clock",
        );
    }

    /// A settings row's change reaches the clock: the loop applies what
    /// [`HostedGame::take_pending_frame_limit`] hands it on the frame the row
    /// fired, and takes the request so a later frame does not re-apply it.
    #[test]
    fn the_loop_applies_a_games_pending_frame_limit_to_its_clock() {
        let mut shell = crcbl_shell::HeadlessShell::new();
        let window = shell
            .create_window(&crcbl_shell::WindowDesc::default())
            .expect("headless always creates a window");
        let mut engine: Loop<_, FakeGame> = Loop::new(
            Booted {
                shell: Box::new(shell),
                window,
                gpu: FakeGpu::at((640, 480)),
                clock_source: Clock::new(false),
                events: 0,
            },
            FakeGame::default(),
            hosted_config(None),
        );
        // The settings screen asks for a new cap on this frame.
        engine.game_mut().pending_limit = Some(FrameLimit::fps(30));
        engine.frame().expect("a frame");
        assert_eq!(
            engine.clock_source().limit(),
            Some(FrameLimit::fps(30)),
            "the taken request did not reach the clock",
        );
        assert_eq!(
            engine.game().pending_limit,
            None,
            "the request was taken, not left for the next frame",
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
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
        for pacing in [Pacing::Auto, Pacing::Vsync, Pacing::Adaptive, Pacing::Off] {
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

    /// The four pacings are four different requests.
    ///
    /// Vsync asks for exactly one mode — it is the one case where a fallback
    /// would silently give the caller the opposite of what they asked for. Auto
    /// asks for the same one, because the swapchain it opens really is a vsync
    /// swapchain; what makes it a different request is the resolution after the
    /// first present, which
    /// `auto_is_the_only_pacing_the_display_can_change` covers.
    #[test]
    fn vsync_asks_for_vsync_and_nothing_else() {
        assert_eq!(Pacing::Vsync.preferences(), &[PresentMode::Fifo]);
        assert_eq!(Pacing::Auto.preferences(), &[PresentMode::Fifo]);
        assert_eq!(Pacing::default(), Pacing::Auto);

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

    /// Every pacing has a name a command line can spell, and nothing else does.
    ///
    /// The `vrr` case is the one worth pinning: it is the word a player knows,
    /// it is **not** the word this enum uses, and a parser that quietly accepted
    /// it would leave `--pacing vrr` meaning `adaptive` here and nothing
    /// anywhere else. Rejected, and `crcbl::args` names the alternative.
    #[test]
    fn every_pacing_has_a_name_and_only_its_own() {
        for (name, pacing) in [
            ("auto", Pacing::Auto),
            ("vsync", Pacing::Vsync),
            ("adaptive", Pacing::Adaptive),
            ("off", Pacing::Off),
        ] {
            assert_eq!(Pacing::from_name(name), Some(pacing));
            assert_eq!(
                Pacing::from_name(&format!(" {} ", name.to_uppercase())),
                Some(pacing),
                "trimmed and case-folded, like --backend",
            );
        }
        for name in ["vrr", "freesync", "g-sync", "fifo", "on", "none", ""] {
            assert_eq!(Pacing::from_name(name), None, "{name} is not a pacing");
        }
    }

    /// One 60 Hz cycle, for the observations below. The figures are arbitrary —
    /// the resolution reads the arm, never the duration — but a plausible one
    /// keeps the failure messages readable.
    const OBSERVED_CYCLE: Duration = Duration::from_nanos(16_666_666);

    /// The quantum a [`DisplayTiming::Stepped`] panel moves in — half the cycle
    /// above, so the pair is the shape the seam's mapping actually produces: a
    /// cycle that is a whole multiple of its step, and not equal to it, which
    /// would have been reported as [`DisplayTiming::Fixed`] instead.
    const OBSERVED_STEP: Duration = Duration::from_nanos(8_333_333);

    /// Every [`DisplayTiming`] arm, for the exhaustive sweeps below.
    ///
    /// Written out rather than derived, so that a fifth arm on the enum is a
    /// compile-time decision here and not a case these tests quietly stop
    /// covering.
    const OBSERVATIONS: [DisplayTiming; 4] = [
        DisplayTiming::Unknown,
        DisplayTiming::Fixed {
            cycle: OBSERVED_CYCLE,
        },
        DisplayTiming::Variable {
            shortest: OBSERVED_CYCLE,
        },
        DisplayTiming::Stepped {
            cycle: OBSERVED_CYCLE,
            step: OBSERVED_STEP,
        },
    ];

    /// Every (requested, observed) pair the resolution can be handed.
    ///
    /// **This test is the only place three of the four `DisplayTiming` arms
    /// execute anywhere in this repo.** Every driver reachable from here
    /// reports `Unknown`, so `Fixed`, `Variable` and `Stepped` reach the
    /// resolution here and nowhere else, on any machine — which is why the
    /// policy is a method with no device in its signature.
    #[test]
    fn auto_is_the_only_pacing_the_display_can_change() {
        for observed in OBSERVATIONS {
            for requested in [Pacing::Vsync, Pacing::Adaptive, Pacing::Off] {
                assert_eq!(
                    requested.resolve(observed),
                    requested,
                    "{requested:?} was asked for by name; {observed:?} must not overrule it"
                );
            }
            assert_ne!(
                Pacing::Auto.resolve(observed),
                Pacing::Auto,
                "{observed:?} left Auto unresolved, so the loop would be pacing on a request"
            );
        }

        // The policy itself, arm by arm, so a change to it has to be written
        // down here as well as in `resolve`.
        assert_eq!(
            Pacing::Auto.resolve(DisplayTiming::Unknown),
            Pacing::Vsync,
            "a display that would not say is the fallback case"
        );
        assert_eq!(
            Pacing::Auto.resolve(DisplayTiming::Fixed {
                cycle: OBSERVED_CYCLE
            }),
            Pacing::Vsync,
            "a fixed cycle is exactly what vsync is for"
        );
        assert_eq!(
            Pacing::Auto.resolve(DisplayTiming::Variable {
                shortest: OBSERVED_CYCLE
            }),
            Pacing::Adaptive,
            "a free-running display is the case adaptive sync exists for"
        );
        assert_eq!(
            Pacing::Auto.resolve(DisplayTiming::Stepped {
                cycle: OBSERVED_CYCLE,
                step: OBSERVED_STEP
            }),
            Pacing::Adaptive,
            "a quantised cycle is not a fixed one, so a fixed-vblank wait is wrong there too"
        );
    }

    /// Which of those resolutions costs a swapchain rebuild, decided from the
    /// surface's own mode list — no device, no display, just the data a
    /// `SurfaceCaps` carries.
    ///
    /// The pairing is the whole point: `Auto` resolving to `Vsync` must leave
    /// the swapchain alone, because that is the case every machine in CI takes
    /// and a rebuild there would be a start-up cost paid by every run for
    /// nothing.
    #[test]
    fn only_a_display_that_is_not_fixed_costs_a_rebuild() {
        use crcbl_hal::{CompositeAlpha, SurfaceCaps};

        let caps = |present_modes: Vec<PresentMode>| SurfaceCaps {
            formats: vec![Format::Bgra8UnormSrgb],
            present_modes,
            composite_alpha: vec![CompositeAlpha::Opaque],
            min_image_count: 2,
            max_image_count: 3,
            current_extent: None,
        };
        let vrr_capable = caps(vec![PresentMode::Fifo, PresentMode::FifoRelaxed]);
        let fifo_only = caps(vec![PresentMode::Fifo]);

        // What a context opened on `Auto` is presenting with when the
        // resolution runs: `Auto`'s preference list, resolved once by the
        // surface. Read from the same call the engine uses rather than written
        // down, so the two cannot drift.
        let opened = vrr_capable.choose_present_mode(Pacing::Auto.preferences());
        assert_eq!(opened, PresentMode::Fifo);

        for observed in OBSERVATIONS {
            let effective = Pacing::Auto.resolve(observed);
            let wanted = vrr_capable.choose_present_mode(effective.preferences());
            let rebuilds = wanted != opened;
            assert_eq!(
                rebuilds,
                matches!(
                    observed,
                    DisplayTiming::Variable { .. } | DisplayTiming::Stepped { .. }
                ),
                "{observed:?} resolved to {effective:?} and wanted {wanted:?}, \
                 which is the wrong side of a swapchain rebuild"
            );
        }

        // The same resolution on a surface that has nothing to rebuild *to*.
        // `choose_present_mode` falls back to Fifo, so the mode does not change
        // and no swapchain is spent finding that out.
        for observed in OBSERVATIONS {
            let effective = Pacing::Auto.resolve(observed);
            assert_eq!(
                fifo_only.choose_present_mode(effective.preferences()),
                fifo_only.choose_present_mode(Pacing::Auto.preferences()),
                "{observed:?} rebuilt a Fifo-only surface to the mode it already had"
            );
        }
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

    /// The start of the frame-limit line [`RealClock::set_limit`] writes — the
    /// prefix `crates/crcbl-shell/tests/run-wayland-e2e.sh` greps for.
    const FRAME_LIMIT_LINE: &str = "engine: the frame limit is ";

    fn is_frame_limit_line(record: &crcbl_core::log::CapturedRecord) -> bool {
        record.message.starts_with(FRAME_LIMIT_LINE)
    }

    /// **What is capping the loop is only ever said here**, and until now the
    /// only thing reading it was an e2e script that needs a Wayland compositor,
    /// so on every other machine the line could have been deleted with the suite
    /// staying green.
    ///
    /// Three claims in one test because they are three parts of one behaviour:
    /// constructing a real clock is not news, setting a limit is, and a manual
    /// clock has no limiter to report. The middle one is also this test's
    /// anti-vacuity anchor — a capture that saw nothing at all would fail it,
    /// which is what keeps the two silences from passing on an empty buffer.
    #[test]
    fn only_a_real_clock_says_what_is_capping_it() {
        let logs = crcbl_core::log::capture();
        let mut clock = Clock::new(false);
        assert!(
            !logs.records().iter().any(is_frame_limit_line),
            "constructing a clock at the default limit is not news: {:?}",
            logs.records()
        );

        clock.set_limit(FrameLimit::fps(30));
        clock.set_limit(FrameLimit::unlimited());
        let records = logs.records();
        let said: Vec<_> = records.iter().filter(|r| is_frame_limit_line(r)).collect();
        assert_eq!(said.len(), 2, "one line per call: {records:?}");
        assert!(said.iter().all(|record| record.level == log::Level::Info));
        assert_eq!(said[0].message, "engine: the frame limit is 30 fps");
        assert_eq!(
            said[1].message, "engine: the frame limit is unlimited",
            "no limit is a limit worth reporting, not a silence",
        );

        // A headless run is stepped by its caller and never waits, so there is
        // nothing to obey and nothing to say.
        let mut headless = Clock::new(true);
        headless.set_limit(FrameLimit::fps(30));
        assert_eq!(headless.limit(), None);
        assert_eq!(
            logs.records()
                .iter()
                .filter(|r| is_frame_limit_line(r))
                .count(),
            said.len(),
            "the manual clock added a line: {:?}",
            logs.records()
        );
    }

    /// The browser's start-up shape, driven on a headless shell with the null
    /// backend: `request_open` never blocks, and polling it produces exactly
    /// the context `open` would have produced.
    ///
    /// **A game's `desc` keeps its own half and takes the run's.**
    ///
    /// The shape every sample's `desc` is written in: the label and the features
    /// are the game's and survive, the backend and the pacing come from the
    /// command line, and the optional features it does *not* name stay the
    /// engine's default rather than being cleared by the update syntax.
    #[test]
    fn a_gpu_options_becomes_the_run_s_half_of_a_desc() {
        let gpu = GpuOptions {
            backend: Some(GpuBackend::Null),
            pacing: Pacing::Off,
        };
        let desc = GpuContextDesc {
            label: "a game",
            ..GpuContextDesc::from(gpu)
        };
        assert_eq!(desc.label, "a game");
        assert_eq!(desc.backend, Some(GpuBackend::Null));
        assert_eq!(desc.pacing, Pacing::Off);
        assert_eq!(
            desc.optional_features,
            GpuContextDesc::default().optional_features,
            "the features a game did not name are still the engine's",
        );

        let defaults = GpuContextDesc::from(GpuOptions::default());
        assert_eq!(defaults.backend, None, "None is 'you pick', and is default");
        assert_eq!(defaults.pacing, Pacing::Auto);
    }

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

    /// The start of topic 39's downgrade line, exactly as
    /// [`PendingGpuContext::poll`] writes it.
    const DOWNGRADE_LINE: &str = "hal: this device does not have";

    /// Opens a context on a null instance the caller picked, which the registry
    /// cannot do: `crcbl::backend`'s null entry is always the `gpu_driven`
    /// preset, and a downgrade needs a device that grants less than that.
    ///
    /// The shell comes back with the context because it owns the window the
    /// surface was made for, and `create_surface`'s contract has that window
    /// outliving the surface.
    fn open_null_context(
        instance: crcbl_hal::null::NullInstance,
        optional_features: Features,
        pacing: Pacing,
    ) -> (crcbl_shell::HeadlessShell, GpuContext) {
        let (shell, window) = shell();
        let extent = (320, 240);
        let target = shell
            .surface_target(window)
            .expect("the headless window is still alive");
        let stage = GpuContext::start_device(
            Box::new(instance),
            &target,
            extent,
            "downgrade test",
            Features::empty(),
            optional_features,
            pacing,
        )
        .expect("the null backend opens everywhere");
        let mut pending = PendingGpuContext {
            stage,
            target,
            extent,
            label: "downgrade test".to_string(),
            required_features: Features::empty(),
            optional_features,
            pacing,
        };
        let gpu = loop {
            if let Some(context) = pending.poll().expect("the null backend cannot fail here") {
                break context;
            }
        };
        (shell, gpu)
    }

    /// **The downgrade line is an assertion target, not decoration.**
    ///
    /// `docs/plan/39-capabilities.md` makes it the engine's only evidence that a
    /// device refused an optional feature. Nothing but this test reads it, so
    /// without it a refactor could delete the `log::info!` in
    /// [`PendingGpuContext::poll`] and leave the suite green.
    #[test]
    fn a_device_that_grants_less_says_so() {
        use crcbl_hal::null::NullInstance;

        let logs = crcbl_core::log::capture();
        let (_shell, mut gpu) = open_null_context(
            // The WebGPU-shaped floor, asked for the set every game gets by
            // default: it has neither bindless nor a GPU-side draw count.
            NullInstance::portable(),
            GpuContextDesc::default().optional_features,
            Pacing::Off,
        );

        let records = logs.records();
        let said: Vec<_> = records
            .iter()
            .filter(|record| record.message.starts_with(DOWNGRADE_LINE))
            .collect();
        assert_eq!(said.len(), 1, "once, at device creation: {records:?}");
        assert_eq!(said[0].level, log::Level::Info);
        assert!(
            said[0].message.contains("DESCRIPTOR_INDEXING -> binding"),
            "the line names the feature and the selector its absence moved: {}",
            said[0].message
        );

        gpu.drain().expect("nothing was submitted");
        gpu.destroy()
            .expect("teardown is in the seam's stated order");
    }

    /// The silence is the other half of the claim and the easier half to lose: a
    /// line logged unconditionally reads as "downgraded to nothing" on every
    /// device that granted the lot, which is most of them.
    #[test]
    fn a_device_that_grants_everything_says_nothing() {
        use crcbl_hal::null::NullInstance;

        let logs = crcbl_core::log::capture();
        let (_shell, mut gpu) = open_null_context(
            NullInstance::gpu_driven(),
            // Every flag here is in that preset's caps. The engine's own default
            // set is deliberately not used: it also asks for `PRESENT_FEEDBACK`
            // and `PRESENT_TIMING`, which this preset does not have, and their
            // absence is a real downgrade.
            Features::GPU_DRIVEN | Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
            Pacing::Off,
        );

        let records = logs.records();
        assert!(
            records
                .iter()
                .any(|record| record.message.contains("null adapter")),
            "the capture is live — this is the adapter line every open logs, and \
             without it the assertion below would pass on an empty buffer: {records:?}"
        );
        assert!(
            !records
                .iter()
                .any(|record| record.message.starts_with(DOWNGRADE_LINE)),
            "nothing was lost, so nothing is said: {records:?}"
        );

        gpu.drain().expect("nothing was submitted");
        gpu.destroy()
            .expect("teardown is in the seam's stated order");
    }

    /// The two halves of the present-feedback line [`GpuContext::finish`] logs,
    /// exactly as it writes them.
    const FEEDBACK_LINE: &str = "hal: pacing on presents";
    const NO_FEEDBACK_LINE: &str = "hal: no present feedback";

    /// **Start-up says which of the two pacing stories this run gets.**
    ///
    /// `docs/backlog.md` records that a richer `wait_until_presented` return —
    /// one distinguishing "waited" from "this device cannot observe presents" —
    /// was declined *because* `caps().features` answers it once here, which is
    /// what lets `acquire` call the wait with no branch on which backend is
    /// underneath. Delete this line and the argument for that decision has no
    /// evidence left.
    ///
    /// No `NullInstance` preset advertises [`Features::PRESENT_FEEDBACK`] —
    /// there is no display under that backend, so `wait_until_presented` returns
    /// at once and the preset is honest about it — so the adapter is built by
    /// hand. Nothing here waits on a present: the line reads the device's caps
    /// and that is the whole of what this exercises.
    #[test]
    fn a_device_that_can_observe_presents_says_so() {
        use crcbl_hal::null::NullInstance;
        use crcbl_hal::{DeviceCaps, Limits};

        let logs = crcbl_core::log::capture();
        let (_shell, mut gpu) = open_null_context(
            NullInstance::new(DeviceCaps {
                features: Features::PRESENT_FEEDBACK,
                limits: Limits::minimum(),
            }),
            Features::PRESENT_FEEDBACK,
            Pacing::Off,
        );

        let records = logs.records();
        let said: Vec<_> = records
            .iter()
            .filter(|record| record.message.starts_with(FEEDBACK_LINE))
            .collect();
        assert_eq!(said.len(), 1, "once, at start-up: {records:?}");
        assert_eq!(said[0].level, log::Level::Info);
        assert_eq!(
            said[0].message,
            format!("hal: pacing on presents, {FRAMES_IN_FLIGHT} frames deep"),
            "the line names the depth the loop actually runs at",
        );
        assert!(
            !records
                .iter()
                .any(|record| record.message.starts_with(NO_FEEDBACK_LINE)),
            "the other story is not this run's: {records:?}"
        );

        gpu.drain().expect("nothing was submitted");
        gpu.destroy()
            .expect("teardown is in the seam's stated order");
    }

    /// The other state, and the one every device in this repo is actually in:
    /// the engine asked for present feedback and the device did not have it, so
    /// the frame limiter is all that paces the loop.
    #[test]
    fn a_device_that_cannot_observe_presents_says_that_instead() {
        use crcbl_hal::null::NullInstance;

        let logs = crcbl_core::log::capture();
        let (_shell, mut gpu) = open_null_context(
            NullInstance::gpu_driven(),
            // The set every game gets by default, which asks for
            // `PRESENT_FEEDBACK`; this preset does not have it.
            GpuContextDesc::default().optional_features,
            Pacing::Off,
        );

        let records = logs.records();
        let said: Vec<_> = records
            .iter()
            .filter(|record| record.message.starts_with(NO_FEEDBACK_LINE))
            .collect();
        assert_eq!(said.len(), 1, "once, at start-up: {records:?}");
        assert_eq!(said[0].level, log::Level::Debug);
        assert_eq!(
            said[0].message,
            "hal: no present feedback; the frame limiter is the only pacing",
        );
        assert!(
            !records
                .iter()
                .any(|record| record.message.starts_with(FEEDBACK_LINE)),
            "a line logged on both paths would report every device as pacing on \
             presents: {records:?}"
        );

        gpu.drain().expect("nothing was submitted");
        gpu.destroy()
            .expect("teardown is in the seam's stated order");
    }

    /// The prefix `crates/crcbl-shell/tests/run-wayland-e2e.sh` greps for, and
    /// the start of the line `GpuContext::settle_pacing` writes.
    const PACING_LINE: &str = "hal: display timing ";

    /// **The engine says which pair it resolved, not just what it landed on.**
    ///
    /// `Pacing::resolve`'s own test walks all sixteen (request, observation)
    /// pairs; what nothing read until now is that a *run* reports the pair it
    /// took. "Asked for `Auto`, display said nothing, running vsync" and "asked
    /// for `Off`" are different runs, and a line naming only the result cannot
    /// tell them apart — so two requests are driven through here and the whole
    /// line asserted for each.
    ///
    /// The observation half is `Unknown` in both, because the null device has no
    /// display to have a cadence and says so; the other three
    /// [`DisplayTiming`] arms need a driver that has never answered anything
    /// else on any machine this repo has run on (see
    /// [`GpuContext::effective_pacing`]).
    #[test]
    fn the_pacing_resolution_says_which_pair_it_took() {
        use crcbl_hal::CommandEncoderDesc;
        use crcbl_hal::null::NullInstance;

        let logs = crcbl_core::log::capture();
        // A present, because the display is only asked after one — see
        // `settle_pacing` for why that order is the only one that can work.
        for pacing in [Pacing::Auto, Pacing::Off] {
            let (_shell, mut gpu) =
                open_null_context(NullInstance::gpu_driven(), Features::empty(), pacing);
            let acquired = gpu.acquire().expect("acquire").expect("no resize happened");
            let encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
                label: Some("pacing test"),
                queue: gpu.queue(),
            });
            let command_buffer = encoder.finish().expect("an empty command buffer");
            assert_eq!(
                gpu.submit_and_present(&acquired, command_buffer)
                    .expect("present"),
                FrameOutcome::Presented,
            );
            gpu.drain().expect("the frame was submitted and presented");
            gpu.destroy()
                .expect("teardown is in the seam's stated order");
        }

        let records = logs.records();
        let said: Vec<_> = records
            .iter()
            .filter(|record| record.message.starts_with(PACING_LINE))
            .collect();
        assert_eq!(said.len(), 2, "once per context: {records:?}");
        assert!(said.iter().all(|record| record.level == log::Level::Info));
        assert_eq!(
            said[0].message, "hal: display timing Unknown; asked for Auto, pacing Vsync",
            "a display that will not say is the vsync fallback, and `Auto` is \
             not the answer it resolved to",
        );
        assert_eq!(
            said[1].message, "hal: display timing Unknown; asked for Off, pacing Off",
            "a concrete request comes back unchanged, and the line still says so",
        );
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

    /// Which present a frame waits for, without a GPU and without spending the
    /// time — the same split as [`FrameLimit::wait_from`].
    #[test]
    fn a_frame_waits_for_the_present_frames_in_flight_behind_it() {
        let depth = FRAMES_IN_FLIGHT as u64;

        // Still filling: the first `FRAMES_IN_FLIGHT` frames have nothing that
        // far back, and present ids start at 1 rather than 0.
        for submitted in 0..depth {
            assert_eq!(
                GpuContext::present_to_wait_for(submitted, Pacing::Vsync),
                None,
                "frame {} has only {submitted} presents behind it",
                submitted + 1
            );
        }

        for submitted in depth..depth + 8 {
            let waited = GpuContext::present_to_wait_for(submitted, Pacing::Vsync)
                .expect("the loop has filled");
            assert_eq!(
                waited,
                submitted + 1 - depth,
                "the frame about to start is {}, so it waits {depth} back",
                submitted + 1
            );
            assert!(
                waited < submitted,
                "waiting on present {submitted}, the one just submitted, would drain the \
                 pipeline to a single frame"
            );
        }
    }

    /// `Pacing::Off` asked not to be paced by the display, and a wait for a
    /// frame to be on screen is exactly that.
    #[test]
    fn pacing_off_waits_for_nothing() {
        for submitted in 0..8 {
            assert_eq!(
                GpuContext::present_to_wait_for(submitted, Pacing::Off),
                None,
                "submitted {submitted}"
            );
            assert!(
                GpuContext::present_to_wait_for(submitted, Pacing::Adaptive).is_some()
                    == GpuContext::present_to_wait_for(submitted, Pacing::Vsync).is_some(),
                "adaptive still follows the display when it can, so it waits when vsync does"
            );
        }
    }

    /// The wiring, read off the device rather than off the engine: every
    /// present is numbered, and every frame past the first `FRAMES_IN_FLIGHT`
    /// waits for one that has already been presented.
    #[test]
    fn the_loop_numbers_its_presents_and_waits_for_the_older_ones() {
        use crcbl_hal::CommandEncoderDesc;
        use crcbl_hal::null::{Event, NullInstance, Recorder};
        use crcbl_shell::{HeadlessShell, WindowDesc};

        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("headless always creates a window");
        let mut shell_events = 0;
        let extent = wait_for_configure(&mut shell, window, &mut shell_events).expect("configured");

        // By hand rather than through the registry, because the point of the
        // test is to hold the recorder the device writes to.
        let recorder = Recorder::new();
        let instance: Box<dyn Instance> =
            Box::new(NullInstance::gpu_driven().with_recorder(recorder.clone()));
        let target = shell
            .surface_target(window)
            .expect("the window is still alive");
        let stage = GpuContext::start_device(
            instance,
            &target,
            extent,
            "present pacing test",
            Features::empty(),
            Features::empty(),
            Pacing::Vsync,
        )
        .expect("the null backend opens everywhere");
        let mut pending = PendingGpuContext {
            stage,
            target,
            extent,
            label: "present pacing test".to_string(),
            required_features: Features::empty(),
            optional_features: Features::empty(),
            pacing: Pacing::Vsync,
        };
        let mut gpu = loop {
            if let Some(context) = pending.poll().expect("the null backend cannot fail here") {
                break context;
            }
        };

        const FRAMES: u64 = 5;
        for _ in 0..FRAMES {
            let acquired = gpu.acquire().expect("acquire").expect("no resize happened");
            let encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
                label: Some("present pacing test"),
                queue: gpu.queue(),
            });
            let command_buffer = encoder.finish().expect("an empty command buffer");
            assert_eq!(
                gpu.submit_and_present(&acquired, command_buffer)
                    .expect("present"),
                FrameOutcome::Presented,
            );
        }

        let events = recorder.events();
        let presented: Vec<Option<u64>> = events
            .iter()
            .filter_map(|event| match event {
                Event::Presented { present_id, .. } => Some(*present_id),
                _ => None,
            })
            .collect();
        assert_eq!(
            presented,
            (1..=FRAMES).map(Some).collect::<Vec<_>>(),
            "every present is numbered, from one, with no gaps"
        );

        let waited: Vec<u64> = events
            .iter()
            .filter_map(|event| match event {
                Event::PresentWaited { present_id, .. } => Some(*present_id),
                _ => None,
            })
            .collect();
        let depth = FRAMES_IN_FLIGHT as u64;
        assert_eq!(
            waited,
            (1..=FRAMES - depth).collect::<Vec<_>>(),
            "the first {depth} frames wait for nothing and the rest wait {depth} back"
        );

        // The same claim again, from the order of the stream rather than from
        // the counts: a wait never names a present that has not happened.
        let mut presented_so_far = 0;
        for event in &events {
            match event {
                Event::Presented { .. } => presented_so_far += 1,
                Event::PresentWaited { present_id, .. } => assert!(
                    *present_id <= presented_so_far,
                    "waited for present {present_id} with only {presented_so_far} presented"
                ),
                _ => {}
            }
        }

        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
    }

    /// The runtime switch, against a backend that records what the swapchain
    /// did: a pacing change rebuilds when — and only when — the present mode
    /// moves.
    ///
    /// # What this cannot see
    ///
    /// That the display is asked **once** is structural (one
    /// `observed_timing.is_some()` guard) and is not observable here: the null
    /// backend answers `DisplayTiming::Unknown` however often it is asked, so a
    /// run that queried every frame would record exactly the same events. What
    /// is observable is the consequence — an `Auto` that resolves to vsync
    /// costs no swapchain, and a later `Auto` settles against the sample
    /// already taken rather than starting again from a request.
    #[test]
    fn a_pacing_switch_rebuilds_the_swapchain_only_when_the_mode_moves() {
        use crcbl_hal::CommandEncoderDesc;
        use crcbl_hal::null::{Event, NullInstance, Recorder};
        use crcbl_shell::{HeadlessShell, WindowDesc};

        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("headless always creates a window");
        let mut shell_events = 0;
        let extent = wait_for_configure(&mut shell, window, &mut shell_events).expect("configured");

        let recorder = Recorder::new();
        let instance: Box<dyn Instance> =
            Box::new(NullInstance::gpu_driven().with_recorder(recorder.clone()));
        let target = shell
            .surface_target(window)
            .expect("the window is still alive");
        let stage = GpuContext::start_device(
            instance,
            &target,
            extent,
            "pacing switch test",
            Features::empty(),
            Features::empty(),
            Pacing::default(),
        )
        .expect("the null backend opens everywhere");
        let mut pending = PendingGpuContext {
            stage,
            target,
            extent,
            label: "pacing switch test".to_string(),
            required_features: Features::empty(),
            optional_features: Features::empty(),
            pacing: Pacing::default(),
        };
        let mut gpu = loop {
            if let Some(context) = pending.poll().expect("the null backend cannot fail here") {
                break context;
            }
        };

        let rebuilds = || {
            recorder
                .events()
                .iter()
                .filter(|event| matches!(event, Event::Reconfigured { .. }))
                .count()
        };

        // Before any present there is no observation, so the request is `Auto`
        // and the answer is the vsync the swapchain was opened on.
        assert_eq!(gpu.pacing(), Pacing::Auto, "the default is Auto");
        assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
        assert_eq!(gpu.config.present_mode, PresentMode::Fifo);

        let present = |gpu: &mut GpuContext| {
            let acquired = gpu.acquire().expect("acquire").expect("no resize happened");
            let encoder = gpu.device().create_command_encoder(&CommandEncoderDesc {
                label: Some("pacing switch test"),
                queue: gpu.queue(),
            });
            let command_buffer = encoder.finish().expect("an empty command buffer");
            assert_eq!(
                gpu.submit_and_present(&acquired, command_buffer)
                    .expect("present"),
                FrameOutcome::Presented,
            );
        };
        for _ in 0..3 {
            present(&mut gpu);
        }

        // The null display says `Unknown`, which is the fallback arm, so the
        // resolution changed nothing and spent nothing.
        assert_eq!(gpu.pacing(), Pacing::Auto, "the request is not rewritten");
        assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
        assert_eq!(
            rebuilds(),
            0,
            "an Auto that resolved to vsync rebuilt the swapchain it was already using"
        );

        // Vsync by name is the mode already presenting: same answer, and a
        // settings screen re-applying it must not cost a swapchain.
        gpu.set_pacing(Pacing::Vsync).expect("switching to vsync");
        assert_eq!(gpu.pacing(), Pacing::Vsync);
        assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
        assert_eq!(rebuilds(), 0, "vsync over vsync rebuilt the swapchain");

        // Off is a different mode on this surface — no `FifoRelaxed`, so
        // `Mailbox` — and that is what a rebuild is for.
        gpu.set_pacing(Pacing::Off).expect("switching to no sync");
        assert_eq!(gpu.effective_pacing(), Pacing::Off);
        assert_eq!(gpu.config.present_mode, PresentMode::Mailbox);
        assert_eq!(rebuilds(), 1, "the mode moved and the swapchain did not");

        gpu.set_pacing(Pacing::Off)
            .expect("switching to no sync again");
        assert_eq!(
            rebuilds(),
            1,
            "re-applying the mode in force rebuilt it again"
        );

        // Back to Auto: it settles against the observation already taken —
        // `Unknown`, so vsync — rather than asking the display a second time.
        gpu.set_pacing(Pacing::Auto)
            .expect("switching back to auto");
        assert_eq!(gpu.pacing(), Pacing::Auto);
        assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
        assert_eq!(gpu.config.present_mode, PresentMode::Fifo);
        assert_eq!(rebuilds(), 2);

        // And the loop still runs on the swapchain all of that rebuilt.
        present(&mut gpu);

        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
    }

    /// The rollback when the swapchain cannot be rebuilt: a settings screen
    /// whose apply fails must be left describing the swapchain that is still
    /// configured, not the pacing that did not take.
    ///
    /// The null backend never fails on its own, so the fault is injected
    /// through the recorder — the same knob a reviewer would use to make the
    /// rollback observable at all.
    #[test]
    fn a_failed_pacing_switch_rolls_back_the_request_the_effective_and_the_mode() {
        use crcbl_hal::null::{Event, NullInstance, Recorder};
        use crcbl_shell::{HeadlessShell, WindowDesc};

        let mut shell = HeadlessShell::new();
        let window = shell
            .create_window(&WindowDesc::default())
            .expect("headless always creates a window");
        let mut shell_events = 0;
        let extent = wait_for_configure(&mut shell, window, &mut shell_events).expect("configured");

        let recorder = Recorder::new();
        let instance: Box<dyn Instance> =
            Box::new(NullInstance::gpu_driven().with_recorder(recorder.clone()));
        let target = shell
            .surface_target(window)
            .expect("the window is still alive");
        let stage = GpuContext::start_device(
            instance,
            &target,
            extent,
            "pacing rollback test",
            Features::empty(),
            Features::empty(),
            Pacing::default(),
        )
        .expect("the null backend opens everywhere");
        let mut pending = PendingGpuContext {
            stage,
            target,
            extent,
            label: "pacing rollback test".to_string(),
            required_features: Features::empty(),
            optional_features: Features::empty(),
            pacing: Pacing::default(),
        };
        let mut gpu = loop {
            if let Some(context) = pending.poll().expect("the null backend cannot fail here") {
                break context;
            }
        };

        // The context is on Auto -> vsync -> Fifo, and Off is a different mode
        // on this surface (no `FifoRelaxed`, so Mailbox), so the switch is the
        // one that has to rebuild — and therefore the one that can fail.
        assert_eq!(gpu.pacing(), Pacing::Auto);
        assert_eq!(gpu.effective_pacing(), Pacing::Vsync);
        assert_eq!(gpu.config.present_mode, PresentMode::Fifo);

        recorder.fail_next_reconfigures(1);
        let error = gpu
            .set_pacing(Pacing::Off)
            .expect_err("the injected fault must surface as an error");

        assert!(
            matches!(
                error,
                GpuError::Surface(SurfaceError::Hal(HalError::OutOfDeviceMemory))
            ),
            "{error}"
        );

        // The request, the effective pacing and the swapchain mode rolled back
        // together, rather than any of them describing the change that failed.
        assert_eq!(gpu.pacing(), Pacing::Auto, "the request is not rewritten");
        assert_eq!(
            gpu.effective_pacing(),
            Pacing::Vsync,
            "the effective pacing does not describe a swapchain that does not exist"
        );
        assert_eq!(
            gpu.config.present_mode,
            PresentMode::Fifo,
            "the swapchain is still the one that was configured"
        );
        assert_eq!(
            recorder
                .events()
                .iter()
                .filter(|event| matches!(event, Event::Reconfigured { .. }))
                .count(),
            0,
            "a failed reconfigure must not record itself as a success"
        );

        // The fault was consumed, not latched: the same switch succeeds now,
        // and the context stayed usable throughout.
        gpu.set_pacing(Pacing::Off)
            .expect("the fault was injected once, not forever");
        assert_eq!(gpu.effective_pacing(), Pacing::Off);
        assert_eq!(gpu.config.present_mode, PresentMode::Mailbox);

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
            Box::new(NullInstance::gpu_driven().with_recorder(recorder.clone()));
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

    // ---- resize and device loss, injected -----------------------------------

    /// A real [`GpuContext`] on the null backend, and the recorder that decides
    /// when its device misbehaves.
    ///
    /// By hand rather than through the registry for the reason the tests above
    /// spell out one by one: the point is to *hold* the recorder. The window is
    /// handed back with it because it outlives the context and still has to be
    /// destroyed.
    fn null_context(
        label: &str,
        pacing: Pacing,
    ) -> (
        crcbl_shell::HeadlessShell,
        WindowId,
        crcbl_hal::null::Recorder,
        GpuContext,
    ) {
        use crcbl_hal::null::{NullInstance, Recorder};

        let (mut shell, window) = shell();
        let mut shell_events = 0;
        let extent = wait_for_configure(&mut shell, window, &mut shell_events).expect("configured");

        let recorder = Recorder::new();
        let instance: Box<dyn Instance> =
            Box::new(NullInstance::gpu_driven().with_recorder(recorder.clone()));
        let target = shell
            .surface_target(window)
            .expect("the window is still alive");
        let stage = GpuContext::start_device(
            instance,
            &target,
            extent,
            label,
            Features::empty(),
            Features::empty(),
            pacing,
        )
        .expect("the null backend opens everywhere");
        let mut pending = PendingGpuContext {
            stage,
            target,
            extent,
            label: label.to_string(),
            required_features: Features::empty(),
            optional_features: Features::empty(),
            pacing,
        };
        let gpu = loop {
            if let Some(context) = pending.poll().expect("the null backend cannot fail here") {
                break context;
            }
        };
        (shell, window, recorder, gpu)
    }

    /// One frame on a null context, shaped like every sample's `draw`: acquire,
    /// an empty command buffer, submit and present — with `Ok(None)` from
    /// `acquire` reported as [`FrameOutcome::Reconfigured`], which is what
    /// `apps/bare` and its four siblings all do with it.
    fn null_frame(gpu: &mut GpuContext) -> Result<FrameOutcome, GpuError> {
        let Some(acquired) = gpu.acquire()? else {
            return Ok(FrameOutcome::Reconfigured);
        };
        let encoder = gpu
            .device()
            .create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                label: Some("null frame"),
                queue: gpu.queue(),
            });
        let command_buffer = encoder.finish()?;
        gpu.submit_and_present(&acquired, command_buffer)
    }

    /// The start of the line [`GpuContext::reconfigure`] writes.
    ///
    /// A rebuild that *failed* records no event, so the recorder alone cannot
    /// tell an engine that never tried to rebuild from one that tried and was
    /// refused — and on a lost device those are the two candidate policies.
    /// This line is what tells them apart.
    const RECONFIGURE_LINE: &str = "hal: reconfiguring the swapchain to ";

    /// How many swapchain rebuilds the recorder saw.
    fn reconfigures(recorder: &crcbl_hal::null::Recorder) -> usize {
        recorder
            .events()
            .iter()
            .filter(|event| matches!(event, crcbl_hal::null::Event::Reconfigured { .. }))
            .count()
    }

    /// How many presents actually reached the swapchain.
    fn presents(recorder: &crcbl_hal::null::Recorder) -> usize {
        recorder
            .events()
            .iter()
            .filter(|event| matches!(event, crcbl_hal::null::Event::Presented { .. }))
            .count()
    }

    /// **An acquire that reports the swapchain out of date is expected traffic,
    /// not a failed frame**: it rebuilds and hands the frame back as skipped.
    ///
    /// Reachable on a real driver only while someone drags a window edge, which
    /// is why `crcbl-vk` had to carry the only test of it. The recorder's
    /// out-of-date latch is what brings it to a machine with no GPU.
    #[test]
    fn an_out_of_date_acquire_reconfigures_the_swapchain_and_skips_the_frame() {
        let (mut shell, window, recorder, mut gpu) =
            null_context("out-of-date acquire test", Pacing::Vsync);

        // Nothing has been submitted, so no pacing wait is due and `acquire`
        // is the *only* call that can report this. The frame after it is the
        // one that would wait, and that case is its own test below.
        assert_eq!(
            GpuContext::present_to_wait_for(gpu.submitted, gpu.effective_pacing()),
            None,
            "this test is about the acquire, so nothing else must be able to answer first"
        );

        recorder.report_swapchain_out_of_date();
        assert!(
            gpu.acquire()
                .expect("out of date is expected traffic, not an error")
                .is_none(),
            "an out-of-date acquire skips the frame; it does not hand out an image"
        );
        assert_eq!(
            reconfigures(&recorder),
            1,
            "the swapchain is rebuilt once, by the arm that caught it"
        );
        assert_eq!(presents(&recorder), 0, "a skipped frame presents nothing");

        // Handling it is what cleared it, so the next frame is an ordinary one.
        assert_eq!(
            null_frame(&mut gpu).expect("the rebuilt swapchain works"),
            FrameOutcome::Presented
        );
        assert_eq!(
            reconfigures(&recorder),
            1,
            "nothing rebuilt it a second time"
        );
        assert_eq!(presents(&recorder), 1);

        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
    }

    /// **Present is the usual place a resize is noticed**, and the engine says
    /// so in a comment; this is the assertion behind the comment.
    ///
    /// The frame is recorded and submitted before the present refuses it, so
    /// the outcome a loop records is [`FrameOutcome::Reconfigured`] — which is
    /// what [`FrameBudget`] counts against its never-presents cap.
    #[test]
    fn an_out_of_date_present_rebuilds_and_reports_the_frame_as_reconfigured() {
        let (mut shell, window, recorder, mut gpu) =
            null_context("out-of-date present test", Pacing::Vsync);

        let acquired = gpu
            .acquire()
            .expect("acquire")
            .expect("the swapchain is healthy when the frame starts");
        // The resize lands after the image was handed out, which is the case
        // acquire cannot catch.
        recorder.report_swapchain_out_of_date();

        let encoder = gpu
            .device()
            .create_command_encoder(&crcbl_hal::CommandEncoderDesc {
                label: Some("out-of-date present test"),
                queue: gpu.queue(),
            });
        let command_buffer = encoder.finish().expect("an empty command buffer");
        assert_eq!(
            gpu.submit_and_present(&acquired, command_buffer)
                .expect("a resize noticed at present is not a failed frame"),
            FrameOutcome::Reconfigured
        );
        assert_eq!(reconfigures(&recorder), 1);
        assert_eq!(
            presents(&recorder),
            0,
            "the present that reported the swapchain out of date did not present"
        );

        assert_eq!(
            null_frame(&mut gpu).expect("the rebuilt swapchain works"),
            FrameOutcome::Presented
        );
        assert_eq!(reconfigures(&recorder), 1);
        assert_eq!(presents(&recorder), 1);

        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
    }

    /// **The pacing wait's own out-of-date arm**, which is the third and least
    /// obvious of the three: it does nothing, deliberately, and leaves the
    /// acquire behind it to reconfigure.
    ///
    /// Without that arm the wait's error would propagate and a resize would
    /// fail the frame — on a code path that only runs once the pipeline is
    /// `FRAMES_IN_FLIGHT` deep and only on a display that is pacing, which is
    /// why it needed a swapchain that can be made out of date on demand.
    #[test]
    fn an_out_of_date_pacing_wait_leaves_the_acquire_behind_it_to_reconfigure() {
        let (mut shell, window, recorder, mut gpu) =
            null_context("out-of-date wait test", Pacing::Vsync);

        // Fill the pipeline, so the next frame really does wait for an older
        // present rather than skipping the wait the way the first ones do.
        for _ in 0..=FRAMES_IN_FLIGHT {
            assert_eq!(
                null_frame(&mut gpu).expect("a healthy frame"),
                FrameOutcome::Presented
            );
        }
        assert!(
            GpuContext::present_to_wait_for(gpu.submitted, gpu.effective_pacing()).is_some(),
            "the rest of this test asserts nothing unless the next frame is one that waits"
        );
        let waits = recorder
            .events()
            .iter()
            .filter(|event| matches!(event, crcbl_hal::null::Event::PresentWaited { .. }))
            .count();
        assert!(
            waits > 0,
            "and nothing unless the waits are actually reaching the device"
        );

        recorder.report_swapchain_out_of_date();
        assert!(
            gpu.acquire()
                .expect("a wait that reports a resize is not a failed frame")
                .is_none(),
            "the acquire behind it is what reconfigures and skips"
        );
        assert_eq!(reconfigures(&recorder), 1);
        assert_eq!(
            recorder
                .events()
                .iter()
                .filter(|event| matches!(event, crcbl_hal::null::Event::PresentWaited { .. }))
                .count(),
            waits,
            "the refused wait recorded nothing, so the arm ran on a real refusal"
        );

        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
    }

    /// **A lost device surfaces and stays surfaced.** The decision in
    /// `docs/backlog.md` is that the engine does not rebuild its way out of one:
    /// `HalError::DeviceLost` propagates, wearing the presentation vocabulary
    /// the seam gives it, and every later frame says the same thing.
    ///
    /// The contrast with `report_device_error` is the point. That one is
    /// one-shot by contract — the test above asserts that taking it clears it —
    /// so until the recorder could express a *permanent* loss, "this device is
    /// gone and stays gone" had no test on any backend.
    #[test]
    fn a_lost_device_fails_every_frame_after_it_and_rebuilds_nothing() {
        let (mut shell, window, recorder, mut gpu) =
            null_context("device loss test", Pacing::Vsync);

        // The control: the same context, one healthy frame.
        assert_eq!(
            null_frame(&mut gpu).expect("a healthy frame"),
            FrameOutcome::Presented
        );
        let presented = presents(&recorder);
        assert_eq!(presented, 1);

        // A rebuild that *fails* records no event, so the recorder alone cannot
        // tell an engine that never tried to rebuild from one that tried and
        // was refused — and those are the two policies. The log line can, and a
        // real resize first is what proves this capture would have seen one.
        let logs = crcbl_core::log::capture();
        let rebuilds_logged = || {
            logs.records()
                .iter()
                .filter(|record| record.message.starts_with(RECONFIGURE_LINE))
                .count()
        };
        gpu.resize((320, 240))
            .expect("a resize the device can still do");
        assert_eq!(rebuilds_logged(), 1, "{:?}", logs.records());
        let rebuilt = reconfigures(&recorder);
        assert_eq!(rebuilt, 1, "and the rebuild really happened");

        recorder.lose_device("gpu hang: the driver reset the adapter");

        let mut errors: Vec<GpuError> = Vec::new();
        for attempt in 0..3u32 {
            match null_frame(&mut gpu) {
                Ok(outcome) => panic!("frame {attempt} ran on a device that is gone: {outcome:?}"),
                Err(error) => errors.push(error),
            }
        }
        assert_eq!(
            errors.len(),
            3,
            "every frame after the loss failed, not just the first"
        );
        for error in &errors {
            assert!(
                matches!(
                    error,
                    GpuError::Surface(SurfaceError::Hal(HalError::DeviceLost(_)))
                ),
                "{error}"
            );
            assert!(
                error.to_string().contains("gpu hang"),
                "the driver's own words are what a player's log has to carry: {error}"
            );
        }

        assert_eq!(
            reconfigures(&recorder),
            rebuilt,
            "the engine must not rebuild the swapchain out from under a lost device"
        );
        assert_eq!(
            rebuilds_logged(),
            1,
            "and must not so much as attempt one: {:?}",
            logs.records()
        );
        assert_eq!(
            presents(&recorder),
            presented,
            "and nothing reached the screen after the loss"
        );

        // Teardown reports it too rather than pretending the wait succeeded.
        let teardown = gpu
            .destroy()
            .expect_err("waiting for a dead device to go idle cannot succeed");
        assert!(
            matches!(teardown, GpuError::Hal(HalError::DeviceLost(_))),
            "{teardown}"
        );
        shell.destroy_window(window).expect("the window goes away");
    }

    /// A loop with nothing in it but the device, so [`drive`] stops for one
    /// reason and there is only one reason it can be.
    struct DeviceOnlyLoop {
        gpu: GpuContext,
        /// Frames actually attempted, shared with the test because `drive`
        /// consumes the loop and a failed run never reaches `finish`.
        frames: std::rc::Rc<std::cell::Cell<u64>>,
        stop_after: u64,
    }

    impl GameLoop for DeviceOnlyLoop {
        type Error = GpuError;
        type Summary = u64;

        fn frame(&mut self) -> Result<Flow, GpuError> {
            if self.frames.get() >= self.stop_after {
                return Ok(Flow::Stop(ExitReason::FrameBudget));
            }
            self.frames.set(self.frames.get() + 1);
            null_frame(&mut self.gpu)?;
            Ok(Flow::Continue)
        }

        fn finish(self, _exit: ExitReason) -> Result<u64, GpuError> {
            let frames = self.frames.get();
            self.gpu.destroy()?;
            Ok(frames)
        }
    }

    /// **The loop stops on a lost device — it does not spin, retry or heal.**
    ///
    /// The end-to-end half of the decision above, through the native driver
    /// rather than through one `GpuContext` call: the run ends after the frame
    /// that hit the loss, with the driver's message, and with a budget it never
    /// spent. The healthy run beside it is what proves the count could have
    /// gone higher.
    #[test]
    fn a_lost_device_stops_the_driven_loop_with_an_error_naming_it() {
        const BUDGET: u64 = 4;

        let (mut shell, window, _recorder, gpu) = null_context("driven control", Pacing::Vsync);
        let frames = std::rc::Rc::new(std::cell::Cell::new(0));
        let ran = drive(DeviceOnlyLoop {
            gpu,
            frames: std::rc::Rc::clone(&frames),
            stop_after: BUDGET,
        })
        .expect("a healthy null device runs every frame it is given");
        assert_eq!(
            ran, BUDGET,
            "the fixture is able to run more than one frame"
        );
        assert_eq!(frames.get(), BUDGET);
        shell.destroy_window(window).expect("the window goes away");

        let (mut shell, window, recorder, gpu) = null_context("driven loss", Pacing::Vsync);
        recorder.lose_device("gpu hang: the driver reset the adapter");
        let frames = std::rc::Rc::new(std::cell::Cell::new(0));
        let error = drive(DeviceOnlyLoop {
            gpu,
            frames: std::rc::Rc::clone(&frames),
            stop_after: BUDGET,
        })
        .expect_err("a lost device is not something a loop drives through");
        assert!(
            matches!(
                error,
                GpuError::Surface(SurfaceError::Hal(HalError::DeviceLost(_)))
            ),
            "{error}"
        );
        assert!(
            error.to_string().contains("gpu hang"),
            "the loop stops with an error naming the loss: {error}"
        );
        assert_eq!(
            frames.get(),
            1,
            "it stopped on the frame that hit the loss, rather than retrying the other {}",
            BUDGET - 1
        );
        assert_eq!(
            reconfigures(&recorder),
            0,
            "and rebuilt nothing on the way out"
        );
        shell.destroy_window(window).expect("the window goes away");
    }

    // ---- the engine-owned loop ---------------------------------------------

    /// Which menu the fixture game shows.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    enum FakeMenu {
        /// Being played: no menu at all, and so no entry in the set.
        #[default]
        None,
        /// Not served yet.
        Start,
        /// The loop has stopped advancing the simulation.
        Paused,
    }

    /// The widget carrying [`Serve::Launch`], numbered where a game's ids start.
    const SERVE_ID: crcbl_ui::WidgetId = FIRST_GAME_ID;

    /// The key `Serve::Launch` fires, so the button reaches the simulation the
    /// same way the keyboard does rather than reaching past it.
    const SERVE_KEY: crcbl_core::input::KeyCode = crcbl_core::input::KeyCode::Space;

    /// A game with no simulation in it, which records what the loop asked of it.
    #[derive(Debug, Default)]
    struct FakeGame {
        ticks: u64,
        /// Every key the loop forwarded, in order.
        keys: Vec<(crcbl_core::input::KeyCode, bool)>,
        /// Every contact the loop forwarded, in order.
        touches: Vec<TouchUpdate>,
        /// What each `draw` was told about its frame.
        draws: Vec<FrameInfo>,
        /// Whether the ball has been served.
        served: bool,
        /// A frame limit a settings screen asked for, taken by the loop.
        pending_limit: Option<FrameLimit>,
        /// A pause an on-screen control asked for, taken by the loop.
        pending_pause: bool,
    }

    /// This game's own summary: the shared half, plus a count only it kept.
    #[derive(Debug, PartialEq, Eq)]
    struct FakeSummary {
        run: RunSummary,
        /// The game's own tally, which must agree with `run.ticks`.
        ticks_the_game_counted: u64,
    }

    impl HostedGame for FakeGame {
        type Error = core::convert::Infallible;
        type Gpu = FakeGpu;
        type MenuKind = FakeMenu;
        type MenuAction = Serve;
        type Summary = FakeSummary;

        const NAME: &'static str = "fake";

        fn menus() -> crcbl_ui::menu::MenuSet<FakeMenu> {
            use crcbl_ui::menu::{Menu, MenuItem, MenuSet};
            MenuSet::new(
                FakeMenu::None,
                vec![
                    (
                        FakeMenu::Start,
                        Menu::new("FAKE", vec![MenuItem::new(SERVE_ID, "PLAY", "SPACE")]),
                    ),
                    (
                        FakeMenu::Paused,
                        Menu::new("PAUSED", vec![MenuItem::new(RESUME_ID, "RESUME", "ESC")]),
                    ),
                ],
            )
        }

        fn tick(&mut self, _gpu: &mut FakeGpu, _tick_dt: f64) {
            self.ticks += 1;
        }

        fn key_event(&mut self, key: crcbl_core::input::KeyCode, pressed: bool) {
            if key == SERVE_KEY && pressed {
                self.served = true;
            }
            self.keys.push((key, pressed));
        }

        fn touch_event(&mut self, touch: TouchUpdate) {
            self.touches.push(touch);
        }

        /// Asserts rather than ignores: the loop promises never to ask about an
        /// id it owns, and a game that answered one would be re-pointing the
        /// resume button at itself.
        fn menu_action(id: crcbl_ui::WidgetId) -> Option<Serve> {
            assert!(
                id >= FIRST_GAME_ID,
                "the loop asked the game about the reserved id {id}",
            );
            (id == SERVE_ID).then_some(Serve::Launch)
        }

        fn apply(&mut self, action: Serve) {
            match action {
                // Press and release together, because serving is an *edge*: a
                // press with no release leaves the action held for the run.
                Serve::Launch => {
                    self.key_event(SERVE_KEY, true);
                    self.key_event(SERVE_KEY, false);
                }
            }
        }

        fn menu_kind(
            &mut self,
            _menus: &mut crcbl_ui::menu::MenuSet<FakeMenu>,
            paused: bool,
        ) -> FakeMenu {
            if paused {
                FakeMenu::Paused
            } else if self.served {
                FakeMenu::None
            } else {
                FakeMenu::Start
            }
        }

        fn draw(
            &mut self,
            _gpu: &mut FakeGpu,
            draw_list: &mut crcbl_ui::draw_list::DrawList,
            frame: FrameInfo,
        ) {
            self.draws.push(frame);
            // Stands in for a HUD, so a test can tell the game's geometry from
            // the menu's.
            draw_list.rect(
                glam::Vec2::ZERO,
                glam::Vec2::new(8.0, 8.0),
                [1.0, 1.0, 1.0, 1.0],
            );
        }

        fn take_pending_frame_limit(&mut self) -> Option<FrameLimit> {
            self.pending_limit.take()
        }

        fn take_pending_pause(&mut self) -> bool {
            std::mem::take(&mut self.pending_pause)
        }

        fn summary(&self, run: RunSummary) -> FakeSummary {
            FakeSummary {
                run,
                ticks_the_game_counted: self.ticks,
            }
        }

        fn log_summary(summary: &FakeSummary) {
            log::info!("fake: {} frames", summary.run.frames);
        }
    }

    /// What a headless test wants: no idling, and a budget only when asked for.
    const fn hosted_config(frames: Option<u64>) -> LoopConfig {
        LoopConfig {
            tick_hz: 60,
            frames,
            debug_overlay: false,
            windowed: false,
            limit: FrameLimit::fps(FrameLimit::DEFAULT_FPS),
        }
    }

    /// A loop hosting [`FakeGame`] on a headless shell.
    ///
    /// [`Booted`] is built by hand rather than through [`PolledBoot`] because
    /// what these tests are about is the frame, not the handshake — the boot
    /// tests above own that.
    fn hosted(frames: Option<u64>) -> Loop<crcbl_shell::HeadlessShell, FakeGame> {
        let mut shell = crcbl_shell::HeadlessShell::new();
        let window = shell
            .create_window(&crcbl_shell::WindowDesc::default())
            .expect("headless always creates a window");
        Loop::new(
            Booted {
                shell: Box::new(shell),
                window,
                gpu: FakeGpu::at((640, 480)),
                clock_source: Clock::new(true),
                events: 0,
            },
            FakeGame::default(),
            hosted_config(frames),
        )
    }

    // -- the frame's spans ---------------------------------------------------

    /// Runs `body` in a process of its own, with the trace on.
    ///
    /// **A mutex would not be enough here.** The gate, the per-thread buffers
    /// and [`crcbl_core::trace::drain`] are all process-wide, `cargo test` runs
    /// this binary's tests as threads, and [`Loop::frame`] now drains — so any
    /// other test's frame, running concurrently while the gate is on, would take
    /// this one's records with it. Serialising the tests that *turn the gate on*
    /// does not fix that: the thief is a test that never asked about the trace at
    /// all. A child process is the only isolation that actually holds, and it
    /// turns the gate on the way a user does, through `CRCBL_TRACE`.
    ///
    /// `name` is this test's full path. It is checked rather than trusted — see
    /// the assertion on the child's report.
    fn in_a_traced_process(name: &str, body: impl FnOnce()) {
        if std::env::var(crcbl_core::trace::ENV_VAR).is_ok() {
            assert!(
                crcbl_core::trace::init_from_env(),
                "the child is launched with the trace on"
            );
            drop(crcbl_core::trace::drain());
            body();
            return;
        }

        let output = std::process::Command::new(
            std::env::current_exe().expect("a test binary knows its own path"),
        )
        .args(["--exact", name])
        .env(crcbl_core::trace::ENV_VAR, "1")
        .output()
        .expect("re-running this test binary");
        let report = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "the traced child failed:\n{report}"
        );
        // `--exact` with a name that matches nothing exits zero, so without this
        // a rename would leave the whole test passing on nothing at all.
        assert!(
            report.contains("1 passed"),
            "the child ran no test — is {name:?} still this test's path?\n{report}"
        );
    }

    /// A record reduced to what these assertions are written against.
    fn span_shapes(
        snapshot: &crcbl_core::trace::Snapshot,
    ) -> Vec<(&'static str, crcbl_core::trace::RecordKind, u16)> {
        snapshot
            .threads
            .iter()
            .flat_map(|thread| &thread.records)
            .map(|record| (record.name, record.kind, record.depth))
            .collect()
    }

    /// **The frame's phases, by name and by depth.**
    ///
    /// Against [`Loop::frame_body`] rather than [`Loop::frame`], because `frame`
    /// drains what it recorded — which is the next test. The tree here is the one
    /// `crate::perf`'s module docs draw, minus `present-wait`, which belongs to
    /// [`GpuContext`] and has a test of its own: [`FakeGpu`] presents without a
    /// swapchain to wait on.
    #[test]
    fn a_traced_frame_records_the_loops_phases_in_order_and_at_their_depths() {
        in_a_traced_process(
            "engine::tests::a_traced_frame_records_the_loops_phases_in_order_and_at_their_depths",
            || {
                use crcbl_core::trace::RecordKind::{SpanBegin, SpanEnd};

                let mut engine = hosted(None);
                assert_eq!(
                    engine.frame_body().expect("the fake never fails"),
                    Flow::Continue
                );

                let snapshot = crcbl_core::trace::drain();
                assert_eq!(snapshot.dropped(), 0, "{}", snapshot.report());
                // Spans only. The frame also samples its counters, whose values
                // are the GPU bundle's rather than the loop's — where they sit
                // and what they hold is
                // `a_traced_frame_samples_the_counters_it_has_and_omits_the_ones_it_does_not`.
                let spans: Vec<_> = span_shapes(&snapshot)
                    .into_iter()
                    .filter(|(_, kind, _)| {
                        !matches!(kind, crcbl_core::trace::RecordKind::Counter(_))
                    })
                    .collect();
                assert_eq!(
                    spans,
                    vec![
                        (crate::perf::FRAME_SPAN, SpanBegin, 0),
                        (crate::perf::INPUT_SPAN, SpanBegin, 1),
                        (crate::perf::INPUT_SPAN, SpanEnd, 1),
                        (crate::perf::PACE_SPAN, SpanBegin, 1),
                        (crate::perf::PACE_SPAN, SpanEnd, 1),
                        (crate::perf::TICK_SPAN, SpanBegin, 1),
                        (crate::perf::TICK_SPAN, SpanEnd, 1),
                        (crate::perf::DRAW_SPAN, SpanBegin, 1),
                        (crate::perf::DRAW_SPAN, SpanEnd, 1),
                        (crate::perf::PRESENT_SPAN, SpanBegin, 1),
                        (crate::perf::PRESENT_SPAN, SpanEnd, 1),
                        (crate::perf::FRAME_SPAN, SpanEnd, 0),
                    ],
                );
            },
        );
    }

    /// **Nothing is open across the frame's drain.**
    ///
    /// The failure this catches is a drain taken *inside* the frame span: the
    /// span would be split, every snapshot would hold one frame's end and the
    /// next one's begin, and `frame_cpu_time` would never find a whole frame. The
    /// observable is a second drain finding anything at all — a stray `frame` end
    /// is exactly what a split leaves behind.
    #[test]
    fn the_frames_drain_leaves_nothing_open_across_it() {
        in_a_traced_process(
            "engine::tests::the_frames_drain_leaves_nothing_open_across_it",
            || {
                let mut engine = hosted(None);
                for _ in 0..3 {
                    engine.frame().expect("the fake never fails");
                }
                let leftover = crcbl_core::trace::drain();
                assert!(
                    leftover.is_empty(),
                    "the frame left records behind: {}\n{:?}",
                    leftover.report(),
                    span_shapes(&leftover),
                );
            },
        );
    }

    /// **The swapchain wait is a span of its own**, so the CPU frame time can
    /// have it taken back out. It is what a vsynced frame spends most of itself
    /// in, and counting it as CPU work is how a row answers "CPU-bound" to every
    /// frame on every machine.
    #[test]
    fn the_swapchain_wait_is_recorded_as_a_span_of_its_own() {
        in_a_traced_process(
            "engine::tests::the_swapchain_wait_is_recorded_as_a_span_of_its_own",
            || {
                use crcbl_core::trace::RecordKind::{SpanBegin, SpanEnd};

                let (_shell, _window, _recorder, mut gpu) =
                    null_context("present-wait span test", Pacing::Vsync);
                // Two frames: the first has nothing submitted to wait on, the
                // second does, and both go through the acquire.
                null_frame(&mut gpu).expect("the null backend presents");
                null_frame(&mut gpu).expect("the null backend presents");

                let snapshot = crcbl_core::trace::drain();
                assert_eq!(
                    span_shapes(&snapshot),
                    vec![
                        (crate::perf::PRESENT_WAIT_SPAN, SpanBegin, 0),
                        (crate::perf::PRESENT_WAIT_SPAN, SpanEnd, 0),
                        (crate::perf::PRESENT_WAIT_SPAN, SpanBegin, 0),
                        (crate::perf::PRESENT_WAIT_SPAN, SpanEnd, 0),
                    ],
                    "a hand-written loop opens no frame span, so these sit at depth zero",
                );
                gpu.destroy().expect("the null backend releases everything");
            },
        );
    }

    /// **A traced run fills the budget row's CPU window**, which is the whole
    /// chain: the span opens, the drain finds it, `frame_cpu_time` reads it and
    /// the row takes it.
    #[test]
    fn a_traced_run_fills_the_budget_rows_cpu_window() {
        in_a_traced_process(
            "engine::tests::a_traced_run_fills_the_budget_rows_cpu_window",
            || {
                let mut engine = hosted(None);
                assert!(
                    !engine.debug.budget.has_samples(),
                    "nothing before the first frame"
                );
                for _ in 0..crcbl_ui::MIN_PERCENTILE_SAMPLES {
                    engine.frame().expect("the fake never fails");
                }
                let (p50, p95) = engine
                    .debug
                    .budget
                    .cpu()
                    .expect("a window this full reports percentiles");
                assert!(p50 > Duration::ZERO, "a frame that cost no CPU time at all");
                assert!(p95 >= p50, "p95 {p95:?} is under p50 {p50:?}");
                // No GPU timers on the fake, so the question stays open rather
                // than being answered from one side.
                assert_eq!(engine.debug.budget.bound(), crcbl_ui::Bound::Unknown);
            },
        );
    }

    /// The other side of the gate: with the trace off, the row has no CPU half
    /// and the panel therefore has no budget section.
    #[test]
    fn an_untraced_run_records_no_cpu_samples_and_shows_no_budget_section() {
        let mut engine = hosted(None);
        assert!(!crcbl_core::trace::is_enabled(), "the gate starts off");
        for _ in 0..4 {
            engine.frame().expect("the fake never fails");
        }
        assert!(engine.debug.budget.cpu().is_none());
        assert!(!engine.debug.budget.has_samples());

        engine.debug.set_visible(true);
        engine.debug.begin_frame();
        assert!(
            engine
                .debug
                .panel
                .sections()
                .iter()
                .all(|section| section.title() != "budget"),
            "a run with neither half has no budget row to show"
        );
    }

    /// One row of one panel section, by title and label.
    ///
    /// Both, deliberately: `DebugModule` labels share one namespace and nothing
    /// detects a collision, so a search of the whole panel for a label can read
    /// another module's row and pass.
    fn panel_row(
        engine: &Loop<crcbl_shell::HeadlessShell, FakeGame>,
        title: &str,
        label: &str,
    ) -> String {
        let section = engine
            .debug
            .panel
            .sections()
            .iter()
            .find(|section| section.title() == title)
            .unwrap_or_else(|| panic!("no {title:?} section on the panel"));
        section
            .rows()
            .iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("no {label:?} row in {:?}", section.rows()))
            .value
            .clone()
    }

    /// **The frame's counters move with what the frame drew, and the panel shows
    /// them one frame later.**
    ///
    /// Two scenes with known answers: a frame with the panel hidden draws the
    /// game's one HUD rect and nothing else, and a frame with the panel up draws
    /// that plus the overlay's own geometry. A counter wired to a constant, or to
    /// the frame number, agrees with neither.
    ///
    /// The last third is the **lag**, asserted rather than left to the docs: the
    /// panel is gathered before [`GameGpu::frame`] records anything, so the row a
    /// frame shows is the previous frame's — every row of it, which is why no row
    /// here is live beside a latent one.
    #[test]
    fn the_counters_row_moves_with_the_frame_and_trails_it_by_one() {
        let mut engine = hosted(None);

        engine.frame().expect("the fake never fails");
        let bare = engine.gpu.counters();
        assert_eq!(bare.draws, 1, "one draw for the whole list");
        assert!(bare.instances > 0, "the frame drew the game's own geometry");
        assert_eq!(bare.drawn, Some(bare.instances));
        assert_eq!(bare.triangles, Some(bare.instances * 2));

        engine.debug.set_visible(true);
        engine.frame().expect("the fake never fails");
        let with_panel = engine.gpu.counters();
        assert!(
            with_panel.instances > bare.instances,
            "the overlay's own geometry must move the counter: {with_panel:?} against {bare:?}",
        );
        assert_eq!(with_panel.drawn, Some(with_panel.instances));
        assert_eq!(with_panel.triangles, Some(with_panel.instances * 2));

        // The next frame's panel is the one that shows it.
        engine.frame().expect("the fake never fails");
        assert_eq!(
            panel_row(&engine, "counters", "instances submitted"),
            with_panel.instances.to_string(),
            "the row shows the frame before it, which is the whole section's lag",
        );
        assert_eq!(panel_row(&engine, "counters", "draws recorded"), "1");
        assert_eq!(
            panel_row(&engine, "counters", "instances drawn"),
            with_panel.instances.to_string(),
        );
    }

    /// **A traced frame carries its counters beside its spans**, and an unknown
    /// counter is left out of the trace rather than sampled as a zero.
    ///
    /// The zero is the failure this is written against: a `None` sampled as `0`
    /// is indistinguishable in a trace from a frame that genuinely drew nothing,
    /// which is `docs/plan/40-profiling.md`'s "counters that lie by omission" in
    /// the one place a consumer would never think to check.
    #[test]
    fn a_traced_frame_samples_the_counters_it_has_and_omits_the_ones_it_does_not() {
        in_a_traced_process(
            "engine::tests::a_traced_frame_samples_the_counters_it_has_and_omits_the_ones_it_does_not",
            || {
                use crcbl_core::trace::RecordKind::Counter;

                let mut engine = hosted(None);
                engine.frame_body().expect("the fake never fails");
                let counters = engine.gpu.counters();
                let sampled: Vec<(&'static str, crcbl_core::trace::RecordKind, u16)> =
                    span_shapes(&crcbl_core::trace::drain())
                        .into_iter()
                        .filter(|(_, kind, _)| matches!(kind, Counter(_)))
                        .collect();
                assert_eq!(
                    sampled,
                    vec![
                        (crate::perf::DRAWS_COUNTER, Counter(counters.draws), 1),
                        (
                            crate::perf::INSTANCES_COUNTER,
                            Counter(counters.instances),
                            1
                        ),
                        (
                            crate::perf::DRAWN_COUNTER,
                            Counter(counters.drawn.expect("nothing here draws indirectly")),
                            1
                        ),
                        (
                            crate::perf::TRIANGLES_COUNTER,
                            Counter(counters.triangles.expect("nor is the index count hidden")),
                            1
                        ),
                        (
                            crate::perf::CLUSTERS_COUNTER,
                            Counter(counters.clusters.expect("nor is a cluster count in flight")),
                            1
                        ),
                    ],
                    "the frame's counters, inside the frame span, with their values",
                );

                // And the half a GPU-driven frame cannot answer: sampled as
                // nothing at all, not as zero.
                crate::perf::sample_counters(crcbl_render::FrameCounters {
                    draws: 9,
                    instances: 7,
                    drawn: None,
                    triangles: None,
                    clusters: None,
                    cull_frame: None,
                });
                assert_eq!(
                    span_shapes(&crcbl_core::trace::drain()),
                    vec![
                        (crate::perf::DRAWS_COUNTER, Counter(9), 0),
                        (crate::perf::INSTANCES_COUNTER, Counter(7), 0),
                    ],
                    "an unknown counter must leave no record, not a zero one",
                );

                // And the frame stamp is sampled with them where there is one,
                // so a consumer can see that `instances-drawn` is about frame 37
                // rather than about the span it arrived in.
                crate::perf::sample_counters(crcbl_render::FrameCounters {
                    draws: 9,
                    instances: 7,
                    drawn: Some(4),
                    triangles: None,
                    clusters: Some(2),
                    cull_frame: Some(37),
                });
                assert_eq!(
                    span_shapes(&crcbl_core::trace::drain()),
                    vec![
                        (crate::perf::DRAWS_COUNTER, Counter(9), 0),
                        (crate::perf::INSTANCES_COUNTER, Counter(7), 0),
                        (crate::perf::DRAWN_COUNTER, Counter(4), 0),
                        (crate::perf::CLUSTERS_COUNTER, Counter(2), 0),
                        (crate::perf::CULL_FRAME_COUNTER, Counter(37), 0),
                    ],
                    "a latent counter must carry the frame it is about",
                );
            },
        );
    }

    /// **The GPU half goes in by frame number**, so the timers' latency does not
    /// fill the window with copies of one report.
    ///
    /// No trace here: the two halves are fed independently, and this is the one
    /// that needs no gate.
    #[test]
    fn the_budget_rows_gpu_window_follows_the_timers_frame_number() {
        let timings = |frame: u64, nanos: u64| crcbl_render::FrameTimings {
            frame,
            passes: vec![crcbl_render::PassTiming {
                label: "forward".to_string(),
                gpu_nanos: nanos,
            }],
        };

        let mut engine = hosted(None);
        engine.gpu.timings = Some(timings(1, 2_000_000));
        engine.frame().expect("the fake never fails");
        engine.frame().expect("the fake never fails");
        assert_eq!(
            engine.debug.budget.gpu_frame(),
            Some(1),
            "the same latent report twice is still one frame"
        );

        engine.gpu.timings = Some(timings(2, 3_000_000));
        engine.frame().expect("the fake never fails");
        assert_eq!(engine.debug.budget.gpu_frame(), Some(2));

        // And an empty report — a device with timers whose ring has not come
        // round — is not a frame that cost nothing.
        let mut pending = hosted(None);
        pending.gpu.timings = Some(crcbl_render::FrameTimings::default());
        pending.frame().expect("the fake never fails");
        assert_eq!(pending.debug.budget.gpu_frame(), None);
        assert!(!pending.debug.budget.has_samples());
    }

    /// **Every frame draws once, and draws after the ticks it reports.**
    ///
    /// The sum is the part worth pinning. A `draw` called *before* `run_ticks`
    /// would still be called once a frame and would still see plausible
    /// numbers — it would just be one frame's worth of ticks behind, which is
    /// how an animation stepped on `FrameInfo::ticks` ends up a frame stale
    /// forever.
    #[test]
    fn a_hosted_frame_ticks_the_game_then_draws_it() {
        let mut engine = hosted(None);
        for _ in 0..4 {
            assert_eq!(
                engine.frame().expect("the fake never fails"),
                Flow::Continue
            );
        }

        assert_eq!(engine.gpu().frames, 4, "one present per frame");
        assert_eq!(engine.game().draws.len(), 4, "one draw per frame");
        assert!(engine.game().ticks > 0, "the simulation never ran");
        assert_eq!(
            engine.game().draws.iter().map(|d| d.ticks).sum::<u64>(),
            engine.game().ticks,
            "a frame reported ticks it had not run yet",
        );
    }

    /// **Pause stops the simulation and nothing else.**
    ///
    /// Frames keep presenting — an unfocused or paused window still has to
    /// redraw — and the menu the loop switches to is the game's `Paused` one.
    #[test]
    fn a_paused_loop_keeps_presenting_and_stops_ticking() {
        let mut engine = hosted(None);
        let window = engine.window();
        engine.frame().expect("the fake never fails");
        let ticks_before = engine.game().ticks;
        let frames_before = engine.gpu().frames;

        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("the fake never fails");
        engine.frame().expect("the fake never fails");

        assert!(engine.is_paused(), "Escape did not stop the simulation");
        assert_eq!(
            engine.game().ticks,
            ticks_before,
            "a paused frame ran the simulation",
        );
        assert_eq!(
            engine.gpu().frames,
            frames_before + 2,
            "a paused frame stopped presenting",
        );
        assert_eq!(engine.menu_kind(), FakeMenu::Paused);
    }

    /// **A press made before a panel opened does not fire that panel's
    /// buttons.**
    ///
    /// The pointer is held down over the field, a menu opens under it, and the
    /// release lands on a button the player never pressed. With a mouse it takes
    /// a deliberate press-and-hold; on a phone it is the ordinary case, because
    /// the thumb holding an on-screen stick *is* the emulated pointer and every
    /// menu opened with the other hand opens under a held press. Found exactly
    /// that way — `web/tools/browser-e2e.mjs` watched horde ask for fullscreen
    /// when a thumb lifted off a pause menu it had never touched.
    ///
    /// The second half is the control: a press made *while* the panel is up
    /// still fires it, so this is not a menu that stopped answering the pointer.
    #[test]
    fn a_press_made_before_a_panel_opened_does_not_fire_its_buttons() {
        let mut engine = hosted(None);
        // Frames first: the fixture's swapchain settles at its final extent a
        // frame or two in, and a menu laid out before that is laid out for a
        // surface that is about to change size.
        for _ in 0..3 {
            engine.frame().expect("the fake never fails");
        }
        let pause = |engine: &mut Loop<crcbl_shell::HeadlessShell, FakeGame>| {
            let window = engine.window();
            for state in [
                crcbl_shell::ButtonState::Pressed,
                crcbl_shell::ButtonState::Released,
            ] {
                engine
                    .shell_mut()
                    .key(window, PAUSE_KEY, state)
                    .expect("the window is live");
            }
            engine.frame().expect("the fake never fails");
        };
        let button = |engine: &mut Loop<crcbl_shell::HeadlessShell, FakeGame>, at, state| {
            let window = engine.window();
            engine
                .shell_mut()
                .button(
                    window,
                    crcbl_core::input::PointerButton::Left,
                    state,
                    Some(at),
                )
                .expect("the window is live");
            engine.frame().expect("the fake never fails");
        };

        // Where RESUME will be, read off the panel itself and then put away
        // again: the press below has to land on a button that is not there yet.
        pause(&mut engine);
        let layout = engine.menu_layout().expect("the pause menu");
        let item = layout.items()[0];
        let centre = (item.min + item.max) * 0.5;
        let over = crcbl_shell::PhysicalPoint {
            x: f64::from(centre.x),
            y: f64::from(centre.y),
        };
        pause(&mut engine);
        assert!(!engine.is_paused(), "the fixture would not un-pause");

        // Press on the field, open the panel under the held press, hold it
        // there for a few frames, lift. **The held frames are the bug**: the
        // press itself lands before the panel exists, and it is the frames
        // after it — pointer still down, button now underneath — that latch it.
        button(&mut engine, over, crcbl_shell::ButtonState::Pressed);
        pause(&mut engine);
        assert!(engine.is_paused(), "the panel never opened");
        for _ in 0..2 {
            engine.frame().expect("the fake never fails");
        }
        button(&mut engine, over, crcbl_shell::ButtonState::Released);
        assert!(
            engine.is_paused(),
            "a press that predates the panel fired the button under it",
        );

        // The control: pressed *on* the panel, the same button still fires.
        button(&mut engine, over, crcbl_shell::ButtonState::Pressed);
        button(&mut engine, over, crcbl_shell::ButtonState::Released);
        assert!(
            !engine.is_paused(),
            "a press made on the panel stopped working",
        );
    }

    /// **A contact survives the round trip through the surface**, so a widget
    /// hit-testing pixels and the loop normalising them agree.
    ///
    /// The pair is one convention with two halves, and the failure it guards is
    /// silent: a `pixels` that dropped the Y flip would put every on-screen
    /// control's hit rect in the mirror image of where it was drawn, and every
    /// existing test would still pass.
    #[test]
    fn a_contact_survives_the_round_trip_through_the_surface() {
        use crcbl_core::input::{ContactId, TouchPhase};
        const EXTENT: (u32, u32) = (960, 720);

        // Named corners, so a mirrored conversion fails on the value rather
        // than on a tolerance.
        for (pixels, want) in [
            (glam::Vec2::new(0.0, 0.0), glam::Vec2::new(-1.0, 1.0)),
            (glam::Vec2::new(960.0, 720.0), glam::Vec2::new(1.0, -1.0)),
            (glam::Vec2::new(480.0, 360.0), glam::Vec2::ZERO),
            (glam::Vec2::new(240.0, 540.0), glam::Vec2::new(-0.5, -0.5)),
        ] {
            let at = normalised(pixels, EXTENT);
            assert_eq!(at, want, "{pixels} normalised wrong");
            let back = TouchUpdate {
                contact: ContactId(1),
                phase: TouchPhase::Moved,
                at,
            }
            .pixels(EXTENT);
            assert_eq!(back, pixels, "{at} did not come back as {pixels}");
        }
    }

    /// **An on-screen control pauses the loop, and un-pauses it again.**
    ///
    /// The half of the pause a phone can reach: no key is pressed anywhere in
    /// this test, and the simulation stops and starts anyway. The tick counts on
    /// either side are what says so — `is_paused` alone would pass on a flag the
    /// loop set and then ignored.
    #[test]
    fn an_on_screen_control_can_pause_and_resume_the_loop() {
        let mut engine = hosted(None);
        // The first frame only establishes the clock's baseline, so it covers no
        // time and runs no ticks — the control below has to be told from *that*,
        // which is why the fixture is made to tick first.
        for _ in 0..3 {
            engine.frame().expect("the fake never fails");
        }
        assert!(
            engine.game().ticks > 0,
            "the fixture ticks when it is not paused",
        );

        engine.game_mut().pending_pause = true;
        engine.frame().expect("the fake never fails");
        engine.frame().expect("the fake never fails");
        assert!(engine.is_paused(), "the control's request was dropped");
        let ticks_paused = engine.game().ticks;
        engine.frame().expect("the fake never fails");
        assert_eq!(
            engine.game().ticks,
            ticks_paused,
            "a paused frame ran the simulation",
        );
        assert_eq!(engine.menu_kind(), FakeMenu::Paused);

        // …and the request is *taken*, so holding the control down does not
        // toggle the pause once a frame.
        engine.frame().expect("the fake never fails");
        assert!(engine.is_paused(), "the pause toggled itself back off");

        engine.game_mut().pending_pause = true;
        engine.frame().expect("the fake never fails");
        assert!(!engine.is_paused(), "a second press did not resume");
        engine.frame().expect("the fake never fails");
        assert!(
            engine.game().ticks > ticks_paused,
            "the simulation did not start again",
        );
    }

    /// **The game is handed every contact, normalised, in order.**
    ///
    /// The loop's half of the seam: two fingers land, one moves, and the game
    /// sees three updates whose ids and positions are the fingers' own. The
    /// positions are the surface's −1…1 with +Y up, like the pointer's — a game
    /// that had to redo the DPI arithmetic per contact would get it wrong on the
    /// displays nobody develops on.
    #[test]
    fn every_contact_reaches_the_game_normalised_to_the_surface() {
        use crcbl_core::input::{ContactId, TouchPhase};
        let mut engine = hosted(None);
        let window = engine.window();
        // The fixture's framebuffer, which is what the normalisation divides by.
        assert_eq!(engine.gpu().extent(), (640, 480));

        for (contact, phase, x, y) in [
            (1, TouchPhase::Began, 0.0, 0.0),
            (2, TouchPhase::Began, 640.0, 480.0),
            (2, TouchPhase::Moved, 320.0, 240.0),
        ] {
            engine
                .shell_mut()
                .touch(
                    window,
                    ContactId(contact),
                    phase,
                    crcbl_shell::PhysicalPoint { x, y },
                )
                .expect("the window is live");
        }
        engine.frame().expect("the fake never fails");

        assert_eq!(
            engine.game().touches,
            vec![
                // Top-left corner: −1 across, +1 up.
                TouchUpdate {
                    contact: ContactId(1),
                    phase: TouchPhase::Began,
                    at: glam::Vec2::new(-1.0, 1.0),
                },
                // Bottom-right corner, which is the other sign in both axes —
                // a missing Y flip would put this at (1, -1)'s mirror.
                TouchUpdate {
                    contact: ContactId(2),
                    phase: TouchPhase::Began,
                    at: glam::Vec2::new(1.0, -1.0),
                },
                TouchUpdate {
                    contact: ContactId(2),
                    phase: TouchPhase::Moved,
                    at: glam::Vec2::ZERO,
                },
            ],
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    /// **A finger still down when the window loses focus is cancelled.**
    ///
    /// The contact's half of the obligation `held_keys` and `pointer_in_game`
    /// discharge: no platform reports the end of a gesture that was interrupted
    /// by the window going away, and a game left holding a finger keeps steering
    /// with a stick nobody is touching.
    ///
    /// **Cancelled and not ended**, because the player did not lift it — the
    /// difference between a stick centring and a charged attack firing at
    /// whatever the camera happened to be pointing at.
    #[test]
    fn a_contact_held_when_focus_is_lost_is_cancelled_where_it_was_last_seen() {
        use crcbl_core::input::{ContactId, TouchPhase};
        let mut engine = hosted(None);
        let window = engine.window();

        engine
            .shell_mut()
            .touch(
                window,
                ContactId(1),
                TouchPhase::Began,
                crcbl_shell::PhysicalPoint { x: 160.0, y: 120.0 },
            )
            .expect("the window is live");
        // A second finger that ends properly, so the cancel below cannot be a
        // blanket "cancel everything the game ever heard about".
        engine
            .shell_mut()
            .touch(
                window,
                ContactId(2),
                TouchPhase::Began,
                crcbl_shell::PhysicalPoint { x: 0.0, y: 0.0 },
            )
            .expect("the window is live");
        engine
            .shell_mut()
            .touch(
                window,
                ContactId(2),
                TouchPhase::Ended,
                crcbl_shell::PhysicalPoint { x: 0.0, y: 0.0 },
            )
            .expect("the window is live");
        engine.frame().expect("the fake never fails");
        let before = engine.game().touches.len();

        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("the fake never fails");

        assert_eq!(
            &engine.game().touches[before..],
            &[TouchUpdate {
                contact: ContactId(1),
                phase: TouchPhase::Cancelled,
                // Where it was last seen: (160, 120) of 640×480.
                at: glam::Vec2::new(-0.5, 0.5),
            }],
            "focus loss owed exactly one cancel, for the finger still down",
        );

        // And it is owed once: a second focus loss has nothing left to cancel.
        engine
            .shell_mut()
            .set_focus(window, true)
            .expect("the window is live");
        engine.frame().expect("the fake never fails");
        engine
            .shell_mut()
            .set_focus(window, false)
            .expect("the window is live");
        engine.frame().expect("the fake never fails");
        assert_eq!(
            engine.game().touches.len(),
            before + 1,
            "the loop cancelled a contact twice: {:?}",
            engine.game().touches,
        );
        engine.finish(ExitReason::FrameBudget).expect("teardown");
    }

    // -- a finger on a menu ---------------------------------------------------

    /// Posts one contact and runs a frame.
    fn finger(
        engine: &mut Loop<crcbl_shell::HeadlessShell, FakeGame>,
        contact: u32,
        phase: crcbl_core::input::TouchPhase,
        at: glam::Vec2,
    ) {
        let window = engine.window();
        engine
            .shell_mut()
            .touch(
                window,
                crcbl_core::input::ContactId(contact),
                phase,
                crcbl_shell::PhysicalPoint {
                    x: f64::from(at.x),
                    y: f64::from(at.y),
                },
            )
            .expect("the headless shell reports TOUCH");
        engine.frame().expect("the fake never fails");
    }

    /// Posts one contact **and the emulated pointer the platform owes for it**,
    /// then runs a frame.
    ///
    /// The obligation on any backend that sets `ShellCaps::TOUCH`: every contact
    /// on the touch stream, and the primary one *also* as a pointer. A test that
    /// scripted only the contacts would be modelling a platform that does not
    /// exist, and would never see the double-fire this file guards against.
    fn primary_finger(
        engine: &mut Loop<crcbl_shell::HeadlessShell, FakeGame>,
        contact: u32,
        phase: crcbl_core::input::TouchPhase,
        at: glam::Vec2,
    ) {
        use crcbl_core::input::TouchPhase;
        let window = engine.window();
        let point = crcbl_shell::PhysicalPoint {
            x: f64::from(at.x),
            y: f64::from(at.y),
        };
        let shell = engine.shell_mut();
        shell
            .touch(window, crcbl_core::input::ContactId(contact), phase, point)
            .expect("the headless shell reports TOUCH");
        match phase {
            TouchPhase::Began | TouchPhase::Ended | TouchPhase::Cancelled => {
                let state = if matches!(phase, TouchPhase::Began) {
                    crcbl_shell::ButtonState::Pressed
                } else {
                    crcbl_shell::ButtonState::Released
                };
                shell
                    .button(
                        window,
                        crcbl_core::input::PointerButton::Left,
                        state,
                        Some(point),
                    )
                    .expect("the window is live");
            }
            TouchPhase::Moved => shell
                .move_pointer(window, point, (0.0, 0.0))
                .expect("the window is live"),
        }
        engine.frame().expect("the fake never fails");
    }

    /// A whole tap of the primary finger **inside one pump**, which is what a
    /// tap on a phone is: the press and the release of a real one arrive in the
    /// same batch, and a check that spread them over two frames is checking a
    /// gesture nobody makes.
    fn primary_tap(
        engine: &mut Loop<crcbl_shell::HeadlessShell, FakeGame>,
        contact: u32,
        at: glam::Vec2,
    ) {
        use crcbl_core::input::TouchPhase;
        let window = engine.window();
        let point = crcbl_shell::PhysicalPoint {
            x: f64::from(at.x),
            y: f64::from(at.y),
        };
        let shell = engine.shell_mut();
        for (phase, state) in [
            (TouchPhase::Began, crcbl_shell::ButtonState::Pressed),
            (TouchPhase::Ended, crcbl_shell::ButtonState::Released),
        ] {
            shell
                .touch(window, crcbl_core::input::ContactId(contact), phase, point)
                .expect("the headless shell reports TOUCH");
            shell
                .button(
                    window,
                    crcbl_core::input::PointerButton::Left,
                    state,
                    Some(point),
                )
                .expect("the window is live");
        }
        engine.frame().expect("the fake never fails");
    }

    /// The centre of the button the menu on screen is showing, in framebuffer
    /// pixels.
    fn menu_button(engine: &Loop<crcbl_shell::HeadlessShell, FakeGame>) -> glam::Vec2 {
        let layout = engine.menu_layout().expect("a menu is on screen");
        let item = layout.items()[0];
        (item.min + item.max) * 0.5
    }

    /// A loop whose swapchain has settled and whose start menu is up.
    fn with_a_menu() -> Loop<crcbl_shell::HeadlessShell, FakeGame> {
        let mut engine = hosted(None);
        // The fixture's swapchain settles at its final extent a frame or two
        // in, and a menu laid out before that is laid out for a surface about
        // to change size.
        for _ in 0..3 {
            engine.frame().expect("the fake never fails");
        }
        assert_eq!(engine.menu_kind(), FakeMenu::Start, "no menu to press");
        engine
    }

    /// Pauses through the key, which is the loop's own route to the panel.
    fn pause_key(engine: &mut Loop<crcbl_shell::HeadlessShell, FakeGame>) {
        let window = engine.window();
        for state in [
            crcbl_shell::ButtonState::Pressed,
            crcbl_shell::ButtonState::Released,
        ] {
            engine
                .shell_mut()
                .key(window, PAUSE_KEY, state)
                .expect("the window is live");
        }
        engine.frame().expect("the fake never fails");
    }

    /// **A second finger presses a menu button while the first holds a
    /// control.**
    ///
    /// The lockout this routing exists for. Only the primary contact drives the
    /// emulated pointer, so while a thumb is down on an on-screen stick the menu
    /// hears nothing from any other finger — and a player who paused had to lift
    /// the stick before `RESUME` could be tapped at all.
    ///
    /// Both halves are asserted. The panel goes away, which is the second finger
    /// having fired the button; and the first contact is **not disturbed** —
    /// nothing is delivered for it, so the control it is holding keeps whatever
    /// value it had, and it is still live afterwards.
    #[test]
    fn a_second_contact_presses_a_menu_while_the_first_holds_a_control() {
        use crcbl_core::input::{ContactId, TouchPhase};
        let mut engine = with_a_menu();

        // The thumb, on the field and down for the rest of this test. It is the
        // first contact of the gesture, so it is the one the platform emulates
        // the pointer with.
        primary_finger(
            &mut engine,
            1,
            TouchPhase::Began,
            glam::Vec2::new(40.0, 400.0),
        );
        pause_key(&mut engine);
        assert!(engine.is_paused(), "the panel never opened");
        assert_eq!(engine.menu_kind(), FakeMenu::Paused);
        let resume = menu_button(&engine);
        let held = engine.game().touches.len();

        // The other hand, on RESUME, with the thumb still down. No pointer
        // event anywhere: a second contact raises none, which is exactly why
        // this could not be done before.
        finger(&mut engine, 2, TouchPhase::Began, resume);
        assert!(engine.is_paused(), "a press is not a tap");
        finger(&mut engine, 2, TouchPhase::Ended, resume);
        assert!(
            !engine.is_paused(),
            "the second finger could not reach the panel: the run is still paused",
        );

        // The thumb was left alone: every contact delivered while the other
        // hand was pressing belongs to the second finger.
        assert!(
            engine.game().touches[held..]
                .iter()
                .all(|touch| touch.contact == ContactId(2)),
            "the menu tap disturbed the contact holding the control: {:?}",
            &engine.game().touches[held..],
        );
        // …and it is still the loop's live, primary contact, so its next move
        // reaches the game and nothing else has inherited the pointer.
        let moved = engine.game().touches.len();
        primary_finger(
            &mut engine,
            1,
            TouchPhase::Moved,
            glam::Vec2::new(60.0, 400.0),
        );
        assert_eq!(
            engine.game().touches[moved..]
                .iter()
                .map(|touch| (touch.contact, touch.phase))
                .collect::<Vec<_>>(),
            vec![(ContactId(1), TouchPhase::Moved)],
            "the thumb stopped being heard from",
        );
    }

    /// **One finger, one press.**
    ///
    /// A tap arrives on both streams — the contact and the pointer the platform
    /// emulates for it — and the button it lands on must fire *once*. The
    /// observable is the game's own key log rather than a state that saturates:
    /// `Serve::Launch` presses `SERVE_KEY` and releases it, so a button that
    /// fired twice logs four events instead of two, and for a toggle like
    /// `FULLSCREEN` the second fire would silently undo the first.
    ///
    /// **The tap is one pump**, which is what makes this able to fail: with the
    /// press and the release in separate batches the pointer's fire clears the
    /// menu's capture before the contact reaches it, and the double fire hides.
    #[test]
    fn a_one_finger_tap_on_a_menu_button_fires_it_once() {
        let mut engine = with_a_menu();
        let play = menu_button(&engine);
        assert!(engine.game().keys.is_empty(), "nothing has been pressed");

        primary_tap(&mut engine, 1, play);
        assert!(engine.game().served, "the tap never fired the button");
        assert_eq!(
            engine.game().keys,
            vec![(SERVE_KEY, true), (SERVE_KEY, false)],
            "the button fired more than once for one finger",
        );
    }

    /// **A contact that landed before the panel presses nothing**, which is the
    /// pointer's rule and has to be the finger's too.
    ///
    /// The finger here is deliberately **not** the primary one — an anchor
    /// contact is down first — so this is about the contact route rather than
    /// about the pointer's `menu_owns_press`, which a primary contact would
    /// have hidden behind.
    #[test]
    fn a_contact_that_landed_before_the_panel_fires_nothing() {
        use crcbl_core::input::TouchPhase;
        let mut engine = with_a_menu();
        // Anchor, so the finger under test is never the emulated pointer.
        primary_finger(&mut engine, 1, TouchPhase::Began, glam::Vec2::new(8.0, 8.0));
        pause_key(&mut engine);
        let resume = menu_button(&engine);
        pause_key(&mut engine);
        assert!(!engine.is_paused(), "the fixture would not un-pause");

        // Down on the field, where the panel is about to open, and lifted after
        // it has.
        finger(&mut engine, 2, TouchPhase::Began, resume);
        pause_key(&mut engine);
        assert!(engine.is_paused(), "the panel never opened");
        finger(&mut engine, 2, TouchPhase::Moved, resume);
        finger(&mut engine, 2, TouchPhase::Ended, resume);
        assert!(
            engine.is_paused(),
            "a finger that was down before the panel opened fired the button \
             that appeared under it",
        );

        // The control: a finger that lands *on* the panel still fires it, so
        // this is not a menu that stopped answering contacts.
        finger(&mut engine, 2, TouchPhase::Began, resume);
        finger(&mut engine, 2, TouchPhase::Ended, resume);
        assert!(
            !engine.is_paused(),
            "a tap made on the panel stopped working"
        );
    }

    /// **A gesture the system took away fires nothing, and frees the panel for
    /// the next finger.**
    ///
    /// The second half is the one that hides: a cancelled press that left the
    /// menu's capture latched is invisible — the panel looks idle — and every
    /// later tap does nothing.
    #[test]
    fn a_cancelled_contact_fires_nothing_and_leaves_the_menu_pressable() {
        use crcbl_core::input::TouchPhase;
        let mut engine = with_a_menu();
        primary_finger(&mut engine, 1, TouchPhase::Began, glam::Vec2::new(8.0, 8.0));
        pause_key(&mut engine);
        let resume = menu_button(&engine);

        finger(&mut engine, 2, TouchPhase::Began, resume);
        finger(&mut engine, 2, TouchPhase::Cancelled, resume);
        assert!(
            engine.is_paused(),
            "the system took the gesture away and the button fired anyway",
        );

        finger(&mut engine, 3, TouchPhase::Began, resume);
        finger(&mut engine, 3, TouchPhase::Ended, resume);
        assert!(
            !engine.is_paused(),
            "the cancelled press left the panel holding a capture nobody could \
             take, so the next tap did nothing",
        );
    }

    /// **A finger resting on the panel's background does not lock the other
    /// hand out**, which is the failure a "the first contact while a panel is
    /// up is the menu's" rule would have introduced in place of the one it fixed.
    #[test]
    fn a_contact_on_the_panels_background_leaves_the_buttons_pressable() {
        use crcbl_core::input::TouchPhase;
        let mut engine = with_a_menu();
        primary_finger(&mut engine, 1, TouchPhase::Began, glam::Vec2::new(8.0, 8.0));
        pause_key(&mut engine);
        let resume = menu_button(&engine);

        // A corner of the screen: on the panel's frame at most, on no button.
        finger(
            &mut engine,
            2,
            TouchPhase::Began,
            glam::Vec2::new(4.0, 470.0),
        );
        finger(&mut engine, 3, TouchPhase::Began, resume);
        finger(&mut engine, 3, TouchPhase::Ended, resume);
        assert!(
            !engine.is_paused(),
            "a finger resting off the buttons held the panel's press slot",
        );
    }

    /// **A menu button of the game's reaches the game as its own action.**
    ///
    /// And the key that fired it does *not*: the menu's three keys are consumed
    /// while a menu is showing, so a game that also bound Enter would not see a
    /// press it never meant to receive.
    #[test]
    fn a_game_menu_button_reaches_the_game_as_a_key() {
        let mut engine = hosted(None);
        let window = engine.window();
        // The first frame is what puts the start menu on screen; the pump reads
        // the menu that was showing when the key was pressed.
        engine.frame().expect("the fake never fails");
        assert_eq!(engine.menu_kind(), FakeMenu::Start);

        engine
            .shell_mut()
            .key_press(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine.frame().expect("the fake never fails");

        assert!(engine.game().served, "the PLAY button did not serve");
        assert!(
            engine.game().keys.contains(&(SERVE_KEY, true))
                && engine.game().keys.contains(&(SERVE_KEY, false)),
            "the button did not arrive as a press *and* a release: {:?}",
            engine.game().keys,
        );
        assert!(
            !engine
                .game()
                .keys
                .iter()
                .any(|(key, _)| *key == MENU_ACTIVATE_KEY),
            "the menu's commit key was forwarded to the game as well",
        );
    }

    /// **The loop answers its own widget ids without asking the game.**
    ///
    /// `FakeGame::menu_action` asserts on a reserved id, so a `from_id` that
    /// forwarded everything would panic here rather than quietly re-pointing
    /// the resume button.
    #[test]
    fn a_reserved_menu_button_is_the_loops_and_resumes_without_the_game() {
        let mut engine = hosted(None);
        let window = engine.window();
        engine
            .shell_mut()
            .key_press(window, PAUSE_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, PAUSE_KEY)
            .expect("the window is live");
        engine.frame().expect("the fake never fails");
        assert!(engine.is_paused(), "the fixture never paused");
        let keys_before = engine.game().keys.len();

        engine
            .shell_mut()
            .key_press(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine
            .shell_mut()
            .key_release(window, MENU_ACTIVATE_KEY)
            .expect("the window is live");
        engine.frame().expect("the fake never fails");

        assert!(!engine.is_paused(), "RESUME did not un-pause the loop");
        assert_eq!(
            engine.game().keys.len(),
            keys_before,
            "the loop's own button reached the game: {:?}",
            engine.game().keys,
        );
    }

    /// **A resize the compositor delivered reaches the swapchain.**
    #[test]
    fn a_resize_observed_by_the_loop_reaches_the_swapchain() {
        let mut engine = hosted(None);
        let window = engine.window();
        engine.frame().expect("the fake never fails");

        engine
            .shell_mut()
            .resize(window, PhysicalSize::new(320, 200))
            .expect("the window is live");
        engine.frame().expect("the fake never fails");

        assert_eq!(engine.extent(), (320, 200));
    }

    /// **The budget stops the run, and both halves of the summary agree.**
    ///
    /// `run.ticks` is the loop's tally and `ticks_the_game_counted` is the
    /// game's; they are counted in different places and a loop that dropped a
    /// tick — or double-counted one — is what makes them disagree.
    #[test]
    fn the_frame_budget_stops_the_run_and_the_summary_reports_it() {
        let summary = drive(hosted(Some(3))).expect("the fake never fails");

        assert_eq!(summary.run.frames, 3, "the budget was not what stopped it");
        assert_eq!(summary.run.exit, ExitReason::FrameBudget);
        assert_eq!(
            summary.run.ticks, summary.ticks_the_game_counted,
            "the loop and the game disagree about how much simulation ran",
        );
        assert_eq!(summary.run.backend, crcbl_shell::ShellBackend::Headless);
    }
}
