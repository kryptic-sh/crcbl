//! Where the two seams meet: a `crcbl-shell` window becomes a `crcbl-hal`
//! surface, a swapchain, and an acquire/present pair.
//!
//! This module was the whole point of P0.7: `crcbl-shell` had been complete
//! since P0.6 and `crcbl-hal` since P0.3, but nothing had ever *joined* them,
//! and the join is where a seam mismatch shows up. P0.7 drove it against
//! [`NullBackend`](crcbl::hal::null); **P1.1 drives the same code against real
//! Vulkan**, and the only thing that changed is which backend
//! [`crcbl::backend::open_backend`] returns.
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
//! acquire → encode(barrier Undefined → ColorAttachment
//!                  render pass: LoadOp::Clear
//!                  barrier ColorAttachment → Present)
//!         → submit(wait acquire, signal present + timeline)
//!         → present(wait present) → retire the command buffer
//! ```
//!
//! — which is `docs/plan/02-vulkan-backend.md`'s **milestone 1**, "clear colour
//! through the graph". The clear is a real render pass with
//! [`LoadOp::Clear`](crcbl::hal::LoadOp), not a `vkCmdClearColorImage`: the
//! attachment, the load op and the layout transition into
//! `COLOR_ATTACHMENT_OPTIMAL` are the machinery every later milestone is built
//! on, and a clear that skipped them would prove almost nothing.
//!
//! # The swapchain hands over its view and its extent
//!
//! A render pass needs an `ImageViewHandle` and a render area. Neither is
//! derived here: [`AcquiredFrame`] carries both, so a frame is
//! acquire-and-render with no per-image bookkeeping and no allocation.
//!
//! Both fields exist *because of this module*. P1.1 first built the view cache
//! by hand — one view per ring slot, keyed on `AcquiredFrame::index`, rebuilt
//! whenever a reconfigure reissued the image handles — and that is duplicated
//! work every consumer and every backend would repeat, which is the same
//! argument that already put the two semaphores on `AcquiredFrame`. The extent
//! is there because the size a swapchain was *configured* at is not always the
//! size that was *asked for*: see finding 5 below.
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
//!    nothing would have caught getting it wrong except a real driver — and at
//!    P1.1 a real driver *is* in the room, with validation on, and it agrees.
//!
//! # What P1.1 added to the list
//!
//! 5. **The swapchain's configured extent was unobservable** — *fixed in the
//!    seam.* `crcbl-vk` may legally have to configure at a size other than the
//!    one asked for: on X11 `minImageExtent == maxImageExtent ==
//!    currentExtent`, so a resize event still in flight leaves *no* legal
//!    swapchain at the shell's size. The backend must clamp, and the seam had
//!    no way to say what it clamped to, so a frame could be rendered at a size
//!    the swapchain did not have. [`AcquiredFrame::extent`] now carries it, the
//!    obligations are reworded around request-versus-answer, and
//!    [`Gpu::extent`] reports what was configured rather than what was asked
//!    for.
//! 6. **A render pass needed a view the seam would not give it** — *fixed in
//!    the seam.* [`AcquiredFrame::view`] now sits beside the semaphores, for
//!    the reason they do.

use std::collections::VecDeque;

use crcbl::backend::GpuBackend;
use crcbl::hal::{
    AcquiredFrame, Barriers, ClearValue, ColorAttachment, CommandBufferHandle, CommandEncoderDesc,
    DeviceDesc, Features, Format, HalError, ImageBarrier, ImageSubresourceRange, LoadOp,
    PresentInfo, PresentMode, QueueHandle, QueueKind, Rect2d, RenderPassDesc, ResourceState,
    SemaphoreDesc, SemaphoreHandle, SemaphoreKind, SemaphoreSignal, SemaphoreWait, StoreOp,
    SubmitInfo, SurfaceError, SurfaceHandle, SwapchainDesc, SwapchainHandle,
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

/// The colour every frame clears to.
///
/// A recognisable non-grey so "the window is black" and "the clear ran" are
/// distinguishable at a glance, and so a channel swap is visible rather than
/// plausible.
const CLEAR_COLOR: [f32; 4] = [0.05, 0.10, 0.18, 1.0];

/// The engine's GPU side, driven entirely through the `crcbl-hal` seam.
///
/// Nothing in this struct names a backend. [`Gpu::open`] asks
/// [`crcbl::backend::open_backend`] for one **by value** and everything after
/// it is `dyn Instance` / `dyn Device` — which is what made P1.1's swap from
/// the null backend to `crcbl-vk` a change to one argument rather than to this
/// file.
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
    /// The extent the swapchain was last *configured* at, from
    /// [`AcquiredFrame::extent`].
    ///
    /// Distinct from `config.extent`, which is what the shell asked for. The
    /// two differ while a window is being dragged on a platform that pins the
    /// permitted range — obligations 2 and 3. Seeded from the request, because
    /// there is no frame to ask until the first acquire.
    configured_extent: (u32, u32),
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
    /// No GPU backend could be opened at all.
    NoBackend(crcbl::backend::GpuError),
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
            Self::NoBackend(error) => write!(
                f,
                "{error}\n\
                 hint: `--backend null` runs the same loop against the recording \
                 no-op backend, which needs no driver."
            ),
            Self::Unusable(what) => write!(f, "the backend is unusable: {what}"),
            Self::Hal(error) => write!(f, "{error}"),
            Self::Surface(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for GpuError {}

impl From<crcbl::backend::GpuError> for GpuError {
    fn from(error: crcbl::backend::GpuError) -> Self {
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

impl Gpu {
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
        backend: Option<GpuBackend>,
    ) -> Result<Self, GpuError> {
        // The line that used to name `NullInstance`. It now names a *value*
        // from a registry, which is the whole difference between "the sandbox
        // knows about Vulkan" and "the sandbox knows there are backends" — the
        // same shape `crcbl_shell::backend::open` has had since P0.5.
        let instance: Box<dyn Instance> = match backend {
            Some(backend) => crcbl::backend::open_backend(backend)?,
            None => crcbl::backend::open()?,
        };

        let adapters = instance.adapters();
        if adapters.is_empty() {
            return Err(GpuError::Unusable("no adapter"));
        }

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

        // **Adapter selection is surface-aware, and has to be.** Taking
        // `adapters()[0]` was fine against the null backend and is wrong against
        // a real one: P1.1 found a discrete radv GPU that enumerates first, is
        // Tier A, and *cannot present to an Xvfb window* because there is no
        // DRI3 — while the software rasteriser sitting behind it can. The seam
        // enumerates adapters without reference to a surface, so the only way to
        // find out is to ask [`Instance::surface_caps`] per adapter, which is
        // exactly what it is for ("caps known before a device exists").
        //
        // An `Err` here therefore means "not this one", not "give up": a backend
        // has no other way to say that a particular adapter/surface pairing does
        // not work. Anything left over after the loop is a real failure.
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
            // Destroy the surface before giving up: the shell's window outlives
            // this call either way, but a leaked surface is a leaked driver
            // object, and `crcbl-vk` says so at teardown.
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

        // The caller's half of the seam's extent rule (finding 1): the shell's
        // size wins, `current_extent` is a cross-check, and a mismatch is worth
        // a line in the log rather than an override — on Wayland the surface
        // has no opinion at all, and on X11 it can be a frame behind the
        // configure that has already been handled.
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
            configured_extent: extent,
            waits: Vec::with_capacity(1),
            signals: Vec::with_capacity(2),
        })
    }

    /// The swapchain's current size — the one it was **configured** at.
    ///
    /// Obligation 3: this is what the last frame actually rendered into, which
    /// is not always what the shell asked for. Before the first acquire it is
    /// the request, because there is no answer yet.
    #[must_use]
    pub fn extent(&self) -> (u32, u32) {
        self.configured_extent
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

        // Obligation 3: the answer, not the request. On a platform that pins
        // the permitted extent this can differ from `config.extent` for as long
        // as a resize is in flight, and everything downstream — the render
        // area here, `Gpu::extent` for the summary — must use this one.
        if acquired.extent != self.configured_extent {
            log::debug!(
                "hal: swapchain configured at {:?} (the shell asked for {:?})",
                acquired.extent,
                self.config.extent
            );
            self.configured_extent = acquired.extent;
        }

        self.record_and_submit(&acquired)?;
        match self.device.present(
            self.queue,
            &PresentInfo {
                swapchain: self.swapchain,
                // `Option::as_slice` is the whole cost of the seam's
                // "the swapchain owns its synchronisation" decision: on a
                // backend with an implicit present this is an empty slice and
                // the code above is unchanged.
                waits: acquired.present_semaphore.as_slice(),
            },
        ) {
            Ok(()) => {}
            // **Present is the usual place a resize is noticed**, not acquire:
            // the compositor learns the window changed size when it is handed a
            // frame, and the acquire that would have reported it never happens.
            // Handling `OutOfDate` only at acquire meant dragging a window edge
            // exited the sandbox with an error, which is precisely the bug the
            // seam warns about — "a caller that treats it as fatal has a bug
            // that only appears when someone drags a window edge".
            Err(SurfaceError::OutOfDate) => {
                self.reconfigure()?;
                return Ok(FrameOutcome::Reconfigured);
            }
            Err(error) => return Err(error.into()),
        }

        if acquired.suboptimal {
            // Legal to ignore for one frame, and the seam says treating it as
            // fatal is a bug — so the frame is presented first, then fixed.
            log::debug!("hal: swapchain suboptimal; reconfiguring after present");
            self.reconfigure()?;
        }
        Ok(FrameOutcome::Presented)
    }

    /// Encodes milestone 1 — a clear through a real render pass — submits it,
    /// and retires whatever fell out of the frames-in-flight window.
    fn record_and_submit(&mut self, acquired: &AcquiredFrame) -> Result<(), GpuError> {
        let range = ImageSubresourceRange::all(self.config.format);
        let mut encoder = self.device.create_command_encoder(&CommandEncoderDesc {
            label: Some("sandbox frame"),
            queue: self.queue,
        });
        // An acquired image's contents are undefined, so the source state is
        // `Undefined` — which discards them, which is free, and which is the
        // only correct source for a swapchain image.
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                acquired.image,
                range,
                ResourceState::Undefined,
                ResourceState::ColorAttachment,
            )],
            ..Barriers::default()
        });
        // `docs/plan/02-vulkan-backend.md`'s milestone 1. Nothing draws yet, so
        // the pass contains no draws — but it is a real pass with a real load
        // op, which is what makes the clear go through the attachment machinery
        // every later milestone needs rather than around it.
        encoder.begin_render_pass(&RenderPassDesc {
            label: Some("clear"),
            color_attachments: &[ColorAttachment {
                // The swapchain's own view of its own image — obligation-free
                // on this side of the seam since P1.1.
                view: acquired.view,
                resolve: None,
                load: LoadOp::Clear,
                store: StoreOp::Store,
                clear: ClearValue::color(CLEAR_COLOR),
            }],
            depth_stencil_attachment: None,
            // Obligation 3: the *configured* extent, not the requested one. On
            // X11 a resize event that has not caught up yet means the swapchain
            // is a frame behind the shell, and rendering at the shell's size
            // would put the render area outside the attachment.
            render_area: Rect2d::from_size(acquired.extent.0, acquired.extent.1),
        });
        encoder.end_render_pass();
        // The compositor may only be handed an image in `Present`. Presenting
        // one that was never transitioned is a validation error and, on some
        // drivers, a black window.
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                acquired.image,
                range,
                ResourceState::ColorAttachment,
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
