//! GPU setup for hud: the shared [`crcbl::engine`] join, and two passes.
//!
//! Everything that is not this sample's — opening a backend, choosing an adapter
//! that can present, the swapchain, the frames-in-flight ring, resize and
//! teardown — lives in [`crcbl::engine::GpuContext`], which every other sample's
//! `gpu.rs` uses too.
//!
//! # There is no sprite pass and no camera
//!
//! Every other sample carries a [`crcbl::render::SpriteRenderer`], a sheet of
//! `.crpix` art and an orthographic camera fitted to its world. This one has no
//! world: the page is measured in pixels against the surface, which is what the
//! UI pass has always drawn in, so there is nothing for a camera to project and
//! nothing for a sprite to be. `docs/plan/sample/00-samples-overview.md` rule 11
//! exempts this sample by name, and its own plan doc says why.
//!
//! # Pass order is declaration order
//!
//! `backdrop` (clear) → `menu` → `ui` (the page, then the debug overlay). The
//! last two load the target rather than clearing it, so declaring the UI pass
//! first would put the pause panel on top of the words it exists to frame.

use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions};
use crcbl::hal::{CommandEncoderDesc, Features};
use crcbl::prelude::*;
use crcbl::render::{
    ForwardRenderer, MenuRenderer, PassTimers, RenderGraph, TransientPool, UiRenderer,
};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// How many passes the timers make room for. Three are declared; the spare is
/// what stops a fourth from silently going untimed.
const MAX_TIMED_PASSES: u32 = 4;

/// This sample's device, its swapchain and the two renderers it draws with.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    pool: TransientPool,
    timers: Option<PassTimers>,
    /// The menu pass: its own sheets, its own screen-space camera, and a pass
    /// that declares nothing on a frame with no menu on it.
    menu: MenuRenderer,
    /// UI compositing — the page and the debug overlay, in one list.
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
    /// The last frame's graph dump, kept only for the loop's own tests: it is
    /// how a test sees whether the UI pass was in the frame at all.
    /// `add_pass` declares nothing when the draw list is empty, so the pass's
    /// presence in this string *is* "the page reached the GPU".
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
        label: "hud",
        // Optional, not required: neither changes what is drawn, only how
        // observable it is. `GPU_DRIVEN` is absent because nothing here issues
        // an indirect draw — the UI pass uploads a vertex buffer a frame.
        optional_features: Features::TIMESTAMP_QUERY | Features::DEBUG_MARKERS,
        ..GpuContextDesc::from(gpu)
    }
}

impl Gpu {
    /// Opens a backend, a surface, a device and a swapchain, and builds the menu
    /// and UI renderers.
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
            #[cfg(test)]
            last_dump: String::new(),
        })
    }

    /// The extent the swapchain is currently configured at.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.ctx.extent()
    }

    /// Takes this frame's draw list, handing the previous frame's allocation
    /// back so the caller can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, dl: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, dl);
    }

    /// Takes this frame's menu, or `None` on a frame that shows none.
    pub fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        self.menu.set_menu(menu);
    }

    /// The most recent pass timings, or `None` on a device without timestamp
    /// queries.
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// The glyph atlas the UI pass renders text from.
    ///
    /// The page centres its banner and its slot labels with it, and must measure
    /// with the *same* atlas the pass draws with or every centred string is off
    /// by the difference.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// The UI geometry this frame handed over, for the loop's own tests — the
    /// list the UI pass actually uploads, page and debug overlay together.
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
            // The only pass that does not load. This sample's whole backdrop —
            // see `crate::page::BACKDROP`.
            graph
                .add_render_pass("backdrop")
                .clear_color(target, crate::page::BACKDROP)
                .execute(|_| {});
            // The scrim dims what is already in the target, so it has to come
            // after nothing here — but the panel is opaque and its labels are
            // UI-pass text, so it has to come *before* the UI or the frame
            // paints over its own words.
            self.menu.add_pass(&mut graph, target);
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        // "The graph must be able to explain itself" — §2.4's debug-tools
        // principle. The dump is also how a test sees the UI pass was in the
        // frame at all.
        #[cfg(test)]
        {
            self.last_dump = compiled.dump();
        }
        if !self.dumped {
            crcbl::log::debug!("render graph for hud:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("hud frame"),
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

/// The frame's half of this bundle, for [`crcbl::engine::Loop`].
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
