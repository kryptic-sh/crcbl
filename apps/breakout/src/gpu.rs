//! GPU setup for breakout: the shared [`crcbl::engine`] join plus this game's
//! own renderers.
//!
//! Everything that is not specific to breakout — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — lives in [`crcbl::engine::GpuContext`], which
//! `apps/sandbox/src/gpu.rs` uses too. This file is what is left: an
//! orthographic camera, the forward renderer, the UI compositor, and the graph
//! that joins them.

use crcbl::backend::GpuBackend;
use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, PendingGpuContext};
use crcbl::hal::{CommandEncoderDesc, Features};
use crcbl::math::{Mat4, Vec3};
use crcbl::prelude::*;
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, PassTimers, Projection, RenderGraph, TransientPool,
    UiRenderer,
};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::text::FontAtlas;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;
const MAX_TIMED_PASSES: u32 = 8;

/// Half the vertical extent of the orthographic camera, in world units.
///
/// Public because `app.rs` derives its world→screen mapping from it: the UI
/// quads that draw the ball and the bricks have to land where this projection
/// puts them, and two copies of the number would drift.
pub const CAMERA_HALF_HEIGHT: f32 = 9.0;

// ---- Gpu --------------------------------------------------------------------

#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    renderer: ForwardRenderer,
    pool: TransientPool,
    timers: Option<PassTimers>,
    camera: Camera,
    light: DirectionalLight,
    /// Paddle X for this frame, from the game state.
    paddle_x: f64,
    /// UI compositing.
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
}

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for.
///
/// One value rather than two copies of the same descriptor: the browser path
/// and the native path must open the *same* device, or a feature that only the
/// blocking path requested is a bug nobody sees until someone loads the page.
fn desc(backend: Option<GpuBackend>) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "breakout",
        backend,
        // The UI pass hands `ui.slang` its viewport size through a push
        // constant and has no other binding to do it through, so
        // `UiRenderer::new` refuses a device that did not enable them.
        optional_features: Features::TIER_A
            | Features::TIMESTAMP_QUERY
            | Features::DEBUG_MARKERS
            | Features::PUSH_CONSTANTS,
        ..GpuContextDesc::default()
    }
}

/// A [`Gpu`] being opened one poll at a time.
///
/// The browser's half of [`Gpu::open`]. `requestDevice` is a promise and the
/// page's own event loop is what resolves it, so a browser that blocked waiting
/// for a device would deadlock against itself — see
/// [`GpuContext::request_open`]. Poll this once per `requestAnimationFrame`
/// until it yields.
#[derive(Debug)]
pub struct PendingGpu {
    pending: PendingGpuContext,
}

impl PendingGpu {
    /// Advances the open. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the device request failed or a renderer refused the
    /// device it produced.
    pub fn poll(&mut self) -> Result<Option<Gpu>, GpuError> {
        match self.pending.poll()? {
            Some(ctx) => Gpu::from_context(ctx).map(Some),
            None => Ok(None),
        }
    }
}

impl Gpu {
    /// Opens a GPU backend, a surface, a device and a swapchain, and builds the
    /// forward and UI renderers. The camera is locked to orthographic.
    ///
    /// **Blocks**, so this is the native path only; a browser calls
    /// [`request_open`](Self::request_open).
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
        Self::from_context(GpuContext::open(shell, window, extent, &desc(backend))?)
    }

    /// Starts opening the same thing without blocking.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the registry has no such backend or the window went away
    /// before its surface could be described. Everything else is reported from
    /// [`PendingGpu::poll`].
    pub fn request_open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        backend: Option<GpuBackend>,
    ) -> Result<PendingGpu, GpuError> {
        Ok(PendingGpu {
            pending: GpuContext::request_open(shell, window, extent, &desc(backend))?,
        })
    }

    /// Builds this game's renderers on an already-open context.
    ///
    /// The half of start-up that is the same however the context arrived, which
    /// is why it is a function rather than a copy in each of the two paths.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the forward renderer or the UI compositor refused the
    /// device.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let renderer = ForwardRenderer::new(ctx.device(), ctx.queue(), format)?;
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        let ui = UiRenderer::new(ctx.device(), ctx.queue(), format).map_err(GpuError::Hal)?;

        // Breakout is a 2D game: orthographic projection with reversed-Z depth.
        let camera = Camera::default().with_projection(Projection::Orthographic {
            half_height: CAMERA_HALF_HEIGHT,
            near: 0.1,
            far: 100.0,
        });

        Ok(Self {
            ctx,
            renderer,
            pool: TransientPool::new(),
            timers,
            camera,
            light: DirectionalLight::default(),
            paddle_x: 0.0,
            ui,
            atlas: FontAtlas::built_in(),
            draw_list: DrawList::new(),
            dumped: false,
        })
    }

    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.ctx.extent()
    }

    /// Set the paddle X for the current frame.
    pub const fn set_paddle_x(&mut self, x: f64) {
        self.paddle_x = x;
    }

    /// Takes this frame's draw list, handing the previous frame's allocation
    /// back so the caller can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, dl: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, dl);
    }

    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// Records, submits and presents one frame.
    ///
    /// # Errors
    ///
    /// [`GpuError`] for anything except a swapchain that has merely gone out of
    /// date, which is reported as [`FrameOutcome::Reconfigured`].
    pub fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
        let Some(acquired) = self.ctx.acquire()? else {
            self.dumped = false;
            return Ok(FrameOutcome::Reconfigured);
        };
        let extent = acquired.extent;

        self.renderer.begin_frame(
            self.ctx.device(),
            &self.camera,
            &self.light,
            paddle_model(self.paddle_x),
            extent,
        )?;
        // Upload UI geometry for this frame: the ball, every live brick, the
        // paddle outline and the HUD.
        self.ui
            .begin_frame(self.ctx.device(), &self.draw_list, &self.atlas, 1.0)
            .map_err(GpuError::Hal)?;

        let format = self.ctx.format();
        let compiled = {
            let mut graph = RenderGraph::new(self.ctx.queue());
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, format, extent),
            );
            let _hdr = self.renderer.add_passes(&mut graph, target, extent);
            // Composite the UI on top of the tonemapped target.
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        if !self.dumped {
            log::debug!("render graph for breakout:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("breakout frame"),
                queue: self.ctx.queue(),
            });
        compiled.execute(
            self.ctx.device(),
            &mut self.pool,
            encoder.as_mut(),
            self.timers.as_mut(),
        )?;
        let command_buffer = encoder.finish()?;

        let outcome = self.ctx.submit_and_present(&acquired, command_buffer)?;
        // Only after the submit's retire, so nothing the pool destroys can
        // still be referenced by a submission that has not completed.
        self.pool.retire_unused(self.ctx.device());
        if outcome == FrameOutcome::Reconfigured {
            self.dumped = false;
        }
        Ok(outcome)
    }

    /// Resizes the swapchain.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the reconfigure failed.
    pub fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        self.ctx.resize(extent)?;
        self.dumped = false;
        Ok(())
    }

    /// Releases everything, in dependency order.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if waiting for outstanding work failed.
    pub fn destroy(mut self) -> Result<(), GpuError> {
        // Nothing may be destroyed while the device might still be using it.
        self.ctx.drain()?;
        self.ui.destroy(self.ctx.device());
        self.pool.destroy(self.ctx.device());
        if let Some(timers) = self.timers.as_mut() {
            timers.destroy(self.ctx.device());
        }
        self.renderer.destroy(self.ctx.device());
        self.ctx.destroy()
    }
}

/// Model matrix for the paddle: a wide, flat box at `(x, PADDLE_Y, 0)`.
///
/// The cube is 1×1×1 centred at origin, so this scales it to the paddle's
/// collider extents and translates it to the paddle position.
///
/// Every number comes from `game.rs`. They used to be re-declared privately in
/// this file with the same values, so changing the collider moved the paddle
/// the ball bounces off without moving the one on screen.
fn paddle_model(x: f64) -> Mat4 {
    use crate::game::{PADDLE_HALF_HEIGHT, PADDLE_HALF_WIDTH, PADDLE_Y};
    let scale = Vec3::new(
        (PADDLE_HALF_WIDTH * 2.0) as f32,
        (PADDLE_HALF_HEIGHT * 2.0) as f32,
        (PADDLE_HALF_HEIGHT * 2.0) as f32,
    );
    let translation = Vec3::new(x as f32, PADDLE_Y as f32, 0.0);
    Mat4::from_scale_rotation_translation(scale, glam::Quat::IDENTITY, translation)
}
