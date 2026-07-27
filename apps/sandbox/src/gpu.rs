//! Where the two seams meet: a `crcbl-shell` window becomes a `crcbl-hal`
//! surface, a swapchain, and an acquire/present pair.
//!
//! This module is the whole point of P0.7. `crcbl-shell` has been complete
//! since P0.6 and `crcbl-hal` since P0.3, but nothing had ever *joined* them —
//! and the join is where a seam mismatch would show up. Driving it now, against
//! [`NullBackend`](crcbl::hal::null), is the cheapest possible version of the
//! check: no driver, no GPU, no window required, and every call recorded.
//!
//! # The frame is a real frame, not a stub
//!
//! It would be shorter to acquire an image and present it immediately. That
//! shape is wrong in a way the null backend cannot catch: **a swapchain image
//! is acquired in an undefined layout and must be transitioned to
//! [`ResourceState::Present`] before the compositor may have it.** On Vulkan,
//! presenting an image that was never transitioned is a validation error and,
//! on some drivers, a black window. So the frame here is the smallest *correct*
//! one —
//!
//! ```text
//! acquire → encode(barrier Undefined → Present) → submit(wait acquire, signal present)
//!         → present(wait present) → retire the command buffer
//! ```
//!
//! — which is exactly the frame `crcbl-vk` will run at P1 with a render pass
//! inserted in the middle. See the [findings](#what-the-join-revealed) below for
//! what driving it turned up.
//!
//! # Frames in flight, not `wait_idle`
//!
//! [`Device::destroy_command_buffer`] may not be called until the submission
//! that used it has completed, and the seam offers exactly two ways to know
//! that: a timeline semaphore, or [`Device::wait_idle`] — which the seam itself
//! documents as "a shutdown and test primitive" that "destroys pipelining". So
//! this keeps a two-deep ring keyed on a timeline semaphore value, and falls
//! back to `wait_idle` only on a Tier B device that has no timeline semaphores.
//! It costs about fifteen lines and it is the shape P1 keeps.
//!
//! # What the join revealed
//!
//! P0.7 was the first time anything drove both seams at once, and
//! `docs/plan/01-foundations.md` freezes neither at P0. Two of the four
//! findings turned into seam changes; the other two are recorded where they
//! are still live.
//!
//! 1. **Two sources of truth for the swapchain extent, with no stated
//!    precedence** — *fixed in the seam.*
//!    [`WindowState::size`](crcbl::shell::WindowState::size) is one;
//!    [`SurfaceCaps::current_extent`](crcbl::hal::SurfaceCaps::current_extent)
//!    is the other, and on Vulkan it is a real size on X11 and deliberately
//!    `0xFFFFFFFF` ("you choose") on Wayland. The null backend reports `None`,
//!    so nothing forced the question until something joined the two seams.
//!    `crcbl-hal`'s [`swapchain`](crcbl::hal::swapchain) module now states the
//!    rule as four numbered backend obligations — the shell's size is
//!    authoritative, `current_extent` is an optional cross-check, the sentinel
//!    never escapes into the seam, and a zero extent means "do not create one
//!    yet". [`Gpu::open`] is the reference implementation of the caller's half.
//! 2. **[`SurfaceTarget::Offscreen`](crcbl::core::SurfaceTarget) embedded a
//!    size, so a headless target went stale on resize** — *fixed by deleting
//!    the size.* Every other variant names handles, which a resize does not
//!    touch, which is why the seam only requires re-querying a target after a
//!    *mode* change. One variant carrying an extent made that rule wrong for
//!    one backend, so `Offscreen` is now fieldless and the rule is complete
//!    again. [`Gpu::resize`] therefore reconfigures the swapchain and nothing
//!    else, on every backend.
//! 3. **`unsafe` at the join is unavoidable and lands in application code.**
//!    [`Instance::create_surface`] is `unsafe` because it dereferences platform
//!    handles, and the safety obligation ("these outlive the surface") is one
//!    only the code holding *both* the shell and the device can discharge. That
//!    is this module, in an app that otherwise contains no `unsafe` at all.
//!    Left as-is for P0 and written down at the seam: the likely answer is a
//!    shell-aware constructor in `crcbl-render`, which does not exist yet.
//! 4. **Teardown order is stated in three places and enforced in none.** The
//!    swapchain must die before the surface, the surface before the window, and
//!    the device may outlive its instance. [`Gpu::destroy`] does it by hand;
//!    nothing would have caught getting it wrong except a real driver.

use std::collections::VecDeque;

use crcbl::hal::null::NullInstance;
use crcbl::hal::{
    AcquiredFrame, Barriers, CommandBufferHandle, CommandEncoderDesc, DeviceDesc, Features, Format,
    HalError, ImageBarrier, ImageSubresourceRange, PresentInfo, PresentMode, QueueHandle,
    QueueKind, ResourceState, SemaphoreDesc, SemaphoreHandle, SemaphoreKind, SemaphoreSignal,
    SemaphoreWait, SubmitInfo, SurfaceError, SurfaceHandle, SwapchainDesc, SwapchainHandle,
};
use crcbl::prelude::*;
use crcbl::shell::WindowId;

/// How many frames may be in flight before the loop waits for the oldest.
///
/// Two is the classic double-buffered default: one frame being recorded while
/// one is executing. The number is here rather than inline because P1's render
/// graph will want to configure it.
const FRAMES_IN_FLIGHT: usize = 2;

/// What one [`Gpu::frame`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    /// The frame was recorded, submitted and presented.
    Presented,
    /// The swapchain no longer matched the surface, so it was reconfigured and
    /// this frame was skipped. Expected traffic during a resize, not an error.
    Reconfigured,
}

/// The engine's GPU side, driven entirely through the `crcbl-hal` seam.
///
/// Nothing in this struct names a backend: [`NullInstance`] appears in exactly
/// one line of [`Gpu::open`] and everything after it is `dyn Instance` /
/// `dyn Device`. Swapping in `crcbl-vk` at P1 changes that one line.
#[derive(Debug)]
pub struct Gpu {
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
    /// Scratch, reused every frame so a steady-state frame allocates nothing.
    waits: Vec<SemaphoreWait>,
    signals: Vec<SemaphoreSignal>,
}

/// The swapchain parameters, kept so a reconfigure changes exactly one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwapchainConfig {
    format: Format,
    extent: (u32, u32),
    image_count: u32,
    present_mode: PresentMode,
}

/// What can go wrong between the window and the device.
#[derive(Debug)]
pub enum GpuError {
    /// The backend has no adapter, no graphics queue, or no usable format.
    Unusable(&'static str),
    /// A HAL call failed.
    Hal(HalError),
    /// A surface or swapchain call failed.
    Surface(SurfaceError),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unusable(what) => write!(f, "the backend is unusable: {what}"),
            Self::Hal(error) => write!(f, "{error}"),
            Self::Surface(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for GpuError {}

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

impl Gpu {
    /// Creates an instance, a surface for `window`, a device and a swapchain.
    ///
    /// `extent` must come from the window system — call this only after the
    /// first configure, because a swapchain needs a size and an unconfigured
    /// window does not have one.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the backend exposes no adapter, no graphics queue or no
    /// surface format, or if any HAL call fails.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
    ) -> Result<Self, GpuError> {
        // The one line that names a backend. P1 replaces it with a registry
        // shaped like `crcbl_shell::backend::open`, and nothing below changes.
        let instance: Box<dyn Instance> = Box::new(NullInstance::tier_a());

        let adapters = instance.adapters();
        let adapter = adapters
            .first()
            .ok_or(GpuError::Unusable("no adapter"))?
            .clone();
        log::info!(
            "hal: {} adapter {:?} ({:?}), tier {:?}",
            instance.backend(),
            adapter.name,
            adapter.device_type,
            adapter.caps.tier()
        );

        // The join. `shell` produced this; only a HAL backend looks inside it.
        let target = shell
            .surface_target(window)
            .map_err(|_| GpuError::Unusable("the window went away before its surface was made"))?;
        log::debug!(
            "hal: creating a surface for a {} target",
            target.platform_name()
        );

        // SAFETY: `target` was produced by `shell` for a window that is live
        // right now, so every handle in it names an object of the stated kind.
        // The caller keeps the shell — and therefore the window — alive until
        // after `destroy`, which tears the swapchain and surface down first.
        // This is the whole `Instance::create_surface` contract, and this
        // module is the only place in the app that can discharge it.
        let surface = unsafe { instance.create_surface(&target) }?;

        let caps = instance.surface_caps(surface, adapter.id)?;
        // The caller's half of the seam's extent rule (finding 1): the shell's
        // size wins, `current_extent` is a cross-check, and a mismatch is worth
        // a line in the log rather than an override — on Wayland the surface
        // has no opinion at all, and on X11 it can be a frame behind the
        // configure that has already been handled.
        if let Some(reported) = caps.current_extent
            && reported != extent
        {
            log::debug!(
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

        let device = instance.create_device(&DeviceDesc {
            label: Some("sandbox"),
            adapter: adapter.id,
            // Nothing here needs a feature, and demanding `TIER_A` would refuse
            // to run on the Tier B devices `docs/plan/02-vulkan-backend.md`
            // requires the engine to support. Ask for everything optionally and
            // branch on what came back — which is exactly what the timeline
            // semaphore below does.
            required_features: Features::empty(),
            optional_features: Features::TIER_A,
            compatible_surface: Some(surface),
        })?;
        let queue = device
            .queue(QueueKind::Graphics)
            .ok_or(GpuError::Unusable("no graphics queue"))?;

        let config = SwapchainConfig {
            format,
            extent,
            image_count,
            present_mode,
        };
        let swapchain = device.create_swapchain(&config.desc(surface))?;
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
                label: Some("sandbox frames in flight"),
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
            waits: Vec::with_capacity(1),
            signals: Vec::with_capacity(2),
        })
    }

    /// The swapchain's current size.
    #[must_use]
    pub fn extent(&self) -> (u32, u32) {
        self.config.extent
    }

    /// The format the swapchain was created with. Test-only; see
    /// `Loop::format` for why the gate is here rather than a `#[allow]`.
    #[cfg(test)]
    pub fn format(&self) -> Format {
        self.config.format
    }

    /// Records, submits and presents one frame.
    ///
    /// # Errors
    ///
    /// [`GpuError`] for anything except a swapchain that has merely gone out of
    /// date, which is handled here and reported as
    /// [`FrameOutcome::Reconfigured`].
    pub fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
        let acquired = match self.device.acquire_next_frame(self.swapchain) {
            Ok(frame) => frame,
            // Expected traffic after a resize, per the seam's docs: reconfigure
            // and let the next frame have the image.
            Err(SurfaceError::OutOfDate) => {
                self.reconfigure()?;
                return Ok(FrameOutcome::Reconfigured);
            }
            Err(error) => return Err(error.into()),
        };

        self.record_and_submit(&acquired)?;
        self.device.present(
            self.queue,
            &PresentInfo {
                swapchain: self.swapchain,
                // `Option::as_slice` is the whole cost of the seam's
                // "the swapchain owns its synchronisation" decision: on a
                // backend with an implicit present this is an empty slice and
                // the code above is unchanged.
                waits: acquired.present_semaphore.as_slice(),
            },
        )?;

        if acquired.suboptimal {
            // Legal to ignore for one frame, and the seam says treating it as
            // fatal is a bug — so the frame is presented first, then fixed.
            log::debug!("hal: swapchain suboptimal; reconfiguring after present");
            self.reconfigure()?;
        }
        Ok(FrameOutcome::Presented)
    }

    /// Encodes the one barrier a no-draw frame still owes, submits it, and
    /// retires whatever fell out of the frames-in-flight window.
    fn record_and_submit(&mut self, acquired: &AcquiredFrame) -> Result<(), GpuError> {
        let mut encoder = self.device.create_command_encoder(&CommandEncoderDesc {
            label: Some("sandbox frame"),
            queue: self.queue,
        });
        // Nothing draws yet. The barrier is not a placeholder: an acquired
        // image's contents are undefined and the compositor may only be handed
        // one in `Present`, so this is the minimum a correct frame contains.
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                acquired.image,
                ImageSubresourceRange::all(self.config.format),
                ResourceState::Undefined,
                ResourceState::Present,
            )],
            ..Barriers::default()
        });
        let command_buffer = encoder.finish()?;

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
        self.signals.extend(self.timeline.map(|semaphore| {
            SemaphoreSignal {
                semaphore,
                // A timeline value is monotonic, so the frame number *is* the
                // value and no rotation is needed.
                value,
            }
        }));

        self.device.submit(
            self.queue,
            &SubmitInfo {
                command_buffers: &[command_buffer],
                waits: &self.waits,
                signals: &self.signals,
            },
        )?;
        self.in_flight.push_back((value, command_buffer));
        self.retire_to(FRAMES_IN_FLIGHT)
    }

    /// Waits for and destroys command buffers until at most `keep` are in
    /// flight.
    fn retire_to(&mut self, keep: usize) -> Result<(), GpuError> {
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
                // line above exists to explain the frame rate.
                None => self.device.wait_idle()?,
            }
            self.device.destroy_command_buffer(command_buffer);
        }
        Ok(())
    }

    /// Resizes the swapchain to `extent`.
    ///
    /// The [`SurfaceTarget`] this surface was created from is *not* re-queried,
    /// and since finding 2 that is true on every backend rather than on most of
    /// them: a target names handles — a `wl_surface*`, an `xcb_window_t` — and
    /// a resize does not touch one. Only a mode change can invalidate a target,
    /// which is the only case the shell seam asks a caller to re-query for.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the reconfigure failed. A zero extent is *not* an error
    /// — a minimized window reports one, and the swapchain is simply left
    /// alone.
    pub fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        if extent == self.config.extent {
            return Ok(());
        }
        if extent.0 == 0 || extent.1 == 0 {
            // Every backend must reject a zero-extent swapchain (Vulkan forbids
            // one outright), so this is the caller's job and not the seam's.
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

    /// Tears everything down in the order the seam requires.
    ///
    /// Explicit rather than `Drop` because every step can fail and a `Drop` that
    /// swallows errors is how a leak becomes invisible. The caller destroys the
    /// window *after* this returns.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if waiting for outstanding work failed. Everything after
    /// the wait is infallible.
    pub fn destroy(mut self) -> Result<(), GpuError> {
        // Nothing may be destroyed while the device might still be using it.
        self.device.wait_idle()?;
        self.retire_to(0)?;
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

impl SwapchainConfig {
    fn desc(self, surface: SurfaceHandle) -> SwapchainDesc<'static> {
        SwapchainDesc {
            label: Some("sandbox swapchain"),
            surface,
            format: self.format,
            extent: self.extent,
            image_count: self.image_count,
            present_mode: self.present_mode,
            composite_alpha: crcbl::hal::CompositeAlpha::Opaque,
        }
    }
}
