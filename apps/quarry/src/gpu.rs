//! Quarry's GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::dag`], and the UI and menu passes rule 4 asks every sample for.
//!
//! Everything that is not this sample's is [`crcbl::engine::GpuContext`]'s —
//! opening a backend, choosing an adapter, the swapchain, the frames-in-flight
//! ring, resize and teardown. What is here is the part that is quarry's: a
//! renderer built over the **cluster DAG** rather than over the flat mesh, and
//! the capability report rule 12 asks for.
//!
//! # The DAG, not the flat scene
//!
//! [`crate::scene::quarry_scene`] describes the face as one flat mesh, drawn at
//! full detail from every camera. That is the right shape for milestone 1's
//! residency proof and the wrong one for a window: this sample's whole subject
//! is per-cluster LOD, and a flat mesh has no cut to select. So the window
//! makes [`crate::dag::dag_scene`] resident, exactly as
//! `tests/device/harness.rs` does for `Levels::Dag`, with one instance at
//! [`Mat4::IDENTITY`](crcbl::math::Mat4::IDENTITY).
//!
//! # Forcing a lesser path is done by not asking for a feature
//!
//! `docs/plan/sample/00-samples-overview.md` rule 12: "every sample accepts a
//! flag forcing a lesser path". There is no switch on the renderer to do it
//! with, and there should not be — the selectors are computed from what the
//! *device* has ([`crcbl::hal::DeviceCaps::geometry_path`] and its two
//! siblings), so the honest way to reach a lesser one is to open a device
//! without the feature that selects the better one. [`Forced`] is that, and
//! [`Paths`] is what says which arm the frame actually took. This is the sample
//! where it matters most: three paths is the widest selector in the engine.
//!
//! # There is no sprite pass
//!
//! `apps/hud/src/gpu.rs`'s shape, and its argument: rule 11's `.crpix` art
//! would be showing the wrong system. `docs/plan/sample/14-quarry.md` exempts
//! this sample by name.

pub use crcbl::engine::{FrameOutcome, GpuError};

use crcbl::engine::{GpuContext, GpuContextDesc, GpuOptions, PendingGpuContext};
use crcbl::hal::{
    BindingModel, CommandEncoderDesc, DeviceCaps, Features, GeometryPath, LightingPath, downgrades,
};
use crcbl::prelude::*;
use crcbl::render::{
    CullStats, DirectionalLight, ForwardRenderer, MAX_TIMED_PASSES, MenuRenderer, PassTimers,
    RenderGraph, TransientPool, UiRenderer,
};
use crcbl::shell::WindowId;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

use crate::{camera, dag, face};

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// Quads per side of the face the window renders.
///
/// **Measured rather than chosen.** Generating the face and coarsening it into
/// the cluster DAG — `face::quarry_face` then `dag::dag_scene`, which is all of
/// what a run waits for before its first frame — takes, on this machine:
///
/// ```text
///           debug     release
///    64     0.43 s     0.04 s     8,192 triangles
///   128     1.87 s     0.17 s    32,768 triangles
///   256     8.01 s     0.63 s   131,072 triangles
/// ```
///
/// 256 is `src/main.rs`'s figure and it is eight seconds of black screen in the
/// build a reviewer actually runs. 64 is the device suite's, chosen there
/// because those tests assert acceptance rather than look at anything. 128 is
/// the windowed answer: dense enough that the DAG has several levels and the
/// cut visibly mixes them, and under two seconds to start even unoptimised.
///
/// A constant rather than a `--cells` flag, because the choice is not per-run:
/// the goldens, the device suite and the charter all pin their own sizes, and a
/// knob here would only produce frames that cannot be compared with any of
/// them.
pub const CELLS: u32 = 128;

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
        // device without it culls no clusters — and per-cluster culling is this
        // sample's own subject, which makes "the best path, completely" the
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
/// debug panel and the headless summary read the same answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paths {
    /// The path the renderer's submission tail actually takes. **The one this
    /// sample exists to report**: the mesh path selects a level per cluster and
    /// the two indirect paths one per instance, so this row is what a frame's
    /// cut has to be read against.
    pub geometry: GeometryPath,
    /// How the fragment stage addresses the base-colour page.
    pub binding: BindingModel,
    /// How indirect lighting is resolved. `Rasterised` on every device today —
    /// see [`Paths::ray_tracing_note`].
    pub lighting: LightingPath,
    /// What the run asked to be held down.
    pub forced: Forced,
}

impl Paths {
    /// What the device opened as, beside what the run asked for.
    #[must_use]
    pub const fn of(caps: &DeviceCaps, forced: Forced) -> Self {
        Self {
            geometry: caps.geometry_path(),
            binding: caps.binding_model(),
            lighting: caps.lighting_path(),
            forced,
        }
    }

    /// Why [`LightingPath::RayTraced`] never appears, in one line for the panel.
    ///
    /// **Not a device answer.** `crcbl-vk` can report `RAY_QUERY` and
    /// `ACCELERATION_STRUCTURE`, so the selector would choose it — but nothing
    /// in `crcbl-render` builds an acceleration structure or traces one, so a
    /// run that selected it would draw the rasterised frame and say it had done
    /// something else. The panel says so rather than implying a choice was
    /// made.
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
    }
}

/// The sun the window lights the face with.
///
/// [`DirectionalLight::default`] rather than the goldens' own fixed sun. The
/// two are deliberately different owners: `tests/device/goldens.rs` pins its
/// direction so a change to the engine's default fails there instead of quietly
/// re-blessing six images, and the window takes whatever the engine's default
/// currently is — which is what makes a windowed frame a look at the engine's
/// own lighting rather than at a number this sample chose.
fn sun() -> DirectionalLight {
    DirectionalLight::default()
}

/// Quarry's GPU side.
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
    /// How many triangles the face this renderer holds is made of, at level 0.
    triangles: usize,
    /// The screen-space error budget the cut is selected under, in pixels.
    ///
    /// Kept here because [`ForwardRenderer`] has no getter for it — the panel
    /// and the summary both name the budget a frame was drawn at, and a report
    /// that restated the command line's value instead would be a copy.
    lod_budget: f32,
    /// Where the frame is seen from. Written every tick by [`crate::app`], which
    /// owns the camera; this is only where the frame reads it.
    camera: crcbl::render::Camera,
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
/// draw the face through different selectors.
fn desc(gpu: GpuOptions, forced: Forced) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "quarry",
        optional_features: forced.optional_features(),
        ..GpuContextDesc::from(gpu)
    }
}

/// A [`Gpu`] being opened one poll at a time.
///
/// It carries the three settings the device request outlives: all of them are
/// read again by [`Gpu::from_context`] once the device arrives, and none of them
/// is anything the engine's polled bring-up knows about.
#[derive(Debug)]
pub struct PendingGpu {
    pending: PendingGpuContext,
    forced: Forced,
    lod_budget: f32,
    lod_view: bool,
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
            Some(ctx) => {
                Gpu::from_context(ctx, self.forced, self.lod_budget, self.lod_view).map(Some)
            }
            None => Ok(None),
        }
    }
}

impl Gpu {
    /// Opens the join and builds the forward renderer over the face's cluster
    /// DAG.
    ///
    /// `extent` must come from the window system — call this only after the
    /// first configure.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened, if the backend exposes no adapter, no
    /// graphics queue or no surface format, if the face will not coarsen, if the
    /// levelled scene is one the pools it asks for cannot hold, or if any HAL
    /// call fails.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        forced: Forced,
        lod_budget: f32,
        lod_view: bool,
    ) -> Result<Self, GpuError> {
        Self::from_context(
            GpuContext::open(shell, window, extent, &desc(gpu, forced))?,
            forced,
            lod_budget,
            lod_view,
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
        lod_budget: f32,
        lod_view: bool,
    ) -> Result<PendingGpu, GpuError> {
        Ok(PendingGpu {
            pending: GpuContext::request_open(shell, window, extent, &desc(gpu, forced))?,
            forced,
            lod_budget,
            lod_view,
        })
    }

    /// Builds the renderer, the face and the two UI passes on an already-open
    /// context — everything both bring-up paths share once the device exists.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the face will not coarsen, if the levelled scene is one
    /// the pools it asks for cannot hold, or if any HAL call fails.
    fn from_context(
        ctx: GpuContext,
        forced: Forced,
        lod_budget: f32,
        lod_view: bool,
    ) -> Result<Self, GpuError> {
        let optional_features = forced.optional_features();
        let caps = ctx.device().caps();
        // Topic 39's "every downgrade is logged once, at device creation,
        // naming the feature and the path it selected" — including the ones
        // `--force-*` asked for, which are downgrades this run made on purpose
        // and which the line above would otherwise be silent about.
        let report = downgrades(optional_features, &caps);
        if report.is_empty() {
            crcbl::log::info!("quarry: device granted every optional feature asked for");
        } else {
            crcbl::log::info!("quarry: {report}");
        }

        let face = face::quarry_face(CELLS);
        let triangles = face.triangles();
        let scene = dag::dag_scene(&face).map_err(|error| {
            GpuError::Hal(crcbl::hal::HalError::InvalidDescriptor(format!(
                "quarry's face does not coarsen into a cluster DAG: {error}"
            )))
        })?;
        let mut renderer =
            ForwardRenderer::with_scene(ctx.device(), ctx.queue(), ctx.format(), &scene)?;
        // One instance at the origin, which is `tests/device/harness.rs`'s
        // shape: the subject is one mesh whose *own surface* spans levels, so a
        // second copy of it would add instance culling to a picture about
        // cluster culling.
        if let Err(error) = renderer.add_instance(&crcbl::render::scene::InstanceDesc {
            mesh: 0,
            material: 0,
            transform: crcbl::math::Mat4::IDENTITY,
        }) {
            renderer.destroy(ctx.device());
            return Err(GpuError::Hal(crcbl::hal::HalError::InvalidDescriptor(
                format!("quarry's face does not fit its own instance pool: {error}"),
            )));
        }
        renderer.set_lod_error_budget(lod_budget);
        renderer.set_lod_view(lod_view);

        let paths = Paths::of(&caps, forced);
        crcbl::log::info!(
            "quarry: {:?} / {:?} / {:?}, {triangles} triangles at a {lod_budget}px budget",
            paths.geometry,
            paths.binding,
            paths.lighting,
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
            triangles,
            lod_budget,
            camera: camera::dolly(camera::DOLLY_START),
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

    /// Triangles in the face this renderer holds, at level 0.
    ///
    /// The DAG's coarser levels hold fewer, so this is the count a cut is a
    /// reduction *of* rather than the count any one frame drew — which is what
    /// `docs/plan/sample/14-quarry.md`'s "triangle count ... at a stated camera
    /// position" is measured against.
    #[must_use]
    pub const fn triangles(&self) -> usize {
        self.triangles
    }

    /// The screen-space error budget this run **asked** for, in pixels.
    ///
    /// What the command line said. [`frame_lod_budget`](Self::frame_lod_budget)
    /// is what a frame was actually selected under, and the two are what tell a
    /// setting that reached the renderer from one that stopped here.
    #[must_use]
    pub const fn lod_budget(&self) -> f32 {
        self.lod_budget
    }

    /// The budget the **last frame** handed the descent, in pixels.
    ///
    /// Read back off the renderer —
    /// [`ForwardRenderer::lod_params`](crcbl::render::ForwardRenderer::lod_params)
    /// is what `begin_frame` wrote — rather than restated from the request, so a
    /// budget that never reached a frame reports as the renderer's own default
    /// instead of as the number that was asked for. `0.0` before the first
    /// frame, which is the whole of what the renderer has written by then.
    #[must_use]
    pub const fn frame_lod_budget(&self) -> f32 {
        self.renderer.lod_params()[1]
    }

    /// Whether the colour pass tints each cluster by its DAG level.
    #[must_use]
    pub const fn lod_view(&self) -> bool {
        self.renderer.lod_view()
    }

    /// Turns the LOD tint on or off, from the pause menu's row.
    pub const fn set_lod_view(&mut self, on: bool) {
        self.renderer.set_lod_view(on);
    }

    /// What the last completed frame's culling kept.
    ///
    /// [`None`] until the readback ring has come round, which is a few frames —
    /// see [`crcbl::render::CullStatsRing`]. The panel says which frame the
    /// numbers are from rather than printing them as this one's.
    #[must_use]
    pub fn cull_stats(&self) -> Option<CullStats> {
        self.renderer.cull_stats()
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

        self.renderer
            .begin_frame(self.ctx.device(), &self.camera, &sun(), extent)?;
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
            // Between the face and the text, on `apps/sandbox`'s terms: the
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
            crcbl::log::debug!("render graph for the quarry frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("quarry frame"),
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
// forced path and its two LOD settings as well.
crcbl::impl_game_gpu!(Gpu);

/// Lets [`crcbl::engine::PolledBoot`] drive this bundle's arrival.
///
/// The extent and the resize are [`crcbl::engine::GpuSurface`]'s, because a
/// running loop asks the same two.
///
/// **Where the flags go.** [`Gpu::request_open`] takes the three this sample
/// adds to a device request and the trait's `request` does not, so this forwards
/// what [`Options`](crate::Options)'s `Default` gives — read off the defaults
/// rather than spelled again, so the two cannot drift. That is the whole truth
/// about the polled path and not a shortcut: it exists for the browser, a page
/// has no argv, and `--force-geometry` and `--lod-budget` therefore have no
/// source there. A caller that has flags to honour has the blocking
/// [`Gpu::open`], which takes them.
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
            defaults.lod_budget,
            defaults.lod_view,
        )
    }

    fn poll_pending(pending: &mut Self::Pending) -> Result<Option<Self>, GpuError> {
        pending.poll()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **An unforced run opens the engine's own bundle**, plus the
    /// amplification stage this sample is about.
    ///
    /// The other samples assert equality with [`GpuContextDesc::default`]'s
    /// optional set. This one cannot: it overrides the field, and it
    /// deliberately adds `TASK_SHADER` on top — so the assertion is
    /// **containment**, which is the part that catches the real failure. If
    /// [`Forced::optional_features`] were ever rewritten as a hand-written list
    /// instead of a subtraction from the engine's default, it would go stale
    /// the moment the engine asked for one more flag, and nothing would say so.
    ///
    /// `TASK_SHADER` is asserted separately rather than folded into the
    /// expected set, so that dropping it fails here instead of being absorbed
    /// by a superset check. Losing it is the failure that would matter most
    /// here: no amplification stage means no per-cluster cut and no cluster
    /// count on the panel, and the frame would look identical.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engine_s_own() {
        let asked = desc(GpuOptions::default(), Forced::default());
        assert_eq!(asked.label, "quarry");

        let engine = GpuContextDesc::default().optional_features;
        assert!(
            asked.optional_features.contains(engine),
            "an unforced run must ask for at least the engine's own set; missing {:?}",
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
            let features = Forced {
                geometry: Some(want),
                binding: None,
            }
            .optional_features();
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

        // And both at once, which is the combination the Pages demo runs and a
        // `--force-geometry indirect-per-batch --force-binding array-pages` run
        // reproduces on this desktop.
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
    /// distinction rule 12's flag exists to make — and in this sample the
    /// geometry row is the whole report.
    #[test]
    fn the_panel_distinguishes_a_forced_path_from_a_devices_own() {
        use crcbl::ui::{DebugModule, DebugSection};

        let device_chose = Paths {
            geometry: GeometryPath::IndirectCount,
            binding: BindingModel::ArrayPages,
            lighting: LightingPath::Rasterised,
            forced: Forced::default(),
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
        assert!(
            plain.iter().any(|(label, _)| label == "ray tracing"),
            "the panel must say why RayTraced never appears: {plain:?}"
        );
    }
}
