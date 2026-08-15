//! Lumen's GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::room`], and the UI and menu passes rule 4 asks every sample for.
//!
//! Everything that is not this sample's is [`crcbl::engine::GpuContext`]'s —
//! opening a backend, choosing an adapter, the swapchain, the frames-in-flight
//! ring, resize and teardown. What is here is the part that is lumen's: a
//! renderer built from an **application's** scene description rather than from
//! [`ForwardRenderer::new`]'s demo one, and the capability report rule 12 asks
//! for.
//!
//! # Forcing a lesser path is done by not asking for a feature
//!
//! `docs/plan/sample/00-samples-overview.md` rule 12: "every sample accepts a
//! flag forcing a lesser path". There is no switch on the renderer to do it
//! with, and there should not be — the selectors are computed from what the
//! *device* has ([`crcbl::hal::DeviceCaps::geometry_path`] and its two
//! siblings), so the honest way to reach a lesser one is to open a device
//! without the feature that selects the better one. [`Forced`] is that, and
//! [`Paths`] is what says which arm the frame actually took.

pub use crcbl::engine::{FrameOutcome, GpuError};

use crcbl::engine::{GpuContext, GpuContextDesc, GpuOptions, Pacing, PendingGpuContext};
use crcbl::hal::{
    BindingModel, CommandEncoderDesc, DeviceCaps, Features, GeometryPath, LightingPath, downgrades,
};
use crcbl::prelude::*;
use crcbl::render::{
    EffectOverride, EffectRequest, ForwardRenderer, MAX_TIMED_PASSES, MenuRenderer, PassTimers,
    RenderEffects, RenderGraph, TransientPool, UiRenderer,
};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

use crate::room;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

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
    /// Starts from [`GpuContextDesc::default`]'s optional set — the one every
    /// other sample opens with — and removes the flags whose presence would
    /// select something better than the forced value.
    ///
    /// **Subtraction rather than a hand-written set per path.** A path's inputs
    /// are `GeometryPath::INPUTS` and `BindingModel::INPUTS`, and the set to
    /// remove is derived from them below, so a selector that grows a flag does
    /// not leave a second table behind still naming the old ones.
    #[must_use]
    pub fn optional_features(self) -> Features {
        // `TASK_SHADER` is not in the default set and is added here: it is what
        // `ForwardRenderer` builds §3.5's amplification stage from, so a mesh
        // device without it culls no clusters — and this sample's whole subject
        // is what the device did, which makes "the best path, completely" the
        // right thing to ask for.
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

/// Which of `docs/plan/39-capabilities.md`'s three selectors this frame was
/// drawn through, and whether the run asked for less than the device offers.
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
    /// How indirect lighting is resolved. `Rasterised` on every device today —
    /// see [`Paths::ray_tracing_note`].
    pub lighting: LightingPath,
    /// What the run asked to be held down.
    pub forced: Forced,
    /// Which of topic 18's effects the frame draws, **resolved** — what came out
    /// of the four layers rather than what the command line asked for.
    ///
    /// Read back off the renderer for that reason: the charter's toggle matrix
    /// is only checkable if the panel and the summary report what the frame did,
    /// and a request the device clamped would otherwise report as granted.
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

    /// The effect set as the panel and the summary spell it: `shadows ao ssr`,
    /// with a switched-off one dropped, and `none` where they all are.
    #[must_use]
    pub fn effects_row(&self) -> String {
        let on: Vec<&str> = [
            (RenderEffects::SHADOWS, "shadows"),
            (RenderEffects::AMBIENT_OCCLUSION, "ao"),
            (RenderEffects::REFLECTIONS, "ssr"),
        ]
        .into_iter()
        .filter(|(feature, _)| self.effects.contains(*feature))
        .map(|(_, name)| name)
        .collect();
        if on.is_empty() {
            "none".to_string()
        } else {
            on.join(" ")
        }
    }

    /// Why [`LightingPath::RayTraced`] never appears, in one line for the panel.
    ///
    /// **Not a device answer.** `crcbl-vk` can report `RAY_QUERY` and
    /// `ACCELERATION_STRUCTURE`, so the selector would choose it — but nothing
    /// in `crcbl-render` builds an acceleration structure or traces one, so a
    /// run that selected it would draw the rasterised frame and say it had done
    /// something else. This sample's charter makes the pair of paths its
    /// milestone 2 and 3; until then there is one path and the panel says so
    /// rather than implying a choice was made.
    #[must_use]
    pub const fn ray_tracing_note() -> &'static str {
        "raster only (P7C)"
    }
}

/// The row the panel and the summary both print for a selector.
///
/// `"MeshShader"` where the device chose it, `"MeshShader (forced)"` where the
/// run asked for it — which is the difference between "this machine is like
/// that" and "this run made it like that", and a report without it is one a
/// reader cannot act on.
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

/// What the room is waiting on, as a panel section.
///
/// **The row that stops a reviewer filing a bug.** Ambient scales the diffuse
/// albedo and a conductor has none — see [`crate::room`] — so a reflection is
/// the whole of what lights either metal surface, and both now get one: the
/// mirror panel takes a screen-space hit at its foot and the irradiance volume
/// [`crate::bounce`] bakes everywhere else on that face, and the brass block,
/// whose roughness is above [`crcbl::shaders::ssr::ROUGHNESS_CUTOFF`], takes
/// that volume directly without marching at all. What is left to say on the
/// screen is what that environment *is*: baked and blurry, and the only answer
/// there is for anything the frame cannot see. Ray tracing is what replaces it,
/// and the `paths` section's `ray tracing` row is where that is named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unbuilt;

impl crcbl::ui::DebugModule for Unbuilt {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("unbuilt");
        section.row_str("metal", "reflection only: SSR hit or baked probe");
        section.row_str("bounce wall", "one analytic bounce, no GI solve");
        // The per-effect toggles used to be a row here. They are built — the
        // `paths` section's `effects` row is what this frame drew — so what is
        // still owed is the two request layers with no source in the tree.
        section.row_str("toggle layers", "no camera RON, no [engine.video]");
    }
}

/// Lumen's GPU side.
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
    /// Seconds of animation, advanced by the loop rather than read from a clock
    /// here — a headless run must render the same room on every machine.
    elapsed: f32,
    ui: UiRenderer,
    menu: MenuRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
    /// The last frame's graph dump, for this crate's own tests: it is how a test
    /// sees whether the UI pass was in the frame at all.
    #[cfg(test)]
    last_dump: String,
}

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for.
///
/// One value rather than two copies, for the reason every sample gives: the two
/// bring-up paths must open the *same* device, or a feature only one of them
/// requested is a bug nobody sees until the other path runs. Here that is more
/// than a tidiness argument — [`Forced::optional_features`] is how this sample
/// forces a lesser path at all, so a path that asked for a different set would
/// draw the room through different selectors.
fn desc(gpu: GpuOptions, forced: Forced) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "lumen",
        optional_features: forced.optional_features(),
        ..GpuContextDesc::from(gpu)
    }
}

/// A [`Gpu`] being opened one poll at a time.
///
/// It carries `forced` and `effects` because the device request outlives the
/// call that started it: both are read again by [`Gpu::from_context`] once the
/// device arrives, and neither is anything the engine's polled bring-up knows
/// about.
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
    /// [`GpuError`] if the device request failed, or if a renderer refused the
    /// device it produced.
    pub fn poll(&mut self) -> Result<Option<Gpu>, GpuError> {
        match self.pending.poll()? {
            Some(ctx) => Gpu::from_context(ctx, self.forced, self.effects).map(Some),
            None => Ok(None),
        }
    }
}

impl Gpu {
    /// Opens the join and builds the forward renderer on [`room::room`].
    ///
    /// `extent` must come from the window system — call this only after the
    /// first configure.
    ///
    /// `effects` is which of topic 18's effects this run **asks** for; what the
    /// frame actually draws is [`Paths::effects`], read back off the renderer
    /// once the four layers have resolved.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened, if the backend exposes no adapter, no
    /// graphics queue or no surface format, if the room's description is one the
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

    /// Builds the renderer, the room and the two UI passes on an already-open
    /// context — everything both bring-up paths share once the device exists.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the room's description is one the pools it asks for
    /// cannot hold, or if any HAL call fails.
    fn from_context(
        ctx: GpuContext,
        forced: Forced,
        effects: RenderEffects,
    ) -> Result<Self, GpuError> {
        let optional_features = forced.optional_features();
        let caps = ctx.device().caps();
        // Topic 39's "every downgrade is logged once, at device creation,
        // naming the feature and the path it selected" — including the ones
        // `--force-*` asked for, which are downgrades this run made on purpose
        // and which the line above would otherwise be silent about.
        let report = downgrades(optional_features, &caps);
        if report.is_empty() {
            crcbl::log::info!("lumen: device granted every optional feature asked for");
        } else {
            crcbl::log::info!("lumen: {report}");
        }
        let mut renderer =
            ForwardRenderer::with_scene(ctx.device(), ctx.queue(), ctx.format(), &room::room())?;
        // Every object of the room, in one fixed order — see `room::place`.
        let placed = match room::place(&mut renderer) {
            Ok(placed) => placed,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(crcbl::hal::HalError::InvalidDescriptor(
                    format!("lumen's room does not fit its own instance pool: {error}"),
                )));
            }
        };
        crcbl::log::info!("lumen: {placed} object(s) placed in the room");

        // The **programmatic** layer of topic 39's resolution order, which is
        // the one a command line has any business driving: the camera stack and
        // `[engine.video]` are a view's and a player's answers, and this run is
        // neither of them. Forcing off rather than clamping the camera layer for
        // the same reason — `--no-shadows` is an instruction, not a preference,
        // and the override is the layer that can say so either way.
        renderer.set_effect_request(EffectRequest {
            programmatic: EffectOverride::none()
                .force(RenderEffects::all().difference(effects), Some(false)),
            ..EffectRequest::default()
        });
        // Resolved rather than requested: the device clamps last, so what the
        // panel and the summary report has to come back off the renderer.
        let paths = Paths::of(&caps, forced, renderer.resolved_effects());
        crcbl::log::info!(
            "lumen: {:?} / {:?} / {:?}, effects {}",
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
            camera: room::fixed_camera(),
            elapsed: 0.0,
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

    /// Which effects this **device** permits: the fourth layer, which clamps
    /// last and cannot be overridden upward.
    ///
    /// What tells an effect a run switched off from one this device could never
    /// draw — the pause menu's rows are the only thing that asks, and a row that
    /// could not tell them apart would offer a switch that does nothing.
    #[must_use]
    pub const fn device_effects(&self) -> RenderEffects {
        self.renderer.device_effects()
    }

    /// Replaces the requested layers, and re-resolves what [`Gpu::paths`]
    /// reports.
    ///
    /// The panel's `effects` row and the headless summary both name what the
    /// frame draws, so a request arriving mid-run has to move them: a report
    /// resolved once at open would go on printing the command line's answer
    /// after a menu row changed it.
    pub fn set_effect_request(&mut self, request: EffectRequest) {
        self.renderer.set_effect_request(request);
        self.paths.effects = self.renderer.resolved_effects();
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

    /// Advances the lamp's orbit by `dt` seconds.
    ///
    /// Driven by the loop's clock rather than read from one here, so a headless
    /// run renders the same room on every machine — which is what makes a golden
    /// of it worth anything.
    pub const fn advance(&mut self, dt: f32) {
        self.elapsed += dt;
    }

    /// Seconds of animation the next frame will draw.
    #[must_use]
    pub const fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// The most recent frame whose per-pass GPU timings have landed.
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// What the last [`Gpu::frame`] recorded, summed over the three passes this
    /// bundle adds.
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.renderer
            .counters()
            .plus(self.menu.counters())
            .plus(self.ui.counters())
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

    /// The UI geometry this frame handed over, for this crate's own tests.
    #[cfg(test)]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
    }

    /// Builds this frame's graph, compiles it, executes it, submits and
    /// presents.
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

        // The only thing in the room that moves. The nine objects were placed
        // once and never rewritten, so a still frame uploads no instance bytes
        // at all — the lamp is a row of the light list, not an instance.
        self.renderer.set_lights(&[room::lamp(self.elapsed)]);
        self.renderer
            .begin_frame(self.ctx.device(), &self.camera, &room::sun(), extent)?;
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
            let _hdr = self.renderer.add_passes(&mut graph, target, extent);
            // Between the scene and the text, on `apps/sandbox`'s terms: the
            // menu's scrim dims what is already in the target and the overlay
            // has to stay readable over both.
            self.menu.add_pass(&mut graph, target);
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        #[cfg(test)]
        {
            self.last_dump = compiled.dump();
        }
        if !self.dumped {
            crcbl::log::debug!("render graph for the lumen frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("lumen frame"),
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
    /// [`GpuError`] if the reconfigure failed. A zero extent is *not* an error:
    /// a minimised window reports one and the swapchain is left alone.
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

    /// The pacing asked for, before resolution against the display. Test-only.
    #[cfg(test)]
    #[must_use]
    pub const fn pacing(&self) -> Pacing {
        self.ctx.pacing()
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

// ---------------------------------------------------------------------------
// The engine's seams
// ---------------------------------------------------------------------------

// The nine forwards `crcbl::engine` calls this bundle through. Every one of
// them is a method above; the macro is what stops a sample forgetting one.
//
// `PolledGpu` is written out below instead of taken from
// `crcbl::impl_polled_gpu!`, because this sample's `request_open` takes its
// forced path and effect set as well.
crcbl::impl_game_gpu!(Gpu);

/// Lets [`crcbl::engine::PolledBoot`] drive this bundle's arrival.
///
/// The extent and the resize are [`crcbl::engine::GpuSurface`]'s, because a
/// running loop asks the same two.
///
/// **Where the flags go.** [`Gpu::request_open`] takes the two this sample adds
/// to a device request and the trait's `request` does not, so this forwards
/// what [`Options`](crate::Options)'s `Default` gives — read off the defaults
/// rather than spelled again, so the two cannot drift. That is the whole truth
/// about the polled path and not a shortcut: it exists for the browser, a page
/// has no argv, and `--force-geometry` and the `--no-*` flags therefore have no
/// source there. A caller that has flags to honour has the blocking
/// [`Gpu::open`], which takes both.
impl crcbl::engine::PolledGpu for Gpu {
    type Pending = PendingGpu;

    fn request<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
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

    /// **An unforced run opens the engine's own bundle**, which is the check
    /// every other sample carries and this one did not.
    ///
    /// The other five assert equality with [`GpuContextDesc::default`]'s
    /// optional set. This sample cannot: it is the one that overrides the
    /// field, and it deliberately adds `TASK_SHADER` on top — so the assertion
    /// is **containment**, which is the part that catches the real failure. If
    /// [`Forced::optional_features`] were ever rewritten as a hand-written list
    /// instead of a subtraction from the engine's default, it would go stale
    /// the moment the engine asked for one more flag, and nothing would say so
    /// — the same silent shape that left `apps/hud` opening a device without
    /// `PRESENT_FEEDBACK`.
    ///
    /// `TASK_SHADER` is asserted separately rather than folded into the
    /// expected set, so that dropping it fails here instead of being absorbed
    /// by a superset check.
    #[test]
    fn an_unforced_run_asks_for_everything_the_engine_does() {
        let asked = desc(GpuOptions::default(), Forced::default());
        assert_eq!(asked.label, "lumen");

        let engine = GpuContextDesc::default().optional_features;
        assert!(
            asked.optional_features.contains(engine),
            "an unforced run must ask for at least the engine's own set; \
             missing {:?}",
            engine.difference(asked.optional_features),
        );
        assert!(
            asked.optional_features.contains(Features::TASK_SHADER),
            "this sample asks for the amplification stage on top",
        );
    }

    /// **Forcing a path removes exactly the flags that select a better one**,
    /// and nothing else.
    ///
    /// The observable is the selector the resulting feature set computes to, not
    /// the bits: a flag removed from the wrong axis, or one removed that no
    /// selector reads, both leave a plausible-looking set. `from_features` is
    /// the same function the device's own caps go through, so this asks the
    /// question the frame will ask.
    #[test]
    fn forcing_a_path_opens_a_device_that_selects_it() {
        let best = Forced::default().optional_features();
        assert_eq!(GeometryPath::from_features(best), GeometryPath::MeshShader);
        assert_eq!(BindingModel::from_features(best), BindingModel::Bindless);

        for want in [
            GeometryPath::MeshShader,
            GeometryPath::IndirectCount,
            GeometryPath::IndirectPerBatch,
        ] {
            let forced = Forced {
                geometry: Some(want),
                binding: None,
            };
            let features = forced.optional_features();
            assert_eq!(
                GeometryPath::from_features(features),
                want,
                "a device given {features:?} does not select {want:?}"
            );
            assert_eq!(
                BindingModel::from_features(features),
                BindingModel::Bindless,
                "{want:?} must not disturb the binding axis"
            );
        }

        for want in [BindingModel::Bindless, BindingModel::ArrayPages] {
            let features = Forced {
                geometry: None,
                binding: Some(want),
            }
            .optional_features();
            assert_eq!(BindingModel::from_features(features), want);
            assert_eq!(
                GeometryPath::from_features(features),
                GeometryPath::MeshShader,
                "{want:?} must not disturb the geometry axis"
            );
        }

        // And both at once, which is the combination a `--force-geometry
        // indirect-per-batch --force-binding array-pages` run asks for: the
        // browser's shape, on this desktop.
        let browserish = Forced {
            geometry: Some(GeometryPath::IndirectPerBatch),
            binding: Some(BindingModel::ArrayPages),
        }
        .optional_features();
        assert_eq!(
            GeometryPath::from_features(browserish),
            GeometryPath::IndirectPerBatch
        );
        assert_eq!(
            BindingModel::from_features(browserish),
            BindingModel::ArrayPages
        );
    }

    /// **The panel names the selector and says whether the run forced it.**
    ///
    /// A report that printed the path alone reads identically for a machine
    /// with no mesh shaders and a run that turned them off, which is the one
    /// distinction rule 12's flag exists to make.
    #[test]
    fn the_panel_distinguishes_a_forced_path_from_a_devices_own() {
        use crcbl::ui::{DebugModule, DebugSection};

        let device_chose = Paths {
            geometry: GeometryPath::IndirectCount,
            binding: BindingModel::ArrayPages,
            lighting: LightingPath::Rasterised,
            forced: Forced::default(),
            effects: RenderEffects::all(),
        };
        let run_chose = Paths {
            forced: Forced {
                geometry: Some(GeometryPath::IndirectCount),
                binding: None,
            },
            ..device_chose
        };

        let rows = |paths: &Paths| {
            let mut section = DebugSection::new("");
            paths.debug_section(&mut section);
            section
                .rows()
                .iter()
                .map(|row| (row.label.to_string(), row.value.to_string()))
                .collect::<Vec<_>>()
        };

        let plain = rows(&device_chose);
        let forced = rows(&run_chose);
        assert_eq!(
            plain[0],
            ("geometry".to_string(), "IndirectCount".to_string())
        );
        assert_eq!(
            forced[0],
            ("geometry".to_string(), "IndirectCount (forced)".to_string()),
        );
        assert_eq!(
            plain[1].1, forced[1].1,
            "forcing the geometry axis must not relabel the binding one"
        );
        // The row that tells a reviewer this frame is the rasterised one is
        // present in both.
        assert!(
            plain.iter().any(|(label, _)| label == "ray tracing"),
            "the panel must say why RayTraced never appears: {plain:?}"
        );
    }
}
