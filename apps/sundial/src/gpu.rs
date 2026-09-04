//! Sundial's GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::plaza`], and the UI and menu passes rule 4 asks every sample for.
//!
//! Everything that is not this sample's is [`crcbl::engine::GpuContext`]'s —
//! opening a backend, choosing an adapter, the swapchain, the frames-in-flight
//! ring, resize and teardown. What is here is the part that is sundial's: a
//! renderer built from an **application's** scene description rather than from
//! [`ForwardRenderer::new`]'s demo one, the capability report rule 12 asks for,
//! the sun the clock moves, and what the shadow work costs.
//!
//! # Forcing a lesser path is done by not asking for a feature
//!
//! `docs/plan/sample/00-samples-overview.md` rule 12: "every sample accepts a
//! flag forcing a lesser path". There is no switch on the renderer to do it
//! with, and there should not be — the selectors are computed from what the
//! *device* has ([`crcbl::hal::DeviceCaps::geometry_path`] and its two siblings),
//! so the honest way to reach a lesser one is to open a device without the
//! feature that selects the better one. [`Forced`] is that, and [`Paths`] is what
//! says which arm the frame actually took.
//!
//! # The sun is written every frame and kept nowhere else
//!
//! [`crate::sun::Clock`] lives on the loop's game state and this bundle holds
//! only the [`DirectionalLight`] the last frame was drawn with, because
//! [`ForwardRenderer::begin_frame`] takes one as an argument. A second copy of
//! the tick here would be a second clock.
//!
//! # What a filter costs, and what it does not
//!
//! [`ShadowCost`] reads the two passes the shadow work is spread across —
//! `shadow`, which draws the atlas, and `forward`, where the filter samples it —
//! off [`PassTimers`]. **There is no per-side row**, and
//! `docs/plan/45-shadows.md`'s fifteenth decision is why: the seam is resolved
//! per fragment inside one scene draw, so a timer either side of it would be
//! measuring half a scene rather than a filter. What prices a rung is the
//! `forward` row across two runs at two settings of `r_shadow_filter`, and this
//! is the row those two runs read.

pub use crcbl::engine::{FrameOutcome, GpuError};

use crcbl::engine::{GpuContext, GpuContextDesc, GpuOptions, Pacing, PendingGpuContext};
use crcbl::hal::{
    BindingModel, CommandEncoderDesc, DeviceCaps, Features, GeometryPath, LightingPath, downgrades,
};
use crcbl::prelude::*;
use crcbl::render::{
    DirectionalLight, EffectOverride, EffectRequest, ForwardRenderer, MAX_TIMED_PASSES,
    MenuRenderer, PassTimers, RenderEffects, RenderGraph, TransientPool, UiRenderer,
};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

use crate::plaza;
use crate::sun::Sky;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// The passes a shadow filter's cost is spread across, in the order they run.
///
/// `crcbl_render::forward` records the atlas render as `shadow` and the scene
/// draw as `forward`, and both matter to a rung: a wider kernel costs taps in the
/// second and nothing at all in the first, so a report naming only one of them
/// would attribute the whole of a filter's cost to a pass it does not touch.
const SHADOW_PASSES: [&str; 2] = ["shadow", "forward"];

/// A selector this run asked to be held **below** what the device offers.
///
/// Each variant names a path, and what it does is withhold the features that
/// select anything better — so a run that forces one is a run on a device that
/// genuinely does not have them, which is the only way a fallback gets executed
/// on hardware that would otherwise never take it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Forced {
    /// The geometry path to hold at, or `None` for whatever the device selects.
    pub geometry: Option<GeometryPath>,
    /// The binding model to hold at, on the same terms.
    pub binding: Option<BindingModel>,
}

impl Forced {
    /// What to ask a device for, given what a run wants held down.
    ///
    /// **Subtraction rather than a hand-written set per path.** A path's inputs
    /// are `GeometryPath::INPUTS` and `BindingModel::INPUTS`, and the set to
    /// remove is derived from them below, so a selector that grows a flag does
    /// not leave a second table behind still naming the old ones.
    #[must_use]
    pub fn optional_features(self) -> Features {
        // `TASK_SHADER` is not in the default set and is added here: it is what
        // `ForwardRenderer` builds the amplification stage from, so a mesh device
        // without it culls no clusters.
        let mut features = GpuContextDesc::default().optional_features | Features::TASK_SHADER;
        match self.geometry {
            None | Some(GeometryPath::MeshShader) => {}
            Some(GeometryPath::IndirectCount) => {
                features.remove(Features::MESH_SHADER | Features::TASK_SHADER);
            }
            Some(GeometryPath::IndirectPerBatch) => {
                features.remove(
                    Features::MESH_SHADER | Features::TASK_SHADER | Features::DRAW_INDIRECT_COUNT,
                );
            }
        }
        match self.binding {
            None | Some(BindingModel::Bindless) => {}
            Some(BindingModel::ArrayPages) => features.remove(Features::DESCRIPTOR_INDEXING),
        }
        features
    }
}

/// Which of `docs/plan/39-capabilities.md`'s three selectors this frame was drawn
/// through, and whether the run asked for less than the device offers.
///
/// Rule 12's "says which it took", as a value rather than as a log line, so the
/// debug panel, the headless summary and the golden suite all read the same
/// answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paths {
    /// The path the renderer's submission tail actually takes.
    pub geometry: GeometryPath,
    /// How the fragment stage addresses the base-colour page.
    pub binding: BindingModel,
    /// How indirect lighting is resolved.
    pub lighting: LightingPath,
    /// What the run asked to be held down.
    pub forced: Forced,
    /// Which of the render effects the frame draws, **resolved** — what came out
    /// of the four layers rather than what the command line asked for.
    pub effects: RenderEffects,
}

impl Paths {
    /// What the device opened as, beside what the run asked for.
    #[must_use]
    pub const fn of(caps: &DeviceCaps, forced: Forced, effects: RenderEffects) -> Self {
        Self {
            geometry: caps.geometry_path(),
            binding: caps.binding_model(),
            lighting: caps.lighting_path(),
            forced,
            effects,
        }
    }

    /// The effect set as the panel and the summary spell it.
    #[must_use]
    pub fn effects_row(&self) -> String {
        self.effects.row()
    }

    /// Why ray-traced shadows never appear, in one line for the panel.
    ///
    /// **Not a device answer.** `crcbl-vk` can report `RAY_QUERY`, so the selector
    /// would choose it — but nothing in `crcbl-render` builds an acceleration
    /// structure, so a run that selected it would draw the rasterised frame and
    /// say it had done something else.
    /// `docs/plan/sample/18-sundial.md`'s milestone 5 is the ray-traced shadow
    /// rung, and until it exists the panel says so rather than implying a choice
    /// was made.
    #[must_use]
    pub const fn ray_tracing_note() -> &'static str {
        "raster only (P7C)"
    }
}

/// The row the panel and the summary both print for a selector.
///
/// `"MeshShader"` where the device chose it, `"MeshShader (forced)"` where the
/// run asked for it — which is the difference between "this machine is like that"
/// and "this run made it like that".
fn selector_row(selected: impl core::fmt::Debug, forced: bool) -> String {
    if forced {
        format!("{selected:?} (forced)")
    } else {
        format!("{selected:?}")
    }
}

impl crcbl::ui::DebugModule for Paths {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("paths");
        section.row_str(
            "geometry",
            &selector_row(self.geometry, self.forced.geometry.is_some()),
        );
        section.row_str(
            "binding",
            &selector_row(self.binding, self.forced.binding.is_some()),
        );
        section.row_str("lighting", &format!("{:?}", self.lighting));
        section.row_str("ray tracing", Self::ray_tracing_note());
        section.row_str("effects", &self.effects_row());
    }
}

/// What the shadow work cost on the GPU, off the last frame whose timestamps have
/// landed.
///
/// **The charter's "cost per technique, per frame"**, in the shape the fifteenth
/// decision leaves available: the atlas render and the scene draw, each named. A
/// person pricing a rung runs the sample twice at two settings of
/// `r_shadow_filter` and compares the `forward` row — the module header says why
/// there is no per-side row to compare instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShadowCost {
    /// Each shadow-related pass the frame ran, in execution order, and its cost
    /// in nanoseconds.
    ///
    /// Which passes those are is this module's `SHADOW_PASSES`, and the timer
    /// rows are filtered through it.
    pub passes: Vec<(String, u64)>,
    /// Whether this device has timestamp queries at all.
    ///
    /// Told apart from "the frame ran no shadow pass", which is what a run with
    /// `--no-shadows` reports and is a different fact.
    pub timed: bool,
}

impl ShadowCost {
    /// The report, as one line for the summary and the panel.
    #[must_use]
    pub fn row(&self) -> String {
        if !self.timed {
            return "no timestamp queries on this device".to_string();
        }
        if self.passes.is_empty() {
            return "no shadow passes in this frame".to_string();
        }
        self.passes
            .iter()
            .map(|(label, nanos)| {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a pass duration is a few million nanoseconds"
                )]
                let ms = *nanos as f64 / 1.0e6;
                format!("{label} {ms:.3} ms")
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl crcbl::ui::DebugModule for ShadowCost {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("shadow cost");
        if self.passes.is_empty() {
            section.row_str("passes", &self.row());
            return;
        }
        for (label, nanos) in &self.passes {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a pass duration is a few million nanoseconds"
            )]
            let ms = *nanos as f64 / 1.0e6;
            section.row(label.as_str(), format_args!("{ms:.3} ms"));
        }
    }
}

/// Sundial's GPU side.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    renderer: ForwardRenderer,
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Which selectors the frame is drawn through, resolved once at open.
    paths: Paths,
    /// Where the frame is seen from. Written every tick by [`crate::app`], which
    /// owns the camera; this is only where the frame reads it.
    camera: crcbl::render::Camera,
    /// The sun the next frame is lit by. Written every frame by [`crate::app`]
    /// out of the clock — see this module's header.
    sun: DirectionalLight,
    ui: UiRenderer,
    menu: MenuRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
    /// The last frame's graph dump, for this crate's own tests.
    #[cfg(test)]
    last_dump: String,
}

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for.
///
/// One value rather than two copies: the two bring-up paths must open the *same*
/// device, or a feature only one of them requested is a bug nobody sees until the
/// other path runs.
fn desc(gpu: GpuOptions, forced: Forced) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "sundial",
        optional_features: forced.optional_features(),
        ..GpuContextDesc::from(gpu)
    }
}

/// A [`Gpu`] being opened one poll at a time.
#[derive(Debug)]
pub struct PendingGpu {
    pending: PendingGpuContext,
    forced: Forced,
    effects: RenderEffects,
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
            Some(ctx) => Gpu::from_context(ctx, self.forced, self.effects).map(Some),
            None => Ok(None),
        }
    }
}

impl Gpu {
    /// Opens the join and builds the forward renderer on [`plaza::plaza`].
    ///
    /// `extent` must come from the window system — call this only after the first
    /// configure.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened, if the backend exposes no adapter, no
    /// graphics queue or no surface format, if the plaza's description is one the
    /// pools it asks for cannot hold, or if any HAL call fails.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        forced: Forced,
        effects: RenderEffects,
    ) -> Result<Self, GpuError> {
        Self::from_context(
            GpuContext::open(shell, window, extent, &desc(gpu, forced))?,
            forced,
            effects,
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
        forced: Forced,
        effects: RenderEffects,
    ) -> Result<PendingGpu, GpuError> {
        Ok(PendingGpu {
            pending: GpuContext::request_open(shell, window, extent, &desc(gpu, forced))?,
            forced,
            effects,
        })
    }

    /// Builds the renderer, the plaza and the two UI passes on an already-open
    /// context — everything both bring-up paths share once the device exists.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the plaza's description is one the pools it asks for
    /// cannot hold, or if any HAL call fails.
    fn from_context(
        ctx: GpuContext,
        forced: Forced,
        effects: RenderEffects,
    ) -> Result<Self, GpuError> {
        let optional_features = forced.optional_features();
        let caps = ctx.device().caps();
        // Topic 39's "every downgrade is logged once, at device creation, naming
        // the feature and the path it selected" — including the ones `--force-*`
        // asked for.
        let report = downgrades(optional_features, &caps);
        if report.is_empty() {
            crcbl::log::info!("sundial: device granted every optional feature asked for");
        } else {
            crcbl::log::info!("sundial: {report}");
        }
        let scene = plaza::plaza();
        let mut renderer =
            ForwardRenderer::with_scene(ctx.device(), ctx.queue(), ctx.format(), &scene)?;
        let placed = match plaza::place(&mut renderer) {
            Ok(placed) => placed,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(crcbl::hal::HalError::InvalidDescriptor(
                    format!("sundial's plaza does not fit its own instance pool: {error}"),
                )));
            }
        };
        crcbl::log::info!("sundial: {placed} object(s) placed in the plaza");

        renderer.set_effect_request(request_for(ctx.video_effects(), effects));
        // Resolved rather than requested: the device clamps last, so what the
        // panel and the summary report has to come back off the renderer.
        let paths = Paths::of(&caps, forced, renderer.resolved_effects());
        crcbl::log::info!(
            "sundial: {:?} / {:?} / {:?}, effects {}",
            paths.geometry,
            paths.binding,
            paths.lighting,
            paths.effects_row(),
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
        let menu = match MenuRenderer::new(ctx.device(), ctx.queue(), ctx.format()) {
            Ok(menu) => menu,
            Err(error) => {
                ui.destroy(ctx.device());
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        Ok(Self {
            ctx,
            renderer,
            pool: TransientPool::new(),
            timers,
            paths,
            camera: plaza::fixed_camera(),
            sun: Sky::at(crate::sun::FIXTURE_TICK).light(),
            ui,
            menu,
            atlas: FontAtlas::built_in(),
            draw_list: DrawList::new(),
            dumped: false,
            #[cfg(test)]
            last_dump: String::new(),
        })
    }

    /// Which selectors this device drew through.
    #[must_use]
    pub const fn paths(&self) -> Paths {
        self.paths
    }

    /// The three requested layers of the toggle resolution order — what a caller
    /// editing one of them starts from.
    #[must_use]
    pub const fn effect_request(&self) -> EffectRequest {
        self.renderer.effect_request()
    }

    /// Which effects this **device** permits: the fourth layer, which clamps last
    /// and cannot be overridden upward.
    #[must_use]
    pub const fn device_effects(&self) -> RenderEffects {
        self.renderer.device_effects()
    }

    /// Replaces the requested layers, and re-resolves what [`Gpu::paths`] reports.
    pub fn set_effect_request(&mut self, request: EffectRequest) {
        self.renderer.set_effect_request(request);
        self.paths.effects = self.renderer.resolved_effects();
    }

    /// The context, for a caller arming `--screenshot` before the first frame.
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

    /// What the next frame is lit by.
    pub const fn set_sun(&mut self, sun: DirectionalLight) {
        self.sun = sun;
    }

    /// The most recent frame whose per-pass GPU timings have landed.
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// What the shadow work cost in the last frame whose timestamps landed.
    #[must_use]
    pub fn shadow_cost(&self) -> ShadowCost {
        let Some(timings) = self.timings() else {
            return ShadowCost::default();
        };
        ShadowCost {
            passes: timings
                .passes
                .iter()
                .filter(|pass| SHADOW_PASSES.contains(&pass.label.as_str()))
                .map(|pass| (pass.label.clone(), pass.gpu_nanos))
                .collect(),
            timed: true,
        }
    }

    /// What the last [`Gpu::frame`] recorded, summed over the passes this bundle
    /// adds.
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.renderer
            .counters()
            .plus(self.menu.counters())
            .plus(self.ui.counters())
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

    /// Takes this frame's menu, or `None` on a frame that shows none.
    pub fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        self.menu.set_menu(menu);
    }

    /// The glyph atlas the UI pass renders text from.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// The last frame's render-graph dump, for this crate's own tests.
    #[cfg(test)]
    pub fn last_dump(&self) -> &str {
        &self.last_dump
    }

    /// Builds this frame's graph, compiles it, executes it, submits and presents.
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

        // The plaza's punctual lights are static and are handed over every frame
        // from one list, so the windowed run and the golden suite cannot disagree
        // about what is lit — `plaza::lights`' doc carries that argument.
        self.renderer.set_lights(&plaza::lights());
        self.renderer
            .begin_frame(self.ctx.device(), &self.camera, &self.sun, extent)?;
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
            let _hdr = self
                .renderer
                .add_passes(&mut graph, &self.pool, target, extent);
            // Between the scene and the text, on `apps/sandbox`'s terms: the
            // menu's scrim dims what is already in the target and the overlay has
            // to stay readable over both.
            self.menu.add_pass(&mut graph, target);
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        #[cfg(test)]
        {
            self.last_dump = compiled.dump();
        }
        if !self.dumped {
            crcbl::log::debug!("render graph for the sundial frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("sundial frame"),
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
    /// [`GpuError`] if the reconfigure failed. A zero extent is *not* an error: a
    /// minimised window reports one and the swapchain is left alone.
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
        self.menu.destroy(self.ctx.device());
        self.pool.destroy(self.ctx.device());
        if let Some(timers) = self.timers.as_mut() {
            timers.destroy(self.ctx.device());
        }
        self.renderer.destroy(self.ctx.device());
        self.ctx.destroy()
    }
}

/// The requested layers, given what the command line asked for and what the
/// player's settings allow.
///
/// **The video layer is the player's and the programmatic layer is the run's.**
/// There is no camera layer to write: this fixture's two fixed poses want the
/// same effect stack, so the layer a view would ask for is the default one.
fn request_for(video: RenderEffects, effects: RenderEffects) -> EffectRequest {
    EffectRequest {
        video,
        programmatic: EffectOverride::none()
            .force(RenderEffects::all().difference(effects), Some(false)),
        ..EffectRequest::default()
    }
}

// ---------------------------------------------------------------------------
// The engine's seams
// ---------------------------------------------------------------------------

/// The two seams `crcbl::settings::apply` reaches a renderer through.
///
/// A second inherent block rather than lines inside the one above, so the
/// forwards `crcbl::impl_game_gpu!(Gpu, with_renderer)` picks up sit beside the
/// invocation that needs them. `docs/plan/52-debug-console.md` decision 3 is where
/// the pair comes from, and `crcbl::settings` holds both bodies.
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
    /// The console's `debug_view` reaches the frame here. This fixture binds no
    /// key to it — a shadow is read off the shaded picture and there is no shadow
    /// channel to draw on its own — but the console can still ask for one, and a
    /// sample that refused would be a sample where a console command silently did
    /// nothing.
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
/// **Where the flags go.** [`Gpu::request_open`] takes the two this sample adds to
/// a device request and the trait's `request` does not, so this forwards what
/// [`crate::Options`]'s `Default` gives — read off the defaults rather than
/// spelled again, so the two cannot drift. That is the whole truth about the
/// polled path: it exists for a browser, a page has no argv, and
/// `--force-geometry` and `--no-shadows` therefore have no source there. A caller
/// that has flags to honour has the blocking [`Gpu::open`], which takes both.
impl crcbl::engine::PolledGpu for Gpu {
    type Pending = PendingGpu;

    /// Built from the window and the options alone: this sample's extra arguments
    /// come from its own defaults, not from a caller.
    type Context = ();

    fn request<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        (): Self::Context,
    ) -> Result<Self::Pending, GpuError> {
        let defaults = crate::args::Options::default();
        Self::request_open(
            shell,
            window,
            extent,
            gpu,
            defaults.forced,
            defaults.effects,
        )
    }

    fn poll_pending(pending: &mut Self::Pending) -> Result<Option<Self>, GpuError> {
        pending.poll()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **An unforced run opens the engine's own bundle**, and forcing a path
    /// withholds exactly the flags that select a better one.
    ///
    /// The assertion is containment rather than equality, on `apps/alcove`'s
    /// ground: this sample overrides the optional set and deliberately adds
    /// `TASK_SHADER` on top, so a superset check is what catches
    /// [`Forced::optional_features`] being rewritten as a hand-written list that
    /// goes stale the moment the engine asks for one more flag.
    #[test]
    fn an_unforced_run_asks_for_everything_the_engine_does() {
        let asked = desc(GpuOptions::default(), Forced::default());
        assert_eq!(asked.label, "sundial");

        let engine = GpuContextDesc::default().optional_features;
        assert!(
            asked.optional_features.contains(engine),
            "an unforced run asks for {:?}, which does not cover the engine's own {engine:?}",
            asked.optional_features,
        );
        assert!(
            asked.optional_features.contains(Features::TASK_SHADER),
            "the amplification stage is what culls clusters, and this fixture wants the best path \
             completely",
        );
    }

    /// **Forcing a path takes away the features that select a better one**, and
    /// takes away nothing else.
    #[test]
    fn forcing_a_path_withholds_the_flags_that_select_a_better_one() {
        let unforced = Forced::default().optional_features();
        for (forced, gone) in [
            (
                Forced {
                    geometry: Some(GeometryPath::IndirectCount),
                    binding: None,
                },
                Features::MESH_SHADER | Features::TASK_SHADER,
            ),
            (
                Forced {
                    geometry: Some(GeometryPath::IndirectPerBatch),
                    binding: None,
                },
                Features::MESH_SHADER | Features::TASK_SHADER | Features::DRAW_INDIRECT_COUNT,
            ),
            (
                Forced {
                    geometry: None,
                    binding: Some(BindingModel::ArrayPages),
                },
                Features::DESCRIPTOR_INDEXING,
            ),
        ] {
            let asked = forced.optional_features();
            assert!(
                !asked.intersects(gone),
                "{forced:?} still asks for {:?}",
                asked.intersection(gone)
            );
            assert_eq!(
                asked,
                unforced.difference(gone),
                "{forced:?} withheld more than the selector's own inputs",
            );
        }
    }

    /// **The cost row tells three states apart**, which is what makes it a report
    /// rather than a number.
    ///
    /// A device with no timestamp queries, a frame that ran no shadow pass — which
    /// is what `--no-shadows` produces — and a frame that drew and sampled the
    /// atlas are three different facts, and a row that printed nothing for the
    /// first two would leave a reader unable to tell "shadows are free" from
    /// "nothing measured them".
    #[test]
    fn the_shadow_cost_row_tells_untimed_from_unshadowed_from_drawn() {
        let untimed = ShadowCost::default();
        assert!(!untimed.timed);
        assert!(
            untimed.row().contains("no timestamp queries"),
            "{}",
            untimed.row()
        );

        let unshadowed = ShadowCost {
            passes: Vec::new(),
            timed: true,
        };
        assert!(
            unshadowed.row().contains("no shadow passes"),
            "{}",
            unshadowed.row()
        );

        let drawn = ShadowCost {
            passes: vec![
                ("shadow".to_string(), 410_000),
                ("forward".to_string(), 1_250_000),
            ],
            timed: true,
        };
        let row = drawn.row();
        assert!(row.contains("shadow 0.410 ms"), "{row}");
        assert!(row.contains("forward 1.250 ms"), "{row}");
        assert_ne!(
            untimed.row(),
            unshadowed.row(),
            "an untimed device and an unshadowed frame must not read alike",
        );
    }
}
