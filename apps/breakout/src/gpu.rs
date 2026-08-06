//! GPU setup for breakout: the shared [`crcbl::engine`] join plus this game's
//! own renderers.
//!
//! Everything that is not specific to breakout — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — lives in [`crcbl::engine::GpuContext`], which
//! `apps/sandbox/src/gpu.rs` uses too. This file is what is left: an
//! orthographic camera, the sprite pass, the UI compositor, and the graph that
//! joins them.
//!
//! # There is no forward pass
//!
//! There was one, and it drew the paddle as a lit cube, because
//! [`ForwardRenderer::begin_frame`] takes a single `model: Mat4` and one
//! instance was all breakout could get out of it — which is why the ball and the
//! forty bricks went through the UI pass as screen-space quads instead. The
//! paddle is a sprite now, so the forward renderer had nothing left to draw, and
//! it was carrying an HDR scene target, a depth buffer, a tonemap pass, a
//! directional light and a cube mesh in order to draw it. All of that is gone;
//! what the pass also did, and what had to be replaced, is **clear the
//! swapchain**, which is now a one-line `clear_color` pass named `surround`.
//! [`ForwardRenderer::present_target`] survives as the import helper, which is
//! an associated function and needs no renderer.
//!
//! # Pass order is declaration order
//!
//! `surround` (clear) → `sprites` (the board) → `ui` (the HUD). The last two
//! both load the target rather than clearing it, so declaring the UI pass first
//! would put the court on top of the score.
//!
//! # The camera is in world units
//!
//! The sprite plane is world units — the art's texels-per-unit scale lives on
//! the nine-slice sources in [`crate::art`], so [`projection`] has nothing to
//! compensate for.

use crcbl::engine::{
    FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions, PendingGpuContext,
};
use crcbl::hal::{CommandEncoderDesc, Features};
use crcbl::math::DVec3;
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

use crate::art::{SURROUND, Scene};
use crate::game::RenderState;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;
const MAX_TIMED_PASSES: u32 = 8;

/// Half the vertical extent of the orthographic camera for a `extent`-sized
/// viewport, in world units.
///
/// [`Projection::Orthographic`] takes a half *height* and derives the width from
/// the aspect ratio, so a fixed 9.0 shows `9 * aspect` of world horizontally —
/// which at 4:3 is 12 units against a play field that runs to
/// [`WORLD_RIGHT`](crate::game::WORLD_RIGHT) = 14. The ball left the screen at
/// x ≈ ±12 and reappeared once it had bounced off a wall the player could not
/// see; the window opens at 960×720 and the web demo's canvas is
/// `aspect-ratio: 4 / 3`, so both were cropped.
///
/// Widening the camera until the whole field fits is the fix, and it holds at
/// every aspect ratio: a viewport too narrow to show the field at 9.0 gets
/// letterboxed vertically instead of cropped horizontally, and a wide one keeps
/// 9.0 and shows some empty margin either side.
///
/// **In world units.** [`projection`] uses it directly — the sprite plane is
/// world units too, so nothing here scales.
#[must_use]
pub fn camera_half_height(extent: (u32, u32)) -> f32 {
    let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
    let half_height = crate::game::WORLD_TOP as f32 + VIEW_MARGIN;
    let half_width = crate::game::WORLD_RIGHT as f32 + VIEW_MARGIN;
    half_height.max(half_width / aspect)
}

/// Slack kept between the play field and the edge of the surface, in world
/// units.
///
/// Framing, and now only framing. It was added to cover a real overshoot:
/// `crcbl-phys`'s swept-sphere query traversed the broadphase with the sphere's
/// **centre line**, so a wall was not offered to the narrow phase until the
/// ball's centre reached its AABB, and the ball was drawn up to
/// [`BALL_RADIUS`](crate::game::BALL_RADIUS) inside the wall on the tick before
/// the bounce registered. `PhysicsWorld::sweep_sphere` now traverses the swept
/// volume, so contact lands when the surfaces meet and the ball never crosses
/// the face at all.
///
/// Kept because a field flush with the surface edge reads as a rendering bug —
/// the ball touching the last row of pixels looks like the clipping this
/// module's other comment is about — and half a unit is cheap.
const VIEW_MARGIN: f32 = 0.5;

/// The camera projection for an `extent`-sized viewport, in world units —
/// matching the sprite plane, whose nine-slice sources carry their own
/// texels-per-unit scale in [`crate::art`].
fn projection(extent: (u32, u32)) -> Projection {
    Projection::Orthographic {
        half_height: camera_half_height(extent),
        near: 0.1,
        far: 100.0,
    }
}

/// The camera for an `extent`-sized viewport: centred on the origin, looking
/// down −Z.
///
/// **Written out rather than left to [`Camera::default`].** Breakout's field is
/// fixed and symmetric about the origin, and [`crate::art::Scene::build`]
/// resolves its layers against a camera of `[0, 0]` because of it — so where the
/// camera is is a fact this game depends on, and a fact it depends on belongs in
/// this file where it can be read and tested rather than inherited from another
/// crate's default. Flappy's camera moves and sets the same two fields for the
/// same reason.
fn camera(extent: (u32, u32)) -> Camera {
    let mut camera = Camera::default().with_projection(projection(extent));
    camera.eye = Vec3::new(0.0, 0.0, 2.0);
    camera.target = Vec3::ZERO;
    camera
}

// ---- Gpu --------------------------------------------------------------------

#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    pool: TransientPool,
    timers: Option<PassTimers>,
    camera: Camera,
    /// Paddle X for this frame, from the game state.
    paddle_x: f64,
    /// Where the ball is this frame.
    ball: DVec3,
    /// The live brick centres this frame, refilled rather than reallocated.
    bricks: Vec<DVec3>,
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
/// One value rather than two copies of the same descriptor: the browser path
/// and the native path must open the *same* device, or a feature that only the
/// blocking path requested is a bug nobody sees until someone loads the page.
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "breakout",
        // `optional_features`, not required: the UI pass hands `ui.slang` its
        // viewport size through a push constant where there are any, and
        // through a uniform buffer where there are none — see
        // `crcbl::render::ConstantDelivery`. A browser is the second case and
        // always will be, since WebGPU has no push constants at all.
        optional_features: Features::TIER_A
            | Features::TIMESTAMP_QUERY
            | Features::DEBUG_MARKERS
            | Features::PUSH_CONSTANTS,
        ..GpuContextDesc::from(gpu)
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
        gpu: GpuOptions,
    ) -> Result<Self, GpuError> {
        Self::from_context(GpuContext::open(shell, window, extent, &desc(gpu))?)
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
        gpu: GpuOptions,
    ) -> Result<PendingGpu, GpuError> {
        Ok(PendingGpu {
            pending: GpuContext::request_open(shell, window, extent, &desc(gpu))?,
        })
    }

    /// Builds this game's renderers on an already-open context.
    ///
    /// The half of start-up that is the same however the context arrived, which
    /// is why it is a function rather than a copy in each of the two paths.
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

        // Breakout is a 2D game: orthographic projection with reversed-Z depth.
        // The extent is the one the context opened at; `frame` re-derives it
        // every frame, because a resize changes the aspect ratio and with it
        // how wide the camera has to be to keep the whole field on screen.
        let camera = camera(ctx.extent());

        Ok(Self {
            ctx,
            pool: TransientPool::new(),
            timers,
            camera,
            paddle_x: 0.0,
            ball: DVec3::ZERO,
            bricks: Vec::new(),
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

    /// Takes this frame's board: the paddle, the ball, and every live brick.
    ///
    /// The brick centres are copied into a `Vec` this struct keeps rather than
    /// borrowed, because [`Gpu::frame`] runs after the caller has moved on and
    /// the list is refilled every frame from a buffer the game reuses.
    pub fn set_board(&mut self, render: &RenderState) {
        self.paddle_x = render.paddle_x;
        self.ball = render.ball;
        self.bricks.clear();
        self.bricks.extend_from_slice(&render.bricks);
    }

    /// The live bricks this frame, for the loop's own tests.
    #[cfg(test)]
    pub fn bricks(&self) -> &[DVec3] {
        &self.bricks
    }

    /// The UI geometry this frame handed over, for the loop's own tests — the
    /// list the UI pass actually uploads, HUD and debug overlay together.
    #[cfg(test)]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
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
        self.camera = camera(extent);

        let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
        let view_projection = self.camera.view_projection(aspect);
        let sprites = self.scene.build(self.paddle_x, self.ball, &self.bricks);
        self.sprites
            .begin_frame(self.ctx.device(), sprites, view_projection, extent)
            .map_err(GpuError::Hal)?;
        self.menu
            .begin_frame(self.ctx.device(), extent)
            .map_err(GpuError::Hal)?;
        // Upload UI geometry for this frame: the HUD, and only the HUD.
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
                .add_render_pass("surround")
                .clear_color(target, SURROUND)
                .execute(|_| {});
            self.sprites.add_pass(&mut graph, target);
            // **Between the board and the text, and that order is the whole
            // join.** The menu's scrim dims what is already in the target, so it
            // has to come after the board; the panel is opaque and the labels
            // are UI-pass text, so it has to come before the UI or the frame
            // paints over its own words.
            self.menu.add_pass(&mut graph, target);
            // Composite the UI on top of the board.
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        if !self.dumped {
            crcbl::log::debug!("render graph for breakout:\n{}", compiled.dump());
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
        self.menu.destroy(self.ctx.device());
        self.sprites.destroy(self.ctx.device());
        self.pool.destroy(self.ctx.device());
        if let Some(timers) = self.timers.as_mut() {
            timers.destroy(self.ctx.device());
        }
        self.ctx.destroy()
    }
}

/// Lets [`crcbl::engine::PolledBoot`] drive this bundle's arrival.
///
/// Two one-line forwards: the methods below already existed for the blocking
/// path, and the trait is what lets the engine own the state machine that used
/// to be written out in every sample's `app.rs`. The extent and the resize are
/// [`crcbl::engine::GpuSurface`]'s, because a running loop asks the same two.
impl crcbl::engine::PolledGpu for Gpu {
    type Pending = PendingGpu;

    fn request<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
    ) -> Result<Self::Pending, GpuError> {
        Self::request_open(shell, window, extent, gpu)
    }

    fn poll_pending(pending: &mut Self::Pending) -> Result<Option<Self>, GpuError> {
        pending.poll()
    }
}

/// The frame's half of this bundle, for [`crcbl::engine::Loop`].
///
/// Six one-line forwards. Every one already existed for the loop that used to
/// call them from `app.rs`; the trait is what lets the engine call them instead.
impl crcbl::engine::GameGpu for Gpu {
    fn atlas(&self) -> &FontAtlas {
        Self::atlas(self)
    }

    fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        Self::set_menu(self, menu);
    }

    fn take_draw_list(&mut self, list: &mut DrawList) {
        Self::take_draw_list(self, list);
    }

    fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        Self::timings(self)
    }

    fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
        Self::frame(self)
    }

    fn destroy(self) -> Result<(), GpuError> {
        Self::destroy(self)
    }
}

/// The two questions both halves of the engine ask a swapchain's owner.
impl crcbl::engine::GpuSurface for Gpu {
    fn extent(&self) -> (u32, u32) {
        Self::extent(self)
    }

    fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        Self::resize(self, extent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{WORLD_LEFT, WORLD_RIGHT, WORLD_TOP};

    /// The whole play field is on screen, at every aspect ratio a window or a
    /// canvas can hand us.
    ///
    /// The camera used to be a fixed `half_height: 9.0` and the projection
    /// derives its width from the aspect ratio, so a 4:3 surface — the size the
    /// window opens at, and the `aspect-ratio: 4 / 3` the web demo's canvas is
    /// styled with — showed 12 world units either side of a field that runs to
    /// 14. The ball vanished off the edge at x ≈ ±12 and came back once it had
    /// bounced off a wall that was never drawn on screen.
    ///
    /// **This moved here from `app.rs` when the board became sprites.** It used
    /// to measure `WorldToScreen`, the hand-written world→pixel mapping the UI
    /// quads went through; that mapping is gone, and the matrix the sprite pass
    /// is actually given is what answers now — which is a stronger question than
    /// the one it replaced, because a projection that disagreed with
    /// `camera_half_height` used to be invisible to it.
    #[test]
    fn the_whole_play_field_is_on_screen_at_every_aspect_ratio() {
        // The walls' inner faces are as far as anything ever gets: a ball
        // resolving against one is put back at `face - radius * 1.01`, and the
        // sweep catches the contact when the surfaces meet.
        for extent in [
            (960, 720),   // what the window opens at, and the canvas's ratio
            (800, 600),   // 4:3 again, smaller
            (1920, 1080), // 16:9
            (1440, 400),  // a canvas clamped by `max-height: 68vh`
            (600, 900),   // taller than it is wide
        ] {
            let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
            let view_projection = camera(extent).view_projection(aspect);

            for x in [WORLD_LEFT, WORLD_RIGHT] {
                for y in [-WORLD_TOP, WORLD_TOP] {
                    // The sprite plane is world units, so this is the point as
                    // submitted.
                    let world = crcbl::math::Vec4::new(x as f32, y as f32, 0.0, 1.0);
                    let clip = view_projection * world;
                    let ndc = crcbl::math::Vec2::new(clip.x / clip.w, clip.y / clip.w);
                    assert!(
                        (-1.0..=1.0).contains(&ndc.x) && (-1.0..=1.0).contains(&ndc.y),
                        "{extent:?} put the corner ({x}, {y}) at {ndc:?} in NDC"
                    );
                }
            }
        }
    }

    /// The camera is centred on the field, so a sprite at the world origin lands
    /// in the middle of the surface — the assumption `art::Scene::build` makes
    /// when it resolves its layers against a camera of `[0, 0]`.
    #[test]
    fn the_camera_is_centred_on_the_field() {
        let extent = (960, 720);
        let aspect = extent.0 as f32 / extent.1 as f32;
        let clip =
            camera(extent).view_projection(aspect) * crcbl::math::Vec4::new(0.0, 0.0, 0.0, 1.0);
        assert!((clip.x / clip.w).abs() < 1e-5, "{clip:?}");
        assert!((clip.y / clip.w).abs() < 1e-5, "{clip:?}");
    }
}
