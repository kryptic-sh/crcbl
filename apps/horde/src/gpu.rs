//! GPU setup for horde: the shared [`crcbl::engine`] join, a clear, the sprite
//! pass, the menu pass and the UI pass.
//!
//! Everything that is not specific to this game — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — lives in [`crcbl::engine::GpuContext`], which every other
//! sample's `gpu.rs` uses too.
//!
//! # The camera follows the player
//!
//! Asteroids and breakout fit their whole field on screen because their fields
//! are the size of a window. This arena is 96 × 72 units against a view of about
//! 37 × 28, because a survivors game needs somewhere to run *to* — so the camera
//! tracks the player and stops at the walls. [`camera_centre`] is that rule, and
//! it is the only place the view's extent is resolved. [`crate::art`] culls
//! against the same function, because a cull box and a projection that disagreed
//! would delete sprites the player can see.
//!
//! **The vertical extent is fixed and the horizontal is not.** A wide window
//! shows more of the arena rather than the same amount at a different zoom,
//! which is what stops the player's speed reading differently at different
//! window shapes. Breakout and asteroids do the opposite — widen until the whole
//! field fits — because their fields are bounded and this one is not framed.
//!
//! # Pass order is declaration order
//!
//! `arena` (clear) → `sprites` (the field) → `menu` → `ui` (the HUD and the
//! debug panel). The last three load rather than clear. The menu is **between**
//! the game and the text for the reason `crcbl_render::menu` gives: its scrim
//! dims what is already in the target, so it must come after the game, and its
//! panel is opaque while its labels are UI-pass text, so it must come before the
//! UI.

use crcbl::backend::GpuBackend;
use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError};
use crcbl::hal::{CommandEncoderDesc, Features};
use crcbl::math::Vec3;
use crcbl::prelude::*;
use crcbl::render::{
    Camera, ForwardRenderer, MenuRenderer, PassTimers, Projection, RenderGraph, SpriteRenderer,
    TransientPool, UiRenderer,
};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;
use glam::DVec3;

use crate::art::{GROUND, Scene, TEXELS_PER_UNIT};
use crate::game::{ARENA_HALF_HEIGHT, ARENA_HALF_WIDTH, RenderState, VIEW_HALF_HEIGHT, clamp_axis};

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;
const MAX_TIMED_PASSES: u32 = 8;

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

/// The camera projection for an `extent`-sized viewport.
///
/// **In sprite units, not world units** — see [`crate::art`]'s header.
fn projection() -> Projection {
    Projection::Orthographic {
        half_height: VIEW_HALF_HEIGHT as f32 * TEXELS_PER_UNIT,
        near: 0.1,
        far: 100.0,
    }
}

/// The camera for a player at `player`, on an `extent`-sized viewport.
///
/// Written out rather than left to [`Camera::default`], as breakout's and
/// asteroids' are: where the camera is is a fact [`crate::art::Scene::build`]
/// depends on — it resolves its layers against the same point — and a fact this
/// game depends on belongs where it can be read and tested.
fn camera(player: DVec3, extent: (u32, u32)) -> Camera {
    let centre = camera_centre(player, extent);
    let (x, y) = (
        centre.x as f32 * TEXELS_PER_UNIT,
        centre.y as f32 * TEXELS_PER_UNIT,
    );
    let mut camera = Camera::default().with_projection(projection());
    camera.eye = Vec3::new(x, y, 2.0);
    camera.target = Vec3::new(x, y, 0.0);
    camera
}

// ---- Gpu --------------------------------------------------------------------

#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    pool: TransientPool,
    timers: Option<PassTimers>,
    camera: Camera,
    /// This frame's world, copied rather than borrowed: [`Gpu::frame`] runs
    /// after the caller has moved on, and the state is refilled every frame.
    render: RenderState,
    /// The sprite pass, and the art it draws.
    sprites: SpriteRenderer,
    scene: Scene,
    /// The menu pass: its own sheets, its own screen-space camera, and a pass
    /// that declares nothing on a frame with no menu on it.
    menu: MenuRenderer,
    /// UI compositing.
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
    /// Opens a backend, a surface, a device and a swapchain, and builds the
    /// sprite, menu and UI renderers.
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

    /// Builds this game's renderers on an already-open context, and uploads the
    /// art.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if a renderer refused the device or a sheet upload failed.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        let mut sprites =
            SpriteRenderer::new(ctx.device(), ctx.queue(), format).map_err(GpuError::Hal)?;
        // Registering a sheet is a blocking staging upload — start-up work, like
        // the glyph atlas below it, and never something a frame does.
        let scene = match Scene::new(ctx.device(), &mut sprites) {
            Ok(scene) => scene,
            Err(error) => {
                sprites.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };
        let menu = match MenuRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(menu) => menu,
            Err(error) => {
                sprites.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(ui) => ui,
            Err(error) => {
                menu.destroy(ctx.device());
                sprites.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        let extent = ctx.extent();
        Ok(Self {
            camera: camera(DVec3::ZERO, extent),
            ctx,
            pool: TransientPool::new(),
            timers,
            render: RenderState::default(),
            sprites,
            scene,
            menu,
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

    /// Takes this frame's world.
    ///
    /// **No `alpha`.** Asteroids interpolates because it turns things; nothing
    /// in this game rotates and nothing is drawn between two ticks, so a frame
    /// draws the last tick's positions exactly — which is also what every other
    /// sample but asteroids does.
    pub fn set_world(&mut self, render: &RenderState) {
        self.render.clone_from(render);
    }

    /// Takes this frame's draw list, handing the previous frame's allocation
    /// back so the caller can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, dl: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, dl);
    }

    /// Takes this frame's menu, or `None` on a frame that shows none.
    ///
    /// CPU only — the upload happens inside [`Gpu::frame`], at the extent the
    /// swapchain was actually acquired at.
    pub fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        self.menu.set_menu(menu);
    }

    /// The sprites the menu pass will draw this frame, for the loop's own tests.
    #[cfg(test)]
    pub fn menu_sprites(&self) -> &[crcbl::render::Sprite] {
        self.menu.frame_sprites()
    }

    /// The UI geometry this frame handed over, for the loop's own tests — the
    /// list the UI pass actually uploads, HUD and debug overlay together.
    #[cfg(test)]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
    }

    /// The game's own sprites for this frame, for the same.
    #[cfg(test)]
    pub fn scene_sprites(&mut self) -> Vec<crcbl::render::Sprite> {
        let render = std::mem::take(&mut self.render);
        let extent = self.ctx.extent();
        let sprites = self.scene.build(&render, extent).to_vec();
        self.render = render;
        sprites
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

        // The swapchain's extent, not the one the resize event reported: on the
        // frame a reconfigure lands they can differ, and the camera must agree
        // with the surface actually being drawn into — and with the cull box
        // `Scene::build` computes from the same extent below.
        self.camera = camera(self.render.player, extent);
        let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
        let view_projection = self.camera.view_projection(aspect);

        let sprites = self.scene.build(&self.render, extent);
        self.sprites
            .begin_frame(self.ctx.device(), sprites, view_projection, extent)
            .map_err(GpuError::Hal)?;
        self.menu
            .begin_frame(self.ctx.device(), extent)
            .map_err(GpuError::Hal)?;
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
            self.sprites.add_pass(&mut graph, target);
            self.menu.add_pass(&mut graph, target);
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
        self.menu.destroy(self.ctx.device());
        self.sprites.destroy(self.ctx.device());
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

    /// Where a world point lands in normalised device coordinates, through the
    /// real projection.
    fn ndc(world: DVec3, player: DVec3, extent: (u32, u32)) -> glam::Vec2 {
        let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
        let view_projection = camera(player, extent).view_projection(aspect);
        let clip = view_projection
            * glam::Vec4::new(
                world.x as f32 * TEXELS_PER_UNIT,
                world.y as f32 * TEXELS_PER_UNIT,
                0.0,
                1.0,
            );
        glam::Vec2::new(clip.x / clip.w, clip.y / clip.w)
    }

    /// **The camera never shows ground outside the arena.**
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

    /// **The player is always inside the view**, measured through the real
    /// projection — which is the half a camera that simply sat at the origin
    /// would fail, and the assumption `art::Scene::build` rests on when it does
    /// not cull the player.
    #[test]
    fn the_player_is_always_inside_the_view() {
        for extent in EXTENTS {
            for player in player_positions() {
                let point = ndc(player, player, extent);
                assert!(
                    (-1.0..=1.0).contains(&point.x) && (-1.0..=1.0).contains(&point.y),
                    "{extent:?} put the player at {player:?} at {point:?} in NDC",
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

    /// The camera's own position is the middle of the view, and `+Y` is up.
    #[test]
    fn the_projection_centres_the_camera_and_keeps_y_up() {
        let extent = (960, 720);
        let player = DVec3::new(-7.0, 3.0, 0.0);
        let centre = ndc(player, player, extent);
        assert!(centre.x.abs() < 1e-5 && centre.y.abs() < 1e-5, "{centre:?}");

        let above = ndc(player + DVec3::Y, player, extent);
        assert!(above.y > 0.0, "world +Y must go up the screen: {above:?}");
        let right = ndc(player + DVec3::X, player, extent);
        assert!(right.x > 0.0, "world +X must go right: {right:?}");
    }

    /// A world unit is the same height on screen whatever the window's width,
    /// so the player's speed reads the same at every aspect ratio and a wide
    /// window shows *more arena* rather than a smaller picture.
    #[test]
    fn one_world_unit_is_the_same_size_at_every_width() {
        let centre = DVec3::ZERO;
        let tall = ndc(DVec3::Y, centre, (600, 720)).y;
        let wide = ndc(DVec3::Y, centre, (4000, 720)).y;
        assert!((tall - wide).abs() < 1e-6, "{tall} vs {wide}");
        // …and the horizontal extent really does grow with the width.
        assert!(view_half_width((4000, 720)) > view_half_width((600, 720)));
    }
}
