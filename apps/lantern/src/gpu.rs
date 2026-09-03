//! Lantern's GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::room`], and the UI and menu passes rule 4 asks every sample for.
//!
//! Everything that is not this sample's is [`crcbl::engine::GpuContext`]'s —
//! opening a backend, choosing an adapter, the swapchain, the frames-in-flight
//! ring, resize and teardown. What is here is the part that is lantern's: a
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
    BindingModel, CommandEncoderDesc, DeviceCaps, Extent3d, Features, Format, GeometryPath,
    ImageAspect, ImageCopy, ImageSubresourceLayers, ImageSubresourceRange, ImageUsage,
    LightingPath, Offset3d, ResourceState, downgrades,
};
use crcbl::prelude::*;
use crcbl::render::{
    Antialiasing, EffectOverride, EffectRequest, ForwardRenderer, ImportedImage, MAX_TIMED_PASSES,
    MenuRenderer, PassTimers, RenderEffects, RenderGraph, TransientImageDesc, TransientPool,
    UiRenderer,
};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

use crate::room;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// The format the monitor's view is rendered into and stored as.
///
/// **The page's own format, and it has to be**: the frame reaches the screen as
/// an image-to-image copy into one layer of the base-colour page, and a copy is
/// between two images that agree about what a texel is. `crcbl-render` uploads a
/// page as `Rgba8UnormSrgb`, so that is what the monitor renderer's tonemap is
/// built for — and it is the right answer twice over, because an sRGB attachment
/// encodes on write and an sRGB sampler decodes on read, so what the fragment
/// stage gets back off the screen is the linear colour the tonemap produced.
const MONITOR_FORMAT: Format = Format::Rgba8UnormSrgb;

/// How many passes one lantern frame records, at most.
///
/// [`MAX_TIMED_PASSES`] is one of each renderer this crate uses, and this frame
/// has **two** forward renderers in it — the room and the monitor's view of it —
/// plus the copy that puts the second on the screen. A timer sized at the shared
/// constant would bracket the first renderer's passes and drop the monitor's,
/// which `PassTimers` reports as one warning and then lives with.
const LANTERN_TIMED_PASSES: u32 = MAX_TIMED_PASSES + ForwardRenderer::MAX_PASSES + 1;

/// The requested layers for `view`, given what the command line asked for and
/// what the player's settings allow.
///
/// **The camera layer is the view's, the video layer is the player's and the
/// programmatic layer is the run's**, which is the whole of
/// `docs/plan/39-capabilities.md`'s order made visible in one function:
/// `--no-shadows` is an instruction about this run and belongs to the layer that
/// can move a decision either way, `[engine.video]` is a standing preference and
/// belongs to the one that may only remove, while "a monitor does not reflect
/// itself" is a fact about the view. A run that turns reflections back on from
/// the pause menu gets them in the frame it can see and still not in the
/// monitor, because the layers are resolved in that order and not merged — and
/// still not at all if the player's file switched them off, because every flag
/// this run has only ever *removes*: `--no-*` sets the programmatic layer's off
/// bits and nothing here forces one back on, so the video layer's clamp is the
/// last word on an effect it took.
///
/// `antialiasing` is the player's too and is the one layer that is not a clamp
/// — see [`EffectRequest::antialiasing`]. A run's `--no-*` flags still have the
/// last word over it, because the override is applied after it.
fn request_for(
    view: room::View,
    video: RenderEffects,
    antialiasing: Option<Antialiasing>,
    effects: RenderEffects,
) -> EffectRequest {
    EffectRequest {
        camera: view.stack(),
        video,
        antialiasing,
        programmatic: EffectOverride::none()
            .force(RenderEffects::all().difference(effects), Some(false)),
    }
}

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
    /// The same question asked of the **monitor's** view, whose camera stack is
    /// [`room::View::Monitor`]'s.
    ///
    /// Two rows rather than one because the camera layer is only observable as a
    /// difference: a panel that named one effect set could not tell a frame
    /// whose two views agree from one whose second view was never drawn.
    pub monitor_effects: RenderEffects,
}

impl Paths {
    /// What the device opened as, beside what the run asked for.
    #[must_use]
    pub const fn of(
        caps: &DeviceCaps,
        forced: Forced,
        effects: RenderEffects,
        monitor_effects: RenderEffects,
    ) -> Self {
        Self {
            geometry: caps.geometry_path(),
            binding: caps.binding_model(),
            lighting: caps.lighting_path(),
            forced,
            effects,
            monitor_effects,
        }
    }

    /// The effect set as the panel and the summary spell it: `shadows ao ssr`,
    /// with a switched-off one dropped, and `none` where they all are.
    ///
    /// [`RenderEffects::row`] does the spelling, for every sample that reports
    /// one. The hand-written table this used to hold named every effect but
    /// [`RenderEffects::BLOOM`], so a frame drawn with bloom reported a row with
    /// no bloom in it.
    #[must_use]
    pub fn effects_row(&self) -> String {
        self.effects.row()
    }

    /// The same spelling for the monitor's own view.
    #[must_use]
    pub fn monitor_row(&self) -> String {
        self.monitor_effects.row()
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
        // The camera layer, as a difference a reader can see: this row and the
        // one above it come out of the same `EffectRequest::resolve` on the same
        // device in the same frame, and what separates them is the stack each
        // view asked for.
        section.row_str("monitor", &self.monitor_row());
    }
}

/// What the room is waiting on, as a panel section.
///
/// **The row that stops a reviewer filing a bug.** Ambient scales the diffuse
/// albedo and a conductor has none — see [`crate::room`] — so a reflection is
/// the whole of what lights either metal surface, and both now get one: the
/// mirror panel takes a screen-space hit at its foot and the irradiance volume
/// [`crate::bounce`] places everywhere else on that face, and the brass block,
/// whose roughness is above [`crcbl::shaders::ssr::ROUGHNESS_CUTOFF`], takes
/// that volume directly without marching at all. What is left to say on the
/// screen is what that environment *is*: one bounce of the sun off whatever the
/// map saw, blurred into a probe grid, and the only answer there is for anything
/// the frame cannot see. Ray tracing is what replaces it,
/// and the `paths` section's `ray tracing` row is where that is named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unbuilt;

impl crcbl::ui::DebugModule for Unbuilt {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("unbuilt");
        section.row_str("metal", "reflection only: SSR hit or probe volume");
        section.row_str("bounce wall", "the sun's first bounce, no GI solve");
        // The per-effect toggles used to be a row here, and so did both unwired
        // request layers. Both have sources now — the monitor's view asks for
        // its own stack, which the `paths` section's `monitor` row reports, and
        // the settings block reaches `request_for` through `video_effects` — so
        // this row names where each layer comes from rather than what is owed.
        section.row_str("toggle layers", "camera: monitor; video: settings");
        section.row_str("monitor", "one frame behind: fed at the frame's tail");
    }
}

/// Lantern's GPU side.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    renderer: ForwardRenderer,
    /// The same room from [`room::monitor_camera`], drawn into the page layer
    /// the screen in that room samples.
    ///
    /// **A second renderer rather than a second pass, and the reason is the
    /// engine's own shape.** `ForwardRenderer::add_passes` takes `&'a mut self`
    /// against the graph's own lifetime, so two calls on one renderer do not
    /// compile at all — and if they did, both would import the one shadow atlas
    /// and the one set of per-frame culling buffers twice, which gives the graph
    /// two independent histories of each and no barrier between the halves.
    /// `begin_frame` is the other half of the argument: it writes *one* camera
    /// into the frame's uniform slot and freezes one resolved effect set, which
    /// is exactly the state a second view needs its own copy of.
    ///
    /// What that costs is a second copy of everything the room makes resident —
    /// the geometry and cluster pools, the material table, the page, the
    /// instance ring — and a second shadow atlas. For a room of this size that
    /// is small; for a scene where it is not, the answer is a view
    /// parameter inside `crcbl-render` rather than a renderer per camera, and
    /// `docs/backlog.md` is where that is written down.
    monitor: ForwardRenderer,
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
        label: "lantern",
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
            crcbl::log::info!("lantern: device granted every optional feature asked for");
        } else {
            crcbl::log::info!("lantern: {report}");
        }
        // One description, two renderers: the room the player sees and the room
        // the monitor sees are the same room, and a second description would be
        // a second place for it to drift.
        let scene = room::room();
        let mut renderer =
            ForwardRenderer::with_scene(ctx.device(), ctx.queue(), ctx.format(), &scene)?;
        // The monitor's view renders into the page's own format rather than the
        // swapchain's, because where it ends up is a layer of that page — see
        // [`MONITOR_FORMAT`].
        let mut monitor =
            match ForwardRenderer::with_scene(ctx.device(), ctx.queue(), MONITOR_FORMAT, &scene) {
                Ok(monitor) => monitor,
                Err(error) => {
                    renderer.destroy(ctx.device());
                    return Err(GpuError::Hal(error));
                }
            };
        // Every object each view draws, in one fixed order — see `room::place`,
        // and `room::Seen` for why the monitor's own renderer is given fewer of
        // them than the main one.
        let placed = match room::place(ctx.device(), ctx.queue(), &mut renderer, room::View::Main)
            .and_then(|placed| {
                room::place(ctx.device(), ctx.queue(), &mut monitor, room::View::Monitor)
                    .map(|_| placed)
            }) {
            Ok(placed) => placed,
            Err(error) => {
                monitor.destroy(ctx.device());
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(crcbl::hal::HalError::InvalidDescriptor(
                    format!("lantern's room does not fit its own instance pool: {error}"),
                )));
            }
        };
        crcbl::log::info!("lantern: {placed} object(s) placed in the room");

        // Topic 39's resolution order, with two of its layers written here: the
        // camera stack is the view's and the programmatic override is the run's
        // — see [`request_for`].
        let video = ctx.video_effects();
        let antialiasing = ctx.antialiasing();
        renderer.set_effect_request(request_for(room::View::Main, video, antialiasing, effects));
        monitor.set_effect_request(request_for(
            room::View::Monitor,
            video,
            antialiasing,
            effects,
        ));
        // Resolved rather than requested: the device clamps last, so what the
        // panel and the summary report has to come back off the renderer.
        let paths = Paths::of(
            &caps,
            forced,
            renderer.resolved_effects(),
            monitor.resolved_effects(),
        );
        crcbl::log::info!(
            "lantern: {:?} / {:?} / {:?}, effects {}, monitor {}",
            paths.geometry,
            paths.binding,
            paths.lighting,
            paths.effects_row(),
            paths.monitor_row(),
        );

        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, LANTERN_TIMED_PASSES);
        if timers.is_none() {
            crcbl::log::info!("hal: no timestamp queries on this device; per-pass timing is off");
        }
        // Rolled back by hand: `Gpu` has no `Drop`, so a `?` here would leak the
        // forward renderer's pipelines rather than release them.
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), ctx.format()) {
            Ok(ui) => ui,
            Err(error) => {
                monitor.destroy(ctx.device());
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };
        let menu = match MenuRenderer::new(ctx.device(), ctx.queue(), ctx.format()) {
            Ok(menu) => menu,
            Err(error) => {
                ui.destroy(ctx.device());
                monitor.destroy(ctx.device());
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        Ok(Self {
            ctx,
            renderer,
            monitor,
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
        // **The camera layer is not the caller's to move.** What arrives here is
        // the main view's request with one of the *other* layers edited — the
        // pause menu writes the programmatic one and leaves the rest alone — so
        // each renderer takes it with its own view's stack put back. Without
        // that, the first menu press would hand the monitor the main view's
        // stack and the camera layer would quietly stop existing.
        self.renderer.set_effect_request(EffectRequest {
            camera: room::View::Main.stack(),
            ..request
        });
        self.monitor.set_effect_request(EffectRequest {
            camera: room::View::Monitor.stack(),
            ..request
        });
        self.paths.effects = self.renderer.resolved_effects();
        self.paths.monitor_effects = self.monitor.resolved_effects();
    }

    /// Which debug channel the two views are drawing, if any.
    ///
    /// The main renderer's answer — the monitor is handed the same view by
    /// `GameGpu::set_debug_view`, so one of them is the pair's. Read back off
    /// [`ForwardRenderer::debug_view`], which resolves five independent switches
    /// by precedence, rather than kept beside them: what a row and the console
    /// both write is `crcbl::debug_view`, and this is what the frame actually
    /// came out as.
    #[must_use]
    pub const fn debug_view(&self) -> crcbl::render::DebugView {
        self.renderer.debug_view()
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
            .plus(self.monitor.counters())
            .plus(self.menu.counters())
            .plus(self.ui.counters())
    }

    /// The `[engine.video]` section this bundle's context read while opening.
    ///
    /// Forwarded rather than answered, so a run reports the player's file
    /// rather than a default — see [`crcbl::engine::GameGpu::video`].
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

        // The room's punctual lights, both views from the one list — see
        // `room::lights`. Only the lamp in it moves; the objects were placed
        // once and never rewritten, so a still frame uploads no instance bytes
        // at all, because a light is a row of the light list and not an
        // instance.
        self.renderer.set_lights(&room::lights(self.elapsed));
        self.renderer
            .begin_frame(self.ctx.device(), &self.camera, &room::sun(), extent)?;
        // The monitor's view of the same room at the same instant: the same
        // lights, the same sun, its own camera and its own extent. Its own
        // `begin_frame` because that call writes one camera into the frame's
        // uniform slot and freezes one resolved effect set — which is what
        // makes two views two renderers.
        self.monitor.set_lights(&room::lights(self.elapsed));
        self.monitor.begin_frame(
            self.ctx.device(),
            &room::monitor_camera(),
            &room::sun(),
            room::MONITOR_EXTENT,
        )?;
        self.menu
            .begin_frame(self.ctx.device(), extent)
            .map_err(GpuError::Hal)?;
        self.ui
            .begin_frame(self.ctx.device(), &self.draw_list, &self.atlas, 1.0)
            .map_err(GpuError::Hal)?;

        let format = self.ctx.format();
        // Read before the renderer is borrowed for the graph, and a `Copy`
        // value, so the copy below can name the page the *main* renderer's
        // materials sample. The monitor renderer has a page of its own and
        // nothing writes it: nothing in the monitor's view samples that layer,
        // because `room::Seen` keeps the screen out of that view entirely.
        let page = self.renderer.base_color_page_import();
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
            // menu's scrim dims what is already in the target and the overlay
            // has to stay readable over both.
            self.menu.add_pass(&mut graph, target);
            self.ui.add_pass(&mut graph, target, extent);
            feed_monitor(&mut graph, &self.pool, &mut self.monitor, page);
            graph.compile(&self.pool)?
        };

        #[cfg(test)]
        {
            self.last_dump = compiled.dump();
        }
        if !self.dumped {
            crcbl::log::debug!("render graph for the lantern frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("lantern frame"),
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
        self.monitor.destroy(self.ctx.device());
        self.renderer.destroy(self.ctx.device());
        self.ctx.destroy()
    }
}

/// Draws the monitor's view and copies it onto the screen in the room.
///
/// # It is the tail of the frame, and the screen is one frame behind
///
/// **A decision, and it stays one.** What the screen shows is the previous
/// frame's view of the room, which is what a monitor on a wall shows anyway; the
/// goldens are blessed against it. Declaring the copy earlier would put this
/// frame's view on the screen at the cost of the monitor renderer's whole pass
/// list running before the room's, and nothing asks for that.
///
/// What is no longer load-bearing is the *ordering*.
/// [`ForwardRenderer::add_passes`] now imports the page and declares a read of
/// it on every pass that binds it, so the graph has the edge and works the
/// barrier out from the declarations wherever this pass sits. This function
/// imports the same handle and therefore gets the same
/// [`crcbl::render::ImageId`] and the same state tracker — see
/// [`ForwardRenderer::base_color_page_import`], which is where the declaration
/// the two share comes from, and which is why nothing here restates the page's
/// format or extent.
fn feed_monitor<'a>(
    graph: &mut RenderGraph<'a>,
    pool: &TransientPool,
    monitor: &'a mut ForwardRenderer,
    page: ImportedImage,
) {
    let (width, height) = room::MONITOR_EXTENT;
    // `TRANSFER_SRC` beside the attachment usage is the whole of what this
    // description adds over an ordinary render target: the frame is copied out
    // of it rather than sampled, so it never needs `SAMPLED`.
    let view = graph.create_image(
        "monitor-view",
        TransientImageDesc::new(
            room::MONITOR_EXTENT,
            MONITOR_FORMAT,
            ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::TRANSFER_SRC),
        ),
    );
    let _hdr = monitor.add_passes(graph, pool, view, room::MONITOR_EXTENT);

    // **The main renderer's own declaration, not a second one that agrees with
    // it.** `add_passes` has already imported this handle, so this call returns
    // the id it issued and the two accesses share one state tracker — which is
    // what gives the copy below a barrier out of the passes that sampled the
    // page. The graph refuses a repeat import that disagrees in any field
    // (`GraphError::ImportDeclarationConflict`), so restating the format and the
    // extent here would be two constants that must stay equal by hand and a
    // frame that stops compiling the day they do not.
    let page_id = graph.import_image(ForwardRenderer::BASE_COLOR_PAGE_LABEL, page);
    // One layer of the page, so the barrier covers the monitor's texels and not
    // the floor's check beside them.
    let layer = ImageSubresourceRange {
        aspect: ImageAspect::COLOR,
        base_mip: 0,
        mip_count: 1,
        base_layer: room::MONITOR_LAYER,
        layer_count: 1,
    };
    graph
        .add_copy_pass("monitor-to-page")
        .use_image(view, ResourceState::TransferSrc)
        .use_subresource(page_id, ResourceState::TransferDst, layer)
        .execute(move |ctx| {
            let source = ctx.image(view);
            let destination = ctx.image(page_id);
            let plane = |base_layer| ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer,
                layer_count: 1,
            };
            ctx.encoder().copy_image_to_image(&ImageCopy {
                src: source,
                src_subresource: plane(0),
                src_offset: Offset3d { x: 0, y: 0, z: 0 },
                dst: destination,
                dst_subresource: plane(room::MONITOR_LAYER),
                dst_offset: Offset3d { x: 0, y: 0, z: 0 },
                extent: Extent3d::d2(width, height),
            });
        });
}

// ---------------------------------------------------------------------------
// The engine's seams
// ---------------------------------------------------------------------------

// The forwards `crcbl::engine` calls this bundle through. Every one of
// them is a method above; the macro is what stops a sample forgetting one.
//
// `PolledGpu` is written out below instead of taken from
// `crcbl::impl_polled_gpu!`, because this sample's `request_open` takes its
// forced path and effect set as well.
/// The two seams `crcbl::settings::apply` reaches a renderer through.
///
/// A second inherent block rather than lines inside the one above, so the
/// forward `crcbl::impl_game_gpu!(Gpu, with_renderer)` picks up sits beside the
/// invocation that needs it. `docs/plan/52-debug-console.md` decision 3 is where
/// the pair comes from, and `crcbl::settings` holds both bodies — every bundle
/// with a `ForwardRenderer` writes exactly these two lines.
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
        // **Both renderers**, because the player's quality clamp is a fact
        // about the player rather than about a view: a monitor drawn at full
        // anisotropy inside a frame the player asked to be cheap is the setting
        // half-applied. Each renderer keeps its own `camera` layer — the monitor
        // asks for no reflections — because `apply_video_to` writes only the
        // `video` and `antialiasing` layers.
        crcbl::settings::apply_video_to(&mut self.renderer, self.ctx.device(), video)?;
        crcbl::settings::apply_video_to(&mut self.monitor, self.ctx.device(), video)
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
        crcbl::settings::set_debug_view_on(&mut self.monitor, view);
        Ok(())
    }
}

crcbl::impl_game_gpu!(Gpu, with_renderer);

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

    // Built from the window and the options alone: this sample's extra
    // arguments come from its own defaults, not from a caller.
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
        assert_eq!(asked.label, "lantern");

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
            monitor_effects: room::MONITOR_STACK,
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

    /// **The panel's two effect rows differ, and they differ by the camera
    /// layer.**
    ///
    /// The `effects` row is what the frame drew and the `monitor` row is what
    /// the monitor's view drew, both resolved through
    /// `EffectRequest::resolve` against the same device. A panel where the two
    /// read alike is a panel reporting a frame with one view in it, which is
    /// exactly the state this slice exists to leave behind.
    #[test]
    fn the_panel_reports_each_views_own_effect_set() {
        use crcbl::ui::{DebugModule, DebugSection};

        let device = RenderEffects::all();
        let all = RenderEffects::all();
        let main = request_for(room::View::Main, all, None, all).resolve(device);
        let monitor = request_for(room::View::Monitor, all, None, all).resolve(device);
        assert!(main.contains(RenderEffects::REFLECTIONS));
        assert!(
            !monitor.contains(RenderEffects::REFLECTIONS),
            "the monitor's camera stack has to survive the resolution order"
        );

        let paths = Paths {
            geometry: GeometryPath::MeshShader,
            binding: BindingModel::Bindless,
            lighting: LightingPath::Rasterised,
            forced: Forced::default(),
            effects: main,
            monitor_effects: monitor,
        };
        let mut section = DebugSection::new("");
        paths.debug_section(&mut section);
        let row = |label: &str| {
            section
                .rows()
                .iter()
                .find(|row| row.label == label)
                .unwrap_or_else(|| panic!("the panel has no {label} row"))
                .value
                .to_string()
        };
        assert_eq!(row("effects"), "shadows ao ssr aa vfog");
        assert_eq!(row("monitor"), "shadows ao aa");
    }

    /// **The player's `[engine.video]` clamp reaches both views, and the run's
    /// own flags cannot lift it.**
    ///
    /// This is the layer `GpuContext::video_effects` feeds, and the thing it
    /// has to survive is being handed to a run that asked for everything: a
    /// player who switched the shadows off in their settings file gets no
    /// shadows in the room *or* on the monitor, whatever the command line said.
    /// Without the argument reaching `EffectRequest::video` the frame is
    /// identical to a run with no settings file at all, which is the shape a
    /// wiring mistake takes here.
    #[test]
    fn the_players_video_clamp_reaches_both_views() {
        let device = RenderEffects::all();
        let all = RenderEffects::all();
        let video = all.difference(RenderEffects::SHADOWS);

        let main = request_for(room::View::Main, video, None, all).resolve(device);
        let monitor = request_for(room::View::Monitor, video, None, all).resolve(device);
        assert_eq!(
            main,
            room::View::Main.stack().difference(RenderEffects::SHADOWS),
            "the settings file's clamp has to reach the room"
        );
        assert_eq!(
            monitor,
            room::MONITOR_STACK.difference(RenderEffects::SHADOWS),
            "and the monitor, whose camera stack drops the reflections anyway"
        );
    }

    /// A settings store holding one `settings.toml`.
    fn settings_file(toml: &str) -> crcbl::store::MemoryStorage {
        use crcbl::store::StorageSource;

        let storage = crcbl::store::MemoryStorage::new();
        storage
            .write(
                std::path::Path::new(crcbl::store::settings::SETTINGS_FILE),
                toml.as_bytes(),
            )
            .expect("memory storage accepts every write");
        storage
    }

    /// The effects one whole start-up resolves to for both views, with
    /// `settings` standing in for the player's file.
    ///
    /// Through [`Gpu::from_context`] rather than around it. The test below it
    /// hands `request_for` its arguments by hand, which proves the resolution
    /// order and nothing about whether this sample passes
    /// [`GpuContext::video_effects`] into it — replacing that argument with
    /// [`RenderEffects::all`] used to red nothing at all.
    fn effects_opened_with(
        settings: crcbl::engine::SettingsSource<'_>,
    ) -> (RenderEffects, RenderEffects) {
        use crcbl::backend::GpuBackend;
        use crcbl::engine::{Clock, open_window, wait_for_configure};
        use crcbl::shell::{HeadlessShell, WindowDesc};

        let mut shell = HeadlessShell::new();
        let clock = Clock::new(true);
        let window = open_window(
            &mut shell,
            &clock,
            &WindowDesc {
                title: "lantern",
                app_id: "sh.kryptic.crcbl.lantern",
                ..WindowDesc::default()
            },
        )
        .expect("headless always creates a window");
        let mut events = 0;
        let extent =
            wait_for_configure(&mut shell, window, &mut events).expect("headless configures");

        let ctx = GpuContext::open(
            &shell,
            window,
            extent,
            &GpuContextDesc {
                // The null backend, so this needs no driver and no window
                // system — and this sample's own label and feature set, so it
                // is lantern's start-up being asked.
                backend: Some(GpuBackend::Null),
                settings,
                ..desc(GpuOptions::default(), Forced::default())
            },
        )
        .expect("the null backend opens everywhere");

        let gpu = Gpu::from_context(ctx, Forced::default(), RenderEffects::all())
            .expect("the null device builds lantern's renderers");
        let paths = gpu.paths();
        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
        (paths.effects, paths.monitor_effects)
    }

    /// **The player's `[engine.video]` clamp reaches the frames this sample
    /// draws**, both of them.
    ///
    /// The guard for one line: `let video = ctx.video_effects()` in
    /// [`Gpu::from_context`]. Replace it with [`RenderEffects::all`] and the
    /// run with no settings file is unchanged, which is why the control below
    /// cannot catch it and only a run with a real clamp in front of it can.
    ///
    /// One arm per effect, because a file that switched them all off resolves
    /// to the empty set however few of the keys were wired.
    #[test]
    fn the_players_video_clamp_reaches_the_frames() {
        let (main, monitor) = effects_opened_with(crcbl::engine::SettingsSource::None);
        assert_eq!(
            (main, monitor),
            (room::View::Main.stack(), room::MONITOR_STACK),
            "a run with no settings at all draws each view's own camera stack, \
             or the comparisons below are against the wrong control",
        );

        for (key, off) in [
            ("shadows", RenderEffects::SHADOWS),
            ("ambient_occlusion", RenderEffects::AMBIENT_OCCLUSION),
            ("reflections", RenderEffects::REFLECTIONS),
        ] {
            let storage = settings_file(&format!("[engine.video]\n{key} = false\n"));
            let (clamped, clamped_monitor) =
                effects_opened_with(crcbl::engine::SettingsSource::Source(&storage));
            assert_eq!(
                clamped,
                main.difference(off),
                "`{key} = false` did not reach the room this sample draws",
            );
            assert_eq!(
                clamped_monitor,
                monitor.difference(off),
                "`{key} = false` did not reach the monitor's own view",
            );
        }

        // **The antialiasing tier, which is the layer that replaces rather than
        // clamps** — the guard for `let antialiasing = ctx.antialiasing()`
        // beside the line above. Both views ask for FXAA by carrying
        // `DEFAULT_STACK`'s resolve bit, so a tier that never arrived leaves
        // them exactly as the control above, and only a file naming the *other*
        // rung can tell the two apart.
        let storage = settings_file("[engine.video]\nantialiasing = \"smaa\"\n");
        let (picked, picked_monitor) =
            effects_opened_with(crcbl::engine::SettingsSource::Source(&storage));
        let swapped = |stack: RenderEffects| {
            stack
                .difference(RenderEffects::ANTIALIASING)
                .union(RenderEffects::SMAA)
        };
        assert_eq!(
            picked,
            swapped(main),
            "`antialiasing = \"smaa\"` did not reach the room this sample draws",
        );
        assert_eq!(
            picked_monitor,
            swapped(monitor),
            "`antialiasing = \"smaa\"` did not reach the monitor's own view",
        );
    }

    /// **A run's `--no-*` flags reach both views, and the camera layer still
    /// separates them.**
    ///
    /// The programmatic layer is the run's and the camera layer is the view's,
    /// and the order is what keeps one from swallowing the other: a run that
    /// switched the shadows off gets a monitor with no shadows either, and a run
    /// that left everything on still gets a monitor with no reflections.
    #[test]
    fn the_run_s_own_flags_reach_the_monitor_and_the_camera_layer_survives_them() {
        let device = RenderEffects::all();
        let without_shadows = RenderEffects::all().difference(RenderEffects::SHADOWS);

        let all = RenderEffects::all();
        let main = request_for(room::View::Main, all, None, without_shadows).resolve(device);
        let monitor = request_for(room::View::Monitor, all, None, without_shadows).resolve(device);
        assert_eq!(
            main,
            room::View::Main.stack().difference(RenderEffects::SHADOWS)
        );
        assert_eq!(
            monitor,
            room::MONITOR_STACK.difference(RenderEffects::SHADOWS)
        );

        // And the override can move a decision *up* past the quality clamp
        // without reaching the camera's stack, which is the one direction the
        // layers are ordered to allow.
        let forced_on = EffectRequest {
            camera: room::View::Monitor.stack(),
            programmatic: EffectOverride::none().force(RenderEffects::REFLECTIONS, Some(true)),
            ..EffectRequest::default()
        };
        assert!(
            forced_on
                .resolve(device)
                .contains(RenderEffects::REFLECTIONS),
            "the programmatic layer is applied after the camera's and may restore an \
             effect it dropped — which is what makes the two layers rather than one"
        );
    }
}
