//! Sparks' GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::stage`], and the UI and menu passes rule 4 asks every sample for.
//!
//! Everything that is not this sample's — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — is [`crcbl::engine::GpuContext`]'s. What is here is the part
//! that is sparks': a renderer built from **this application's** scene
//! description, and the several hundred instances in it that are rewritten
//! every frame.
//!
//! # There is no particle pass, and that is the design
//!
//! `docs/plan/20-particles.md` puts mesh particles on the stage 3 instance
//! path, so a particle is an ordinary instance and this file adds no pass of
//! its own for them. The cost of a frame's worth of effects is
//! [`ForwardRenderer::set_instance`] per live particle — a host-side write into
//! a pool whose dirty slots coalesce into runs, so a block's worth of particles
//! is one upload rather than one per particle, and nothing is recorded per
//! object at all.
//!
//! # Pass order is declaration order
//!
//! The forward frame → `menu` → `ui`. The last two load the target rather than
//! clearing it, so declaring the UI pass first would put the pause panel on top
//! of the words it exists to frame.

use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions};
use crcbl::hal::{CommandEncoderDesc, HalError};
use crcbl::render::{
    Camera, ForwardRenderer, MAX_TIMED_PASSES, MenuRenderer, PassTimers, RenderGraph,
    TransientPool, UiRenderer,
};
use crcbl::shaders::mesh::GpuMaterial;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

use crate::effects;
use crate::show::Show;
use crate::stage;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// This sample's device, its swapchain and the three renderers it draws with.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    /// The stage, made resident once, and the three particle blocks over it.
    renderer: ForwardRenderer,
    drawn: stage::Drawn,
    /// The baked colour rows, one palette per effect, in the order
    /// [`stage::scene`] appended them.
    ///
    /// Held here rather than re-baked per frame: `effects::nearest_row` reads
    /// them once per live particle, and evaluating three gradients every frame
    /// to hand it the same answer would be work with no output.
    palettes: Palettes,
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Where the frame is seen from. Written every frame by [`crate::app`],
    /// which owns the camera; this is only where the frame reads it.
    camera: Camera,
    /// The menu pass: its own sheets, its own screen-space camera, and a pass
    /// that declares nothing on a frame with no menu on it.
    menu: MenuRenderer,
    /// UI compositing — the overlay and the debug panel, in one list.
    ui: UiRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    dumped: bool,
    /// The last frame's graph dump, kept only for the loop's own tests: it is
    /// how a test sees whether a pass was in the frame at all.
    #[cfg(test)]
    last_dump: String,
}

/// One baked palette per effect.
#[derive(Debug)]
struct Palettes {
    sparks: Vec<GpuMaterial>,
    puff: Vec<GpuMaterial>,
    spam: Vec<GpuMaterial>,
}

impl Palettes {
    /// The same three gradients [`stage::scene`] baked into the material table,
    /// baked again — this time as the lookup that turns a particle's colour
    /// back into a row index.
    fn bake() -> Self {
        Self {
            sparks: effects::palette(&effects::impact_sparks().modifiers.color),
            puff: effects::palette(&effects::smoke_puff().modifiers.color),
            spam: effects::palette(&effects::spam().modifiers.color),
        }
    }
}

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for.
///
/// One value rather than two copies, for the reason every sample gives: the two
/// bring-up paths must open the *same* device, or a feature only one of them
/// requested is a bug nobody sees until the other path runs.
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "sparks",
        // The engine's whole optional bundle, not a subset spelled out here: a
        // hand-written list is a copy, and a copy goes stale the moment
        // `GpuContextDesc::default` gains a flag.
        ..GpuContextDesc::from(gpu)
    }
}

// `PendingGpu`, its `poll`, and the blocking and polled `open`s — both routed
// through `desc` above, so the two bring-up paths ask for the same device.
crcbl::impl_polled_bundle!(gpu: Gpu, pending: PendingGpu, desc: desc);

impl Gpu {
    /// Builds this sample's renderers on an already-open context, and makes
    /// [`crate::stage`] resident.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the stage's description is one the pools it asks for
    /// cannot hold, or if the menu pass or the UI compositor refused the
    /// device.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let scene = stage::scene();
        let mut renderer = ForwardRenderer::with_scene(ctx.device(), ctx.queue(), format, &scene)?;
        // Rolled back by hand from here on: `Gpu` has no `Drop`, so a `?` would
        // leak the forward renderer's pipelines rather than release them.
        let drawn = match stage::place(&mut renderer) {
            Ok(drawn) => drawn,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(HalError::InvalidDescriptor(format!(
                    "sparks' stage does not fit its own pools: {error}"
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

        Ok(Self {
            ctx,
            renderer,
            drawn,
            palettes: Palettes::bake(),
            pool: TransientPool::new(),
            timers,
            // Replaced before the first frame is recorded — `crate::app::draw`
            // writes it from the frame's own clock — and a camera rather than
            // an `Option` because a frame must never be drawn from nowhere.
            camera: stage::camera(0.0),
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

    /// Points the drawn instances at wherever the simulation put the particles.
    ///
    /// Returns how many were written, which is what `crate::app` reports and
    /// what says the instance path is carrying the effects rather than the
    /// effects existing only in the pool.
    ///
    /// An effect with nothing alive — the sparks between bursts, the puff while
    /// it is off — parks its whole block rather than being skipped. Skipping it
    /// would leave the last frame's particles standing in the air, which is a
    /// picture that looks like an effect and is a bug.
    pub fn place_particles(&mut self, show: &Show) -> usize {
        let mut drawn = 0;
        for (block, live, palette) in [
            (&mut self.drawn.sparks, show.sparks(), &self.palettes.sparks),
            (&mut self.drawn.puff, show.puff(), &self.palettes.puff),
            (&mut self.drawn.spam, show.spam(), &self.palettes.spam),
        ] {
            match live {
                Some(live) => drawn += block.write(&mut self.renderer, live, palette),
                None => block.clear(&mut self.renderer),
            }
        }
        drawn
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
            .begin_frame(self.ctx.device(), &self.camera, &stage::sun(), extent)?;
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
            // already in the target and the overlay has to stay readable over
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
            crcbl::log::debug!("render graph for the sparks frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("sparks frame"),
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

// Start-up, driven by `crcbl::engine::PolledBoot` rather than blocked on.
crcbl::impl_polled_gpu!(gpu: Gpu, pending: PendingGpu);

#[cfg(test)]
mod tests {
    use super::*;

    /// **The bundle this sample opens with is the engine's own.** A hand-written
    /// list is a copy, and a copy goes stale the moment
    /// [`GpuContextDesc::default`] gains a flag.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engine_s_own() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, "sparks");
        assert_eq!(
            asked.optional_features,
            GpuContextDesc::default().optional_features,
            "a subset spelled out here is a copy, and a copy goes stale",
        );
    }

    /// The palettes this file looks rows up in are the ones the scene made
    /// resident. Two bakes of the same gradient, and a drift between them is a
    /// particle drawn in another effect's colour.
    #[test]
    fn the_lookup_palettes_are_the_rows_the_scene_declared() {
        let scene = stage::scene();
        let palettes = Palettes::bake();
        let baked: Vec<_> = palettes
            .sparks
            .iter()
            .chain(&palettes.puff)
            .chain(&palettes.spam)
            .map(|row| row.base_color)
            .collect();
        let declared: Vec<_> = scene
            .materials
            .iter()
            .skip(scene.materials.len() - baked.len())
            .map(|row| row.base_color)
            .collect();
        assert_eq!(baked, declared);
    }
}
