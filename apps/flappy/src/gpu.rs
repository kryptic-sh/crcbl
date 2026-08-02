//! GPU setup for flappy: the shared [`crcbl::engine`] join plus a camera that
//! moves.
//!
//! Everything that is not specific to this game — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — lives in [`crcbl::engine::GpuContext`], which
//! `apps/breakout/src/gpu.rs` and `apps/sandbox/src/gpu.rs` use too.
//!
//! # The camera scrolls, and that is the new thing
//!
//! Breakout's camera never moves: the field is fixed and the whole of it is on
//! screen at once, so the only question the camera had to answer was how wide to
//! be. Flappy's world has no right-hand edge, so the camera follows the bird —
//! and every quad the UI pass draws has to be placed against the *same* moving
//! frame, or the pipes slide against the bird they are supposed to be fixed
//! relative to.
//!
//! That is why [`camera_x`] is public and `app.rs` derives its world→screen
//! mapping from it rather than keeping a second copy of the offset. Two copies
//! of a number that has to agree is how the first version of breakout's camera
//! went wrong.

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

/// Slack kept above the ceiling and below the floor, in world units, so the band
/// the bird flies in is not flush with the edge of the surface.
const VIEW_MARGIN: f32 = 0.5;

/// Where the bird sits across the view, as a fraction from the left.
///
/// Not the middle. A side-scroller is played by looking at what is coming, so
/// the useful half of the screen is the half in front of the bird; at 0.3 there
/// is more than twice as much course ahead as behind.
const BIRD_SCREEN_FRACTION: f32 = 0.3;

/// Half the vertical extent of the orthographic camera, in world units.
///
/// A constant, unlike breakout's: this world is bounded vertically and unbounded
/// horizontally, so the height is what the camera is fitted to and the width is
/// whatever the aspect ratio then gives. A wider window sees more course, which
/// is the correct answer for a side-scroller rather than a compromise.
#[must_use]
pub fn camera_half_height() -> f32 {
    crate::game::WORLD_CEILING as f32 + VIEW_MARGIN
}

/// Half the horizontal extent of the camera at `extent`, in world units.
#[must_use]
pub fn camera_half_width(extent: (u32, u32)) -> f32 {
    let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
    camera_half_height() * aspect
}

/// Where the camera is centred when the bird is at `bird_x`.
///
/// Public because `app.rs`'s world→screen mapping has to use the same value:
/// the pipes are drawn as UI quads and the bird as a lit cube through the
/// forward pass, and the two only line up if both are placed against this
/// number.
#[must_use]
pub fn camera_x(bird_x: f64, extent: (u32, u32)) -> f32 {
    let half_width = camera_half_width(extent);
    // `0.5 - fraction` of the view sits behind the bird's own offset from the
    // centre, so a fraction of 0.5 would be a camera locked to the bird.
    bird_x as f32 + half_width * (1.0 - 2.0 * BIRD_SCREEN_FRACTION)
}

/// The camera projection for an `extent`-sized viewport.
fn projection() -> Projection {
    Projection::Orthographic {
        half_height: camera_half_height(),
        near: 0.1,
        far: 100.0,
    }
}

// ---- Gpu --------------------------------------------------------------------

#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    renderer: ForwardRenderer,
    pool: TransientPool,
    timers: Option<PassTimers>,
    camera: Camera,
    light: DirectionalLight,
    /// Where the bird is this frame, from the game. Drives both the camera and
    /// the cube the forward pass draws.
    bird: glam::DVec3,
    /// UI compositing.
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
}

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for.
///
/// One value rather than two copies: the browser path and the native path must
/// open the *same* device, or a feature only the blocking path requested is a
/// bug nobody sees until someone loads the page.
fn desc(backend: Option<GpuBackend>) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "flappy",
        backend,
        // Optional, not required: the UI pass hands its shader the viewport size
        // through a push constant where there are any and through a uniform
        // buffer where there are none. A browser is always the second case.
        optional_features: Features::TIER_A
            | Features::TIMESTAMP_QUERY
            | Features::DEBUG_MARKERS
            | Features::PUSH_CONSTANTS,
        ..GpuContextDesc::default()
    }
}

/// A [`Gpu`] being opened one poll at a time — the browser's half of
/// [`Gpu::open`].
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
    /// Opens a backend, a surface, a device and a swapchain, and builds the
    /// forward and UI renderers.
    ///
    /// **Blocks**, so this is the native path only.
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
    /// before its surface could be described.
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
    /// # Errors
    ///
    /// [`GpuError`] if the forward renderer or the UI compositor refused the
    /// device.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let renderer = ForwardRenderer::new(ctx.device(), ctx.queue(), format)?;
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        let ui = UiRenderer::new(ctx.device(), ctx.queue(), format).map_err(GpuError::Hal)?;

        Ok(Self {
            ctx,
            renderer,
            pool: TransientPool::new(),
            timers,
            camera: Camera::default().with_projection(projection()),
            light: DirectionalLight::default(),
            bird: crate::game::BIRD_START,
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

    /// Set the bird's world position for the current frame.
    pub const fn set_bird(&mut self, bird: glam::DVec3) {
        self.bird = bird;
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

        // The swapchain's extent, not the one the resize event reported: on the
        // frame a reconfigure lands they can differ, and the camera must agree
        // with the surface actually being drawn into.
        let centre = camera_x(self.bird.x, extent);
        self.camera.projection = projection();
        self.camera.eye = Vec3::new(centre, 0.0, 2.0);
        self.camera.target = Vec3::new(centre, 0.0, 0.0);

        self.renderer.begin_frame(
            self.ctx.device(),
            &self.camera,
            &self.light,
            bird_model(self.bird),
            extent,
        )?;
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
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        if !self.dumped {
            log::debug!("render graph for flappy:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("flappy frame"),
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

/// Model matrix for the bird: a small cube at its world position.
///
/// The forward pass draws exactly one instance — `begin_frame` takes a single
/// `model: Mat4` — so the bird is the one thing in this game that can go through
/// it. The pipes are UI quads for the same reason breakout's bricks are.
fn bird_model(bird: glam::DVec3) -> Mat4 {
    let size = (crate::game::BIRD_RADIUS * 2.0) as f32;
    Mat4::from_scale_rotation_translation(
        Vec3::splat(size),
        glam::Quat::IDENTITY,
        Vec3::new(bird.x as f32, bird.y as f32, 0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bird sits where the camera says it does, at every aspect ratio: a
    /// third of the way in from the left, with the rest of the view ahead of it.
    #[test]
    fn the_camera_keeps_the_bird_a_third_of_the_way_across() {
        for extent in [(960, 720), (1920, 1080), (600, 900), (1440, 400)] {
            let half_width = camera_half_width(extent);
            for bird_x in [0.0, 12.5, 900.0] {
                let centre = camera_x(bird_x, extent);
                let left = centre - half_width;
                let fraction = (bird_x as f32 - left) / (2.0 * half_width);
                assert!(
                    (fraction - BIRD_SCREEN_FRACTION).abs() < 1e-4,
                    "{extent:?} at x={bird_x} put the bird {fraction} across"
                );
            }
        }
    }

    /// The whole playable band is on screen whatever the window's shape, because
    /// the camera is fitted to the height and never to the width.
    #[test]
    fn the_playable_band_is_always_on_screen() {
        let half_height = camera_half_height();
        assert!(half_height > crate::game::WORLD_CEILING as f32);
        assert!(-half_height < crate::game::WORLD_FLOOR as f32);
    }
}
