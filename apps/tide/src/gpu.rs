//! Tide's GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::scene`], and the UI pass — the menu drawn in it — rule 4 asks
//! every sample for.
//!
//! Everything that is not this sample's is [`crcbl::engine::GpuContext`]'s. What
//! is here is tide's: a renderer built from the gallery's own description, the
//! [`Stage`] that switches it between scenes, the capability report rule 12 asks
//! for, and what the water costs. `apps/sundial/src/gpu.rs` is the shape, and
//! its header carries the argument for exact geometry requests: a forced
//! geometry builds that tail or fails, and [`Paths`] reports the tail built.
//!
//! # The scene is staged inside the frame
//!
//! [`Gpu::show`] records which scene and medium the next frame wants, and
//! [`Gpu::frame`] moves the [`Stage`] there before it records anything — so a
//! body that cannot be meshed is a [`GpuError`] out of the frame rather than a
//! panic inside `draw`, which has no way to return one.
//!
//! # What the water costs
//!
//! [`WaterCost`] reads `water-copy` and `water` off [`PassTimers`] — the two
//! passes `crcbl_render`'s water module records after the reflection composite.
//! A stub scene has no body, and a renderer with no body records neither pass,
//! so its row says so rather than printing two zeros.

pub use crcbl::engine::{FrameOutcome, GpuError};

use crcbl::engine::{
    DevicePathRows, ForcedPaths, GpuContext, GpuContextDesc, GpuOptions, Pacing, PendingGpuContext,
};
use crcbl::hal::{
    BindingModel, CommandEncoderDesc, DeviceCaps, GeometryPath, LightingPath, downgrades,
};
use crcbl::prelude::*;
use crcbl::render::{
    EffectRequest, ForwardRenderer, MAX_TIMED_PASSES, PassTimers, RenderEffects, RenderGraph,
    TransientPool, UiRenderer,
};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::MenuSkin;
use crcbl::ui::text::FontAtlas;

use crate::medium::Preset;
use crate::scene::{self, Scene, Stage};

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// The passes the water's cost is spread across, in the order they run —
/// `crcbl_render`'s labels for them.
pub const WATER_PASSES: [&str; 2] = ["water-copy", "water"];

/// Which of topic 39's three selectors this frame was drawn
/// through, and whether the run asked for less than the device offers — rule
/// 12's "says which it took", as a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paths {
    /// The path the renderer's submission tail actually takes.
    pub geometry: GeometryPath,
    /// How the fragment stage addresses the base-colour page.
    pub binding: BindingModel,
    /// How indirect lighting is resolved.
    pub lighting: LightingPath,
    /// What the run asked to be held down.
    pub forced: ForcedPaths,
    /// Which render effects the frame draws, resolved.
    pub effects: RenderEffects,
}

impl Paths {
    /// Actual renderer geometry and array-page binding, beside the request.
    #[must_use]
    pub const fn of(
        caps: &DeviceCaps,
        geometry: GeometryPath,
        forced: ForcedPaths,
        effects: RenderEffects,
    ) -> Self {
        Self {
            geometry,
            binding: BindingModel::ArrayPages,
            lighting: caps.lighting_path(),
            forced,
            effects,
        }
    }
}

impl crcbl::ui::DebugModule for Paths {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("paths");
        DevicePathRows::new(self.geometry, self.binding, self.lighting, self.forced).write(section);
        section.row_str("ray tracing", crcbl::render::ray_tracing_note());
        section.row_str("effects", &self.effects.row());
    }
}

/// What the water passes cost on the GPU, off the last frame whose timestamps
/// have landed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WaterCost {
    /// Each water pass the frame ran, in execution order, and its cost in
    /// nanoseconds — [`WATER_PASSES`], filtered off the timer rows.
    pub passes: Vec<(String, u64)>,
    /// Whether this device has timestamp queries at all, told apart from "the
    /// frame drew no water".
    pub timed: bool,
}

impl WaterCost {
    /// The report, as one line for the summary, the heartbeat and the panel.
    #[must_use]
    pub fn row(&self) -> String {
        if !self.timed {
            return "no timestamp queries on this device".to_string();
        }
        if self.passes.is_empty() {
            return "no water passes in this frame".to_string();
        }
        self.passes
            .iter()
            .map(|(label, nanos)| format!("{label} {:.3} ms", millis(*nanos)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A pass duration in milliseconds.
fn millis(nanos: u64) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a pass duration is a few million nanoseconds"
    )]
    let nanos = nanos as f64;
    nanos / 1.0e6
}

impl crcbl::ui::DebugModule for WaterCost {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("water cost");
        if self.passes.is_empty() {
            section.row_str("passes", &self.row());
            return;
        }
        for (label, nanos) in &self.passes {
            section.row(label.as_str(), format_args!("{:.3} ms", millis(*nanos)));
        }
    }
}

/// Tide's GPU side.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    renderer: ForwardRenderer,
    /// Which scene and medium the renderer holds.
    stage: Stage,
    /// Which scene and medium the next frame asks the stage for.
    wanted: (Scene, Preset),
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Which selectors the frame is drawn through, resolved once at open.
    paths: Paths,
    /// Where the frame is seen from, written every frame by [`crate::app`].
    camera: crcbl::render::Camera,
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
}

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for — one
/// value, so the two bring-up paths open the same device.
fn desc(gpu: GpuOptions, forced: ForcedPaths) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "tide",
        optional_features: forced.optional_features(),
        ..GpuContextDesc::from(gpu)
    }
}

/// A [`Gpu`] being opened one poll at a time.
#[derive(Debug)]
pub struct PendingGpu {
    pending: PendingGpuContext,
    forced: ForcedPaths,
}

impl PendingGpu {
    /// Advances the open. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the device request failed, or if the renderer refused the
    /// device it produced.
    pub fn poll(&mut self) -> Result<Option<Gpu>, GpuError> {
        match self.pending.poll()? {
            Some(ctx) => Gpu::from_context(ctx, self.forced).map(Some),
            None => Ok(None),
        }
    }
}

impl Gpu {
    /// Opens the join and builds the forward renderer on [`scene::desc`].
    ///
    /// `extent` must come from the window system — call this only after the first
    /// configure.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened, if the backend exposes no adapter, no
    /// graphics queue or no surface format, if the gallery's description does not
    /// fit what it reserves, or if any HAL call fails.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        forced: ForcedPaths,
    ) -> Result<Self, GpuError> {
        Self::from_context(
            GpuContext::open(shell, window, extent, &desc(gpu, forced))?,
            forced,
        )
    }

    /// Starts opening the same thing without blocking — the browser's half of
    /// [`Gpu::open`].
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
        forced: ForcedPaths,
    ) -> Result<PendingGpu, GpuError> {
        Ok(PendingGpu {
            pending: GpuContext::request_open(shell, window, extent, &desc(gpu, forced))?,
            forced,
        })
    }

    /// Builds the renderer, stages the knobs' scene and the two UI passes on an
    /// already-open context.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the description does not fit what it reserves, or if any
    /// HAL call fails.
    fn from_context(ctx: GpuContext, forced: ForcedPaths) -> Result<Self, GpuError> {
        let caps = ctx.device().caps();
        let report = downgrades(forced.optional_features(), &caps);
        if report.is_empty() {
            crcbl::log::info!("tide: device granted every optional feature asked for");
        } else {
            crcbl::log::info!("tide: {report}");
        }
        let mut renderer = scene::renderer(
            ctx.device(),
            ctx.queue(),
            ctx.format(),
            forced
                .geometry
                .unwrap_or_else(|| ctx.device().preferred_geometry_path()),
        )?;
        let knobs = crate::knobs::read();
        let stage = match Stage::new(&mut renderer, knobs.scene, knobs.medium) {
            Ok(stage) => stage,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        renderer.set_effect_request(EffectRequest {
            video: ctx.video_effects(),
            ..EffectRequest::default()
        });
        // Resolved rather than requested: the device clamps last.
        let paths = Paths::of(
            &caps,
            renderer.geometry_path(),
            forced,
            renderer.resolved_effects(),
        );
        crcbl::log::info!(
            "tide: {:?} / {:?} / {:?}, effects {}",
            paths.geometry,
            paths.binding,
            paths.lighting,
            paths.effects.row(),
        );

        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        if timers.is_none() {
            crcbl::log::info!("hal: no timestamp queries on this device; per-pass timing is off");
        }
        // Rolled back by hand: `Gpu` has no `Drop`, so a `?` here would leak the
        // forward renderer's pipelines rather than release them.
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), ctx.format()) {
            Ok(ui) => ui,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        Ok(Self {
            ctx,
            renderer,
            wanted: (stage.scene(), stage.medium()),
            stage,
            pool: TransientPool::new(),
            timers,
            paths,
            camera: scene::fixed_camera(),
            ui,
            atlas: FontAtlas::built_in(),
            draw_list: DrawList::new(),
            dumped: false,
        })
    }

    /// Which selectors this device drew through.
    #[must_use]
    pub const fn paths(&self) -> Paths {
        self.paths
    }

    /// Which scene and medium the renderer holds — what the last frame drew, not
    /// what [`Gpu::show`] last asked for.
    #[must_use]
    pub const fn staged(&self) -> (Scene, Preset) {
        (self.stage.scene(), self.stage.medium())
    }

    /// The engine's context, for the run-level knobs that are not this sample's.
    pub const fn context_mut(&mut self) -> &mut GpuContext {
        &mut self.ctx
    }

    /// The swapchain's current size — the one it was **configured** at.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.ctx.extent()
    }

    /// Where the next frame is seen from.
    pub const fn set_camera(&mut self, camera: crcbl::render::Camera) {
        self.camera = camera;
    }

    /// Which scene, in which medium, the next frame draws.
    pub const fn show(&mut self, scene: Scene, medium: Preset) {
        self.wanted = (scene, medium);
    }

    /// The most recent frame whose per-pass GPU timings have landed.
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// What the water cost in the last frame whose timestamps landed.
    #[must_use]
    pub fn water_cost(&self) -> WaterCost {
        let Some(timings) = self.timings() else {
            return WaterCost::default();
        };
        WaterCost {
            passes: timings
                .passes
                .iter()
                .filter(|pass| WATER_PASSES.contains(&pass.label.as_str()))
                .map(|pass| (pass.label.clone(), pass.gpu_nanos))
                .collect(),
            timed: true,
        }
    }

    /// What the last [`Gpu::frame`] recorded, summed over the passes this bundle
    /// adds.
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.renderer.counters().plus(self.ui.counters())
    }

    /// The `[engine.video]` section this bundle's context read while opening.
    #[must_use]
    pub const fn video(&self) -> &crcbl::settings::VideoSettings {
        self.ctx.video()
    }

    /// Takes this frame's UI geometry, handing the previous frame's allocation
    /// back so the caller can refill it.
    pub fn take_draw_list(&mut self, dl: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, dl);
    }

    /// The menu art the UI pass's atlas holds — see
    /// [`crcbl::engine::GameGpu::menu_skin`].
    #[must_use]
    pub const fn menu_skin(&self) -> &MenuSkin {
        self.ui.menu_skin()
    }

    /// The glyph atlas the UI pass renders text from.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// Stages the wanted scene, then builds this frame's graph, compiles it,
    /// executes it, submits and presents.
    ///
    /// # Errors
    ///
    /// [`GpuError`] for a scene that cannot be staged, and for anything except a
    /// swapchain that has merely gone out of date, which is reported as
    /// [`FrameOutcome::Reconfigured`].
    pub fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
        let (scene, medium) = self.wanted;
        self.stage.show(&mut self.renderer, scene, medium)?;

        let Some(acquired) = self.ctx.acquire()? else {
            self.dumped = false;
            return Ok(FrameOutcome::Reconfigured);
        };
        let extent = acquired.extent;

        self.renderer
            .begin_frame(self.ctx.device(), &self.camera, &scene::sun(), extent)?;
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
            self.ui.add_passes(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        if !self.dumped {
            crcbl::log::debug!("render graph for the tide frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("tide frame"),
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

    /// Resizes the swapchain to `extent`.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the reconfigure failed. A zero extent is *not* an error.
    pub fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        self.ctx.resize(extent)?;
        self.dumped = false;
        Ok(())
    }

    /// Changes how presented frames are paced, mid-run.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the swapchain could not be rebuilt; the old one stays.
    pub fn set_pacing(&mut self, pacing: Pacing) -> Result<(), GpuError> {
        self.ctx.set_pacing(pacing)
    }

    /// Tears everything down in the order the seam requires.
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

/// The two seams `crcbl::settings::apply` reaches a renderer through — sundial's
/// pair, forwarded for `crcbl::impl_game_gpu!(Gpu, with_renderer)`.
impl Gpu {
    /// Put the player's `[engine.video]` section into force now.
    ///
    /// # Errors
    ///
    /// `crcbl::settings::apply_video_to`'s.
    fn apply_video(
        &mut self,
        video: &crcbl::settings::VideoSettings,
    ) -> Result<(), crcbl::settings::Unsupported> {
        crcbl::settings::apply_video_to(&mut self.renderer, self.ctx.device(), video)
    }

    /// Draw `view` instead of the shaded picture — the console's `debug_view`,
    /// which this fixture binds no key to but does not refuse.
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

/// Lets [`crcbl::engine::PolledBoot`] drive this bundle's arrival.
///
/// The forcing flags come from [`crate::Options`]'s `Default`, on sundial's
/// terms: the polled path exists for a browser, and a page has no argv.
impl crcbl::engine::PolledGpu for Gpu {
    type Pending = PendingGpu;
    type Context = ();

    fn request<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        (): Self::Context,
    ) -> Result<Self::Pending, GpuError> {
        Self::request_open(
            shell,
            window,
            extent,
            gpu,
            crate::args::Options::default().forced,
        )
    }

    fn poll_pending(pending: &mut Self::Pending) -> Result<Option<Self>, GpuError> {
        pending.poll()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_metadata_reports_execution_instead_of_capabilities() {
        let caps = DeviceCaps {
            features: crcbl::hal::Features::MESH_SHADER | crcbl::hal::Features::DESCRIPTOR_INDEXING,
            limits: crcbl::hal::Limits::desktop(),
        };
        assert_eq!(caps.geometry_path(), GeometryPath::MeshShader);
        assert_eq!(caps.binding_model(), BindingModel::Bindless);
        let paths = Paths::of(
            &caps,
            GeometryPath::IndirectPerBatch,
            ForcedPaths {
                geometry: Some(GeometryPath::IndirectPerBatch),
                binding: Some(BindingModel::Bindless),
            },
            RenderEffects::DEFAULT_STACK,
        );
        assert_eq!(paths.geometry, GeometryPath::IndirectPerBatch);
        assert_eq!(paths.binding, BindingModel::ArrayPages);
        use crcbl::ui::{DebugModule, DebugSection};
        let mut section = DebugSection::new("");
        paths.debug_section(&mut section);
        assert_eq!(
            section.rows()[0].value.to_string(),
            "IndirectPerBatch (forced)"
        );
        assert_eq!(
            section.rows()[1].value.to_string(),
            "ArrayPages (requested ceiling: Bindless)"
        );
    }

    /// **An unforced run asks for everything the engine does.**
    #[test]
    fn an_unforced_run_asks_for_everything_the_engine_does() {
        let asked = desc(GpuOptions::default(), ForcedPaths::default());
        assert_eq!(asked.label, "tide");
        let engine = GpuContextDesc::default().optional_features;
        assert!(
            asked.optional_features.contains(engine),
            "an unforced run asks for {:?}, which does not cover the engine's own {engine:?}",
            asked.optional_features,
        );
    }

    /// **The cost row tells three states apart**: an untimed device, a frame
    /// with no water — which every stub scene is — and one that drew it.
    #[test]
    fn the_water_cost_row_tells_untimed_from_dry_from_drawn() {
        let untimed = WaterCost::default();
        assert!(
            untimed.row().contains("no timestamp queries"),
            "{}",
            untimed.row()
        );

        let dry = WaterCost {
            passes: Vec::new(),
            timed: true,
        };
        assert!(dry.row().contains("no water passes"), "{}", dry.row());

        let drawn = WaterCost {
            passes: vec![
                ("water-copy".to_string(), 21_000),
                ("water".to_string(), 132_000),
            ],
            timed: true,
        };
        assert_eq!(drawn.row(), "water-copy 0.021 ms, water 0.132 ms");
        assert_ne!(untimed.row(), dry.row());
    }
}
