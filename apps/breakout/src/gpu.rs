//! GPU setup for breakout: shell surface → device → swapchain → render graph.
//!
//! Mirrors `apps/sandbox/src/gpu.rs` but locks the camera to orthographic
//! projection. The ForwardRenderer still draws the lit cube — Slice 2+ will add
//! 2D sprite/quad rendering on top.

use std::collections::VecDeque;

use crcbl::backend::GpuBackend;
use crcbl::hal::{
    AcquiredFrame, CommandBufferHandle, CommandEncoderDesc, DeviceDesc, Features, Format, HalError,
    PresentInfo, PresentMode, QueueHandle, QueueKind, SemaphoreDesc, SemaphoreHandle,
    SemaphoreKind, SemaphoreSignal, SemaphoreWait, SubmitInfo, SurfaceError, SurfaceHandle,
    SwapchainDesc, SwapchainHandle,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::prelude::*;
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, PassTimers, Projection, RenderGraph, TransientPool,
};
use crcbl::shell::WindowId;

const FRAMES_IN_FLIGHT: usize = crcbl::render::forward::FRAMES_IN_FLIGHT;
const MAX_TIMED_PASSES: u32 = 8;

// ---- outcome ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameOutcome {
    Presented,
    Reconfigured,
}

// ---- error ------------------------------------------------------------------

#[derive(Debug)]
pub enum GpuError {
    NoBackend(crcbl::backend::GpuError),
    Unusable(&'static str),
    Hal(HalError),
    Surface(SurfaceError),
    Graph(crcbl::render::GraphError),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoBackend(error) => write!(f, "{error}"),
            Self::Unusable(what) => write!(f, "backend unusable: {what}"),
            Self::Hal(error) => write!(f, "{error}"),
            Self::Surface(error) => write!(f, "{error}"),
            Self::Graph(error) => write!(f, "render graph: {error}"),
        }
    }
}

impl std::error::Error for GpuError {}

impl From<crcbl::backend::GpuError> for GpuError {
    fn from(e: crcbl::backend::GpuError) -> Self {
        Self::NoBackend(e)
    }
}
impl From<HalError> for GpuError {
    fn from(e: HalError) -> Self {
        Self::Hal(e)
    }
}
impl From<SurfaceError> for GpuError {
    fn from(e: SurfaceError) -> Self {
        Self::Surface(e)
    }
}
impl From<crcbl::render::GraphError> for GpuError {
    fn from(e: crcbl::render::GraphError) -> Self {
        Self::Graph(e)
    }
}

// ---- config -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SwapchainConfig {
    format: Format,
    extent: (u32, u32),
    image_count: u32,
    present_mode: PresentMode,
}

impl SwapchainConfig {
    fn desc(self, surface: SurfaceHandle) -> SwapchainDesc<'static> {
        SwapchainDesc {
            label: Some("breakout swapchain"),
            surface,
            format: self.format,
            extent: self.extent,
            image_count: self.image_count,
            present_mode: self.present_mode,
            composite_alpha: crcbl::hal::CompositeAlpha::Opaque,
        }
    }
}

// ---- Gpu --------------------------------------------------------------------

#[derive(Debug)]
pub struct Gpu {
    instance: Box<dyn Instance>,
    device: Box<dyn Device>,
    queue: QueueHandle,
    surface: SurfaceHandle,
    swapchain: SwapchainHandle,
    config: SwapchainConfig,
    timeline: Option<SemaphoreHandle>,
    submitted: u64,
    in_flight: VecDeque<(u64, CommandBufferHandle)>,
    configured_extent: (u32, u32),
    waits: Vec<SemaphoreWait>,
    signals: Vec<SemaphoreSignal>,

    renderer: ForwardRenderer,
    pool: TransientPool,
    timers: Option<PassTimers>,
    camera: Camera,
    light: DirectionalLight,
    /// Interpolated paddle X position (updated each frame from game state).
    paddle_x: f64,
    dumped: bool,
}

impl Gpu {
    /// Opens a GPU backend, creates a surface, device, swapchain, and forward
    /// renderer. Camera is locked to orthographic projection.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened or any HAL call failed.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        backend: Option<GpuBackend>,
    ) -> Result<Self, GpuError> {
        let instance: Box<dyn Instance> = match backend {
            Some(backend) => crcbl::backend::open_backend(backend)?,
            None => crcbl::backend::open()?,
        };

        let adapters = instance.adapters();
        if adapters.is_empty() {
            return Err(GpuError::Unusable("no adapter"));
        }

        let target = shell
            .surface_target(window)
            .map_err(|_| GpuError::Unusable("the window went away before its surface was made"))?;
        log::debug!(
            "hal: creating a surface for a {} target",
            target.platform_name()
        );

        // SAFETY: `target` was produced by a live window; the shell outlives
        // the surface because `destroy` tears it down first.
        let surface = unsafe { instance.create_surface(&target) }?;

        let mut chosen = None;
        let mut last_error = None;
        for adapter in &adapters {
            match instance.surface_caps(surface, adapter.id) {
                Ok(caps) if caps.preferred_format().is_some() => {
                    chosen = Some((adapter.clone(), caps));
                    break;
                }
                Ok(_) => log::debug!(
                    "hal: adapter {:?} offers no usable surface format; trying next",
                    adapter.name
                ),
                Err(error) => {
                    log::debug!(
                        "hal: adapter {:?} cannot serve this surface ({error}); trying next",
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

        let format = caps
            .preferred_format()
            .ok_or(GpuError::Unusable("the surface offers no format"))?;
        let present_mode = caps.choose_present_mode(&[PresentMode::Mailbox, PresentMode::Fifo]);
        let image_count = caps
            .min_image_count
            .saturating_add(1)
            .min(caps.max_image_count);

        let device = instance.create_device(&DeviceDesc {
            label: Some("breakout"),
            adapter: adapter.id,
            required_features: Features::empty(),
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
                label: Some("breakout frames in flight"),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            })?)
        } else {
            log::debug!("hal: no timeline semaphores; retiring command buffers with wait_idle");
            None
        };

        let renderer = ForwardRenderer::new(device.as_ref(), queue, format)?;
        let timers = PassTimers::new(device.as_ref(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);

        // Breakout is a 2D game: orthographic projection with reversed-Z depth.
        let camera = Camera::default().with_projection(Projection::Orthographic {
            half_height: 9.0,
            near: 0.1,
            far: 100.0,
        });

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
            camera,
            light: DirectionalLight::default(),
            paddle_x: 0.0,
            dumped: false,
        })
    }

    #[must_use]
    pub fn extent(&self) -> (u32, u32) {
        self.configured_extent
    }

    /// Set the interpolated paddle X for the current frame.
    pub fn set_paddle_x(&mut self, x: f64) {
        self.paddle_x = x;
    }

    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// Records, submits and presents one frame.
    pub fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
        let acquired = match self.device.acquire_next_frame(self.swapchain) {
            Ok(frame) => frame,
            Err(SurfaceError::OutOfDate) => {
                self.reconfigure()?;
                return Ok(FrameOutcome::Reconfigured);
            }
            Err(error) => return Err(error.into()),
        };

        if acquired.extent != self.configured_extent {
            log::debug!(
                "hal: swapchain configured at {:?} (asked {:?})",
                acquired.extent,
                self.config.extent
            );
            self.configured_extent = acquired.extent;
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

    fn record_and_submit(&mut self, acquired: &AcquiredFrame) -> Result<(), GpuError> {
        let extent = acquired.extent;
        self.renderer.begin_frame(
            self.device.as_ref(),
            &self.camera,
            &self.light,
            paddle_model(self.paddle_x),
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
            graph.compile(&self.pool)?
        };

        if !self.dumped {
            log::debug!("render graph for breakout:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDesc {
            label: Some("breakout frame"),
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
        self.pool.retire_unused(self.device.as_ref());
        Ok(())
    }

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
                None => self.device.wait_idle()?,
            }
            self.device.destroy_command_buffer(command_buffer);
        }
        Ok(())
    }

    pub fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        if extent == self.config.extent {
            return Ok(());
        }
        if extent.0 == 0 || extent.1 == 0 {
            log::debug!("hal: window has empty extent {extent:?}; keeping swapchain");
            return Ok(());
        }
        self.config.extent = extent;
        self.reconfigure()
    }

    fn reconfigure(&mut self) -> Result<(), GpuError> {
        log::debug!(
            "hal: reconfiguring swapchain to {}x{}",
            self.config.extent.0,
            self.config.extent.1
        );
        self.device
            .reconfigure_swapchain(self.swapchain, &self.config.desc(self.surface))?;
        self.dumped = false;
        Ok(())
    }

    pub fn destroy(mut self) -> Result<(), GpuError> {
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
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        Ok(())
    }
}

/// Model matrix for the paddle: a wide, flat rectangle at `(x, PADDLE_Y, 0)`.
///
/// The cube is 1×1×1 centred at origin. We scale X to the paddle width,
/// flatten Y and Z, and translate to the paddle position.
///
/// # Constants
const PADDLE_HALF_WIDTH: f64 = 5.0;
const PADDLE_Y: f64 = -8.0;
const PADDLE_THICKNESS: f64 = 0.3;

fn paddle_model(x: f64) -> Mat4 {
    let scale = Vec3::new(
        PADDLE_HALF_WIDTH as f32 * 2.0,
        PADDLE_THICKNESS as f32,
        PADDLE_THICKNESS as f32,
    );
    let translation = Vec3::new(x as f32, PADDLE_Y as f32, 0.0);
    Mat4::from_scale_rotation_translation(scale, glam::Quat::IDENTITY, translation)
}
