//! Breach's GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::map`], and the UI and menu passes rule 4 asks every sample for.
//!
//! Everything that is not this sample's — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — is [`crcbl::engine::GpuContext`]'s. What is here is the part
//! that is breach's: a renderer built from **this application's** scene
//! description rather than from [`ForwardRenderer::new`]'s demo one, the
//! instances in it that move — the range's three plates or the practice map's
//! three bots, which is what [`Movers`] picks between — and [`Paths`].
//!
//! # Rule 12, as a value rather than a log line
//!
//! `docs/plan/sample/00-samples-overview.md` rule 12 asks every sample to say
//! which of `docs/plan/39-capabilities.md`'s selectors its frames took, in the
//! debug panel **and** in the summary. [`Paths`] is that answer read once off
//! [`DeviceCaps`], so the panel, the `[HUD]` heartbeat and the summary line all
//! print the same three words rather than three readings that could disagree.
//! `apps/lantern` and `apps/quarry` carry richer versions of the same type;
//! this one has no `Forced` beside it because slice 1 ships no flag to hold a
//! path down — `docs/backlog.md` carries what rule 12 is still owed here.
//!
//! **The browser is the reason it is on the heartbeat.** A wasm build has no
//! ray query, no mesh stage and no bindless, so `IndirectPerBatch`,
//! `ArrayPages` and `Rasterised` are what a visitor's frame is drawn through by
//! construction — and `web/tools/browser-e2e.mjs` reads the line that names
//! them, which is the only place anything checks it.
//!
//! # Shadows are on because nothing turns them off
//!
//! A renderer nobody hands an [`EffectRequest`](crcbl::render::EffectRequest)
//! draws [`RenderEffects::DEFAULT_STACK`](crcbl::render::RenderEffects), which
//! is every effect that models the scene's own light transport. There is no
//! `set_effect_request` call in this file and no flag in front of one. What
//! casts is [`crate::map::house_light`], which in a room with a ceiling is
//! almost entirely ambient — see that function.
//!
//! # Pass order is declaration order
//!
//! The forward frame → `menu` → `ui`. The last two load the target rather than
//! clearing it, so declaring the UI pass first would put the pause panel on top
//! of the words it exists to frame.

use crcbl::engine::{
    FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions, PendingGpuContext,
};
use crcbl::hal::{
    BindingModel, CommandEncoderDesc, DeviceCaps, GeometryPath, HalError, LightingPath,
};
use crcbl::render::{
    Camera, ForwardRenderer, MAX_TIMED_PASSES, MenuRenderer, PassTimers, RenderGraph,
    TransientPool, UiRenderer,
};
use crcbl::shell::{Shell, WindowId};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

use crate::map::practice::{self, BotView};
use crate::map::{self, MapChoice};

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// Which of `docs/plan/39-capabilities.md`'s three selectors this device drew
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
        section.row("geometry", format_args!("{:?}", self.geometry));
        section.row("binding", format_args!("{:?}", self.binding));
        section.row("lighting", format_args!("{:?}", self.lighting));
    }
}

/// The instances a frame rewrites, which are the map's and differ between them.
///
/// One arm per [`MapChoice`], for [`crate::game::Scene`]'s reason: a range frame
/// has three plates that stand and fall, a practice frame has three bots that
/// walk, and neither has the other's handles to write.
#[derive(Debug)]
enum Movers {
    /// The range's target plates.
    Range(map::Targets),
    /// The practice map's bots.
    Practice(practice::Figures),
}

/// This sample's device, its swapchain and the three renderers it draws with.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    /// Whichever map this run opened, made resident once and drawn every frame.
    renderer: ForwardRenderer,
    /// The instances in it that are rewritten — see [`Movers`].
    movers: Movers,
    /// Which map that is, so the frame's ambient row and this bundle's own
    /// reports name the room they belong to.
    map: MapChoice,
    /// Which selectors this device drew through — see [`Paths`].
    paths: Paths,
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Where the frame is seen from. Written every frame by [`crate::app`],
    /// which owns the view; this is only where the frame reads it.
    camera: Camera,
    /// The menu pass: its own sheets, its own screen-space camera, and a pass
    /// that declares nothing on a frame with no menu on it.
    menu: MenuRenderer,
    /// UI compositing — the crosshair, the readout and the debug panel, in one
    /// list.
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
        label: "breach",
        // The engine's whole optional bundle, not a subset spelled out here: a
        // hand-written list is a copy, and a copy goes stale the moment
        // `GpuContextDesc::default` gains a flag.
        ..GpuContextDesc::from(gpu)
    }
}

/// The device request this bundle is waiting on, and which map it will make
/// resident when it arrives.
///
/// **The map is carried through the wait**, which is what
/// [`crcbl::engine::PolledGpu::Context`] exists for: a browser's device request
/// is a promise the page's own event loop resolves, and the choice of map was
/// made before `boot` was ever called. `apps/viewer` carries its document
/// through the same seam. Written out rather than taken from
/// `crcbl::impl_polled_bundle!`, because that macro is for a bundle with nothing
/// of its own to open with.
#[derive(Debug)]
pub struct PendingGpu {
    pending: PendingGpuContext,
    map: MapChoice,
}

impl PendingGpu {
    /// Advances the open. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the device request failed or a renderer refused the
    /// device it produced.
    pub fn poll(&mut self) -> Result<Option<Gpu>, GpuError> {
        match self.pending.poll()? {
            Some(ctx) => Gpu::from_context(ctx, self.map).map(Some),
            None => Ok(None),
        }
    }
}

impl Gpu {
    /// Opens a backend, a surface, a device and a swapchain, and makes `map`
    /// resident.
    ///
    /// **Blocks**, so this is the native path only; a browser calls
    /// [`request_open`](Self::request_open).
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened or any HAL call failed.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        map: MapChoice,
    ) -> Result<Self, GpuError> {
        Self::from_context(GpuContext::open(shell, window, extent, &desc(gpu))?, map)
    }

    /// Asks for a device and returns at once, keeping `map` for the build.
    ///
    /// The non-blocking half of [`Gpu::open`], routed through the same `desc` so
    /// the two paths cannot ask for different devices.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend could be opened at `extent`.
    pub fn request_open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        map: MapChoice,
    ) -> Result<PendingGpu, GpuError> {
        Ok(PendingGpu {
            pending: GpuContext::request_open(shell, window, extent, &desc(gpu))?,
            map,
        })
    }

    /// Builds this sample's renderers on an already-open context, and makes
    /// `map` resident.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the map's description is one the pools it asks for
    /// cannot hold, if the menu pass or the UI compositor refused the device, or
    /// if any HAL call failed.
    fn from_context(ctx: GpuContext, map: MapChoice) -> Result<Self, GpuError> {
        let format = ctx.format();
        let paths = Paths::of(&ctx.device().caps());
        let scene = match map {
            MapChoice::Range => map::scene(),
            MapChoice::Practice => practice::scene(),
        };
        let mut renderer = ForwardRenderer::with_scene(ctx.device(), ctx.queue(), format, &scene)?;
        // Rolled back by hand from here on: `Gpu` has no `Drop`, so a `?` would
        // leak the forward renderer's pipelines rather than release them.
        let placed = match map {
            MapChoice::Range => map::place(&mut renderer).map(Movers::Range),
            MapChoice::Practice => practice::place(&mut renderer).map(Movers::Practice),
        };
        let movers = match placed {
            Ok(movers) => movers,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(HalError::InvalidDescriptor(format!(
                    "breach's {} map does not fit its own pools: {error}",
                    map.name(),
                ))));
            }
        };
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        if timers.is_none() {
            crcbl::log::info!("hal: no timestamp queries on this device; per-pass timing is off");
        }
        let menu = match MenuRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(menu) => menu,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(ui) => ui,
            Err(error) => {
                menu.destroy(ctx.device());
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        crcbl::log::info!(
            "render: the {} map, geometry {:?}, binding {:?}, lighting {:?}",
            map.name(),
            paths.geometry,
            paths.binding,
            paths.lighting,
        );

        Ok(Self {
            ctx,
            renderer,
            movers,
            map,
            paths,
            pool: TransientPool::new(),
            timers,
            // Replaced before the first frame is recorded — `crate::app::draw`
            // writes it from wherever the simulation put the player — and a
            // camera rather than an `Option` because a frame must never be
            // drawn from nowhere.
            camera: Camera::default(),
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

    /// Which selectors this device drew through — rule 12's answer.
    #[must_use]
    pub const fn paths(&self) -> Paths {
        self.paths
    }

    /// The engine's context, for the run-level knobs that are not this
    /// sample's — `--screenshot` is the one that needs it.
    #[cfg(not(target_arch = "wasm32"))]
    pub const fn context_mut(&mut self) -> &mut GpuContext {
        &mut self.ctx
    }

    /// Where the next frame is seen from.
    pub const fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Which map this bundle made resident.
    #[must_use]
    pub const fn map(&self) -> MapChoice {
        self.map
    }

    /// Draws each plate where and how the simulation says.
    ///
    /// Written every frame rather than only when one changes, for
    /// `apps/puppet`'s reason: the frame is handed a snapshot, and a renderer
    /// that had to be told about an edge would need a second copy of the
    /// plates' state to compare against — and one of them travels anyway.
    ///
    /// A no-op on a bundle that opened the practice map, which has no plates in
    /// it — see [`Gpu::set_bots`] for the other half of the same pair.
    pub fn set_plates(&mut self, x: [f64; map::LANES], down: [bool; map::LANES]) {
        let Movers::Range(targets) = &self.movers else {
            return;
        };
        for (lane, (&x, &down)) in x.iter().zip(down.iter()).enumerate() {
            targets.set_plate(&mut self.renderer, lane, x, down);
        }
    }

    /// Draws each bot where and how the simulation says. The practice map's half
    /// of [`Gpu::set_plates`].
    pub fn set_bots(&mut self, bots: &[BotView; practice::BOTS]) {
        let Movers::Practice(figures) = &self.movers else {
            return;
        };
        for (index, view) in bots.iter().enumerate() {
            figures.set_bot(&mut self.renderer, index, view);
        }
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

    /// What the last [`Gpu::frame`] recorded, summed over the passes this
    /// bundle adds.
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.renderer
            .counters()
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
            .begin_frame(self.ctx.device(), &self.camera, &map::house_light(), extent)?;
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
            // Between the scene and the text: the menu's scrim dims what is
            // already in the target and the crosshair has to stay readable over
            // both.
            self.menu.add_pass(&mut graph, target);
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        // "The graph must be able to explain itself" — §2.4's debug-tools
        // principle.
        #[cfg(test)]
        {
            self.last_dump = compiled.dump();
        }
        if !self.dumped {
            crcbl::log::debug!("render graph for the breach frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("breach frame"),
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

// The forwards `crcbl::engine` calls this bundle through. Every one of them is
// a method above; the macro is what stops a sample forgetting one.
crcbl::impl_game_gpu!(Gpu);

/// Lets [`crcbl::engine::PolledBoot`] drive this bundle's arrival.
///
/// Written out rather than taken from `crcbl::impl_polled_gpu!` because that
/// macro is for a bundle with no context: it declares `type Context = ()` and
/// calls a four-argument `request_open`. This one opens with a map.
impl crcbl::engine::PolledGpu for Gpu {
    type Pending = PendingGpu;

    /// Which map to make resident. See `PendingGpu`.
    type Context = MapChoice;

    fn request<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        map: Self::Context,
    ) -> Result<Self::Pending, GpuError> {
        Self::request_open(shell, window, extent, gpu, map)
    }

    fn poll_pending(pending: &mut Self::Pending) -> Result<Option<Self>, GpuError> {
        pending.poll()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The bundle this sample opens with is the engine's own.** A hand-written
    /// list is a copy, and a copy goes stale the moment
    /// [`GpuContextDesc::default`] gains a flag. The failure is silent both
    /// ways: the missing capability changes no picture, only whether the
    /// engine's pacing loop and display-timing query can reach anything — and
    /// here also which [`Paths`] the run reports.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engine_s_own() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, "breach");
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
        // fallbacks, which on this target are not a fallback at all.
        let browser = DeviceCaps {
            features: Features::empty(),
            limits: Limits::minimum(),
        };
        let browser_paths = Paths::of(&browser);
        assert_eq!(browser_paths.geometry, browser.geometry_path());
        assert_eq!(browser_paths.binding, browser.binding_model());
        assert_eq!(browser_paths.lighting, browser.lighting_path());
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

        // …and a device with the lot, which must select something else.
        let desktop = DeviceCaps {
            features: Features::all(),
            limits: Limits::minimum(),
        };
        let desktop_paths = Paths::of(&desktop);
        assert_ne!(
            desktop_paths, browser_paths,
            "every device reports the same three paths",
        );
        assert_eq!(
            rows_of(&desktop_paths),
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
    }
}
