//! Presenting to a window: the device, the swapchain and the frame ring.
//!
//! [`Presenter`] is everything between a configured window and a presented
//! frame that does not depend on what is drawn: the backend walk with its
//! fall-through, the adapter pin, the composite alpha a transparent window
//! needs, the surface, the device, the swapchain, the ring of frames in flight
//! and the pacing against the display. A program that draws only a UI — a
//! status bar, a launcher — opens one of these and records its own frames
//! into it.
//!
//! The engine's `crcbl::engine::GpuContext` wraps one and adds what a game
//! needs on top: the player's `[engine.video]` settings, the renderer's
//! effect request and `--screenshot`. Those reach crates this one must not
//! link, which is why the split is here.
//!
//! The frame is:
//!
//! ```text
//! acquire → record → submit(wait acquire, signal present + timeline)
//!         → present(wait present) → retire the command buffer
//! ```
//!
//! # Frames in flight, not `wait_idle`
//!
//! [`Device::destroy_command_buffer`] may not be called until the submission
//! that used it has completed, and the seam offers exactly two ways to know
//! that: a timeline semaphore, or [`Device::wait_idle`] — which the seam itself
//! documents as "a shutdown and test primitive" that "destroys pipelining". So
//! [`Presenter`] keeps a ring keyed on a timeline semaphore value, and falls
//! back to `wait_idle` only on a device that has no timeline semaphores.
//!
//! # The caller's half of the swapchain rules
//!
//! `crcbl-hal`'s [`swapchain`](crcbl_hal::swapchain) module states the extent
//! rule as numbered backend obligations, and [`Presenter::request_open`] is the
//! reference implementation of the caller's half: the window system's size is
//! the request, and the extent [`AcquiredFrame::extent`] reports is the answer
//! rendered at. [`Presenter::resize`] reconfigures the swapchain and nothing
//! else, on every backend, and [`Presenter::destroy`] tears down in the order
//! the seam states — swapchain before surface, surface before the window the
//! caller owns.
//!
//! # Presenting when the window system asks
//!
//! A status bar on an output the compositor has turned off is never asked for
//! a frame there, and a FIFO present to it can block the loop — input, close
//! button and all — for as long as the output stays off. A loop that presents
//! only when asked does three things a frame, and the presenter keeps the
//! state between them:
//!
//! 1. Hands every event [`Shell::pump`] delivers to [`Presenter::observe`].
//! 2. Calls [`Presenter::request_frame`] after [`Presenter::acquire`] hands
//!    over a frame and before [`Presenter::submit_and_present`].
//! 3. Skips the frame when `acquire` hands over none. From a present that
//!    carried a request until the answer is observed, `acquire` answers
//!    `Ok(None)` without touching the device, and [`Presenter::frame_due`]
//!    says so ahead of time, for a loop that would rather sleep in
//!    [`Shell::wait_events`] than spin. A loop that counts its frames records
//!    that one as [`FrameOutcome::Held`], not as a reconfigure.
//!
//! A presenter whose caller never requests a frame presents exactly as it
//! always has. [`Presenter::frame_due`] says why the gate behaves the same
//! whether or not the shell has
//! [`ShellCaps::FRAME_CALLBACK`](crcbl_shell::ShellCaps::FRAME_CALLBACK).
//!
//! # Example
//!
//! ```
//! use crcbl_app::backend::GpuBackend;
//! use crcbl_app::present::{FrameOutcome, Presenter, PresenterDesc};
//! use crcbl_app::window::{open_window, wait_for_configure};
//! use crcbl_hal::CommandEncoderDesc;
//! use crcbl_shell::{HeadlessShell, Shell, WindowDesc};
//! use std::time::Duration;
//!
//! let mut shell = HeadlessShell::new();
//! let window = open_window(&mut shell, Duration::ZERO, &WindowDesc::default())?;
//! let mut events = 0;
//! let extent = wait_for_configure(&mut shell, window, &mut |_| events += 1)?;
//!
//! let desc = PresenterDesc {
//!     label: "launcher",
//!     backend: Some(GpuBackend::Null),
//!     ..PresenterDesc::default()
//! };
//! let mut presenter = Presenter::open(&shell, window, extent, &desc)?;
//! let mut presented = 0;
//! for _ in 0..3 {
//!     shell.pump(&mut |event| presenter.observe(&event));
//!     let Some(frame) = presenter.acquire()? else {
//!         continue;
//!     };
//!     let encoder = presenter.device().create_command_encoder(&CommandEncoderDesc {
//!         label: Some("launcher frame"),
//!         queue: presenter.queue(),
//!     });
//!     // Record into `frame.view` here.
//!     let commands = encoder.finish()?;
//!     presenter.request_frame(&mut shell)?;
//!     assert_eq!(
//!         presenter.submit_and_present(&frame, commands)?,
//!         FrameOutcome::Presented
//!     );
//!     presented += 1;
//! }
//! // No compositor holds the answer back here, so each came at the next pump.
//! assert_eq!(presented, 3);
//! presenter.destroy()?;
//! shell.destroy_window(window)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use crcbl_hal::{
    AcquiredFrame, CommandBufferHandle, CompositeAlpha, DeviceDesc, DisplayTiming, Features,
    Format, HalError, PresentInfo, PresentMode, QueueHandle, QueueKind, SemaphoreDesc,
    SemaphoreHandle, SemaphoreKind, SemaphoreSignal, SemaphoreWait, SubmitInfo, SurfaceError,
    SurfaceHandle, SwapchainDesc, SwapchainHandle,
};
use crcbl_hal::{Device, Instance};
use crcbl_shell::{Shell, ShellError, ShellEvent, WindowId};

use crate::adapter::{ADAPTER_ENV_VAR, AdapterPick, Choice, PinMiss, PowerPreference};
use crate::backend::{GpuBackend, PendingInstance};
use crate::gpu_choice::{ChoiceOutcome, GpuChoice, Ignored};

use frame_gate::FrameGate;

// Whether the window may present again, between a frame request and its
// answer.
mod frame_gate;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How many frames may be in flight before the loop waits for the oldest: the
/// default for [`PresenterDesc::frames_in_flight`].
///
/// Two is the classic double-buffered default: one frame being recorded while
/// one is executing. The engine's renderer keeps a uniform ring of the same
/// depth — one buffer per frame in flight, or a spinning camera is a
/// read-after-write hazard across submissions — and the `crcbl` umbrella
/// asserts at compile time that its constant and this one agree.
pub const FRAMES_IN_FLIGHT: usize = 2;

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

/// The trace span around the swapchain wait inside [`Presenter::acquire`]:
/// the closed-loop present wait and the acquire itself, plus waits for old
/// submissions in [`Presenter::retire_to`]. Resource destruction stays counted
/// as CPU work.
///
/// Waiting, not working: `crcbl::perf::frame_cpu_time` subtracts it.
pub const PRESENT_WAIT_SPAN: &str = "present-wait";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// What can go wrong between the window and the device.
#[derive(Debug)]
pub enum PresentError {
    /// No GPU backend could be opened at all.
    NoBackend(crate::backend::GpuError),
    /// The backend has no adapter, no graphics queue, or no usable format.
    Unusable(&'static str),
    /// [`ADAPTER_ENV_VAR`] named no adapter here, or named one that cannot
    /// present to this window.
    ///
    /// Never a fallback, for the reason [`crate::adapter`] gives: a run on an
    /// adapter nobody asked for is a green result about the wrong device.
    AdapterPin(PinMiss),
    /// A HAL call failed.
    Hal(HalError),
    /// A surface or swapchain call failed.
    Surface(SurfaceError),
}

impl std::fmt::Display for PresentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBackend(error) => write!(
                f,
                "{error}\n\
                 hint: `--backend null` runs the same loop against the recording \
                 no-op backend, which needs no driver."
            ),
            Self::Unusable(what) => write!(f, "the backend is unusable: {what}"),
            Self::AdapterPin(error) => write!(f, "{error}"),
            Self::Hal(error) => write!(f, "{error}"),
            Self::Surface(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for PresentError {}

impl From<crate::backend::GpuError> for PresentError {
    fn from(error: crate::backend::GpuError) -> Self {
        Self::NoBackend(error)
    }
}

impl From<HalError> for PresentError {
    fn from(error: HalError) -> Self {
        Self::Hal(error)
    }
}

impl From<PinMiss> for PresentError {
    fn from(error: PinMiss) -> Self {
        Self::AdapterPin(error)
    }
}

impl From<SurfaceError> for PresentError {
    fn from(error: SurfaceError) -> Self {
        Self::Surface(error)
    }
}

/// Why [`Presenter::start_device`] started no device.
///
/// A [`PresentError`] with one refusal told apart: the backend's
/// `create_surface` answering [`HalError::Unsupported`], which says this
/// backend cannot present to the window at all (Vulkan and a transparent
/// Win32 window). An unpinned open answers that one by trying the next
/// backend, through [`PendingInstance::fall_through`]; everything else ends
/// the open.
#[derive(Debug)]
enum StartRefusal {
    /// `create_surface` refused the target as unsupported.
    Surface(HalError),
    /// Anything else.
    Failed(PresentError),
}

impl core::fmt::Display for StartRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Surface(error) => write!(f, "{error}"),
            Self::Failed(error) => write!(f, "{error}"),
        }
    }
}

impl From<StartRefusal> for PresentError {
    fn from(refusal: StartRefusal) -> Self {
        match refusal {
            StartRefusal::Surface(error) => Self::Hal(error),
            StartRefusal::Failed(error) => error,
        }
    }
}

impl From<PresentError> for StartRefusal {
    fn from(error: PresentError) -> Self {
        Self::Failed(error)
    }
}

impl From<HalError> for StartRefusal {
    fn from(error: HalError) -> Self {
        Self::Failed(error.into())
    }
}

impl From<PinMiss> for StartRefusal {
    fn from(error: PinMiss) -> Self {
        Self::Failed(error.into())
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
    /// The window system has not asked for this frame yet, so nothing was
    /// acquired or presented: the last present carried a
    /// [`Presenter::request_frame`] whose
    /// [`FrameReady`](ShellEvent::FrameReady) has not been observed
    /// ([`Presenter::frame_due`] is `false`).
    ///
    /// **Not a failed present, and counted as neither kind.** A window on an
    /// output the compositor turned off is held here for as long as the output
    /// stays off, which is no reason to give up on the surface; a loop that
    /// counts frames that failed to present in a row leaves this out of the
    /// count, and one that counts presented frames leaves it out of that.
    /// [`acquire`](Presenter::acquire) reports it as `Ok(None)`, the answer it
    /// gives a reconfigure too, and `frame_due` is what tells the two apart.
    Held,
}

// ---------------------------------------------------------------------------
// Pacing
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
/// engine answers **once**: [`Presenter`] asks [`Device::display_timing`]
/// after its first present, settles [`Auto`](Self::Auto) against the answer,
/// and never asks again. See [`Presenter::effective_pacing`] for what that
/// resolution can and cannot see.
///
/// A caller that names [`Vsync`](Self::Vsync), [`Adaptive`](Self::Adaptive) or
/// [`Off`](Self::Off) is never overridden by that observation — it refines
/// [`Auto`](Self::Auto) and nothing else — and any of the four can be switched
/// mid-run with [`Presenter::set_pacing`].
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
    /// for benchmarks, where the frame limiter (the engine's
    /// `crcbl::engine::FrameLimit`) is the only thing pacing the loop.
    Off,
}

impl Pacing {
    /// Every pacing, in the order a settings row steps through them.
    pub const ALL: [Self; 4] = [Self::Auto, Self::Vsync, Self::Adaptive, Self::Off];

    /// The word [`from_name`](Self::from_name) reads back as this pacing: the
    /// one `--pacing` takes and `[engine.video] present_mode` holds.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Vsync => "vsync",
            Self::Adaptive => "adaptive",
            Self::Off => "off",
        }
    }

    /// The value a command line spells `name`, or `None` for a word this is not
    /// one of.
    ///
    /// Trimmed and case-folded like [`GpuBackend::from_name`], so a wrapper
    /// script that quoted a value with a stray space still works.
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
    /// arms are reachable at all (see [`Presenter::effective_pacing`]).
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
    /// [`composite_alpha_for`]'s answer at open. Kept here, not recomputed,
    /// so a resize or a pacing change rebuilds the swapchain with the alpha it
    /// was opened with; D3D12 refuses a reconfigure that changes it.
    composite_alpha: CompositeAlpha,
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
            composite_alpha: self.composite_alpha,
        }
    }
}

/// The swapchain a [`Presenter`] creates on a surface offering `caps`: the
/// preferred format, the present mode `pacing` asks for, one image more than
/// the minimum.
///
/// # Errors
///
/// [`PresentError::Unusable`] if the surface offers no format.
fn swapchain_config(
    caps: &crcbl_hal::SurfaceCaps,
    extent: (u32, u32),
    pacing: Pacing,
    composite_alpha: CompositeAlpha,
) -> Result<SwapchainConfig, PresentError> {
    let format = caps
        .preferred_format()
        .ok_or(PresentError::Unusable("the surface offers no format"))?;
    Ok(SwapchainConfig {
        label: "crcbl swapchain",
        format,
        extent,
        image_count: caps
            .min_image_count
            .saturating_add(1)
            .min(caps.max_image_count),
        present_mode: caps.choose_present_mode(pacing.preferences()),
        composite_alpha,
    })
}

/// One instance and device, shared by every [`Presenter`] opened on it.
///
/// From [`Presenter::shared`]; cloning it is cheap, and the device is torn down
/// only when every presenter on it and every handle are gone. A program with
/// windows that come and go — a bar per monitor, popups, toasts — keeps one of
/// these and opens each window with [`open`](Self::open), so no window's close
/// takes the device from the others. See
/// [`Presenter`'s docs](Presenter#one-device-for-several-windows) for why.
#[derive(Clone, Debug)]
pub struct SharedGpu {
    instance: Arc<dyn Instance>,
    device: Arc<dyn Device>,
    adapter: crcbl_hal::AdapterId,
}

impl SharedGpu {
    /// A presenter for `window` on this handle's instance and device; see
    /// [`Presenter`'s docs](Presenter#one-device-for-several-windows).
    ///
    /// Only the window's own half of `desc` is read: the label, the pacing,
    /// [`PresenterDesc::frames_in_flight`] and
    /// [`PresenterDesc::transparency`]. The backend, the adapter and its
    /// choice, and the optional features are the shared device's already; a
    /// [`PresenterDesc::required_features`] the shared device lacks is refused
    /// rather than ignored. Blocking needs no polling here, because the device
    /// already exists.
    ///
    /// The window must stay alive until the returned presenter is destroyed,
    /// as [`Presenter::request_open`] requires.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if the window went away, if
    /// [`PresenterDesc::frames_in_flight`] is zero, if the shared device lacks a
    /// required feature, if the shared adapter cannot present to the window —
    /// one on a display driven by another GPU, say, which wants a presenter of
    /// its own from [`Presenter::request_open`] — or if a HAL call fails.
    pub fn open<S: Shell + ?Sized>(
        &self,
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        desc: &PresenterDesc<'_>,
    ) -> Result<Presenter, PresentError> {
        let granted = self.device.caps().features;
        if !granted.contains(desc.required_features) {
            return Err(PresentError::Unusable(
                "the shared device lacks a feature this window requires",
            ));
        }
        let target = Presenter::target(shell, window)?;
        let mut open = PendingPresenter::new(OpenStage::Done, target, extent, desc)?;
        open.window = Some(window);

        // SAFETY: as in `start_device` — `target` came from the caller's shell
        // for a window that must outlive the presenter, which is this
        // function's documented obligation, and `destroy` tears the swapchain
        // and surface down before the caller may destroy the window.
        let surface = unsafe { self.instance.create_surface(&open.target) }?;
        let caps = match self.instance.surface_caps(surface, self.adapter) {
            Ok(caps) if caps.preferred_format().is_some() => caps,
            Ok(_) | Err(_) => {
                self.instance.destroy_surface(surface);
                return Err(PresentError::Unusable(
                    "the shared adapter cannot present to that window",
                ));
            }
        };
        let composite_alpha = match composite_alpha_for(
            open.target.transparent(),
            &caps.composite_alpha,
        ) {
            Some(alpha) => alpha,
            None if open.transparency == Transparency::Preferred => {
                log::warn!(
                    "hal: the shared adapter offers no premultiplied alpha for this transparent \
                     window; presenting it opaque, as Transparency::Preferred allows"
                );
                CompositeAlpha::Opaque
            }
            None => {
                self.instance.destroy_surface(surface);
                return Err(PresentError::Unusable(NO_PREMULTIPLIED_ALPHA));
            }
        };
        let config = match swapchain_config(&caps, extent, open.pacing, composite_alpha) {
            Ok(config) => config,
            Err(error) => {
                self.instance.destroy_surface(surface);
                return Err(error);
            }
        };
        log::info!("hal: {:?} opened on a shared device", open.label);
        Presenter::finish(
            Arc::clone(&self.instance),
            surface,
            self.adapter,
            config,
            Arc::clone(&self.device),
            &open,
        )
    }
}

/// Why an adapter whose surface offers no premultiplied alpha cannot present a
/// transparent window. See [`composite_alpha_for`].
const NO_PREMULTIPLIED_ALPHA: &str =
    "the window is transparent, and the surface offers no premultiplied alpha to present it with";

/// The composite alpha a [`Presenter`] asks for, from whether the window is
/// transparent
/// ([`SurfaceTarget::transparent`](crcbl_hal::SurfaceTarget::transparent)) and
/// what the surface offers; `None` when it cannot present the window as it
/// was made.
///
/// A transparent window gets [`CompositeAlpha::PreMultiplied`], which is the
/// alpha `WindowDesc::transparent` promises and the one every window system
/// here composites by. A surface that does not offer it is refused rather than
/// drawn opaque: a window that silently is not what was asked is the failure
/// `crcbl-vk`'s refusal of a transparent Win32 window exists to prevent, and
/// unlike a pacing preference there is no fallback that still looks right.
/// A program that would rather have a solid background says so with
/// [`Transparency::Preferred`], which the adapter walk reads, not this.
///
/// An opaque window gets [`CompositeAlpha::Opaque`], as a presenter always
/// has, even where premultiplied is also offered: Vulkan on Wayland can offer
/// it for any surface, and an opaque window composited by whatever alpha the
/// renderer happened to write would show the desktop through it.
fn composite_alpha_for(transparent: bool, offered: &[CompositeAlpha]) -> Option<CompositeAlpha> {
    if !transparent {
        return Some(CompositeAlpha::Opaque);
    }
    offered
        .contains(&CompositeAlpha::PreMultiplied)
        .then_some(CompositeAlpha::PreMultiplied)
}

/// What a [`Presenter`] does with a transparent window that no adapter can
/// composite.
///
/// Read only for a window made with `WindowDesc::transparent`; an opaque
/// window is opaque whatever this says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transparency {
    /// Refuse the open with [`PresentError::Unusable`], as a window that is
    /// silently not what was asked is worse than none. The default.
    #[default]
    Required,
    /// Fall back to the first adapter that can present the window at all, and
    /// present it opaque, with a warning in the log. For a program that would
    /// rather draw a solid background than no window; it asks
    /// [`Presenter::composite_alpha`] what it got.
    Preferred,
}

// ---------------------------------------------------------------------------
// Presenter
// ---------------------------------------------------------------------------

/// What [`Presenter::request_open`] should ask the device for.
///
/// Every field is a value the caller has already decided: which backend,
/// which adapter, which pacing. Nothing here reads a settings file or the
/// environment — the `crcbl` umbrella's `GpuContextDesc` is where a game's
/// settings become these values.
#[derive(Clone, Copy, Debug)]
pub struct PresenterDesc<'a> {
    /// The debug label for the device.
    pub label: &'a str,
    /// Which backend to open, or `None` to let [`crate::backend`]'s own table
    /// choose — and fall through past one that cannot present to the window.
    pub backend: Option<GpuBackend>,
    /// Which adapter class to open the device on, in
    /// [`ADAPTER_ENV_VAR`]'s vocabulary, or `None` to walk every adapter.
    ///
    /// [`adapter::pin`](crate::adapter::pin) is the process's own pin; a
    /// program that wants `CRCBL_ADAPTER` honoured passes
    /// `crcbl_app::adapter::pin().as_deref()` here. See [`Presenter::request_open`]
    /// for what a pin does to the walk.
    pub adapter_pin: Option<&'a str>,
    /// The adapter the program names itself, or `None` to let
    /// [`power_preference`](Self::power_preference) and the display rank them.
    /// [`adapter_pin`](Self::adapter_pin) wins over it.
    pub adapter_pick: Option<AdapterPick<'a>>,
    /// Which kind of adapter to rank first when nothing names one;
    /// [`PowerPreference::LowPower`] by default, because a program on this
    /// crate is an app. See [`crate::adapter::rank`].
    pub power_preference: PowerPreference,
    /// What to do with a transparent window no adapter can composite:
    /// [`Transparency::Required`] refuses the open, [`Transparency::Preferred`]
    /// presents it opaque.
    pub transparency: Transparency,
    /// Features the device must have. A device without them is an error.
    pub required_features: Features,
    /// Features to enable if present. Absent ones are simply not enabled — ask
    /// the returned [`Presenter::device`] what it got.
    pub optional_features: Features,
    /// How presented frames are paced against the display.
    ///
    /// The swapchain's present mode comes from this. A caller that wants to
    /// change it after start-up calls [`Presenter::set_pacing`], which rebuilds
    /// the swapchain: the mode is a swapchain property and cannot be edited in
    /// place.
    pub pacing: Pacing,
    /// How many frames may be in flight before a frame waits for the oldest;
    /// [`FRAMES_IN_FLIGHT`] by default. At least one.
    ///
    /// The depth of whatever per-frame ring the caller records into: a frame
    /// is recorded once the frame this many back has finished executing, so a
    /// ring this deep is never written while the device is reading it.
    pub frames_in_flight: usize,
    /// The GPU an earlier run saved from [`Presenter::gpu_choice`], to open on
    /// again; see [`crate::gpu_choice`]. [`Presenter::gpu_choice_outcome`]
    /// says what became of it. `None` by default.
    pub gpu_choice: Option<&'a GpuChoice>,
}

impl Default for PresenterDesc<'_> {
    fn default() -> Self {
        Self {
            label: "crcbl",
            backend: None,
            adapter_pin: None,
            adapter_pick: None,
            power_preference: PowerPreference::LowPower,
            transparency: Transparency::Required,
            required_features: Features::empty(),
            // `PRESENT_FEEDBACK` is asked for here because `acquire` already
            // calls `wait_until_presented` every frame: a device that has the
            // capability and was never asked for it answers that call
            // immediately forever, and the closed loop is dead code nothing can
            // reach. `PRESENT_TIMING` is the same argument for the same reason:
            // `submit_and_present` calls `display_timing` every frame, and
            // unasked-for it answers `Unknown` forever, so the presenter would
            // report every panel as unreadable and never negotiate the
            // extension chain that could have told it otherwise. Optional —
            // a device without them just keeps the open-loop frame limiter and
            // the `Unknown` cadence.
            optional_features: Features::PRESENT_FEEDBACK | Features::PRESENT_TIMING,
            pacing: Pacing::default(),
            frames_in_flight: FRAMES_IN_FLIGHT,
            gpu_choice: None,
        }
    }
}

/// A window's device and swapchain, driven entirely through the `crcbl-hal`
/// seam.
///
/// Nothing in this struct names a backend. [`Presenter::request_open`] asks
/// [`crate::backend`] for one **by value** and everything after it is
/// `dyn Instance` / `dyn Device` — which is what made the swap from the null
/// backend to `crcbl-vk` a change to one argument.
///
/// # One device for several windows
///
/// [`SharedGpu::open`] opens a second window's presenter on the first
/// one's instance and device, which both then hold: each window keeps its own
/// surface, swapchain and frames in flight, and the device goes when the last
/// of them does. Cheaper per window than a device each — measured on 2026-10-22
/// at about 110 ms and 25 MiB a window on NVIDIA's 615 driver, and 16 ms and
/// under 1 MiB on Mesa's ANV — and the only way to have two windows on NVIDIA
/// under Wayland that survives one of them closing: that driver crashes in the
/// second window's swapchain teardown once the first window's own device and
/// instance are gone.
#[derive(Debug)]
pub struct Presenter {
    instance: Arc<dyn Instance>,
    device: Arc<dyn Device>,
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
    /// [`PresenterDesc::frames_in_flight`].
    frames_in_flight: usize,
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
    /// The window presented to, or `None` offscreen: what
    /// [`request_frame`](Self::request_frame) asks for and which
    /// [`FrameReady`](ShellEvent::FrameReady) [`observe`](Self::observe)
    /// takes as this presenter's answer.
    window: Option<WindowId>,
    /// Whether a frame request can be answered: `false` for a device that
    /// records rather than draws, presenting to a real window's surface.
    ///
    /// The answer to a request rides the commit a present makes, and the null
    /// device's present commits nothing, so on Wayland a request it carried
    /// would never be answered and the gate would stay shut for good. A
    /// headless window's target is
    /// [`Offscreen`](crcbl_hal::SurfaceTarget::Offscreen), where the shell
    /// answers at the next pump whatever the device did, so requests there are
    /// kept: they are how the gate is tested.
    answers_requests: bool,
    /// Where the window's frame request stands; see [`frame_gate`].
    frame: FrameGate,
    /// The choice of the adapter this presenter opened on, for the program to
    /// save; see [`Presenter::gpu_choice`].
    gpu_choice: GpuChoice,
    /// What became of the choice the program gave; see
    /// [`Presenter::gpu_choice_outcome`].
    gpu_choice_outcome: ChoiceOutcome,
    /// Scratch, reused every frame so a steady-state frame allocates nothing.
    waits: Vec<SemaphoreWait>,
    signals: Vec<SemaphoreSignal>,
    batch: Vec<CommandBufferHandle>,
}

/// How far [`PendingPresenter`] has got.
///
/// Exactly the two steps that can take longer than an instant, in order: the
/// backend instance (adapter enumeration is a promise on the web) and then the
/// device (`requestDevice` is a promise everywhere). Everything between them —
/// surface creation, surface-aware adapter selection, format and present-mode
/// choice — is synchronous on every backend and happens in one step when the
/// instance lands.
#[derive(Debug)]
enum OpenStage {
    Instance(PendingInstance),
    Device {
        instance: Box<dyn Instance>,
        surface: SurfaceHandle,
        adapter: crcbl_hal::AdapterId,
        config: SwapchainConfig,
        pending: Box<dyn crcbl_hal::PendingDevice>,
    },
    /// The presenter has been handed over, or a step failed.
    Done,
}

/// A [`Presenter`] being opened, one poll at a time.
///
/// From [`Presenter::request_open`]. Poll it until it yields; see
/// [`crcbl_hal::device`] for why start-up is shaped this way rather than
/// blocking, and [`crate::backend`] for the instance half of it.
///
/// Dropping one mid-flight abandons the open. The surface, if one was already
/// created, is destroyed with the instance — the same teardown obligation
/// [`Presenter::destroy`] discharges, except that no swapchain exists yet.
#[derive(Debug)]
pub struct PendingPresenter {
    stage: OpenStage,
    target: crcbl_hal::SurfaceTarget,
    /// The window `target` describes, or `None` offscreen.
    window: Option<WindowId>,
    extent: (u32, u32),
    label: String,
    required_features: Features,
    optional_features: Features,
    /// Carried from the desc because the swapchain is built at the end of the
    /// open, several polls after the caller handed it over.
    pacing: Pacing,
    frames_in_flight: usize,
    /// [`PresenterDesc::adapter_pin`], owned because the open outlives the
    /// desc it came from.
    adapter_pin: Option<String>,
    /// [`PresenterDesc::adapter_pick`], owned for the same reason.
    adapter_pick: Option<OwnedPick>,
    /// [`PresenterDesc::power_preference`].
    power_preference: PowerPreference,
    /// [`PresenterDesc::transparency`].
    transparency: Transparency,
    /// [`PresenterDesc::backend`], kept to open again without a saved choice
    /// that turned out not to hold.
    backend: Option<GpuBackend>,
    /// The saved choice while it is still being tried: given, not ignored and
    /// not yet found stale.
    choice: Option<GpuChoice>,
    /// Whether the instance being opened holds the choice alone — its Vulkan
    /// driver, or its D3D12 adapter — so a refusal there means reopening
    /// without it.
    scoped: bool,
    /// What has become of the choice so far.
    outcome: ChoiceOutcome,
}

/// An [`AdapterPick`] that owns its name, for a [`PendingPresenter`] that
/// outlives the desc the pick was borrowed from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum OwnedPick {
    Class(crcbl_hal::DeviceType),
    Name(String),
}

impl OwnedPick {
    fn new(pick: AdapterPick<'_>) -> Self {
        match pick {
            AdapterPick::Class(class) => Self::Class(class),
            AdapterPick::Name(name) => Self::Name(name.to_owned()),
        }
    }

    fn borrow(&self) -> AdapterPick<'_> {
        match self {
            Self::Class(class) => AdapterPick::Class(*class),
            Self::Name(name) => AdapterPick::Name(name),
        }
    }
}

impl PendingPresenter {
    /// A request for `target` whose instance is `stage`'s, with everything
    /// else from `desc`.
    fn new(
        stage: OpenStage,
        target: crcbl_hal::SurfaceTarget,
        extent: (u32, u32),
        desc: &PresenterDesc<'_>,
    ) -> Result<Self, PresentError> {
        if desc.frames_in_flight == 0 {
            return Err(PresentError::Unusable(
                "a presenter needs at least one frame in flight",
            ));
        }
        Ok(Self {
            stage,
            target,
            window: None,
            extent,
            label: desc.label.to_string(),
            required_features: desc.required_features,
            optional_features: desc.optional_features,
            pacing: desc.pacing,
            frames_in_flight: desc.frames_in_flight,
            adapter_pin: desc.adapter_pin.map(str::to_owned),
            adapter_pick: desc.adapter_pick.map(OwnedPick::new),
            power_preference: desc.power_preference,
            transparency: desc.transparency,
            backend: desc.backend,
            choice: None,
            scoped: false,
            outcome: ChoiceOutcome::NotGiven,
        })
    }

    /// Drops the saved choice as no longer holding, for `why`, and opens again
    /// on every driver when the instance was on its driver alone.
    fn choice_stale(&mut self, why: String) -> Result<(), PresentError> {
        log::info!("hal: the saved GPU choice no longer holds ({why}); opening without it");
        self.outcome = ChoiceOutcome::Stale(why);
        self.choice = None;
        if core::mem::take(&mut self.scoped) {
            self.stage = OpenStage::Instance(request_backend(self.backend)?);
        }
        Ok(())
    }

    /// Advances the open. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`PresentError`] if no backend opened, if the backend exposes no
    /// adapter, no graphics queue or no surface format, if the adapter pin
    /// names an adapter that is not here or cannot present (see
    /// [`Presenter::request_open`]), or if any HAL call failed. Polling after the
    /// presenter was handed over is a caller bug and reports
    /// [`PresentError::Unusable`].
    pub fn poll(&mut self) -> Result<Option<Presenter>, PresentError> {
        loop {
            match core::mem::replace(&mut self.stage, OpenStage::Done) {
                OpenStage::Instance(mut pending) => {
                    let Some(instance) = pending.poll()? else {
                        self.stage = OpenStage::Instance(pending);
                        return Ok(None);
                    };
                    // The stage is left `Done` if this fails, which is what
                    // makes a failed open stay failed rather than retrying a
                    // half-built presenter on the next frame.
                    let backend = instance.backend();
                    // An instance on every driver, checked against the saved
                    // choice: one on its driver alone was checked as it opened.
                    let on = GpuBackend::of_kind(backend);
                    let (other_backend, absent) =
                        self.choice.as_ref().map_or((false, false), |choice| {
                            let other = on != choice.backend();
                            let named = instance
                                .adapters()
                                .iter()
                                .any(|adapter| choice.names(adapter));
                            (other, !other && !named)
                        });
                    if other_backend {
                        self.outcome = ChoiceOutcome::Ignored(Ignored::OtherBackend(on));
                        self.choice = None;
                    } else if absent {
                        self.choice_stale(format!("no {on} adapter here is the one it names"))?;
                    }
                    let scoped = self.scoped;
                    match Presenter::start_device(instance, self) {
                        Err(refusal) if scoped => {
                            let error: PresentError = refusal.into();
                            self.choice_stale(error.to_string())?;
                        }
                        Ok(stage) => self.stage = stage,
                        Err(StartRefusal::Surface(error)) if pending.fall_through(&error) => {
                            // Never silent: which backend a window ended up
                            // on, and why not the first, is the question a
                            // bug report starts with.
                            log::info!(
                                "hal: the {backend} backend cannot present to this {} window \
                                 ({error}); trying the next backend",
                                self.target.platform_name()
                            );
                            self.stage = OpenStage::Instance(pending);
                        }
                        Err(refusal) => return Err(refusal.into()),
                    }
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
                        return Presenter::finish(
                            Arc::from(instance),
                            surface,
                            adapter,
                            config,
                            Arc::from(device),
                            self,
                        )
                        .map(Some);
                    }
                },
                OpenStage::Done => {
                    return Err(PresentError::Unusable(
                        "this GPU context was already opened",
                    ));
                }
            }
        }
    }
}

/// The backend instance `backend` names, or the table's choice, not yet open.
fn request_backend(backend: Option<GpuBackend>) -> Result<PendingInstance, PresentError> {
    Ok(match backend {
        Some(backend) => crate::backend::request_open_backend(backend)?,
        None => crate::backend::request_open()?,
    })
}

/// Where an open with `desc` starts: on the saved choice's Vulkan driver
/// alone where it has one and that driver still shows its adapter, on its
/// D3D12 adapter alone where that is still here, else as `desc` asks. Sets
/// `open`'s choice, its outcome so far and whether the instance holds the
/// choice alone; see [`crate::gpu_choice`].
fn start_open(desc: &PresenterDesc<'_>, open: &mut PendingPresenter) -> Result<(), PresentError> {
    let Some(choice) = desc.gpu_choice else {
        open.stage = OpenStage::Instance(request_backend(desc.backend)?);
        return Ok(());
    };
    let backend = choice.backend();
    let pinned = desc.adapter_pin.is_some();
    let picked = desc.adapter_pick.is_some();
    open.outcome = if let Some(why) = crate::gpu_choice::overridden(pinned, picked, backend) {
        ChoiceOutcome::Ignored(Ignored::Overridden(why))
    } else if desc.backend.is_some_and(|wanted| wanted != backend) {
        ChoiceOutcome::Ignored(Ignored::OtherBackend(backend))
    } else if matches!(backend, GpuBackend::Vulkan | GpuBackend::Dx12) {
        open.choice = Some(choice.clone());
        ChoiceOutcome::Used
    } else {
        ChoiceOutcome::Ignored(Ignored::Unsupported(backend))
    };
    #[cfg(target_os = "windows")]
    if let Some(choice) = open
        .choice
        .as_ref()
        .filter(|choice| choice.backend() == GpuBackend::Dx12)
    {
        match crcbl_dx12::Dx12Instance::open_adapter(choice.luid(), |adapter| choice.names(adapter))
        {
            Ok(instance) => {
                log::info!("hal: opening on the saved GPU choice's D3D12 adapter alone");
                open.scoped = true;
                open.stage = OpenStage::Instance(crate::backend::opened(
                    GpuBackend::Dx12,
                    Box::new(instance),
                ));
                return Ok(());
            }
            Err(why) => open.choice_stale(why)?,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(manifest) = open.choice.as_ref().and_then(GpuChoice::vulkan_driver) {
        let opened = crcbl_vk::drivers::Driver::read(manifest)
            .map_err(|error| error.to_string())
            .and_then(|driver| {
                crcbl_vk::VkInstance::open_driver(&driver).map_err(|error| error.to_string())
            });
        match opened {
            Ok(instance) => {
                log::info!(
                    "hal: opening on the saved GPU choice's driver alone, {}",
                    manifest.display()
                );
                open.scoped = true;
                open.stage = OpenStage::Instance(crate::backend::opened(
                    GpuBackend::Vulkan,
                    Box::new(instance),
                ));
                return Ok(());
            }
            Err(why) => open.choice_stale(why)?,
        }
    }
    open.stage = OpenStage::Instance(request_backend(desc.backend)?);
    Ok(())
}

impl Presenter {
    /// Creates an instance, a surface for `window`, a device and a swapchain.
    ///
    /// `extent` must come from the window system — call this only after the
    /// first configure ([`crate::window::wait_for_configure`]), because a
    /// swapchain needs a size and an unconfigured window does not have one.
    ///
    /// Blocks until the device arrives, so it does not exist on `wasm32`: in a
    /// browser the promise behind that device is resolved by the event loop
    /// this call would be *inside*. Browser code calls
    /// [`request_open`](Self::request_open) and polls.
    ///
    /// [`request_open`](Self::request_open) says which adapter is taken.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if no backend opened, if the backend exposes no
    /// adapter, no graphics queue or no surface format, if the adapter pin
    /// names an adapter that is not here or cannot present, or if any HAL call
    /// fails.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        desc: &PresenterDesc<'_>,
    ) -> Result<Self, PresentError> {
        Self::request_open(shell, window, extent, desc)?.block()
    }

    /// The same presenter with no window behind it, at `extent`.
    ///
    /// [`SurfaceTarget::Offscreen`](crcbl_hal::SurfaceTarget) is a target every
    /// backend implements and none of them dereferences, so this needs no shell,
    /// no display server and no `unsafe` from the caller. Frames are acquired,
    /// submitted and "presented" exactly as they are on a window; a present to
    /// an offscreen ring simply returns the image to the ring.
    ///
    /// Blocking, like [`open`](Self::open), and absent on `wasm32` for the same
    /// reason.
    ///
    /// # Errors
    ///
    /// [`PresentError`] on everything [`open`](Self::open) reports it for,
    /// minus the window.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_offscreen(
        extent: (u32, u32),
        desc: &PresenterDesc<'_>,
    ) -> Result<Self, PresentError> {
        Self::request_open_offscreen(extent, desc)?.block()
    }

    /// Starts opening the instance, surface, device and swapchain, without
    /// blocking.
    ///
    /// The non-blocking form of `open`, and the only one a browser
    /// can use: both halves of start-up that a browser defers — adapter
    /// enumeration and `requestDevice` — are promises there. Poll the returned
    /// [`PendingPresenter`] once per frame until it yields.
    ///
    /// The window must stay alive from this call until the presenter is
    /// destroyed; the surface is created part-way through, once an instance
    /// exists.
    ///
    /// # Which adapter
    ///
    /// [`crate::adapter::choose`] decides. With neither
    /// [`PresenterDesc::adapter_pin`] nor [`PresenterDesc::adapter_pick`] set,
    /// every adapter is walked in [`crate::adapter::rank`]'s order for
    /// [`PresenterDesc::power_preference`] and the first that can present to
    /// the window is taken — a GPU that cannot present is stepped past. With
    /// either set, only the adapter it names is tried, and one that cannot
    /// present fails the open with [`PresentError::AdapterPin`] rather than
    /// falling back to an adapter nobody named. The start-up log line says
    /// which adapter was taken and what chose it.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if the registry has no such backend, if
    /// [`PresenterDesc::frames_in_flight`] is zero, or if the window went away
    /// before its surface could be described. Everything else is reported from
    /// [`PendingPresenter::poll`].
    pub fn request_open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        desc: &PresenterDesc<'_>,
    ) -> Result<PendingPresenter, PresentError> {
        // A value from a registry rather than a type, which is the whole
        // difference between "the caller knows about Vulkan" and "the caller
        // knows there are backends".
        let target = Self::target(shell, window)?;
        let mut pending = PendingPresenter::new(OpenStage::Done, target, extent, desc)?;
        pending.window = Some(window);
        start_open(desc, &mut pending)?;
        Ok(pending)
    }

    /// [`request_open`](Self::request_open) with no window behind it, as
    /// `open_offscreen` opens.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if the registry has no such backend or
    /// [`PresenterDesc::frames_in_flight`] is zero.
    pub fn request_open_offscreen(
        extent: (u32, u32),
        desc: &PresenterDesc<'_>,
    ) -> Result<PendingPresenter, PresentError> {
        let mut pending = PendingPresenter::new(
            OpenStage::Done,
            crcbl_hal::SurfaceTarget::Offscreen,
            extent,
            desc,
        )?;
        start_open(desc, &mut pending)?;
        Ok(pending)
    }

    /// [`request_open`](Self::request_open) on an instance the caller already
    /// opened, rather than one from [`crate::backend`]'s table.
    ///
    /// For a caller that has to hold the instance itself: a test holding the
    /// null backend's recorder, or a tool that enumerated the adapters before
    /// deciding. [`PresenterDesc::backend`] is not read — the instance already
    /// is one — and nothing falls through to another backend if this one
    /// cannot present to the window.
    ///
    /// The surface is created and the adapter chosen here, so a window this
    /// instance cannot serve is refused by this call rather than by a poll.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if the window went away, if
    /// [`PresenterDesc::frames_in_flight`] is zero, or on anything
    /// [`PendingPresenter::poll`] reports while choosing the adapter and
    /// starting the device.
    pub fn request_open_on<S: Shell + ?Sized>(
        instance: Box<dyn Instance>,
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        desc: &PresenterDesc<'_>,
    ) -> Result<PendingPresenter, PresentError> {
        let target = Self::target(shell, window)?;
        let mut pending = PendingPresenter::new(OpenStage::Done, target, extent, desc)?;
        pending.window = Some(window);
        pending.stage = Self::start_device(instance, &pending)?;
        Ok(pending)
    }

    /// A handle on this presenter's instance and device, for opening more
    /// windows on them with [`SharedGpu::open`]. See [the type's
    /// docs](Self#one-device-for-several-windows).
    #[must_use]
    pub fn shared(&self) -> SharedGpu {
        SharedGpu {
            instance: Arc::clone(&self.instance),
            device: Arc::clone(&self.device),
            adapter: self.adapter,
        }
    }

    /// The surface target `shell` describes `window` with.
    ///
    /// The join: `shell` produced this, and only a HAL backend looks inside
    /// it. Taken when the open starts rather than at poll time so the shell is
    /// not borrowed for the whole of start-up.
    fn target<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
    ) -> Result<crcbl_hal::SurfaceTarget, PresentError> {
        let target = shell.surface_target(window).map_err(|_| {
            PresentError::Unusable("the window went away before its surface was made")
        })?;
        log::debug!(
            "hal: creating a surface for a {} target",
            target.platform_name()
        );
        Ok(target)
    }

    /// Creates the surface, picks an adapter and starts the device request.
    ///
    /// `open` is the request being started, for the same reason
    /// [`finish`](Self::finish) takes it.
    fn start_device(
        instance: Box<dyn Instance>,
        open: &PendingPresenter,
    ) -> Result<OpenStage, StartRefusal> {
        let (target, extent, label, pacing) = (&open.target, open.extent, &open.label, open.pacing);
        let adapters = instance.adapters();
        if adapters.is_empty() {
            return Err(PresentError::Unusable("no adapter").into());
        }
        // Resolved before the surface exists, so a pin that names nothing here
        // leaves nothing to tear down.
        let pin = open.adapter_pin.as_deref();
        let pick = open.adapter_pick.as_ref().map(OwnedPick::borrow);
        let mut choice = crate::adapter::choose(&adapters, pin, pick, open.power_preference, None)?;

        // SAFETY: `target` was produced by the caller's shell for a window that
        // must still be live — `request_open` documents that obligation — so
        // every handle in it names an object of the stated kind. The caller
        // keeps the shell, and therefore the window, alive until after
        // `destroy`, which tears the swapchain and surface down first. This is
        // the whole `Instance::create_surface` contract.
        let surface = match unsafe { instance.create_surface(target) } {
            Ok(surface) => surface,
            Err(error @ HalError::Unsupported { .. }) => return Err(StartRefusal::Surface(error)),
            Err(error) => return Err(error.into()),
        };

        // Ranked again now that there is a surface to ask about: which adapter
        // drives the window's display can overrule the preference. A backend
        // that cannot tell, or a query that fails, leaves the preference alone.
        let display = match instance.display_adapter(surface) {
            Ok(display) => display,
            Err(error) => {
                log::debug!("hal: cannot tell which adapter drives this window's display: {error}");
                None
            }
        };
        if let Choice::Ranked(ranked) = &mut choice {
            *ranked = crate::adapter::rank(&adapters, open.power_preference, display);
            // The saved choice ahead of the ranking: it is what this program
            // ran on, and the walk still goes on past it if it cannot present.
            if let Some(saved) = &open.choice
                && let Some(at) = ranked.iter().position(|adapter| saved.names(adapter))
            {
                let adapter = ranked.remove(at);
                ranked.insert(0, adapter);
            }
        }
        let candidates = choice.candidates();

        // **Adapter selection is surface-aware, and has to be.** P1.1 found a
        // discrete radv GPU that enumerates first, is Tier A, and *cannot
        // present to an Xvfb window* — while the software rasteriser behind it
        // can. An `Err` here means "not this one", not "give up". A pin narrows
        // the walk to one adapter, so there it does mean "give up".
        //
        // A transparent window narrows it again: an adapter whose surface
        // offers no premultiplied alpha cannot present it as it was made, so
        // the walk goes on to the next one, and only when none can is the
        // open refused (see `composite_alpha_for`).
        //
        // **The adapter's caps are asked last, of the one adapter that passed
        // everything else**, because on D3D12 asking creates a driver device —
        // the start-up cost lazy description exists to avoid paying for every
        // adapter. One the driver will not open is "not this one" too.
        let transparent = target.transparent();
        let mut chosen = None;
        let mut last_error = None;
        let mut alpha_refused = false;
        let mut opaque_fallback = None;
        for &adapter in candidates {
            match instance.surface_caps(surface, adapter.id) {
                Ok(caps) if caps.preferred_format().is_some() => {
                    let Some(alpha) = composite_alpha_for(transparent, &caps.composite_alpha)
                    else {
                        log::debug!(
                            "hal: adapter {:?} offers no premultiplied alpha for this transparent \
                             window (only {:?}); trying the next",
                            adapter.name,
                            caps.composite_alpha
                        );
                        alpha_refused = true;
                        if opaque_fallback.is_none() {
                            opaque_fallback = Some((adapter, caps));
                        }
                        continue;
                    };
                    let device_caps = match instance.adapter_caps(adapter.id) {
                        Ok(device_caps) => device_caps,
                        Err(error) => {
                            log::debug!(
                                "hal: adapter {:?} will not open ({error}); trying the next",
                                adapter.name
                            );
                            last_error = Some(error);
                            continue;
                        }
                    };
                    chosen = Some((adapter.clone(), caps, alpha, device_caps));
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
        // Only once every adapter was asked: one that composites the window as
        // it was made always wins over drawing it opaque.
        if chosen.is_none()
            && open.transparency == Transparency::Preferred
            && let Some((adapter, caps)) = opaque_fallback
        {
            match instance.adapter_caps(adapter.id) {
                Ok(device_caps) => {
                    log::warn!(
                        "hal: no adapter offers premultiplied alpha for this transparent \
                         window; presenting it opaque on {:?}, as Transparency::Preferred \
                         allows",
                        adapter.name
                    );
                    chosen = Some((adapter.clone(), caps, CompositeAlpha::Opaque, device_caps));
                }
                Err(error) => last_error = Some(error),
            }
        }
        let Some((adapter, caps, composite_alpha, device_caps)) = chosen else {
            instance.destroy_surface(surface);
            let named = match (&choice, pin, pick) {
                (Choice::Pinned(named), Some(pin), _) => Some((
                    *named,
                    format!("{ADAPTER_ENV_VAR}={}", pin.trim()),
                    format!("unset {ADAPTER_ENV_VAR} to walk every adapter"),
                )),
                (Choice::Picked(named), _, Some(pick)) => Some((
                    *named,
                    format!("the program's pick {pick:?}"),
                    "pick none to walk every adapter".to_owned(),
                )),
                _ => None,
            };
            return Err(StartRefusal::Failed(match (named, last_error) {
                (Some((named, who, remedy)), error) => {
                    let reason = match error {
                        Some(error) => error.to_string(),
                        None if alpha_refused => NO_PREMULTIPLIED_ALPHA.to_owned(),
                        None => "it offers no usable surface format".to_owned(),
                    };
                    PresentError::AdapterPin(PinMiss::new(format!(
                        "{who} chose adapter {id} {name:?} type={kind:?}, and it cannot present \
                         to this window: {reason}. No other adapter is tried, because that \
                         would be a run on a device nobody named; {remedy}.",
                        id = named.id.0,
                        name = named.name,
                        kind = named.device_type,
                    )))
                }
                // Before an adapter that could not see the surface at all: one
                // that could, and lacked only the alpha, is the closer miss.
                _ if alpha_refused => PresentError::Unusable(NO_PREMULTIPLIED_ALPHA),
                (_, Some(error)) => error.into(),
                (_, None) => PresentError::Unusable("no adapter can present to this window"),
            }));
        };
        let why = match (&choice, pin, pick) {
            (Choice::Pinned(_), Some(pin), _) => {
                format!("pinned by {ADAPTER_ENV_VAR}={}", pin.trim())
            }
            (Choice::Picked(_), _, Some(pick)) => format!("picked by the program, {pick:?}"),
            _ => format!(
                "the first of {} enumerated that can present, ranked for {:?} with the display \
                 on adapter {}, {ADAPTER_ENV_VAR} unset",
                adapters.len(),
                open.power_preference,
                display.map_or_else(|| "unknown".to_owned(), |id| id.0.to_string()),
            ),
        };
        log::info!(
            "hal: {} adapter {:?} ({:?}), {why}, geometry {:?}, binding {:?}, lighting {:?}",
            instance.backend(),
            adapter.name,
            adapter.device_type,
            device_caps.geometry_path(),
            device_caps.binding_model(),
            device_caps.lighting_path()
        );

        if let Some(reported) = caps.current_extent
            && reported != extent
        {
            log::info!(
                "hal: surface reports {reported:?} but the shell configured {extent:?}; \
                 using the shell's size"
            );
        }
        let config = swapchain_config(&caps, extent, pacing, composite_alpha)?;

        // The one call that may take more than an instant. On the web it is a
        // promise; here it is a `PendingDevice` the caller polls.
        let pending = instance.request_device(&DeviceDesc {
            label: Some(label),
            adapter: adapter.id,
            required_features: open.required_features,
            optional_features: open.optional_features,
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

    /// Builds the presenter once the device has arrived.
    ///
    /// `open` is the request being finished — everything decided before the
    /// device arrived and still needed after it: the extent, the pacing and
    /// the ring depth. Carried as the object rather than unpacked into more
    /// parameters, because the next thing start-up learns early and uses late
    /// would be one more.
    fn finish(
        instance: Arc<dyn Instance>,
        surface: SurfaceHandle,
        adapter: crcbl_hal::AdapterId,
        config: SwapchainConfig,
        device: Arc<dyn Device>,
        open: &PendingPresenter,
    ) -> Result<Self, PresentError> {
        let (extent, pacing, frames_in_flight) = (open.extent, open.pacing, open.frames_in_flight);
        let queue = device
            .queue(QueueKind::Graphics)
            .ok_or(PresentError::Unusable("no graphics queue"))?;
        // `is_gpu` rather than a capability: what is asked is whether a present
        // reaches the window at all, which no real backend fails and the
        // recorder never does — not a difference between backends that draw.
        let answers_requests =
            device.backend().is_gpu() || matches!(open.target, crcbl_hal::SurfaceTarget::Offscreen);

        let swapchain = device.create_swapchain(&config.desc(surface))?;
        let (format, present_mode, image_count, alpha) = (
            config.format,
            config.present_mode,
            config.image_count,
            config.composite_alpha,
        );
        log::info!(
            "hal: swapchain {}x{} {format:?} {present_mode:?} {alpha:?} alpha ({image_count} images)",
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
            log::info!("hal: pacing on presents, {frames_in_flight} frames deep");
        } else {
            log::debug!("hal: no present feedback; the frame limiter is the only pacing");
        }

        let opened_on = instance
            .adapters()
            .into_iter()
            .find(|info| info.id == adapter)
            .ok_or(PresentError::Unusable(
                "the opened adapter is not the instance's",
            ))?;
        let mut gpu_choice = GpuChoice::of(GpuBackend::of_kind(instance.backend()), &opened_on);
        let mut gpu_choice_outcome = open.outcome.clone();
        if let Some(saved) = &open.choice {
            if saved.names(&opened_on) {
                if let Some(driver) = saved.vulkan_driver().filter(|_| open.scoped) {
                    gpu_choice = gpu_choice.with_vulkan_driver(driver.to_owned());
                }
            } else {
                gpu_choice_outcome =
                    ChoiceOutcome::Stale("its adapter cannot present this window".to_owned());
            }
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
            frames_in_flight,
            in_flight: VecDeque::with_capacity(frames_in_flight + 1),
            configured_extent: extent,
            // Not asked yet, and the query is only meaningful after a present —
            // see `settle_pacing`.
            observed_timing: None,
            window: open.window,
            answers_requests,
            frame: FrameGate::Open,
            gpu_choice,
            gpu_choice_outcome,
            waits: Vec::with_capacity(1),
            signals: Vec::with_capacity(2),
            batch: Vec::with_capacity(1),
        })
    }

    /// The device, for building renderers and recording work.
    #[must_use]
    pub fn device(&self) -> &dyn Device {
        self.device.as_ref()
    }

    /// The choice of the adapter this presenter opened on, for the program to
    /// save and give back as [`PresenterDesc::gpu_choice`] next time; see
    /// [`crate::gpu_choice`]. On Vulkan it carries the driver's manifest when
    /// the open used one; otherwise
    /// `GpuChoice::locate_vulkan_driver` finds it, off the start-up path.
    #[must_use]
    pub fn gpu_choice(&self) -> &GpuChoice {
        &self.gpu_choice
    }

    /// What became of [`PresenterDesc::gpu_choice`]: used, stale — save
    /// [`gpu_choice`](Self::gpu_choice) in its place — ignored, or not given.
    #[must_use]
    pub fn gpu_choice_outcome(&self) -> &ChoiceOutcome {
        &self.gpu_choice_outcome
    }

    /// The swapchain everything renders into, for a caller that wants the
    /// seam's present-feedback calls directly — `Device::wait_until_presented`
    /// is how a frame is held for the display.
    #[must_use]
    pub const fn swapchain(&self) -> SwapchainHandle {
        self.swapchain
    }

    /// The adapter this presenter opened its device on.
    ///
    /// **The only way an application can say which GPU it is running on.** Every
    /// other place that names one is inside a backend, writing to the log, so a
    /// caller that wants it on screen or in a test's output has nothing to read
    /// — and "which device produced this frame" is the first question asked of
    /// any measurement.
    ///
    /// `None` only if the adapter list changed under the presenter, which no
    /// backend does while a device is open.
    #[must_use]
    pub fn adapter(&self) -> Option<crcbl_hal::AdapterInfo> {
        self.instance
            .adapters()
            .into_iter()
            .find(|info| info.id == self.adapter)
    }

    /// The composite alpha the swapchain presents with:
    /// [`CompositeAlpha::PreMultiplied`] for a transparent window composited
    /// as it was made, [`CompositeAlpha::Opaque`] for an opaque one, or for a
    /// transparent one [`Transparency::Preferred`] let fall back.
    #[must_use]
    pub const fn composite_alpha(&self) -> CompositeAlpha {
        self.config.composite_alpha
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

    /// How many frames may be in flight: [`PresenterDesc::frames_in_flight`].
    #[must_use]
    pub const fn frames_in_flight(&self) -> usize {
        self.frames_in_flight
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
    /// settles this for the life of the presenter. It is not re-read on a
    /// resize, a display-mode change, or a window dragged to another monitor,
    /// and [`set_pacing`](Self::set_pacing) does not re-read it either.
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
    /// Detection happens once per presenter, and this call is not it: `Auto`
    /// resolves against the sample the first present already took. Before that
    /// present there is no sample, so `Auto` set here is vsync until the first
    /// present takes one — the same start-up path a presenter opened on `Auto`
    /// follows, and the same single query.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if the swapchain could not be rebuilt. The old
    /// swapchain is still configured when that happens — `reconfigure_swapchain`
    /// replaces nothing until the new one is built — so the presenter stays
    /// usable, and [`pacing`](Self::pacing),
    /// [`effective_pacing`](Self::effective_pacing) and the swapchain's own mode
    /// are rolled back together rather than left describing a change that did
    /// not take.
    pub fn set_pacing(&mut self, pacing: Pacing) -> Result<(), PresentError> {
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
    /// **`depth` behind it**, `depth` being the frames in flight. The two ends
    /// of that range are both wrong: waiting for `submitted + 1` is waiting for
    /// something that has not been submitted, and waiting for `submitted` — the
    /// frame just sent — empties the pipeline every frame, so the CPU sits out
    /// a whole display period doing nothing and the result is worse than not
    /// waiting at all. `pacing` is the **effective** one — [`Pacing::Auto`] has
    /// already been resolved by the time a frame is paced, and the `Off` test
    /// below would otherwise have to guess what `Auto` meant.
    ///
    /// One `depth` back is the id whose display leaves exactly `depth` presents
    /// outstanding, which is the depth [`retire_to`](Self::retire_to) already
    /// holds command buffers to; the wait therefore paces the loop without
    /// changing that depth.
    ///
    /// `None` in two cases. The first `depth` frames have nothing that far
    /// behind them — ids start at 1, so the subtraction lands on 0 or
    /// underflows — and the loop is still filling anyway. And [`Pacing::Off`]
    /// asked *not* to be paced by the display: blocking until a frame is on
    /// screen would pin the loop to the refresh rate, which is the one thing
    /// that mode exists to avoid.
    fn present_to_wait_for(submitted: u64, pacing: Pacing, depth: usize) -> Option<u64> {
        if matches!(pacing, Pacing::Off) {
            return None;
        }
        submitted
            .saturating_add(1)
            .checked_sub(depth as u64)
            .filter(|id| *id > 0)
    }

    /// Asks the shell for a [`FrameReady`](ShellEvent::FrameReady) for this
    /// presenter's window, to pace the frame about to be presented.
    ///
    /// Call it after [`acquire`](Self::acquire) hands over a frame and before
    /// [`submit_and_present`](Self::submit_and_present): on Wayland the
    /// request is surface state that rides the present's commit, and a
    /// request made after the present waits for a commit that may never come.
    /// Once that present has reached the swapchain, `acquire` hands over no
    /// frame until [`observe`](Self::observe) has seen the answer — see the
    /// [module docs](self) for the whole loop.
    ///
    /// A presenter whose caller never calls this paces exactly as before.
    /// One with no window behind it
    /// ([`request_open_offscreen`](Self::request_open_offscreen)) has nothing
    /// to ask for, and this does nothing; nor does one on the null backend
    /// presenting to a real window, whose presents commit nothing that could
    /// carry the request, so the answer would never come.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] if the window has gone away; the gate is
    /// left as it was, so a window that is gone never holds it shut.
    pub fn request_frame<S: Shell + ?Sized>(&mut self, shell: &mut S) -> Result<(), ShellError> {
        let Some(window) = self.window.filter(|_| self.answers_requests) else {
            return Ok(());
        };
        shell.request_frame(window)?;
        self.frame = self.frame.requested();
        Ok(())
    }

    /// Reads one shell event for what it says about this presenter's frame
    /// request: the [`FrameReady`](ShellEvent::FrameReady) that answers it,
    /// or the [`WindowDestroyed`](ShellEvent::WindowDestroyed) after which no
    /// answer will come. Either lets [`acquire`](Self::acquire) hand over
    /// frames again; every other event, and either one for another window,
    /// changes nothing.
    ///
    /// Hand it every event [`Shell::pump`] delivers. A caller that requests
    /// frames and does not feed this waits for an answer that never arrives,
    /// on every backend alike — see [`frame_due`](Self::frame_due).
    pub fn observe(&mut self, event: &ShellEvent) {
        if let ShellEvent::FrameReady { window } | ShellEvent::WindowDestroyed { window } = event
            && self.window == Some(*window)
        {
            self.frame = FrameGate::Open;
        }
    }

    /// Whether [`acquire`](Self::acquire) would hand over a frame, as far as
    /// frame pacing goes: `false` from a present that carried a
    /// [`request_frame`](Self::request_frame) until [`observe`](Self::observe)
    /// sees its answer.
    ///
    /// A loop with nothing else to do waits for the shell's next event while
    /// this is `false` — [`Shell::wait_events`] — rather than spinning; the
    /// answer is an event, so it wakes the wait.
    ///
    /// # The same gate on every backend
    ///
    /// Where [`ShellCaps::FRAME_CALLBACK`](crcbl_shell::ShellCaps::FRAME_CALLBACK)
    /// is set the compositor answers, and holds the answer back while nothing
    /// of the window is shown — an output it turned off, where a FIFO present
    /// could block forever. Elsewhere the answer comes at the next pump. The
    /// gate does not read the capability: a loop that pumps once a frame
    /// presents every frame where the cap is clear, as it did without the
    /// gate, and one code path means a caller that forgets
    /// [`observe`](Self::observe) stops at its second frame on every backend
    /// and in every headless test, not only on Wayland. What it changes there
    /// is that two presents need a pump between them.
    #[must_use]
    pub const fn frame_due(&self) -> bool {
        self.frame.is_due()
    }

    /// Acquires the next swapchain image, or says why not.
    ///
    /// `Ok(None)` means skip this frame and try the next one, for one of two
    /// reasons: the swapchain was out of date and has been reconfigured, or
    /// the last present carried a [`request_frame`](Self::request_frame) whose
    /// answer [`observe`](Self::observe) has not seen yet
    /// ([`frame_due`](Self::frame_due) is `false`). The second touches no
    /// device: an acquire is where a FIFO swapchain on an output that is off
    /// blocks.
    ///
    /// # Errors
    ///
    /// [`PresentError`] for anything except a swapchain that has merely gone
    /// out of date, which is handled here.
    pub fn acquire(&mut self) -> Result<Option<AcquiredFrame>, PresentError> {
        if !self.frame.is_due() {
            return Ok(None);
        }

        // Before anything else, because an error the device reported out of
        // band is a reason not to record another frame. On a backend that
        // reports every failure through its return values this is `None` every
        // time; on WebGPU it is the *only* way a failed pipeline is ever heard
        // from, and until this call existed the answer was a black canvas over
        // a game that reported itself as playing. Start of the frame rather
        // than end of the last one so that failures during start-up — where
        // every pipeline is built — are caught by the first frame.
        if let Some(message) = self.device.take_error() {
            return Err(PresentError::Hal(HalError::Backend(message)));
        }

        // The two blocking calls, and only those, under one span: this is where
        // a frame waits for the display rather than for the CPU, and
        // `crcbl::perf::frame_cpu_time` subtracts it. The reconfigure the match
        // below can run is deliberately outside — it is work, not waiting.
        let next = {
            let _waiting = crcbl_core::trace::span(PRESENT_WAIT_SPAN);

            // Closed-loop pacing, before an image is taken and before any work is
            // recorded: this is the point in a frame where the CPU can be held back
            // without holding anything else up. A device without present feedback
            // answers immediately, which is why there is no branch here on which
            // backend is underneath.
            if let Some(present_id) = Self::present_to_wait_for(
                self.submitted,
                self.effective_pacing,
                self.frames_in_flight,
            ) {
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
    /// [`PresentError`] if the submit, the present or the retire failed. A
    /// present that reports the swapchain out of date is not an error: it
    /// reconfigures and reports [`FrameOutcome::Reconfigured`].
    pub fn submit_and_present(
        &mut self,
        acquired: &AcquiredFrame,
        command_buffer: CommandBufferHandle,
    ) -> Result<FrameOutcome, PresentError> {
        self.submit_and_present_with(acquired, command_buffer, &[], |_, _| Ok(()))
    }

    /// [`submit_and_present`](Self::submit_and_present), with `also` submitted
    /// in the same batch after `command_buffer`, and `after_present` run once
    /// the present has answered.
    ///
    /// What a caller that copies the presented image out uses — the engine's
    /// `--screenshot`: the copy has to ride in the *same* batch as the frame
    /// that drew it, because submission order is what puts the copy's barrier
    /// after the frame's last pass, and a second submit would be a second batch
    /// with nothing ordering it against the first. `also` is the caller's to
    /// destroy; only `command_buffer` joins the ring of frames in flight.
    ///
    /// `after_present` is handed the device and what the present did:
    ///
    /// - [`FrameOutcome::Presented`] after the frame reached the swapchain and
    ///   the pacing settled against it, and before a suboptimal swapchain is
    ///   rebuilt — the image `also` read is still the one that was presented.
    /// - [`FrameOutcome::Reconfigured`] when the present reported the swapchain
    ///   out of date, before it is rebuilt.
    ///
    /// An error from `after_present` ends the call with it, and nothing after
    /// that point runs.
    ///
    /// # Errors
    ///
    /// [`PresentError`] on everything
    /// [`submit_and_present`](Self::submit_and_present) reports it for,
    /// converted into `E`, and whatever `after_present` returns.
    pub fn submit_and_present_with<E: From<PresentError>>(
        &mut self,
        acquired: &AcquiredFrame,
        command_buffer: CommandBufferHandle,
        also: &[CommandBufferHandle],
        after_present: impl FnOnce(&dyn Device, FrameOutcome) -> Result<(), E>,
    ) -> Result<FrameOutcome, E> {
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

        // The frame's own command buffer first and whatever rides with it
        // after, in one batch: submission order is what orders them.
        self.batch.clear();
        self.batch.push(command_buffer);
        self.batch.extend_from_slice(also);

        self.device
            .submit(
                self.queue,
                &SubmitInfo {
                    command_buffers: &self.batch,
                    waits: &self.waits,
                    signals: &self.signals,
                },
            )
            .map_err(PresentError::from)?;
        self.in_flight.push_back((value, command_buffer));
        // One frame left executing, not the whole ring: the next frame records
        // into the per-frame ring slot this wait frees, and the ring is
        // `frames_in_flight` deep. Keeping the whole ring in flight let the
        // next frame write the slot the oldest was still reading — hidden by
        // the present wait in a window, and a live race offscreen and under
        // `Pacing::Off`, where nothing else holds the CPU back.
        self.retire_to(self.frames_in_flight - 1)?;

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
                after_present(self.device.as_ref(), FrameOutcome::Reconfigured)?;
                self.reconfigure()?;
                return Ok(FrameOutcome::Reconfigured);
            }
            Err(error) => return Err(PresentError::from(error).into()),
        }

        // The present committed, so a request made for this frame is now owed
        // an answer, and the next frame waits for it.
        self.frame = self.frame.presented();

        // After the present, never before it — and after *this* present rather
        // than at the top of the next frame, because the whole question is what
        // the display did with a frame it has been given.
        self.settle_pacing()?;

        after_present(self.device.as_ref(), FrameOutcome::Presented)?;

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
    /// diagnostic into a `PresentError` would let it kill a loop that is
    /// working. A failed *rebuild* is another matter and is reported: the
    /// caller asked for a pacing the presenter then could not deliver, and
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
    fn settle_pacing(&mut self) -> Result<(), PresentError> {
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
    /// [`PresentError`] if the surface could not be queried or the swapchain
    /// could not be rebuilt. On a failed rebuild the old swapchain is still the
    /// configured one, so `config` is rolled back to describe it rather than
    /// the mode that did not take.
    fn apply_present_mode(&mut self) -> Result<(), PresentError> {
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
    /// # An unsatisfied wait stops the retirement
    ///
    /// [`Device::wait_semaphores`] returns `Ok(false)` for a wait that was not
    /// satisfied, and the seam is explicit that this is an outcome rather than
    /// an error. Destroying the command buffer anyway would free memory the
    /// device may still be reading — the exact use-after-free the wait exists
    /// to prevent — so an unsatisfied wait is reported and the buffer is put
    /// back at the front of the queue, unretired, for a later call to wait on
    /// again.
    ///
    /// `u64::MAX` does not make this unreachable. It means "no timeout" on the
    /// APIs that have one, but the seam takes `timeout_ns` as a number and
    /// `crcbl-hal`'s null device answers from the recorded timeline without
    /// consulting it at all — so a value nothing has signalled comes back
    /// `Ok(false)` there however long the caller asked to wait.
    /// `crcbl_render::MeshPool::flush` treats the same answer the same way.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if waiting failed, and [`PresentError::Unusable`] if a
    /// wait completed without being satisfied.
    pub fn retire_to(&mut self, keep: usize) -> Result<(), PresentError> {
        while self.in_flight.len() > keep {
            let (value, command_buffer) = self
                .in_flight
                .pop_front()
                .unwrap_or_else(|| unreachable!("the queue is non-empty above"));
            match self.timeline {
                Some(semaphore) => {
                    let satisfied = {
                        let _waiting = crcbl_core::trace::span(PRESENT_WAIT_SPAN);
                        self.device
                            .wait_semaphores(&[SemaphoreWait { semaphore, value }], u64::MAX)?
                    };
                    if !satisfied {
                        // Back where it came from, still owned and still
                        // undestroyed: the caller may retry, and a buffer
                        // dropped here would leak instead of being freed once
                        // the timeline catches up.
                        self.in_flight.push_front((value, command_buffer));
                        return Err(PresentError::Unusable(
                            "a command buffer's timeline wait finished unsatisfied, so the \
                             device may still be reading it",
                        ));
                    }
                }
                // The Tier B fallback. Correct, and coarse enough that the log
                // line in `open` exists to explain the frame rate.
                None => {
                    let _waiting = crcbl_core::trace::span(PRESENT_WAIT_SPAN);
                    self.device.wait_idle()?;
                }
            }
            self.device.destroy_command_buffer(command_buffer);
        }
        Ok(())
    }

    /// Resizes the swapchain to `extent`.
    ///
    /// Waits for every frame in flight and retires it first, as
    /// [`drain`](Self::drain) does, so a caller resizing after presenting has
    /// nothing to drain itself.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if the reconfigure failed. A zero extent is *not* an
    /// error — a minimized window reports one, and the swapchain is left alone.
    pub fn resize(&mut self, extent: (u32, u32)) -> Result<(), PresentError> {
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

    fn reconfigure(&mut self) -> Result<(), PresentError> {
        log::debug!(
            "hal: reconfiguring the swapchain to {}x{}",
            self.config.extent.0,
            self.config.extent.1
        );
        // **Nothing in flight may survive into the reconfigure.** A command
        // buffer that drew into a back buffer holds its own reference to it
        // until it is destroyed, and `ResizeBuffers` refuses while any
        // reference is left (`DXGI_ERROR_INVALID_CALL`): the backend can wait
        // for the queue, but it cannot destroy a command buffer this presenter
        // still owns. Every caller that resized after presenting hit that on
        // D3D12, so the presenter drains here rather than asking each one to.
        self.drain()?;
        // Reconfigure, never destroy-and-recreate: the handle stays valid
        // across a resize storm, which is what the seam promises callers.
        self.device
            .reconfigure_swapchain(self.swapchain, &self.config.desc(self.surface))?;
        // `observed_timing` is deliberately *not* cleared. A rebuilt swapchain
        // has had nothing presented to it and would answer `Unknown` again, so
        // clearing the latch would re-run the whole resolution on every resize
        // — and on a driver that never says anything but `Unknown`, that is the
        // per-frame query this design exists to avoid. Detection happens once
        // per presenter; a window dragged to a VRR monitor keeps the pacing it
        // started with until the caller asks for another with `set_pacing`.
        Ok(())
    }

    /// Waits for the device to go idle and retires everything still in flight.
    ///
    /// Call this before destroying anything the caller owns — renderers, pools,
    /// timers — and [`Presenter::destroy`] afterwards.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if waiting failed.
    pub fn drain(&mut self) -> Result<(), PresentError> {
        self.device.wait_idle()?;
        self.retire_to(0)
    }

    /// Tears the swapchain and the surface down, in the order the seam
    /// requires. The device must already be idle — call [`Presenter::drain`]
    /// and release the caller's own objects first.
    ///
    /// # Errors
    ///
    /// [`PresentError`] if a final wait failed, or if the device had an
    /// out-of-band error left to report.
    pub fn destroy(mut self) -> Result<(), PresentError> {
        self.drain()?;
        if let Some(semaphore) = self.timeline.take() {
            self.device.destroy_semaphore(semaphore);
        }
        // Swapchain before surface, surface before the window the caller owns;
        // the device is allowed to outlive its instance, so their drop order is
        // the one the struct declares.
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);

        // **The last chance anything has to read the out-of-band channel.**
        // `acquire` drains it at the top of every frame, which leaves the whole
        // of teardown uncovered: the final submit's errors do not arrive until
        // it has completed, and there is no frame after that to hear them. A
        // run that violated the specification on its way out used to exit 0.
        // After the destroys above rather than before, so the teardown calls
        // themselves are inside what this reports.
        if let Some(message) = self.device.take_error() {
            return Err(PresentError::Hal(HalError::Backend(message)));
        }
        Ok(())
    }
}

impl PendingPresenter {
    /// Polls until the presenter arrives. The blocking half of
    /// [`Presenter::open`].
    #[cfg(not(target_arch = "wasm32"))]
    fn block(mut self) -> Result<Presenter, PresentError> {
        loop {
            if let Some(presenter) = self.poll()? {
                return Ok(presenter);
            }
            std::thread::yield_now();
        }
    }
}

#[cfg(test)]
mod tests;
