//! GPU setup for asteroids: the shared [`crcbl::engine`] join, a clear, the
//! sprite pass, the menu pass and the UI pass.
//!
//! Everything that is not specific to this game — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — lives in [`crcbl::engine::GpuContext`], which
//! `apps/breakout/src/gpu.rs`, `apps/flappy/src/gpu.rs` and
//! `apps/sandbox/src/gpu.rs` use too.
//!
//! # The camera is fitted to the field, and the field never moves
//!
//! Breakout's shape rather than flappy's: this world is bounded on both axes,
//! the whole of it has to be on screen at once, and the origin is the middle of
//! it. A rock the player cannot see is indistinguishable from a bug in a game
//! whose defining move is disappearing off one edge and arriving at the other,
//! so [`camera_half_height`] widens the view until the field fits and lets a
//! narrow window letterbox rather than crop.
//!
//! There is no forward pass and no UI-pass placeholder any more: what this drew
//! as untextured quads is [`crate::art`]'s five sheets now.
//! [`ForwardRenderer::present_target`] survives as the import helper, which is an
//! associated function and needs no renderer.
//!
//! # Pass order is declaration order
//!
//! `space` (clear) → `sprites` (the game) → `menu` → `ui` (the HUD and the debug
//! panel). The last three load rather than clear. The menu is **between** the
//! game and the text for the reason `crcbl::render::menu` gives: its scrim dims
//! what is already in the target, so it must come after the game, and its panel
//! is opaque while its labels are UI-pass text, so it must come before the UI.

use crcbl::engine::{
    FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions, PendingGpuContext,
};
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

use crate::art::{SPACE, Scene, TEXELS_PER_UNIT};
use crate::game::{RenderState, WORLD_HALF_HEIGHT, WORLD_HALF_WIDTH};

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;
const MAX_TIMED_PASSES: u32 = 8;

/// Slack kept outside the playfield, in world units.
///
/// Framing only. The field wraps, so nothing is ever *outside* it — but a rock
/// whose art is flush with the last row of pixels on the surface reads as
/// clipping rather than as a wrap, and half a unit is cheap.
const VIEW_MARGIN: f32 = 0.5;

/// Half the vertical extent of the orthographic camera, **in world units**.
///
/// [`Projection::Orthographic`] takes a half *height* and derives the width from
/// the aspect ratio, so a fixed 12.5 shows `12.5 * aspect` horizontally — which
/// at 4:3 is 16.7 against a field that runs to [`WORLD_HALF_WIDTH`] = 16, and at
/// 5:4 is 15.6, which crops it. Widening the camera until the whole field fits
/// holds at every aspect ratio: a viewport too narrow at 12.5 letterboxes
/// vertically instead of cropping horizontally.
///
/// [`projection`] is what scales this into sprite units, and is the only caller
/// that may.
#[must_use]
pub fn camera_half_height(extent: (u32, u32)) -> f32 {
    let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
    let half_height = WORLD_HALF_HEIGHT as f32 + VIEW_MARGIN;
    let half_width = WORLD_HALF_WIDTH as f32 + VIEW_MARGIN;
    half_height.max(half_width / aspect)
}

/// The camera projection for an `extent`-sized viewport.
///
/// **In sprite units, not world units** — see [`crate::art`]'s header for what
/// that convention is and, in this sample, what it is not.
fn projection(extent: (u32, u32)) -> Projection {
    Projection::Orthographic {
        half_height: camera_half_height(extent) * TEXELS_PER_UNIT,
        near: 0.1,
        far: 100.0,
    }
}

/// The camera for an `extent`-sized viewport: centred on the origin, looking
/// down −Z.
///
/// Written out rather than left to [`Camera::default`], as breakout's is: where
/// the camera is is a fact [`crate::art::Scene::build`] depends on — it resolves
/// its layers against `[0, 0]` — and a fact this game depends on belongs in the
/// file where it can be read and tested.
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
    /// This frame's world, copied rather than borrowed: [`Gpu::frame`] runs
    /// after the caller has moved on, and the state is refilled every frame.
    render: RenderState,
    /// How far this frame sits between the last tick and the next.
    alpha: f32,
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
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "asteroids",
        // Optional, not required: the UI pass hands its shader the viewport size
        // through a push constant where there are any and through a uniform
        // buffer where there are none.
        optional_features: Features::TIER_A
            | Features::TIMESTAMP_QUERY
            | Features::DEBUG_MARKERS
            | Features::PUSH_CONSTANTS,
        ..GpuContextDesc::from(gpu)
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
    /// sprite, menu and UI renderers.
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
        gpu: GpuOptions,
    ) -> Result<Self, GpuError> {
        Self::from_context(GpuContext::open(shell, window, extent, &desc(gpu))?)
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
        gpu: GpuOptions,
    ) -> Result<PendingGpu, GpuError> {
        Ok(PendingGpu {
            pending: GpuContext::request_open(shell, window, extent, &desc(gpu))?,
        })
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
            camera: camera(extent),
            ctx,
            pool: TransientPool::new(),
            timers,
            render: RenderState::default(),
            alpha: 0.0,
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

    /// Takes this frame's world and how far through a tick the frame sits.
    ///
    /// The alpha rides with the state because the two have to be from the same
    /// frame: last frame's alpha applied to this frame's angles is a smaller
    /// stutter than no interpolation at all, and a harder one to see.
    pub fn set_world(&mut self, render: &RenderState, alpha: f32) {
        self.render.clone_from(render);
        self.alpha = alpha;
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

    /// How far through a tick this frame was drawn at, for the loop's own tests.
    #[cfg(test)]
    pub const fn alpha_for_test(&self) -> f32 {
        self.alpha
    }

    /// The game's own sprites for this frame, for the same.
    #[cfg(test)]
    pub fn scene_sprites(&mut self) -> Vec<crcbl::render::Sprite> {
        let alpha = self.alpha;
        let render = std::mem::take(&mut self.render);
        let sprites = self.scene.build(&render, alpha).to_vec();
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
        // with the surface actually being drawn into.
        self.camera = camera(extent);
        let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
        let view_projection = self.camera.view_projection(aspect);

        let sprites = self.scene.build(&self.render, self.alpha);
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
                .add_render_pass("space")
                .clear_color(target, SPACE)
                .execute(|_| {});
            self.sprites.add_pass(&mut graph, target);
            self.menu.add_pass(&mut graph, target);
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        if !self.dumped {
            crcbl::log::debug!("render graph for asteroids:\n{}", compiled.dump());
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

    /// The whole playfield is on screen whatever the window's shape, measured
    /// **through the real projection** rather than through a second mapping.
    ///
    /// A rock the player cannot see is indistinguishable from a rock that
    /// leaked, and in a wrapping game it is indistinguishable from a bug in the
    /// wrap.
    #[test]
    fn the_whole_field_fits_at_every_aspect_ratio() {
        for extent in [
            (960, 720),   // what the window opens at
            (800, 600),   // 4:3 again, smaller
            (1920, 1080), // 16:9
            (1440, 400),  // a canvas clamped by `max-height: 68vh`
            (600, 900),   // taller than it is wide
            (1000, 800),  // 5:4 — the one a fixed half-height would crop
        ] {
            let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
            let view_projection = camera(extent).view_projection(aspect);
            for (x, y) in [
                (WORLD_HALF_WIDTH, WORLD_HALF_HEIGHT),
                (-WORLD_HALF_WIDTH, -WORLD_HALF_HEIGHT),
                (WORLD_HALF_WIDTH, -WORLD_HALF_HEIGHT),
                (-WORLD_HALF_WIDTH, WORLD_HALF_HEIGHT),
            ] {
                let world = crcbl::math::Vec4::new(
                    x as f32 * TEXELS_PER_UNIT,
                    y as f32 * TEXELS_PER_UNIT,
                    0.0,
                    1.0,
                );
                let clip = view_projection * world;
                let ndc = crcbl::math::Vec2::new(clip.x / clip.w, clip.y / clip.w);
                assert!(
                    (-1.0..=1.0).contains(&ndc.x) && (-1.0..=1.0).contains(&ndc.y),
                    "{extent:?} put the corner ({x}, {y}) at {ndc:?} in NDC",
                );
            }
        }
    }

    /// The origin is the middle of the view, and `+Y` is up the screen.
    #[test]
    fn the_camera_centres_the_origin_and_keeps_y_up() {
        let extent = (960, 720);
        let aspect = extent.0 as f32 / extent.1 as f32;
        let view_projection = camera(extent).view_projection(aspect);

        let centre = view_projection * crcbl::math::Vec4::new(0.0, 0.0, 0.0, 1.0);
        assert!((centre.x / centre.w).abs() < 1e-5);
        assert!((centre.y / centre.w).abs() < 1e-5);

        let above = view_projection * crcbl::math::Vec4::new(0.0, TEXELS_PER_UNIT, 0.0, 1.0);
        assert!(
            above.y / above.w > 0.0,
            "world +Y must be the top of the NDC cube",
        );
    }
}
