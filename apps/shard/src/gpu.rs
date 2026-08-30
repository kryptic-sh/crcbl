//! Shard's GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::zone`], and the UI and menu passes rule 4 asks every sample for.
//!
//! Everything that is not this sample's — opening a backend, choosing an adapter
//! that can present, the swapchain, the frames-in-flight ring, resize and
//! teardown — is [`crcbl::engine::GpuContext`]'s. What is here is the part that
//! is shard's: a renderer built from **this application's** scene description,
//! the one instance in it that moves (the character), the light list [`crate::light`]
//! rebuilds every frame, and [`Paths`].
//!
//! # Rule 12, as a value rather than a log line
//!
//! `docs/plan/sample/00-samples-overview.md` rule 12 asks every sample to say
//! which of `docs/plan/39-capabilities.md`'s selectors its frames took, in the
//! debug panel **and** in the summary. [`Paths`] is that answer read once off
//! [`DeviceCaps`], so the panel, the `[HUD]` heartbeat and the summary line all
//! print the same words rather than three readings that could disagree.
//!
//! **This sample is where that matters most, and `docs/plan/sample/15-shard.md`
//! says so**: it is the one where the fallback paths carry real content. A wasm
//! build has no ray query, no mesh stage and no bindless, so `IndirectPerBatch`,
//! `ArrayPages` and `Rasterised` are what a visitor's frame is drawn through by
//! construction — which is the whole reason milestone 1 is built before the
//! native world rather than after it. `web/tools/browser-e2e.mjs` reads the line
//! that names them.
//!
//! # The effects are the four-layer request, resolved and reported
//!
//! [`crcbl::render::EffectRequest`] has four layers — the view's own stack, the
//! player's `[engine.video]` clamp, a programmatic override and the device —
//! and this sample sets the first from the engine's default stack and takes the
//! second from the context, exactly as `apps/quarry` does. What goes on the panel
//! and the heartbeat is [`ForwardRenderer::resolved_effects`], **not** the
//! request: a stack the device clamped would otherwise report as granted, and
//! this is the sample whose whole subject is which effects actually ran.
//!
//! `apps/lantern` carries a richer version of the same type, with a `Forced`
//! beside it; shard has no flag to hold a path down, and `docs/backlog.md`
//! carries what rule 12 is still owed here.
//!
//! # The light list is rebuilt every frame, and that is the flicker
//!
//! [`Gpu::set_lighting`] is called once a draw with the simulated seconds and
//! whether the torches are lit. [`crate::light::torches`] is a pure function of
//! those two, so the frame's lights are decided by the simulation's clock and by
//! one key — and nothing in this file decides how bright anything is.
//!
//! # Pass order is declaration order
//!
//! The forward frame → `menu` → `ui`. The last two load the target rather than
//! clearing it, so declaring the UI pass first would put the pause panel on top
//! of the words it exists to frame.

use crate::foe::FoeView;
use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions};
use crcbl::hal::{
    BindingModel, CommandEncoderDesc, DeviceCaps, GeometryPath, HalError, LightingPath,
};
use crcbl::math::DVec3;
use crcbl::render::{
    Camera, EffectRequest, ForwardRenderer, MAX_TIMED_PASSES, MenuRenderer, PassTimers,
    RenderEffects, RenderGraph, TransientPool, UiRenderer,
};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

use crate::light;
use crate::zone::{self, Figure, Foes};

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// Which of `docs/plan/39-capabilities.md`'s selectors this device drew through,
/// and which of topic 18's effects came out of the four-layer request — rule
/// 12's "says which it took", as a value.
///
/// The three selectors are read once at start-up because that is when they are
/// decided: they are a function of the device's capabilities and nothing in a run
/// changes them. The effect set is read off the renderer for the reason
/// `apps/lantern` gives — it is what came out of the layers rather than what was
/// asked for, and a request the device clamped would otherwise report as
/// granted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paths {
    /// The path the renderer's submission tail takes.
    pub geometry: GeometryPath,
    /// How the fragment stage addresses the base-colour page.
    pub binding: BindingModel,
    /// How indirect lighting is resolved.
    pub lighting: LightingPath,
    /// Which of topic 18's effects the frame draws, **resolved**.
    pub effects: RenderEffects,
}

impl Paths {
    /// What the device opened as, and what its renderer resolved.
    #[must_use]
    pub const fn of(caps: &DeviceCaps, effects: RenderEffects) -> Self {
        Self {
            geometry: caps.geometry_path(),
            binding: caps.binding_model(),
            lighting: caps.lighting_path(),
            effects,
        }
    }

    /// The effect set as one word per effect — [`RenderEffects::row`], so this
    /// sample and every other spell it the same way.
    #[must_use]
    pub fn effects_row(&self) -> String {
        self.effects.row()
    }

    /// What this sample's frames are, in the row the panel prints beside the
    /// three selectors.
    ///
    /// A constant rather than a reading: `docs/plan/sample/15-shard.md`'s
    /// milestone 1 is the **rasterised** twin under load, and its ray-traced half
    /// is milestone 2 on native. A row that read the device would say
    /// "`Rasterised`" on a machine that has ray query and imply the sample had
    /// chosen it.
    #[must_use]
    pub const fn ray_tracing_note() -> &'static str {
        "raster only (milestone 1)"
    }
}

impl crcbl::ui::DebugModule for Paths {
    fn debug_section(&self, section: &mut crcbl::ui::DebugSection) {
        section.set_title("paths");
        section.row("geometry", format_args!("{:?}", self.geometry));
        section.row("binding", format_args!("{:?}", self.binding));
        section.row("lighting", format_args!("{:?}", self.lighting));
        section.row_str("ray tracing", Self::ray_tracing_note());
        section.row_str("effects", &self.effects_row());
    }
}

/// This sample's device, its swapchain and the three renderers it draws with.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    /// The zone, made resident once and drawn every frame.
    renderer: ForwardRenderer,
    /// The one instance in it that is the character.
    figure: Figure,
    /// The other instances that move: one body per [`crate::foe::POSTS`] row.
    foes: Foes,
    /// Which selectors and effects this device drew through — see [`Paths`].
    paths: Paths,
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Where the frame is seen from. Written every frame by [`crate::app`], which
    /// owns the view; this is only where the frame reads it.
    camera: Camera,
    /// The menu pass: its own sheets, its own screen-space camera, and a pass that
    /// declares nothing on a frame with no menu on it.
    menu: MenuRenderer,
    /// UI compositing — the readout and the debug panel, in one list.
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
    /// The last frame's graph dump, kept only for the loop's own tests: it is how
    /// a test sees whether a pass was in the frame at all.
    #[cfg(test)]
    last_dump: String,
}

/// What both `Gpu::open` and `Gpu::request_open` ask the engine for.
///
/// One value rather than two copies, for the reason every sample gives: the two
/// bring-up paths must open the *same* device, or a feature only one of them
/// requested is a bug nobody sees until the other path runs — and here it would
/// also be a [`Paths`] that depends on which door the run came in through.
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "shard",
        // The engine's whole optional bundle, not a subset spelled out here: a
        // hand-written list is a copy, and a copy goes stale the moment
        // `GpuContextDesc::default` gains a flag.
        ..GpuContextDesc::from(gpu)
    }
}

crcbl::impl_polled_bundle!(gpu: Gpu, pending: PendingGpu, desc: desc);

impl Gpu {
    /// Builds this sample's renderers on an already-open context and makes the
    /// zone resident.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the zone's description is one the pools it asks for cannot
    /// hold, if the menu pass or the UI compositor refused the device, or if any
    /// HAL call failed.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let scene = zone::scene();
        let mut renderer = ForwardRenderer::with_scene(ctx.device(), ctx.queue(), format, &scene)?;
        // **The view's own stack, over the player's clamp.** `effect_request`
        // carries what `[engine.video]` allows; the camera layer is the engine's
        // default stack, which is every effect that models the scene's own light
        // transport. Slice 1 offers no way to change either — see the module docs.
        renderer.set_effect_request(EffectRequest {
            camera: RenderEffects::DEFAULT_STACK,
            ..ctx.effect_request()
        });
        let paths = Paths::of(&ctx.device().caps(), renderer.resolved_effects());
        // Rolled back by hand from here on: `Gpu` has no `Drop`, so a `?` would
        // leak the forward renderer's pipelines rather than release them.
        let placed = match zone::place(&mut renderer) {
            Ok(placed) => placed,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(HalError::InvalidDescriptor(format!(
                    "shard's zone does not fit its own pools: {error}"
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
            "render: geometry {:?}, binding {:?}, lighting {:?}, effects {}",
            paths.geometry,
            paths.binding,
            paths.lighting,
            paths.effects_row(),
        );

        Ok(Self {
            ctx,
            renderer,
            figure: placed.figure,
            foes: placed.foes,
            paths,
            pool: TransientPool::new(),
            timers,
            // Replaced before the first frame is recorded — `crate::app::draw`
            // writes it from wherever the simulation put the character — and a
            // camera rather than an `Option` because a frame must never be drawn
            // from nowhere.
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

    /// Which selectors and effects this device drew through — rule 12's answer.
    #[must_use]
    pub const fn paths(&self) -> Paths {
        self.paths
    }

    /// The engine's context, for the run-level knobs that are not this sample's —
    /// `--screenshot` is the one that needs it.
    #[cfg(not(target_arch = "wasm32"))]
    pub const fn context_mut(&mut self) -> &mut GpuContext {
        &mut self.ctx
    }

    /// Where the next frame is seen from.
    pub const fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Draws the character where the simulation says.
    ///
    /// Written every frame rather than only when it changes, for `apps/puppet`'s
    /// reason: the frame is handed a snapshot, and a renderer that had to be told
    /// about an edge would need a second copy of the character's position to
    /// compare against.
    pub fn set_figure(&mut self, feet: DVec3) {
        self.figure.set_feet(&mut self.renderer, feet);
    }

    /// Draws each foe where and how the simulation says. See
    /// [`Gpu::set_figure`] for the other half of the same pair, and
    /// [`crate::zone::foe_material_of`] for what a body's colour means.
    pub fn set_foes(&mut self, views: &[FoeView; crate::foe::FOES]) {
        for (index, view) in views.iter().enumerate() {
            self.foes.set(&mut self.renderer, index, view);
        }
    }

    /// Writes the zone's lights for the next frame: the torches at `seconds`,
    /// or only the shrine's spot when they have been put out.
    ///
    /// See [`crate::light::torches`], which is the whole of the decision — this
    /// call is the seam and nothing else.
    pub fn set_lighting(&mut self, seconds: f64, lit: bool) {
        self.renderer.set_lights(&light::torches(seconds, lit));
    }

    /// Takes this frame's draw list, handing the previous frame's allocation back
    /// so the caller can refill it instead of building a new one.
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
    /// *same* atlas the pass draws with or every measured string lands off by the
    /// difference.
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

        self.renderer.begin_frame(
            self.ctx.device(),
            &self.camera,
            &zone::house_light(),
            extent,
        )?;
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
            // already in the target and the readout has to stay legible over both.
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
            crcbl::log::debug!("render graph for the shard frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("shard frame"),
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
    /// [`GpuError`] if the reconfigure failed. A zero extent is *not* an error: a
    /// minimised window reports one and the swapchain is left alone.
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

// The forwards `crcbl::engine` calls this bundle through. Every one of them is a
// method above; the macro is what stops a sample forgetting one.
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

// …and the polled half, which is how a browser opens a device it may not block
// on. Shard opens with nothing of its own, so the generated `Context = ()` is
// the honest shape — `apps/breach` writes this out by hand because it carries a
// map choice through the wait.
crcbl::impl_polled_gpu!(gpu: Gpu, pending: PendingGpu);

#[cfg(test)]
mod tests {
    use super::*;

    /// **The bundle this sample opens with is the engine's own.** A hand-written
    /// list is a copy, and a copy goes stale the moment
    /// [`GpuContextDesc::default`] gains a flag. The failure is silent both ways:
    /// the missing capability changes no picture, only whether the engine's
    /// pacing loop and display-timing query can reach anything — and here also
    /// which [`Paths`] the run reports.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engine_s_own() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, "shard");
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
    /// same words whatever it was handed would pass against one.
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
        // fallbacks, which on this target are not a fallback at all — and which
        // `docs/plan/sample/15-shard.md` names as the whole point of milestone 1.
        let browser = DeviceCaps {
            features: Features::empty(),
            limits: Limits::minimum(),
        };
        let stack = RenderEffects::DEFAULT_STACK;
        let browser_paths = Paths::of(&browser, stack);
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
                (
                    "ray tracing".to_string(),
                    Paths::ray_tracing_note().to_string()
                ),
                ("effects".to_string(), stack.row()),
            ],
        );

        // …and a device with the lot, which must select something else.
        let desktop = DeviceCaps {
            features: Features::all(),
            limits: Limits::minimum(),
        };
        let desktop_paths = Paths::of(&desktop, stack);
        assert_ne!(
            desktop_paths, browser_paths,
            "every device reports the same three paths",
        );
    }

    /// **The effect row is the resolved set and not the request**, which is the
    /// distinction this whole sample is about: a stack the device clamped must
    /// report as clamped.
    ///
    /// The control is a second set with an effect missing, which has to print
    /// differently — a row that spelled `DEFAULT_STACK` whatever it held would
    /// pass against one reading.
    #[test]
    fn the_effect_row_says_what_the_frame_resolved() {
        use crcbl::hal::{Features, Limits};

        let caps = DeviceCaps {
            features: Features::empty(),
            limits: Limits::minimum(),
        };
        let full = Paths::of(&caps, RenderEffects::DEFAULT_STACK);
        assert_eq!(full.effects_row(), RenderEffects::DEFAULT_STACK.row());
        assert!(full.effects_row().contains("shadows"));

        let clamped = Paths::of(
            &caps,
            RenderEffects::DEFAULT_STACK.difference(RenderEffects::REFLECTIONS),
        );
        assert_ne!(
            clamped.effects_row(),
            full.effects_row(),
            "a clamped stack prints as the whole one",
        );
        assert!(!clamped.effects_row().contains("ssr"));

        // And an empty set is a word rather than an empty string, so a heartbeat
        // that names it cannot be read as a missing field.
        assert_eq!(
            Paths::of(&caps, RenderEffects::empty()).effects_row(),
            "none",
        );
    }
}
