//! Tumble's GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::stage`], and the UI pass — the menu drawn in it — rule 4 asks every
//! sample for.
//!
//! Everything that is not this sample's — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — is [`crcbl::engine::GpuContext`]'s. What is here is a
//! renderer built from [`crate::stage`]'s description, and the body instances
//! in it that follow the physics every frame.
//!
//! # Pass order is declaration order
//!
//! The forward frame → `ui`. The UI loads the target rather than clearing it,
//! so it has to be declared after the frame it composites over.

use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions};
use crcbl::hal::CommandEncoderDesc;
use crcbl::render::{
    Camera, ForwardRenderer, MAX_TIMED_PASSES, PassTimers, RenderGraph, TransientPool, UiRenderer,
};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::MenuSkin;
use crcbl::ui::text::FontAtlas;

use crate::scene::Scenes;
use crate::stage;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// This sample's device, its swapchain and the three renderers it draws with.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    /// The stage, made resident once, and the body instances in it.
    renderer: ForwardRenderer,
    drawn: stage::Drawn,
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Where the frame is seen from: [`stage::camera`] of the room on screen.
    camera: Camera,
    /// UI compositing — the overlay and the debug panel, in one list.
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
    /// The last frame's graph dump, kept only for the loop's own tests: it is
    /// how a test sees whether a pass was in the frame at all.
    #[cfg(test)]
    last_dump: String,
}

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for.
///
/// One value rather than two copies, for the reason every sample gives: the two
/// bring-up paths must open the *same* device, or a feature only one of them
/// requested is a bug nobody sees until the other path runs.
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "tumble",
        // The engine's whole optional bundle, not a subset spelled out here: a
        // hand-written list is a copy, and a copy goes stale the moment
        // `GpuContextDesc::default` gains a flag.
        ..GpuContextDesc::from(gpu)
    }
}

// `PendingGpu`, its `poll`, and the blocking and polled `open`s — both routed
// through `desc` above, so the two bring-up paths ask for the same device.
crcbl::impl_polled_bundle!(gpu: Gpu, pending: PendingGpu, desc: desc);

impl Gpu {
    /// Builds this sample's renderers on an already-open context, and makes
    /// [`crate::stage`] resident.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the stage's description is one the pools it asks for
    /// cannot hold, or if the UI compositor refused the device.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let scene = stage::scene();
        let mut renderer = ForwardRenderer::with_scene(ctx.device(), ctx.queue(), format, &scene)?;
        // Rolled back by hand from here on: `Gpu` has no `Drop`, so a `?` would
        // leak the forward renderer's pipelines rather than release them.
        let drawn = match stage::place(&mut renderer, &Scenes::new()) {
            Ok(drawn) => drawn,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::pools("tumble's stage", &error));
            }
        };
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        if timers.is_none() {
            crcbl::log::info!("hal: no timestamp queries on this device; per-pass timing is off");
        }
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(ui) => ui,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        Ok(Self {
            ctx,
            renderer,
            drawn,
            pool: TransientPool::new(),
            timers,
            camera: stage::camera(crate::scene::View::default()),
            ui,
            atlas: FontAtlas::built_in(),
            draw_list: DrawList::new(),
            dumped: false,
            #[cfg(test)]
            last_dump: String::new(),
        })
    }

    /// The extent the swapchain is currently configured at.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.ctx.extent()
    }

    /// The engine's context, for the run-level knobs that are not this sample's.
    ///
    /// `crcbl::impl_game_gpu!` forwards
    /// [`HoldsContext`](crcbl::engine::HoldsContext) to this, and
    /// [`arm_screenshot`](crcbl::engine::arm_screenshot) is what reaches it.
    pub const fn context_mut(&mut self) -> &mut GpuContext {
        &mut self.ctx
    }

    /// Poses the body instances where the scenes have them now, and points
    /// the camera at the room on screen.
    ///
    /// A pool too small for a new body is logged and that body goes undrawn;
    /// the stage's own test holds the capacities to what the rooms can reach.
    pub fn place_bodies(&mut self, scenes: &Scenes) {
        self.camera = stage::camera(scenes.view());
        if let Err(error) = self.drawn.update(&mut self.renderer, scenes) {
            crcbl::log::warn!("tumble: a body went undrawn: {error}");
        }
    }

    /// Takes this frame's draw list, handing the previous frame's allocation
    /// back so the caller can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, dl: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, dl);
    }

    /// The menu art the UI pass's atlas holds — see
    /// [`crcbl::engine::GameGpu::menu_skin`].
    #[must_use]
    pub const fn menu_skin(&self) -> &MenuSkin {
        self.ui.menu_skin()
    }

    /// The most recent pass timings, or `None` on a device without timestamp
    /// queries.
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// What the last [`Gpu::frame`] recorded, summed over the passes this
    /// bundle adds.
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.renderer.counters().plus(self.ui.counters())
    }

    /// The `[engine.video]` section this bundle's context read while opening.
    ///
    /// Forwarded rather than answered, so a run reports the player's file
    /// rather than a default — see [`crcbl::engine::GameGpu::video`].
    #[must_use]
    pub const fn video(&self) -> &crcbl::settings::VideoSettings {
        self.ctx.video()
    }

    /// The glyph atlas the UI pass renders text from.
    ///
    /// The overlay right-aligns its readings with it, and must measure with the
    /// *same* atlas the pass draws with or every measured string lands off by
    /// the difference.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// The UI geometry this frame handed over, for the loop's own tests.
    #[cfg(test)]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
    }

    /// The last frame's render-graph dump, for the loop's own tests.
    #[cfg(test)]
    pub fn last_dump(&self) -> &str {
        &self.last_dump
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

        self.renderer
            .begin_frame(self.ctx.device(), &self.camera, &stage::sun(), extent)?;
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
            let _hdr = self
                .renderer
                .add_passes(&mut graph, &self.pool, target, extent);
            // The game's HUD, then the menu, the debug overlay and the
            // console over it — the order the draw list was filled in.
            self.ui.add_passes(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        // "The graph must be able to explain itself" — §2.4's debug-tools
        // principle.
        #[cfg(test)]
        {
            self.last_dump = compiled.dump();
        }
        if !self.dumped {
            crcbl::log::debug!("render graph for the tumble frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("tumble frame"),
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
    /// [`GpuError`] if the reconfigure failed. A zero extent is *not* an error:
    /// a minimised window reports one and the swapchain is left alone.
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

// ---------------------------------------------------------------------------
// The engine's seams
// ---------------------------------------------------------------------------

// The forwards `crcbl::engine` calls this bundle through. Every one of them is
// a method above; the macro is what stops a sample forgetting one.
/// The two seams `crcbl::settings::apply` reaches a renderer through.
///
/// A second inherent block rather than lines inside the one above, so the
/// forward `crcbl::impl_game_gpu!(Gpu, with_renderer)` picks up sits beside the
/// invocation that needs it. Debug-console decision 3 in
/// `docs/notes/tooling.md` is where the pair comes from, and `crcbl::settings`
/// holds both bodies — every bundle with a `ForwardRenderer` writes exactly
/// these two lines.
impl Gpu {
    /// Put the player's `[engine.video]` section into force now.
    ///
    /// # Errors
    ///
    /// `crcbl::settings::apply_video_to`'s: the device refused the sampler the
    /// anisotropy asked for.
    fn apply_video(
        &mut self,
        video: &crcbl::settings::VideoSettings,
    ) -> Result<(), crcbl::settings::Unsupported> {
        crcbl::settings::apply_video_to(&mut self.renderer, self.ctx.device(), video)
    }

    /// Draw `view` instead of the shaded picture.
    ///
    /// # Errors
    ///
    /// None: this bundle has the renderer the view needs.
    fn set_debug_view(
        &mut self,
        view: crcbl::render::DebugView,
    ) -> Result<(), crcbl::settings::Unsupported> {
        crcbl::settings::set_debug_view_on(&mut self.renderer, view);
        Ok(())
    }
}

crcbl::impl_game_gpu!(Gpu, with_renderer);

// Start-up, driven by `crcbl::engine::PolledBoot` rather than blocked on.
crcbl::impl_polled_gpu!(gpu: Gpu, pending: PendingGpu);

#[cfg(test)]
mod tests {
    use super::*;

    /// **The bundle this sample opens with is the engine's own.** A hand-written
    /// list is a copy, and a copy goes stale the moment
    /// [`GpuContextDesc::default`] gains a flag.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engine_s_own() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, "tumble");
        assert_eq!(
            asked.optional_features,
            GpuContextDesc::default().optional_features,
            "a subset spelled out here is a copy, and a copy goes stale",
        );
    }
}
