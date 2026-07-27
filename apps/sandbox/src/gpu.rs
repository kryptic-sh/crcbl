//! Where the two seams meet: a `crcbl-shell` window becomes a `crcbl-hal`
//! surface, a swapchain, and — since P1.3 — a **render graph**.
//!
//! This module was the whole point of P0.7: `crcbl-shell` had been complete
//! since P0.6 and `crcbl-hal` since P0.3, but nothing had ever *joined* them,
//! and the join is where a seam mismatch shows up. P0.7 drove it against
//! [`NullBackend`](crcbl::hal::null); P1.1 drove the same code against real
//! Vulkan; P1.2 drew a triangle through it. **P1.3 hands the frame to
//! `crcbl-render`.**
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
//! graph, ever**", and both of those are gone. What replaced them is a
//! *declaration* — the swapchain image is imported into the graph saying "it
//! arrives [`Undefined`](crcbl::hal::ResourceState::Undefined) and must leave
//! [`Present`](crcbl::hal::ResourceState::Present)" — and the graph computes the
//! rest, including the transition of the HDR scene target from a colour
//! attachment into a sampled texture, which the hand-written version never had
//! to think about because there was no second pass.
//!
//! The frame is now:
//!
//! ```text
//! acquire → build the graph → compile → execute (barriers computed)
//!         → submit(wait acquire, signal present + timeline)
//!         → present(wait present) → retire the command buffer
//! ```
//!
//! and the graph's own dump explains it — `CRCBL_LOG=debug` prints it once, and
//! once per resize.
//!
//! # HDR from P1
//!
//! The mesh is drawn into a transient `Rgba16Float` target with a `D32Float`
//! reversed-Z depth buffer, and a second pass tonemaps that into the swapchain.
//! `docs/plan/ROADMAP.md`'s correction asks for exactly that from the first lit
//! mesh, "even with no HDR content", so P7's real stack does not re-bless every
//! golden image in the repository. Both targets are graph transients: this file
//! never names an image, a view or a size for either.
//!
//! # Frames in flight, not `wait_idle`
//!
//! [`Device::destroy_command_buffer`] may not be called until the submission
//! that used it has completed, and the seam offers exactly two ways to know
//! that: a timeline semaphore, or [`Device::wait_idle`] — which the seam itself
//! documents as "a shutdown and test primitive" that "destroys pipelining". So
//! this keeps a two-deep ring keyed on a timeline semaphore value, and falls
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
//!    [`WindowState::size`](crcbl::shell::WindowState::size) is one;
//!    [`SurfaceCaps::current_extent`](crcbl::hal::SurfaceCaps::current_extent)
//!    is the other, and on Vulkan it is a real size on X11 and deliberately
//!    `0xFFFFFFFF` ("you choose") on Wayland. `crcbl-hal`'s
//!    [`swapchain`](crcbl::hal::swapchain) module now states the rule as four
//!    numbered backend obligations, and [`Gpu::open`] is the reference
//!    implementation of the caller's half.
//! 2. **[`SurfaceTarget::Offscreen`](crcbl::core::SurfaceTarget) embedded a
//!    size, so a headless target went stale on resize** — *fixed by deleting
//!    the size.* [`Gpu::resize`] therefore reconfigures the swapchain and
//!    nothing else, on every backend.
//! 3. **`unsafe` at the join is unavoidable and lands in application code.**
//!    [`Instance::create_surface`] is `unsafe` because it dereferences platform
//!    handles, and the safety obligation ("these outlive the surface") is one
//!    only the code holding *both* the shell and the device can discharge.
//!    Still open at P1.3: the seam's own TODO suggests "a shell-aware
//!    constructor in `crcbl-render`", and P1.3 deliberately did not add one —
//!    `crcbl-render` owning window handles would put a `SurfaceTarget` in the
//!    renderer's constructor and make the render graph's crate the place
//!    windowing lives. The right home is an engine-setup helper in the `crcbl`
//!    umbrella, which is where both seams already meet.
//! 4. **Teardown order is stated in three places and enforced in none.** The
//!    swapchain must die before the surface, the surface before the window, and
//!    the device may outlive its instance. [`Gpu::destroy`] does it by hand;
//!    at P1.1 a real driver with validation on agreed.
//! 5. **The swapchain's configured extent was unobservable** — *fixed in the
//!    seam*, [`AcquiredFrame::extent`].
//! 6. **A render pass needed a view the seam would not give it** — *fixed in
//!    the seam*, [`AcquiredFrame::view`].

use std::collections::VecDeque;

use crcbl::backend::GpuBackend;
use crcbl::hal::{
    AcquiredFrame, CommandBufferHandle, CommandEncoderDesc, DeviceDesc, Features, Format, HalError,
    PresentInfo, PresentMode, QueueHandle, QueueKind, SemaphoreDesc, SemaphoreHandle,
    SemaphoreKind, SemaphoreSignal, SemaphoreWait, SubmitInfo, SurfaceError, SurfaceHandle,
    SwapchainDesc, SwapchainHandle,
};
use crcbl::prelude::*;
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, PassTimers, RenderGraph, TransientPool,
};
use crcbl::shell::WindowId;

/// How many frames may be in flight before the loop waits for the oldest.
///
/// Two is the classic double-buffered default: one frame being recorded while
/// one is executing. It is `crcbl-render`'s constant because the uniform ring
/// has to be the same depth — one buffer per frame in flight, or a spinning
/// camera is a read-after-write hazard across submissions.
const FRAMES_IN_FLIGHT: usize = crcbl::render::forward::FRAMES_IN_FLIGHT;

/// How many passes the per-pass GPU timers can bracket.
///
/// The frame has two. Eight leaves room for a debug pass or two without a
/// resize of the query sets, and costs sixteen timestamps.
const MAX_TIMED_PASSES: u32 = 8;

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
    /// [`AcquiredFrame::extent`]. Distinct from `config.extent`, which is what
    /// the shell asked for.
    configured_extent: (u32, u32),
    /// Scratch, reused every frame so a steady-state frame allocates nothing.
    waits: Vec<SemaphoreWait>,
    signals: Vec<SemaphoreSignal>,

    // --- P1.3: the frame is a graph ---
    renderer: ForwardRenderer,
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Where the camera is and how it projects. Milestone 5 is a write to
    /// `camera.projection` and nothing else.
    pub camera: Camera,
    /// The single directional light of milestone 4.
    pub light: DirectionalLight,
    /// Seconds of animation, advanced by the loop rather than read from a clock
    /// here — a headless run must produce the same picture on every machine.
    elapsed: f32,
    /// Whether the graph dump has been logged since the last shape change.
    dumped: bool,
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
    /// The render graph refused the frame.
    Graph(crcbl::render::GraphError),
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

impl From<crcbl::render::GraphError> for GpuError {
    fn from(error: crcbl::render::GraphError) -> Self {
        Self::Graph(error)
    }
}

impl Gpu {
    /// Creates an instance, a surface for `window`, a device, a swapchain and
    /// the forward renderer.
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
        projection: crcbl::render::Projection,
    ) -> Result<Self, GpuError> {
        // The line that used to name `NullInstance`. It now names a *value*
        // from a registry, which is the whole difference between "the sandbox
        // knows about Vulkan" and "the sandbox knows there are backends".
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

        let device = instance.create_device(&DeviceDesc {
            label: Some("sandbox"),
            adapter: adapter.id,
            // Nothing here needs a feature, and demanding `TIER_A` would refuse
            // to run on the Tier B devices `docs/plan/02-vulkan-backend.md`
            // requires the engine to support. Ask for everything optionally and
            // branch on what came back.
            required_features: Features::empty(),
            // `TIMESTAMP_QUERY` is deliberately not part of `TIER_A` — topic
            // 10's browsers may lack it — so the per-pass timers have to be
            // asked for by name. Absent, `PassTimers::new` declines and the
            // frame runs untimed.
            optional_features: Features::TIER_A
                | Features::TIMESTAMP_QUERY
                | Features::DEBUG_MARKERS,
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

        // Milestones 3–5. Built after the swapchain because the tonemap
        // pipeline has to name the colour format the pass will render to.
        let renderer = ForwardRenderer::new(device.as_ref(), queue, format)?;
        let timers = PassTimers::new(device.as_ref(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        if timers.is_none() {
            log::info!("hal: no timestamp queries on this device; per-pass timing is off");
        }

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
            renderer,
            pool: TransientPool::new(),
            timers,
            camera: Camera::default().with_projection(projection),
            light: DirectionalLight::default(),
            elapsed: 0.0,
            dumped: false,
        })
    }

    /// The swapchain's current size — the one it was **configured** at.
    #[must_use]
    pub fn extent(&self) -> (u32, u32) {
        self.configured_extent
    }

    /// The format the swapchain was created with. Test-only.
    #[cfg(test)]
    pub fn format(&self) -> Format {
        self.config.format
    }

    /// The most recent frame whose per-pass GPU timings have landed.
    ///
    /// Empty on a device with no timestamp queries, and empty for the first few
    /// frames — the report is deliberately frames latent; see
    /// [`crcbl::render::PassTimers`].
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// Advances the animation by `dt` seconds.
    ///
    /// Driven by the loop's clock rather than read from one here, so a headless
    /// run renders the same cube on every machine — which is what makes a
    /// golden image of it worth anything.
    pub fn advance(&mut self, dt: f32) {
        self.elapsed += dt;
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

        // Obligation 3: the answer, not the request.
        if acquired.extent != self.configured_extent {
            log::debug!(
                "hal: swapchain configured at {:?} (the shell asked for {:?})",
                acquired.extent,
                self.config.extent
            );
            self.configured_extent = acquired.extent;
            // The graph's shape changed, so the dump is worth printing again.
            self.dumped = false;
        }

        self.record_and_submit(&acquired)?;
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

    /// Builds this frame's graph, compiles it, executes it and submits.
    ///
    /// **The whole frame, and not one barrier in it.**
    fn record_and_submit(&mut self, acquired: &AcquiredFrame) -> Result<(), GpuError> {
        let extent = acquired.extent;
        self.renderer.begin_frame(
            self.device.as_ref(),
            &self.camera,
            &self.light,
            ForwardRenderer::spin(self.elapsed),
            extent,
        )?;

        let compiled = {
            let mut graph = RenderGraph::new(self.queue);
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(
                    acquired.image,
                    acquired.view,
                    self.config.format,
                    extent,
                ),
            );
            let _hdr = self.renderer.add_passes(&mut graph, target, extent);
            // The pool is what remembers the previous frame, so the barriers
            // that open this one are ordered against it rather than against
            // nothing.
            graph.compile(&self.pool)?
        };

        // "The graph must be able to explain itself" — §2.4's debug-tools
        // principle. Once per shape rather than once per frame, because a dump
        // every frame is a log nobody reads.
        if !self.dumped {
            log::debug!("render graph for the sandbox frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDesc {
            label: Some("sandbox frame"),
            queue: self.queue,
        });
        compiled.execute(
            self.device.as_ref(),
            &mut self.pool,
            encoder.as_mut(),
            self.timers.as_mut(),
        )?;
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
        self.retire_to(FRAMES_IN_FLIGHT)?;
        // Only after the retire above, so nothing the pool destroys can still be
        // referenced by a submission that has not completed.
        self.pool.retire_unused(self.device.as_ref());
        Ok(())
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
        self.dumped = false;
        Ok(())
    }

    /// Tears everything down in the order the seam requires.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if waiting for outstanding work failed.
    pub fn destroy(mut self) -> Result<(), GpuError> {
        // Nothing may be destroyed while the device might still be using it.
        self.device.wait_idle()?;
        self.retire_to(0)?;
        self.pool.destroy(self.device.as_ref());
        if let Some(timers) = self.timers.as_mut() {
            timers.destroy(self.device.as_ref());
        }
        self.renderer.destroy(self.device.as_ref());
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
