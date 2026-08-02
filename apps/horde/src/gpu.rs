//! GPU setup for horde: the shared [`crcbl::engine`] join, a clear, and the UI
//! pass.
//!
//! # There is deliberately no sprite pass yet
//!
//! `docs/plan/sample/03-horde.md` calls for `.crpix` sprites, and this sub-slice
//! is the simulation. A sprite pass with no sheet to draw is not a placeholder,
//! it is a texture upload of nothing: [`SpriteRenderer`] takes a registered
//! sheet, a sheet comes from `build.rs` baking authored `.crpix` files, and none
//! of that is cheaper than the alternative below. So the arena is drawn as
//! untextured quads through the UI pass — a rectangle per enemy, per bolt and
//! for the player — which is exactly what asteroids did before its art sub-slice
//! replaced it wholesale.
//!
//! **That placeholder does not scale, and it is not meant to.** One `DrawList`
//! quad per enemy is six vertices uploaded per enemy per frame through the UI
//! pass' per-frame vertex buffer, which is the opposite of the instanced path
//! the plan's 10k claim rests on. The art sub-slice moves the field to
//! [`SpriteRenderer`], which batches per sheet; until then the honest ceiling on
//! what this window can draw is a few thousand, and it is lower than the
//! ceiling on what the *simulation* can tick. `crate::app` caps the draw rather
//! than the simulation, so the two numbers stay separable.
//!
//! # The camera follows the player
//!
//! Asteroids and breakout fit their whole field on screen because their fields
//! are the size of a window. This arena is 96 × 72 units against a view of about
//! 37 × 28, because a survivors game needs somewhere to run *to* — so the camera
//! tracks the player and stops at the walls. [`camera_centre`] is that rule, and
//! it is the only place the view's extent is resolved.
//!
//! # Pass order is declaration order
//!
//! `arena` (clear) → `ui` (the field and the HUD). The UI pass loads rather than
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
use glam::{DVec3, Vec2};

use crate::game::{ARENA_HALF_HEIGHT, ARENA_HALF_WIDTH, VIEW_HALF_HEIGHT, clamp_axis};

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;
const MAX_TIMED_PASSES: u32 = 4;

/// What the swapchain is cleared to. A dark, slightly warm ground, so the
/// arena's border and the pale bodies on it both read against it.
pub const GROUND: [f32; 4] = [0.05, 0.045, 0.055, 1.0];

/// How many pixels one world unit is, at `extent`.
///
/// Fixed by the **vertical** extent alone, because the camera follows: a wide
/// window shows more of the arena rather than the same amount at a different
/// zoom, which is what stops the player's speed reading differently at different
/// window shapes.
#[must_use]
pub fn pixels_per_unit(extent: (u32, u32)) -> f32 {
    extent.1.max(1) as f32 / (2.0 * VIEW_HALF_HEIGHT as f32)
}

/// Half the view's horizontal extent, in world units, at `extent`.
#[must_use]
pub fn view_half_width(extent: (u32, u32)) -> f64 {
    let aspect = f64::from(extent.0.max(1)) / f64::from(extent.1.max(1));
    VIEW_HALF_HEIGHT * aspect
}

/// Where the camera sits when the player is at `player`.
///
/// The player, clamped so the view never looks past a wall — the rule every
/// follow camera in a bounded arena needs, and the reason the player drifts off
/// centre near an edge instead of the arena's edge drifting into view.
///
/// If the view is *wider* than the arena on an axis — a very wide window, or a
/// future smaller arena — there is no clamp that satisfies both, so the camera
/// centres that axis on the arena and the margin is symmetric.
/// [`crate::game::clamp_axis`] is what makes that case fall out rather than
/// needing a branch here: a negative half is the middle.
#[must_use]
pub fn camera_centre(player: DVec3, extent: (u32, u32)) -> DVec3 {
    DVec3::new(
        clamp_axis(player.x, ARENA_HALF_WIDTH - view_half_width(extent)),
        clamp_axis(player.y, ARENA_HALF_HEIGHT - VIEW_HALF_HEIGHT),
        0.0,
    )
}

/// Where a world point lands in framebuffer pixels, for a camera at `camera`.
///
/// Y is flipped: the world's `+Y` is up and the framebuffer's is down.
#[must_use]
pub fn world_to_screen(world: DVec3, camera: DVec3, extent: (u32, u32)) -> Vec2 {
    let scale = pixels_per_unit(extent);
    let screen_centre = Vec2::new(extent.0 as f32 / 2.0, extent.1 as f32 / 2.0);
    let offset = world - camera;
    screen_centre + Vec2::new(offset.x as f32 * scale, -offset.y as f32 * scale)
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
        label: "horde",
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

    /// The UI geometry this frame handed over, for the loop's own tests.
    #[cfg(test)]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
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
                .add_render_pass("arena")
                .clear_color(target, GROUND)
                .execute(|_| {});
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        if !self.dumped {
            log::debug!("render graph for horde:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("horde frame"),
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
    use crate::game::PLAYER_RADIUS;

    /// Every window shape the samples are run at, plus the two that break a
    /// naive camera.
    const EXTENTS: [(u32, u32); 6] = [
        (960, 720),   // what the window opens at
        (1920, 1080), // 16:9
        (1440, 400),  // a canvas clamped by `max-height: 68vh`
        (600, 900),   // taller than it is wide
        (1000, 800),  // 5:4
        (4000, 400),  // wider than the arena, which is the case with no clamp
    ];

    /// Where the player can legitimately be: the arena, minus its own radius,
    /// which is what `game::clamp_to_arena` guarantees.
    fn player_positions() -> Vec<DVec3> {
        let (w, h) = (
            ARENA_HALF_WIDTH - PLAYER_RADIUS,
            ARENA_HALF_HEIGHT - PLAYER_RADIUS,
        );
        vec![
            DVec3::ZERO,
            DVec3::new(w, h, 0.0),
            DVec3::new(-w, -h, 0.0),
            DVec3::new(w, -h, 0.0),
            DVec3::new(-w, h, 0.0),
            DVec3::new(3.5, -11.25, 0.0),
        ]
    }

    /// **The camera never looks past a wall.**
    ///
    /// The whole point of clamping it: an arena edge sliding into view is the
    /// one thing that tells the player the world is smaller than it looks. The
    /// exception is an axis where the view is genuinely wider than the arena —
    /// there is no camera that satisfies both, and the test says which case it
    /// is rather than skipping it.
    #[test]
    fn the_camera_never_shows_ground_outside_the_arena() {
        // The vertical half of the assertion below only means anything while
        // the arena is taller than the view; a `const` assertion, because both
        // are constants and a runtime one would only be checked here.
        const { assert!(VIEW_HALF_HEIGHT <= ARENA_HALF_HEIGHT) };
        for extent in EXTENTS {
            let half_x = view_half_width(extent);
            for player in player_positions() {
                let centre = camera_centre(player, extent);
                if half_x <= ARENA_HALF_WIDTH {
                    assert!(
                        centre.x.abs() <= ARENA_HALF_WIDTH - half_x + 1e-9,
                        "{extent:?} at {player:?} looked past the side wall: {centre:?}",
                    );
                } else {
                    assert_eq!(centre.x, 0.0, "a view wider than the arena is centred");
                }
                assert!(
                    centre.y.abs() <= ARENA_HALF_HEIGHT - VIEW_HALF_HEIGHT + 1e-9,
                    "{extent:?} at {player:?} looked past the top wall: {centre:?}",
                );
            }
        }
    }

    /// **The player is always on screen**, which is the other half of the same
    /// clamp and the half a camera that simply sat at the origin would fail.
    #[test]
    fn the_player_is_always_inside_the_view() {
        for extent in EXTENTS {
            for player in player_positions() {
                let centre = camera_centre(player, extent);
                let pixel = world_to_screen(player, centre, extent);
                assert!(
                    (0.0..=extent.0 as f32).contains(&pixel.x)
                        && (0.0..=extent.1 as f32).contains(&pixel.y),
                    "{extent:?} put the player at {player:?} off screen at {pixel:?}",
                );
            }
        }
    }

    /// A camera that never moved would pass the test above at the origin and
    /// fail it in a corner, so this asserts the camera actually tracks.
    #[test]
    fn the_camera_tracks_the_player_away_from_the_walls() {
        let extent = (960, 720);
        let near = DVec3::new(4.0, -3.0, 0.0);
        assert_eq!(
            camera_centre(near, extent),
            near,
            "well inside the arena the camera is the player",
        );
        assert_ne!(
            camera_centre(DVec3::new(20.0, 0.0, 0.0), extent),
            camera_centre(DVec3::ZERO, extent),
            "the camera did not move at all",
        );
    }

    /// The camera's own position is the middle of the window, and `+Y` is up.
    #[test]
    fn the_mapping_centres_the_camera_and_flips_y() {
        let extent = (960, 720);
        let camera = DVec3::new(-7.0, 3.0, 0.0);
        assert_eq!(
            world_to_screen(camera, camera, extent),
            Vec2::new(480.0, 360.0)
        );
        let above = world_to_screen(camera + DVec3::Y, camera, extent);
        assert!(above.y < 360.0, "world +Y must go up the screen: {above:?}",);
        let right = world_to_screen(camera + DVec3::X, camera, extent);
        assert!(right.x > 480.0, "world +X must go right: {right:?}");
    }

    /// A world unit is the same number of pixels whatever the window's width,
    /// so the player's speed reads the same at every aspect ratio.
    #[test]
    fn one_world_unit_is_the_same_size_at_every_width() {
        let tall = pixels_per_unit((600, 720));
        let wide = pixels_per_unit((4000, 720));
        assert_eq!(tall, wide);
        assert!((tall - 720.0 / (2.0 * VIEW_HALF_HEIGHT as f32)).abs() < 1e-6);
        // …and a taller window is a bigger picture of the same world.
        assert!(pixels_per_unit((960, 1440)) > tall);
    }
}
