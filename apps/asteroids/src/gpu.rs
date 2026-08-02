//! GPU setup for asteroids: the shared [`crcbl::engine`] join, a clear, and the
//! UI pass.
//!
//! # There is deliberately no sprite pass yet
//!
//! `docs/plan/sample/02-asteroids.md` calls for `.crpix` sprites, and this
//! sub-slice is the simulation. A sprite pass with no sheet to draw is not a
//! placeholder, it is a texture upload of nothing: [`SpriteRenderer`] takes a
//! registered sheet, a sheet comes from `build.rs` baking authored `.crpix`
//! files, and none of that is cheaper than the two-line alternative below. So
//! the field is drawn as untextured quads through the UI pass — a rectangle per
//! rock, per bullet and for the ship — which is exactly what breakout and flappy
//! did before the sprite pass existed, and which the art sub-slice replaces
//! wholesale rather than builds on.
//!
//! The mapping from world to pixels lives in [`world_to_screen`] here, beside
//! the extent it depends on, rather than in `app.rs`.
//!
//! # Pass order is declaration order
//!
//! `space` (clear) → `ui` (the field and the HUD). The UI pass loads rather than
//! clears, so declaring it first would paint under the clear.
//!
//! [`SpriteRenderer`]: crcbl::render::SpriteRenderer

use crcbl::backend::GpuBackend;
use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError};
use crcbl::hal::{CommandEncoderDesc, Features};
use crcbl::prelude::*;
use crcbl::render::{ForwardRenderer, PassTimers, RenderGraph, TransientPool, UiRenderer};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::text::FontAtlas;
use glam::Vec2;

use crate::game::{WORLD_HALF_HEIGHT, WORLD_HALF_WIDTH};

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;
const MAX_TIMED_PASSES: u32 = 4;

/// What the swapchain is cleared to. Not black: a field with no visible edge
/// makes the wrap impossible to read, and a near-black lets the border rectangle
/// the loop draws show against it.
pub const SPACE: [f32; 4] = [0.02, 0.02, 0.06, 1.0];

/// How many pixels one world unit is, at `extent`.
///
/// The **smaller** of the two fits, so the whole playfield is on screen whatever
/// shape the window is. A field that overflowed the viewport would put rocks
/// where the player cannot see them, and in a wrapping game an off-screen rock
/// is indistinguishable from a bug.
#[must_use]
pub fn pixels_per_unit(extent: (u32, u32)) -> f32 {
    let by_width = extent.0.max(1) as f32 / (2.0 * WORLD_HALF_WIDTH as f32);
    let by_height = extent.1.max(1) as f32 / (2.0 * WORLD_HALF_HEIGHT as f32);
    by_width.min(by_height)
}

/// Where a world point lands in framebuffer pixels.
///
/// Y is flipped: the world's `+Y` is up and the framebuffer's is down.
#[must_use]
pub fn world_to_screen(world: glam::DVec3, extent: (u32, u32)) -> Vec2 {
    let scale = pixels_per_unit(extent);
    let centre = Vec2::new(extent.0 as f32 / 2.0, extent.1 as f32 / 2.0);
    centre + Vec2::new(world.x as f32 * scale, -world.y as f32 * scale)
}

// ---- Gpu --------------------------------------------------------------------

#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    pool: TransientPool,
    timers: Option<PassTimers>,
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
}

/// What [`Gpu::open`] asks the engine for.
fn desc(backend: Option<GpuBackend>) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "asteroids",
        backend,
        // Optional, not required: the UI pass hands its shader the viewport size
        // through a push constant where there are any and through a uniform
        // buffer where there are none.
        optional_features: Features::TIER_A
            | Features::TIMESTAMP_QUERY
            | Features::DEBUG_MARKERS
            | Features::PUSH_CONSTANTS,
        ..GpuContextDesc::default()
    }
}

impl Gpu {
    /// Opens a backend, a surface, a device and a swapchain, and builds the UI
    /// renderer.
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
        let ctx = GpuContext::open(shell, window, extent, &desc(backend))?;
        let format = ctx.format();
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        let ui = UiRenderer::new(ctx.device(), ctx.queue(), format).map_err(GpuError::Hal)?;
        Ok(Self {
            ctx,
            pool: TransientPool::new(),
            timers,
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

    /// Takes this frame's draw list, handing the previous frame's allocation
    /// back so the caller can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, dl: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, dl);
    }

    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// The glyph atlas the UI pass renders text from.
    ///
    /// The debug overlay measures its own panel with it, and must measure with
    /// the *same* atlas the pass draws with or the background rect is the wrong
    /// size for the text inside it.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
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
            graph
                .add_render_pass("space")
                .clear_color(target, SPACE)
                .execute(|_| {});
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        if !self.dumped {
            log::debug!("render graph for asteroids:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("asteroids frame"),
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
        self.ctx.destroy()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    /// The whole playfield is on screen whatever the window's shape. A rock the
    /// player cannot see is indistinguishable from a rock that leaked.
    #[test]
    fn the_whole_field_fits_at_every_aspect_ratio() {
        for extent in [(960, 720), (1920, 1080), (600, 900), (1440, 400)] {
            for corner in [
                DVec3::new(WORLD_HALF_WIDTH, WORLD_HALF_HEIGHT, 0.0),
                DVec3::new(-WORLD_HALF_WIDTH, -WORLD_HALF_HEIGHT, 0.0),
                DVec3::new(WORLD_HALF_WIDTH, -WORLD_HALF_HEIGHT, 0.0),
                DVec3::new(-WORLD_HALF_WIDTH, WORLD_HALF_HEIGHT, 0.0),
            ] {
                let pixel = world_to_screen(corner, extent);
                assert!(
                    (-0.5..=extent.0 as f32 + 0.5).contains(&pixel.x)
                        && (-0.5..=extent.1 as f32 + 0.5).contains(&pixel.y),
                    "{extent:?} put the corner {corner:?} at {pixel:?}"
                );
            }
        }
    }

    /// The origin is the middle of the window, and `+Y` is up.
    #[test]
    fn the_mapping_centres_the_origin_and_flips_y() {
        let extent = (960, 720);
        let centre = world_to_screen(DVec3::ZERO, extent);
        assert_eq!(centre, Vec2::new(480.0, 360.0));
        let above = world_to_screen(DVec3::new(0.0, 1.0, 0.0), extent);
        assert!(
            above.y < centre.y,
            "world +Y must go up the screen: {above:?} against {centre:?}"
        );
    }
}
