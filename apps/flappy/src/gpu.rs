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
//! That is why [`camera_x`] is public and everything placed against the view —
//! the sprites, and the parallax offset every background band takes — is
//! derived from **one** call to it per frame, in [`Gpu::frame`]. Two copies of
//! a number that has to agree is how the first version of breakout's camera
//! went wrong.
//!
//! # There is no forward pass
//!
//! There was one, and it drew the bird as a lit cube, because
//! `ForwardRenderer::begin_frame` takes a single `model: Mat4` and one instance
//! was all flappy could get out of it. The bird is a sprite now, so the forward
//! renderer had nothing left to draw — and it was carrying an HDR scene target,
//! a depth buffer, a tonemap pass and a cube mesh to draw it. All of that is
//! gone; what the pass also did, and what had to be replaced, is **clear the
//! swapchain**, which is now a one-line `clear_color` pass named `sky`.
//! [`ForwardRenderer::present_target`] survives as the import helper, which is
//! an associated function and needs no renderer.
//!
//! # Pass order is declaration order
//!
//! `sky` (clear) → `sprites` (the game) → `ui` (the HUD). The last two both
//! load the target rather than clearing it, so declaring the UI pass first
//! would put the course on top of the score.

use crcbl::backend::GpuBackend;
use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, PendingGpuContext};
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

use crate::art::{Scene, TEXELS_PER_UNIT};
use crate::game::{PipeView, RenderState};

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
/// Public because everything on screen is placed against it: the sprite pass's
/// view-projection, and — through [`crate::art::Scene::build`] — the offset
/// every parallax band takes. [`Gpu::frame`] calls it **once** and passes the
/// result to both.
#[must_use]
pub fn camera_x(bird_x: f64, extent: (u32, u32)) -> f32 {
    let half_width = camera_half_width(extent);
    // `0.5 - fraction` of the view sits behind the bird's own offset from the
    // centre, so a fraction of 0.5 would be a camera locked to the bird.
    bird_x as f32 + half_width * (1.0 - 2.0 * BIRD_SCREEN_FRACTION)
}

/// The camera projection for an `extent`-sized viewport.
///
/// **In sprite units, not world units.** [`crate::art`]'s header sets out why
/// the sprite plane is scaled: a nine-slice's fixed bands are its insets taken
/// as one target unit per texel, so the pipe's cap only keeps its shape if one
/// texel is one unit of the space the sprites are drawn in. The camera is
/// scaled to match here, in the same function that decides the half-height, so
/// there is one place the two can be made to disagree and it is this one.
fn projection() -> Projection {
    Projection::Orthographic {
        half_height: camera_half_height() * TEXELS_PER_UNIT,
        near: 0.1,
        far: 100.0,
    }
}

// ---- Gpu --------------------------------------------------------------------

#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    pool: TransientPool,
    timers: Option<PassTimers>,
    camera: Camera,
    /// Where the bird is this frame, from the game. Drives the camera and the
    /// bird sprite.
    bird: glam::DVec3,
    /// The course this frame, refilled rather than reallocated.
    pipes: Vec<PipeView>,
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

    /// Builds this game's renderers on an already-open context, and uploads the
    /// art.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the sprite pass or the UI compositor refused the device,
    /// or if a sheet upload failed.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        let mut sprites =
            SpriteRenderer::new(ctx.device(), ctx.queue(), format).map_err(GpuError::Hal)?;
        // Registering a sheet is a blocking staging upload — start-up work,
        // like the glyph atlas below it, and never something a frame does.
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

        Ok(Self {
            ctx,
            pool: TransientPool::new(),
            timers,
            camera: Camera::default().with_projection(projection()),
            bird: crate::game::BIRD_START,
            pipes: Vec::new(),
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

    /// Takes this frame's world: where the bird is, how fast it is climbing, and
    /// where the course is.
    ///
    /// The pipes are copied into a `Vec` this struct keeps rather than borrowed,
    /// because [`Gpu::frame`] runs after the caller has moved on and the list is
    /// refilled every frame from a treadmill that hands over a fresh one.
    ///
    /// The velocity goes to [`Scene::observe`], which starts the wing beat over
    /// on a flap — see its docs for why the animation reads the bird's motion
    /// rather than the button. It happens **here**, before
    /// [`Gpu::advance_animation`], so a frame in which the player flapped
    /// restarts the clip and then advances it by that frame's ticks, exactly as
    /// a frame that did not flap advances the clip it was already on.
    pub fn set_world(&mut self, render: &RenderState) {
        self.bird = render.bird;
        self.scene.observe(render.bird_velocity.y);
        self.pipes.clear();
        self.pipes.extend_from_slice(&render.pipes);
    }

    /// Moves the bird's flap on by whole simulation ticks.
    pub const fn advance_animation(&mut self, ticks: u64) {
        self.scene.advance(ticks);
    }

    /// The course this frame, for the loop's own tests.
    #[cfg(test)]
    pub fn pipes(&self) -> &[PipeView] {
        &self.pipes
    }

    /// How many ticks the bird's flap has been advanced by, for the same.
    #[cfg(test)]
    pub const fn animation_ticks(&self) -> u64 {
        self.scene.animation_ticks()
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

    /// The UI geometry this frame handed over, for the loop's own tests — the
    /// list the UI pass actually uploads, HUD and debug overlay together.
    #[cfg(test)]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
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
        //
        // **One call to `camera_x`, and everything else is derived from it.**
        // The sprite plane is `TEXELS_PER_UNIT` times the world (see
        // `crate::art`), so the scale is applied here, once, to the value both
        // the projection and the parallax offsets are built from.
        let centre = camera_x(self.bird.x, extent) * TEXELS_PER_UNIT;
        let half_width = camera_half_width(extent) * TEXELS_PER_UNIT;
        self.camera.projection = projection();
        self.camera.eye = Vec3::new(centre, 0.0, 2.0);
        self.camera.target = Vec3::new(centre, 0.0, 0.0);

        let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
        let view_projection = self.camera.view_projection(aspect);
        let sprites = self.scene.build(self.bird, &self.pipes, centre, half_width);
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
            // The clear the forward pass used to do on its way to a tonemap.
            // Declared first, and it is the only pass that does not load.
            graph
                .add_render_pass("sky")
                .clear_color(target, crate::art::SKY)
                .execute(|_| {});
            self.sprites.add_pass(&mut graph, target);
            // **Between the course and the text, and that order is the whole
            // join.** The menu's scrim dims what is already in the target, so it
            // has to come after the course; the panel is opaque and the labels
            // are UI-pass text, so it has to come before the UI or the frame
            // paints over its own words.
            self.menu.add_pass(&mut graph, target);
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
    ///
    /// Measured **through the real projection**, at every aspect ratio a window
    /// or a canvas can hand us: this is where `app.rs`'s
    /// `the_playable_band_is_on_screen_at_every_aspect_ratio` moved to when
    /// `WorldToScreen` — the hand-written mapping it used to check — went away,
    /// and it is a stronger test there than it was here, because the matrix the
    /// sprite pass is actually given is what answers.
    #[test]
    fn the_playable_band_is_on_screen_at_every_aspect_ratio() {
        let half_height = camera_half_height();
        assert!(half_height > crate::game::WORLD_CEILING as f32);
        assert!(-half_height < crate::game::WORLD_FLOOR as f32);

        for extent in [
            (960, 720),   // what the window opens at, and the canvas's ratio
            (800, 600),   // 4:3 again, smaller
            (1920, 1080), // 16:9
            (1440, 400),  // a canvas clamped by `max-height: 68vh`
            (600, 900),   // taller than it is wide
        ] {
            let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
            let centre = camera_x(0.0, extent) * TEXELS_PER_UNIT;
            let mut camera = Camera::default().with_projection(projection());
            camera.eye = Vec3::new(centre, 0.0, 2.0);
            camera.target = Vec3::new(centre, 0.0, 0.0);
            let view_projection = camera.view_projection(aspect);

            for y in [crate::game::WORLD_CEILING, crate::game::WORLD_FLOOR] {
                // The bird's own column — `camera_x` was asked where the view
                // goes when the bird is at 0 — so the horizontal assertion below
                // is about the bird and not about the middle of the screen.
                let world = glam::Vec4::new(0.0, y as f32 * TEXELS_PER_UNIT, 0.0, 1.0);
                let clip = view_projection * world;
                let ndc = clip.y / clip.w;
                assert!(
                    (-1.0..=1.0).contains(&ndc),
                    "{extent:?} put y = {y} at {ndc} in NDC"
                );
                // And the horizontal placement is the bird's third, in the same
                // matrix rather than in a second mapping.
                let across = (clip.x / clip.w) * 0.5 + 0.5;
                assert!(
                    (0.25..0.35).contains(&across),
                    "{extent:?} put the bird {across} across"
                );
            }
        }
    }
}
