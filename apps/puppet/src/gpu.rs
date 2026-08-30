//! Puppet's GPU side: the shared shell↔HAL join, the forward renderer over
//! [`crate::map`], and the UI and menu passes rule 4 asks every sample for.
//!
//! Everything that is not this sample's — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — is [`crcbl::engine::GpuContext`]'s. What is here is the part
//! that is puppet's: a renderer built from **this application's** scene
//! description rather than from [`ForwardRenderer::new`]'s demo one, and the two
//! instances in it that move.
//!
//! # Shadows are on because nothing turns them off
//!
//! `docs/plan/sample/09-puppet.md`'s milestone 1 asks for the map "with shadows
//! already on", and that is what a renderer nobody hands an
//! [`EffectRequest`](crcbl::render::EffectRequest) draws:
//! [`RenderEffects::DEFAULT_STACK`](crcbl::render::RenderEffects) is every
//! effect that models the scene's own light transport, shadows included, and it
//! is what a view that has declared no stack asks for. So there is no
//! `set_effect_request` call in this file and no flag in front of one — the sun
//! in [`crate::map::sun`] casts, and the character's shadow on the ground is the
//! "does it read as grounded" test that milestone is really about.
//!
//! # Pass order is declaration order
//!
//! The forward frame → `menu` → `ui`. The last two load the target rather than
//! clearing it, so declaring the UI pass first would put the pause panel on top
//! of the words it exists to frame.

use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions};
use crcbl::hal::{CommandEncoderDesc, HalError};
use crcbl::math::{DVec3, Mat4};
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, MAX_TIMED_PASSES, MenuRenderer, PassTimers,
    RenderGraph, Skinning, SkinningDesc, SkinningError, TransientPool, UiRenderer,
};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

use crate::map;

const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// This sample's device, its swapchain and the three renderers it draws with.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    /// The map, made resident once and drawn every frame.
    renderer: ForwardRenderer,
    /// The instances in it that are rewritten every frame: the character's
    /// limbs and the block that says which way it faces.
    character: map::Character,
    /// The compute pass that deforms those limbs, and the buffers it reads.
    ///
    /// Built against `renderer`'s **own** vertex pool and no other, so it is
    /// released with the renderer rather than outliving it.
    skinning: Skinning,
    /// This frame's skinning matrices, one per joint of [`crate::rig`].
    ///
    /// Copied in rather than borrowed, for `apps/viewer`'s reason: the palette
    /// is composed on the frame's own clock in [`crate::anim`] and consumed
    /// when the frame is recorded, and the two are not the same call.
    palette: Vec<Mat4>,
    pool: TransientPool,
    /// `None` on a device without timestamp queries — the report degrades, the
    /// frame does not.
    timers: Option<PassTimers>,
    /// Where the frame is seen from. Written every frame by [`crate::app`],
    /// which owns the camera; this is only where the frame reads it.
    camera: Camera,
    /// What lights and shadows it, on the same terms: [`crate::map::sun`] turns
    /// on the **simulation's** clock, so the frame is handed the light rather
    /// than reading one from here.
    sun: DirectionalLight,
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

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for.
///
/// One value rather than two copies, for the reason every sample gives: the two
/// bring-up paths must open the *same* device, or a feature only one of them
/// requested is a bug nobody sees until the other path runs.
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "puppet",
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
    /// [`crate::map`] resident.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the map's description is one the pools it asks for
    /// cannot hold, if the menu pass or the UI compositor refused the device, or
    /// if any HAL call failed.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let format = ctx.format();
        let scene = map::scene();
        let mut renderer = ForwardRenderer::with_scene(ctx.device(), ctx.queue(), format, &scene)?;
        // Rolled back by hand from here on: `Gpu` has no `Drop`, so a `?` would
        // leak the forward renderer's pipelines rather than release them.
        let character = match map::place(&mut renderer) {
            Ok(character) => character,
            Err(error) => {
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(HalError::InvalidDescriptor(format!(
                    "puppet's map does not fit its own pools: {error}"
                ))));
            }
        };
        let (ranges, joints, bindings) = character.skinning_capacities();
        let skinning = match Skinning::new(
            ctx.device(),
            &SkinningDesc {
                label: Some("puppet skinning"),
                frames: FRAMES_IN_FLIGHT,
                ranges,
                joints,
                bindings,
                // **This pool and no other.** A pass built against a different
                // buffer writes vertices no draw of this renderer can reach,
                // and the picture is the bind pose for ever.
                vertices: renderer.vertex_buffer(),
                // And the boundary between that pool's two streams, which the
                // dispatch writes both of.
                attribute_base: renderer.attribute_base(),
            },
        ) {
            Ok(skinning) => skinning,
            Err(error) => {
                character.release(&mut renderer);
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(skinning_error(error)));
            }
        };
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        if timers.is_none() {
            crcbl::log::info!("hal: no timestamp queries on this device; per-pass timing is off");
        }
        let menu = match MenuRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(menu) => menu,
            Err(error) => {
                character.release(&mut renderer);
                skinning.destroy(ctx.device());
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(ui) => ui,
            Err(error) => {
                menu.destroy(ctx.device());
                character.release(&mut renderer);
                skinning.destroy(ctx.device());
                renderer.destroy(ctx.device());
                return Err(GpuError::Hal(error));
            }
        };

        Ok(Self {
            ctx,
            renderer,
            character,
            skinning,
            // Sized for the palette every frame brings, so the refill in
            // `set_palette` never grows it.
            palette: Vec::with_capacity(crate::rig::JOINTS),
            pool: TransientPool::new(),
            timers,
            // Replaced before the first frame is recorded — `crate::app::draw`
            // writes it from wherever the simulation put the character — and a
            // camera rather than an `Option` because a frame must never be
            // drawn from nowhere.
            camera: Camera::default(),
            sun: map::sun(0.0),
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

    /// What lights and shadows the next frame.
    pub const fn set_sun(&mut self, sun: DirectionalLight) {
        self.sun = sun;
    }

    /// Moves the drawn character to where the simulation put it.
    ///
    /// `position` is the **centre** of the controller's capsule and `facing` is
    /// the yaw the body is turned to; [`map::Character::place_at`] is what turns
    /// the pair into the two transforms.
    pub fn place_character(&mut self, position: DVec3, facing: f64) {
        self.character
            .place_at(&mut self.renderer, position, facing);
    }

    /// Hands over the pose the next frame deforms the character with.
    ///
    /// [`crate::anim`] composes it; this is only where the frame reads it. A
    /// frame that is handed none draws whatever the last one was given, which
    /// is why [`crate::app`] writes it on every draw rather than only when it
    /// changes.
    pub fn set_palette(&mut self, palette: &[Mat4]) {
        self.palette.clear();
        self.palette.extend_from_slice(palette);
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

        // **The skinned entry point, not `begin_frame` with a flag.** It rotates
        // the instance ring, uploads the palette and the skins, moves the
        // ping-pong and then re-points every skinned instance at the half this
        // frame's dispatch fills. A frame that went through `begin_frame`
        // instead would leave the character pointing at last frame's vertices,
        // for ever, with nothing reporting it.
        let ranges = self.character.ranges(&self.palette);
        self.renderer
            .begin_skinned_frame(
                self.ctx.device(),
                &mut self.skinning,
                &ranges,
                &self.camera,
                &self.sun,
                extent,
            )
            .map_err(|error| GpuError::Hal(skinning_error(error)))?;
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
            // The dispatch is added by the renderer rather than beside it: the
            // three passes that pull vertices have to declare a read of the
            // pool node the compute pass writes, and only the renderer knows
            // which those are.
            let _hdr = self.renderer.add_skinned_passes(
                &mut graph,
                &self.pool,
                target,
                extent,
                &self.skinning,
            );
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
            crcbl::log::debug!("render graph for the puppet frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("puppet frame"),
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
        // The limbs give their pool runs back before the pool goes, and the
        // pass that writes into them goes before the renderer that owns it.
        self.character.release(&mut self.renderer);
        self.skinning.destroy(self.ctx.device());
        self.pool.destroy(self.ctx.device());
        if let Some(timers) = self.timers.as_mut() {
            timers.destroy(self.ctx.device());
        }
        self.renderer.destroy(self.ctx.device());
        self.ctx.destroy()
    }
}

/// A skinning refusal, as the error this bundle reports.
///
/// [`SkinningError::Hal`] is passed through unchanged; everything else is a
/// description this sample got wrong — a palette too small for the joints it
/// named, a binding count that disagrees with a mesh — and reaches the caller
/// as the sentence it is, because there is no [`HalError`] that means it.
fn skinning_error(error: SkinningError) -> HalError {
    match error {
        SkinningError::Hal(hal) => hal,
        other => {
            HalError::InvalidDescriptor(format!("puppet's character cannot be skinned: {other}"))
        }
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
    /// [`GpuContextDesc::default`] gains a flag. The failure is silent both
    /// ways: the missing capability changes no picture, only whether the
    /// engine's pacing loop and display-timing query can reach anything.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engine_s_own() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, "puppet");
        assert_eq!(
            asked.optional_features,
            GpuContextDesc::default().optional_features,
            "a subset spelled out here is a copy, and a copy goes stale",
        );
    }
}
