//! GPU setup for options: the shared [`crcbl::engine`] join, and two passes.
//!
//! Everything that is not this sample's — opening a backend, choosing an adapter
//! that can present, the swapchain, the frames-in-flight ring, resize and
//! teardown — lives in [`crcbl::engine::GpuContext`], which every other sample's
//! `gpu.rs` uses too.
//!
//! # There is no scene, and that is the sample
//!
//! A settings screen is measured in pixels against the surface, which is what
//! the menu and UI passes have always drawn in, so there is nothing here for a
//! camera to project and nothing for a sprite to be —
//! `docs/plan/sample/20-options.md` claims sample rule 11's exemption on the
//! same ground `apps/hud` does. What is behind the panel is one clear colour.
//!
//! # Pass order is declaration order
//!
//! `backdrop` (clear) → `menu` → `ui` (the debug overlay). The last two load the
//! target rather than clearing it, so declaring the UI pass first would put the
//! panel on top of the overlay that is meant to sit over it.

use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions};
use crcbl::hal::CommandEncoderDesc;
use crcbl::render::{
    ForwardRenderer, MAX_TIMED_PASSES, MenuRenderer, PassTimers, RenderGraph, TransientPool,
    UiRenderer,
};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// What the screen is drawn over: a flat, dark ground that leaves the panel the
/// brightest thing in the frame.
pub const BACKDROP: [f32; 4] = [0.04, 0.05, 0.07, 1.0];

/// This sample's device, its swapchain and the two renderers it draws with.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    pool: TransientPool,
    timers: Option<PassTimers>,
    /// The menu pass: the settings screen itself.
    menu: MenuRenderer,
    /// UI compositing — here, the debug overlay and nothing else.
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
}

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for.
///
/// One value rather than two copies, for the reason every sample gives: the two
/// bring-up paths must open the *same* device, or a feature only one of them
/// requested is a bug nobody sees until the other path runs.
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: crate::APP_NAME,
        // The engine's whole bundle, asserted below. A subset spelled out here
        // is a copy, and a copy goes stale the moment the default gains a flag.
        ..GpuContextDesc::from(gpu)
    }
}

// `PendingGpu`, its `poll`, and the blocking and polled `open`s — both routed
// through `desc` above, so the two bring-up paths ask for the same device.
crcbl::impl_polled_bundle!(gpu: Gpu, pending: PendingGpu, desc: desc);

impl Gpu {
    /// Builds this sample's renderers on an already-open context.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the menu pass or the UI compositor refused the device.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        let menu = MenuRenderer::new(ctx.device(), ctx.queue(), format).map_err(GpuError::Hal)?;
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(ui) => ui,
            Err(error) => {
                menu.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        Ok(Self {
            ctx,
            pool: TransientPool::new(),
            timers,
            menu,
            ui,
            atlas: FontAtlas::built_in(),
            draw_list: DrawList::new(),
            dumped: false,
        })
    }

    /// The extent the swapchain is currently configured at.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.ctx.extent()
    }

    /// The engine's context, for the run-level knobs that are not this game's.
    ///
    /// `--screenshot` is the one that needs it: the frame to write out is the
    /// one this bundle presents, so the arming ([`GpuContext::set_screenshot`])
    /// has to reach the context this bundle holds.
    #[cfg(not(target_arch = "wasm32"))]
    pub const fn context_mut(&mut self) -> &mut GpuContext {
        &mut self.ctx
    }

    /// Takes this frame's draw list, handing the previous frame's allocation
    /// back so the caller can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, dl: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, dl);
    }

    /// Takes this frame's menu, or `None` on a frame that shows none — which
    /// here is no frame at all.
    pub fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        self.menu.set_menu(menu);
    }

    /// The most recent pass timings, or `None` on a device without timestamp
    /// queries.
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// What the last [`Gpu::frame`] recorded, summed over the two passes this
    /// bundle adds.
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.menu.counters().plus(self.ui.counters())
    }

    /// The `[engine.video]` section this bundle's context read while opening.
    ///
    /// Forwarded rather than answered, so a run reports the player's file
    /// rather than a default — see [`crcbl::engine::GameGpu::video`].
    #[must_use]
    pub const fn video(&self) -> &crcbl::settings::VideoSettings {
        self.ctx.video()
    }

    /// The glyph atlas the passes render text from.
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
            // The only pass that does not load. This sample's whole scene.
            graph
                .add_render_pass("backdrop")
                .clear_color(target, BACKDROP)
                .execute(|_| {});
            self.menu.add_pass(&mut graph, target);
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        // "The graph must be able to explain itself" — §2.4's debug-tools
        // principle.
        if !self.dumped {
            crcbl::log::debug!("render graph for options:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("options frame"),
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
        self.pool.destroy(self.ctx.device());
        if let Some(timers) = self.timers.as_mut() {
            timers.destroy(self.ctx.device());
        }
        self.ctx.destroy()
    }
}

// ---------------------------------------------------------------------------
// The engine's seams
// ---------------------------------------------------------------------------

// The forwards `crcbl::engine` calls this bundle through. Every one of them is a
// method above; the macro is what stops a sample forgetting one.
crcbl::impl_game_gpu!(Gpu);

// Start-up, driven by `crcbl::engine::PolledBoot` rather than blocked on.
crcbl::impl_polled_gpu!(gpu: Gpu, pending: PendingGpu);

#[cfg(test)]
mod tests {
    use super::*;

    /// **The bundle this sample opens with is the engine's own.** A hand-written
    /// list is a copy, and a copy goes stale the moment
    /// [`GpuContextDesc::default`] gains a flag — silently, because the missing
    /// capability changes no picture, only whether the engine's pacing loop and
    /// display-timing query can reach anything.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engine_s_own() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, crate::APP_NAME);
        assert_eq!(
            asked.optional_features,
            GpuContextDesc::default().optional_features,
            "a subset spelled out here is a copy, and a copy goes stale",
        );
    }
}
