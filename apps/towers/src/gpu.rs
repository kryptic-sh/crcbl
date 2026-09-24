//! Towers' GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::map`], and the UI pass — the menu drawn in it — rule 4 asks
//! every sample for.
//!
//! Everything that is not this sample's — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — is [`crcbl::engine::GpuContext`]'s. What is here is the part
//! that is towers': a renderer built from **this application's** scene
//! description rather than from [`ForwardRenderer::new`]'s demo one, the pools
//! of instances that move, and [`Paths`].
//!
//! # Rule 12, as a value rather than a log line
//!
//! `docs/plan/sample/00-samples-overview.md` rule 12 asks every sample to say
//! which of topic 39's selectors its frames took, in the
//! debug panel **and** in the summary. [`Paths`] is that answer read once off
//! [`DeviceCaps`], so the panel, the `[HUD]` heartbeat and the summary line all
//! print the same three words rather than three readings that could disagree.
//! `apps/breach` and `apps/shard` carry the same type; this one has no `Forced`
//! beside it because slice 1 ships no flag to hold a path down, and
//! `docs/plan/sample/07-towers.md` records that as owed.
//!
//! # Pass order is declaration order
//!
//! The forward frame → `ui`, the pause menu in the draw list ahead of its own
//! words. The UI loads the target rather than clearing it, so it has to be
//! declared after the frame it composites over.

use crcbl::engine::{
    DevicePathRows, ForcedPaths, FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions,
};
use crcbl::hal::{BindingModel, CommandEncoderDesc, DeviceCaps, GeometryPath, LightingPath};
use crcbl::render::{
    Camera, ForwardRenderer, MAX_TIMED_PASSES, PassTimers, RenderGraph, TransientPool, UiRenderer,
};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::MenuSkin;
use crcbl::ui::text::FontAtlas;

use crate::game::RenderState;
use crate::map::{self, Field};

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// Which of topic 39's three selectors this device drew
/// through — rule 12's "says which it took", as a value.
///
/// Read once at start-up because that is when it is decided: the selectors are
/// a function of the device's capabilities, and nothing in a run changes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paths {
    /// The path the renderer's submission tail takes.
    pub geometry: GeometryPath,
    /// How the fragment stage addresses the base-colour page.
    pub binding: BindingModel,
    /// How indirect lighting is resolved.
    pub lighting: LightingPath,
}

impl Paths {
    /// What the device opened as.
    #[must_use]
    pub const fn of(caps: &DeviceCaps) -> Self {
        Self {
            geometry: caps.geometry_path(),
            binding: caps.binding_model(),
            lighting: caps.lighting_path(),
        }
    }
}

impl crcbl::ui::DebugModule for Paths {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("paths");
        DevicePathRows::new(
            self.geometry,
            self.binding,
            self.lighting,
            ForcedPaths::default(),
        )
        .write(section);
    }
}

/// This sample's device, its swapchain and the three renderers it draws with.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    /// The field, made resident once and drawn every frame.
    renderer: ForwardRenderer,
    /// The instances in it that are rewritten — the creeps, the towers and the
    /// bolts.
    field: Field,
    /// Which selectors this device drew through — see [`Paths`].
    paths: Paths,
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Where the frame is seen from. Fixed — see [`crate::camera`] — but held
    /// here because the frame is what reads it.
    camera: Camera,
    /// UI compositing — the readout and the debug panel, in one list.
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
/// requested is a bug nobody sees until the other path runs — and here it would
/// also be a [`Paths`] that depends on which door the run came in through.
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "towers",
        // The engine's whole optional bundle, not a subset spelled out here: a
        // hand-written list is a copy, and a copy goes stale the moment
        // `GpuContextDesc::default` gains a flag.
        ..GpuContextDesc::from(gpu)
    }
}

impl Gpu {
    /// Builds this sample's renderers on an already-open context.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the map's description is one the pools it asks for
    /// cannot hold, if the UI compositor refused the device,
    /// or if any HAL call failed.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let paths = Paths::of(&ctx.device().caps());
        let mut renderer =
            ForwardRenderer::with_scene(ctx.device(), ctx.queue(), format, &map::scene())?;
        // Rolled back by hand from here on: `Gpu` has no `Drop`, so a `?` would
        // leak the forward renderer's pipelines rather than release them.
        let field = match map::place(&mut renderer) {
            Ok(field) => field,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::pools("towers' field", &error));
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

        crcbl::log::info!(
            "render: geometry {:?}, binding {:?}, lighting {:?}",
            paths.geometry,
            paths.binding,
            paths.lighting,
        );

        Ok(Self {
            ctx,
            renderer,
            field,
            paths,
            pool: TransientPool::new(),
            timers,
            camera: crate::camera::camera(),
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

    /// Which selectors this device drew through — rule 12's answer.
    #[must_use]
    pub const fn paths(&self) -> Paths {
        self.paths
    }

    /// The engine's context, for the run-level knobs that are not this sample's.
    ///
    /// `crcbl::impl_game_gpu!` forwards
    /// [`HoldsContext`](crcbl::engine::HoldsContext) to this, and
    /// [`arm_screenshot`](crcbl::engine::arm_screenshot) is what reaches it.
    pub const fn context_mut(&mut self) -> &mut GpuContext {
        &mut self.ctx
    }

    /// Draws the creeps, the towers, the bolts and the splash bursts where the
    /// simulation says.
    ///
    /// Written every frame rather than only when one changes, for
    /// `apps/breach`'s reason: the frame is handed a snapshot, and a renderer
    /// that had to be told about an edge would need a second copy of the
    /// field's state to compare against — and the creeps move anyway. The
    /// slots past what the simulation is using are parked under the ground; see
    /// [`crate::map::PARK`].
    pub fn set_field(&mut self, state: &RenderState) {
        for index in 0..crate::wave::MAX_CREEPS {
            let view = (index < state.creeps_alive).then(|| state.creeps[index]);
            self.field.set_creep(&mut self.renderer, index, view);
        }
        for (plot, tower) in state.towers.iter().enumerate() {
            self.field.set_tower(&mut self.renderer, plot, *tower);
        }
        for index in 0..map::MAX_BOLTS {
            let at = (index < state.bolts_flying).then(|| state.bolts[index]);
            self.field.set_bolt(&mut self.renderer, index, at);
        }
        for index in 0..map::MAX_BURSTS {
            let burst = (index < state.bursts_live).then(|| state.bursts[index]);
            self.field.set_burst(&mut self.renderer, index, burst);
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
            .begin_frame(self.ctx.device(), &self.camera, &map::sun(), extent)?;
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
            crcbl::log::debug!("render graph for the towers frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("towers frame"),
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

// The `Pending` type, `open`, `request_open` and the `PolledGpu` forwards, all
// routed through the one `desc` above so the blocking and non-blocking bring-up
// paths cannot ask for different devices. Written out only by the samples whose
// pending state carries something of their own; this one's does not.
crcbl::impl_polled_bundle!(gpu: Gpu, pending: PendingGpu, desc: desc);
crcbl::impl_polled_gpu!(gpu: Gpu, pending: PendingGpu);

#[cfg(test)]
mod tests {
    use super::*;

    /// **The bundle this sample opens with is the engine's own.** A
    /// hand-written list is a copy, and a copy goes stale the moment
    /// [`GpuContextDesc::default`] gains a flag. The failure is silent both
    /// ways: the missing capability changes no picture, only whether the
    /// engine's pacing loop and display-timing query can reach anything — and
    /// here also which [`Paths`] the run reports.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engine_s_own() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, "towers");
        assert_eq!(
            asked.optional_features,
            GpuContextDesc::default().optional_features,
            "a subset spelled out here is a copy, and a copy goes stale",
        );
    }

    /// **The paths a device reports are the ones the panel prints.** Read off a
    /// `DeviceCaps` rather than off a live device, so the mapping is checked on
    /// every machine including the ones with no GPU at all.
    ///
    /// Two devices, and the second is the control: a `Paths` that reported the
    /// same three words whatever it was handed would pass against one.
    #[test]
    fn the_paths_row_names_what_the_device_selected() {
        use crcbl::hal::{Features, Limits};
        use crcbl::ui::{DebugModule, DebugSection};

        let rows_of = |paths: &Paths| {
            let mut section = DebugSection::default();
            paths.debug_section(&mut section);
            assert_eq!(section.title(), "paths");
            section
                .rows()
                .iter()
                .map(|row| (row.label.clone(), row.value.clone()))
                .collect::<Vec<_>>()
        };

        // What a browser is: no mesh stage, no bindless, no ray query. The
        // fallbacks, which on the target the next slice publishes to are not a
        // fallback at all.
        let browser = DeviceCaps {
            features: Features::empty(),
            limits: Limits::minimum(),
        };
        let browser_paths = Paths::of(&browser);
        assert_eq!(
            rows_of(&browser_paths),
            vec![
                (
                    "geometry".to_string(),
                    format!("{:?}", browser.geometry_path())
                ),
                (
                    "binding".to_string(),
                    format!("{:?}", browser.binding_model())
                ),
                (
                    "lighting".to_string(),
                    format!("{:?}", browser.lighting_path())
                ),
            ],
        );

        // …and a device with the lot, whose rows must be **its** answers and
        // not the ones above. Without this half a `Paths` that hard-coded a
        // browser's selector would pass: the browser rows would still match and
        // the inequality below would still hold on the other two axes.
        let desktop = DeviceCaps {
            features: Features::all(),
            limits: Limits::minimum(),
        };
        assert_ne!(
            desktop.geometry_path(),
            browser.geometry_path(),
            "the two devices select the same geometry path, so this proves nothing",
        );
        assert_eq!(
            rows_of(&Paths::of(&desktop)),
            vec![
                (
                    "geometry".to_string(),
                    format!("{:?}", desktop.geometry_path())
                ),
                (
                    "binding".to_string(),
                    format!("{:?}", desktop.binding_model())
                ),
                (
                    "lighting".to_string(),
                    format!("{:?}", desktop.lighting_path())
                ),
            ],
        );
        assert_ne!(
            Paths::of(&desktop),
            browser_paths,
            "every device reports the same three paths",
        );
    }
}
