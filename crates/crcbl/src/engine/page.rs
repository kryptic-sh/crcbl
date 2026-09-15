//! The bundle a sample draws with when its whole frame is a menu and a page of
//! UI.
//!
//! ```text
//!   acquire ─▶ ui.begin_frame ─▶ graph
//!                                 │ backdrop  (clear)
//!                                 │ ui        (the page, the menu, the overlay)
//!                                 ▼
//!                compile ─▶ encode ─▶ present
//! ```
//!
//! # Why this is not each sample's own
//!
//! `apps/{hud,orbit,bracket,options}` had it four times over, and comparing the
//! comment-stripped files the only differences were the sample's **name** — in
//! the context label, the graph log line and the encoder label — and the
//! **clear colour**. Everything else is one piece of knowledge: the frame's
//! acquire → begin-frame → graph → compile → encode → present order, and that
//! the backdrop is the only pass that does not load, so declaring the UI pass
//! first would clear the page away.
//!
//! # What stays with the sample
//!
//! Its `desc` — the [`GpuContextDesc`](super::GpuContextDesc) both bring-up
//! paths ask for, which cannot be generated here without making every sample's
//! "the features I ask for are the engine's own" test vacuous — and the newtype
//! that carries [`impl_game_gpu!`](crate::impl_game_gpu) and
//! [`impl_polled_gpu!`](crate::impl_polled_gpu). Those macros forward to
//! *inherent* methods, which is what catches a bundle wired to the wrong one,
//! so each sample's forwards are one line each rather than a blanket impl.
//!
//! **A sample with a scene is not this.** `apps/breach` and its neighbours hold
//! a [`ForwardRenderer`], a camera and instance
//! pools; what they share with each other is a different bundle and is not this
//! one.

use crcbl_hal::CommandEncoderDesc;
use crcbl_render::{
    ForwardRenderer, MAX_TIMED_PASSES, PassTimers, RenderGraph, TransientPool, UiRenderer,
};
use crcbl_ui::draw_list::DrawList;
use crcbl_ui::menu::MenuSkin;
use crcbl_ui::text::FontAtlas;

use super::{FRAMES_IN_FLIGHT, FrameOutcome, GpuContext, GpuError};

/// A device, a swapchain and a UI pass — the whole of a sample
/// whose frame has no scene in it.
///
/// Built from a label and a clear colour; see the module docs for what is
/// shared and what stays with the sample.
#[derive(Debug)]
pub struct PageBundle {
    ctx: GpuContext,
    pool: TransientPool,
    timers: Option<PassTimers>,
    /// UI compositing — the page and the debug overlay, in one list.
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    /// The whole backdrop of a frame whose draw list is empty. The only pass
    /// that does not load the target.
    clear: [f32; 4],
    /// The sample's name, which is what its graph dump is logged under.
    label: &'static str,
    /// `"<label> frame"`, built once. The encoder wants a `&str` every frame and
    /// formatting one per frame would allocate on the hot path.
    encoder_label: String,
    dumped: bool,
    /// Whether [`frame`](Self::frame) keeps its graph dump. Off unless a caller
    /// asked, because building the string is per-frame work on the frame path
    /// and only a test ever reads it.
    keep_dump: bool,
    /// The last frame's graph dump, when [`keep_dump`](Self::keep_dump) is on.
    /// It is how a test sees whether the UI pass was in the frame at all:
    /// `add_passes` declares nothing when the draw list is empty, so the pass's
    /// presence in this string *is* "the page reached the GPU".
    last_dump: String,
}

impl PageBundle {
    /// Builds the two renderers on an already-open context.
    ///
    /// `label` is the sample's name and `clear` the colour its backdrop pass
    /// writes.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the UI compositor refused the device.
    pub fn new(ctx: GpuContext, label: &'static str, clear: [f32; 4]) -> Result<Self, GpuError> {
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
            clear,
            label,
            encoder_label: format!("{label} frame"),
            dumped: false,
            keep_dump: false,
            last_dump: String::new(),
        })
    }

    /// Keeps every frame's render-graph dump, for a caller that reads it back.
    ///
    /// **Off by default and turned on under `#[cfg(test)]`**, which is where a
    /// sample's own tests ask whether the UI pass was in the frame at all. The
    /// string is formatted from the compiled graph on every frame it is on for,
    /// so a shipped build must not be paying for it — that is why this is a
    /// switch rather than something [`new`](Self::new) always does.
    #[must_use]
    pub fn recording_graph_dumps(mut self) -> Self {
        self.keep_dump = true;
        self
    }

    /// The extent the swapchain is currently configured at.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.ctx.extent()
    }

    /// The engine's context, for the run-level knobs that are not a sample's.
    ///
    /// [`impl_game_gpu!`](crate::impl_game_gpu) forwards
    /// [`HoldsContext`](super::HoldsContext) to a sample's own `context_mut`,
    /// which forwards here, and [`arm_screenshot`](super::arm_screenshot) is
    /// what reaches it.
    pub const fn context_mut(&mut self) -> &mut GpuContext {
        &mut self.ctx
    }

    /// Takes this frame's draw list, handing the previous frame's allocation
    /// back so the caller can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, list: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, list);
    }

    /// The menu art the UI pass's atlas holds — see
    /// [`GameGpu::menu_skin`](super::GameGpu::menu_skin).
    #[must_use]
    pub const fn menu_skin(&self) -> &MenuSkin {
        self.ui.menu_skin()
    }

    /// The most recent pass timings, or `None` on a device without timestamp
    /// queries.
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl_render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// What the last [`frame`](Self::frame) recorded: draws, instances and
    /// triangles — the UI pass's own answer.
    ///
    /// The renderer's rather than a count kept here — see
    /// [`crcbl_render::counters`], which is where that argument is made.
    #[must_use]
    pub fn counters(&self) -> crcbl_render::FrameCounters {
        self.ui.counters()
    }

    /// The `[engine.video]` section this bundle's context read while opening.
    ///
    /// Forwarded rather than answered, so a run reports the player's file rather
    /// than a default — see [`GameGpu::video`](super::GameGpu::video).
    #[must_use]
    pub const fn video(&self) -> &crate::settings::VideoSettings {
        self.ctx.video()
    }

    /// The glyph atlas the UI pass renders text from.
    ///
    /// A page that centres anything must measure with the *same* atlas the pass
    /// draws with, or every centred string is off by the difference.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// The UI geometry this frame handed over — the list the UI pass actually
    /// uploads, page and debug overlay together.
    ///
    /// For a sample's own tests; a sample re-exposes it behind its own
    /// `#[cfg(test)]`, which is what keeps it out of the shipped surface.
    #[must_use]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
    }

    /// The last frame's render-graph dump, or `""` if
    /// [`recording_graph_dumps`](Self::recording_graph_dumps) was never asked
    /// for.
    #[must_use]
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
            // The only pass that does not load — see [`Self::clear`].
            graph
                .add_render_pass("backdrop")
                .clear_color(target, self.clear)
                .execute(|_| {});
            // The page, then the menu, the debug overlay and the console over
            // it — the order the draw list was filled in.
            self.ui.add_passes(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        // "The graph must be able to explain itself" — §2.4's debug-tools
        // principle. The dump is also how a test sees the UI pass was in the
        // frame at all.
        if self.keep_dump {
            self.last_dump = compiled.dump();
        }
        if !self.dumped {
            crate::log::debug!("render graph for {}:\n{}", self.label, compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some(&self.encoder_label),
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
