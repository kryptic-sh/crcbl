//! The device, the swapchain and the three passes this milestone draws.
//!
//! Everything that is not this application's — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — lives in [`crcbl::engine::GpuContext`], the same join every
//! sample's `gpu.rs` uses.
//!
//! # The document, the menu, the grid and the panel
//!
//! `docs/plan/sample/05-viewer.md`'s milestone 1 is "load + orbit + grid", and
//! all three are here. The grid floor is
//! [`crcbl::render::grid`]'s screen-space pass, switched on through
//! [`ForwardRenderer::set_ground_grid`] — **not** a mesh in the scene. That
//! matters for this sample in particular: the
//! [`SceneDesc`](crcbl::render::scene::SceneDesc) is sized by the glTF
//! conversion for the user's document alone, so a grid made of geometry would
//! need a resident mesh and a material row that the document did not ask for,
//! and its lines would change width with the zoom the turntable is at.
//!
//! It is drawn after the tonemap and depth-tested against the scene, so it sits
//! *under* the model and keeps the same colour at any exposure — which is what
//! makes it a reference rather than part of the picture.
//!
//! The menu and UI passes are here because this sample is hosted by
//! [`crcbl::engine::Loop`] now and the loop draws through them — see
//! [`crate::app`]. Nothing of the viewer's own goes through the UI pass: it has
//! no HUD, and rule 4's debug panel is the engine's.
//!
//! # The key light rides with the camera
//!
//! Every other sample's sun is a fixture of its world, because its world is
//! authored and lit on purpose. This one has no world: it has whatever file the
//! user opened, and a sun fixed in world space would leave a model in silhouette
//! from half the angles a turntable reaches — which for a tool whose whole job
//! is showing an asset is the failure, not a style. So the light is built from
//! the camera each frame, over the viewer's shoulder and to one side, which is
//! the three-quarter key every DCC viewport opens with. See [`key_light`].

use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions};
use crcbl::hal::CommandEncoderDesc;
use crcbl::math::Vec3;
use crcbl::prelude::*;
use crcbl::render::{
    MAX_TIMED_PASSES, MenuRenderer, PassTimers, TransientPool, UiRenderer, grid::GridStyle,
    scene::InstanceDesc,
};
use crcbl::shell::{Shell, WindowId};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

/// How many frames the swapchain keeps in flight, which is what the pass timers
/// have to be sized for.
const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// How far behind the eye the key light sits, relative to how far above and
/// aside — see [`key_light`]. The three together are a direction, so only their
/// ratio matters.
const KEY_BEHIND: f32 = 1.0;
/// How far above.
const KEY_ABOVE: f32 = 0.7;
/// How far to the viewer's left.
const KEY_ASIDE: f32 = 0.5;

/// How bright the key light is.
///
/// Above 1.0 like every other light in this engine: the scene target is
/// `Rgba16Float` and the tonemap is what brings it back, so a key that peaked at
/// 1.0 would be one this pipeline could not tell from a dimmer one.
const KEY_INTENSITY: f32 = 2.0;

/// The flat ambient term, which is the only indirect light there is.
///
/// Larger than `apps/lumen`'s, and deliberately: that fixture is lit to show
/// what its lighting does, and this one has to keep the side of a model that
/// faces away from the key readable, because a user turning a model round is
/// looking at that side on purpose.
const AMBIENT: f32 = 0.25;

/// This application's device, its swapchain and its one renderer.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    renderer: ForwardRenderer,
    pool: TransientPool,
    /// This frame's camera, written by [`Gpu::set_camera`] before the frame is
    /// recorded. The light is derived from it — see [`key_light`] — so there is
    /// no second field that could disagree with it.
    camera: Camera,
    /// UI compositing, for the debug overlay.
    ///
    /// The viewer has no HUD and is not getting one — it draws the user's
    /// document and nothing of its own on top. It has a UI pass because
    /// `docs/plan/sample/00-samples-overview.md` rule 4 applies to it too, and
    /// while this application owned its loop it could not honour that at all:
    /// `--debug-overlay` parsed and reached nothing. See [`crate::app`].
    ui: UiRenderer,
    /// The menu pass: its own sheets, its own screen-space camera, and a pass
    /// that declares nothing on a frame with no menu on it.
    menu: MenuRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    /// `None` on a device without timestamp queries — the debug panel's GPU
    /// section degrades, the frame does not.
    timers: Option<PassTimers>,
    /// Whether the graph dump has been logged since the graph last changed
    /// shape. Once per shape rather than once per frame, because a dump every
    /// frame is a log nobody reads.
    dumped: bool,
    /// The last frame's graph dump, kept only for this crate's own tests: it is
    /// how a test sees whether the UI pass was in the frame at all. `add_pass`
    /// declares nothing when the draw list is empty, so the pass's presence in
    /// this string *is* "the overlay reached the GPU".
    #[cfg(test)]
    last_dump: String,
}

/// What both this application's bring-up and any future polled one ask for.
///
/// One value rather than two copies, for the reason every sample gives: two
/// paths that open different devices differ in a way nobody sees until the
/// other one runs.
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: "viewer",
        ..GpuContextDesc::from(gpu)
    }
}

impl Gpu {
    /// Opens a device on `window` and makes `scene` resident, with one instance
    /// per entry of `instances`.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened, or if the renderer refused the
    /// description — which for a converted glTF means a document larger than
    /// the pools it asked to be sized for.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        scene: &crcbl::render::scene::SceneDesc<'_>,
        instances: &[InstanceDesc],
    ) -> Result<Self, GpuError> {
        let ctx = GpuContext::open(shell, window, extent, &desc(gpu))?;
        let format = ctx.format();
        let mut renderer =
            match ForwardRenderer::with_scene(ctx.device(), ctx.queue(), format, scene) {
                Ok(renderer) => renderer,
                Err(error) => {
                    ctx.destroy()?;
                    return Err(GpuError::Hal(error));
                }
            };
        // `docs/plan/sample/05-viewer.md` milestone 1's grid floor — see the
        // [module docs](self) for why it is a pass rather than a mesh.
        //
        // [`GridStyle::default`] and not a style of this sample's own: its fine
        // cell is one metre, which is the unit glTF is authored in and so the
        // unit the documents this application opens are already in.
        //
        // Unwound by hand for the reason the loop below is.
        if let Err(error) = renderer.set_ground_grid(ctx.device(), Some(GridStyle::default())) {
            renderer.destroy(ctx.device());
            ctx.destroy()?;
            return Err(GpuError::Hal(error));
        }

        // Unwound by hand, because `Gpu` has no `Drop`: a `?` here would leak
        // every pipeline and buffer the renderer just made.
        for instance in instances {
            if let Err(error) = renderer.add_instance(instance) {
                renderer.destroy(ctx.device());
                ctx.destroy()?;
                // The seam has no vocabulary for "this pool is full", so
                // `crcbl-render` flattens one into a `HalError` that renders
                // verbatim and keeps the numbers.
                return Err(GpuError::Hal(error.into()));
            }
        }

        // Unwound by hand for the reason the instance loop above is: `Gpu` has
        // no `Drop`, so a `?` here would leak every pipeline and buffer the
        // renderers before it have made.
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(ui) => ui,
            Err(error) => {
                renderer.destroy(ctx.device());
                ctx.destroy()?;
                return Err(GpuError::Hal(error));
            }
        };
        let menu = match MenuRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(menu) => menu,
            Err(error) => {
                ui.destroy(ctx.device());
                renderer.destroy(ctx.device());
                ctx.destroy()?;
                return Err(GpuError::Hal(error));
            }
        };
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        if timers.is_none() {
            crcbl::log::info!("hal: no timestamp queries on this device; per-pass timing is off");
        }

        Ok(Self {
            ctx,
            renderer,
            pool: TransientPool::new(),
            camera: Camera::default(),
            ui,
            menu,
            atlas: FontAtlas::built_in(),
            draw_list: DrawList::new(),
            timers,
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

    /// Where the next frame is drawn from.
    pub const fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// The glyph atlas the UI pass renders text from.
    ///
    /// The menu lays itself out with it and the debug overlay measures its own
    /// panel with it, and both must use the *same* atlas the pass draws with or
    /// the background rect is the wrong size for the text inside it.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// Takes this frame's menu, or `None` on a frame that shows none.
    ///
    /// CPU only — the upload happens inside [`Gpu::frame`], at the extent the
    /// swapchain was actually acquired at.
    pub fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        self.menu.set_menu(menu);
    }

    /// Takes this frame's UI geometry, handing the previous frame's allocation
    /// back so the loop can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, list: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, list);
    }

    /// The most recent frame whose per-pass GPU timings have landed.
    ///
    /// `None` on a device with no timestamp queries, and empty for the first few
    /// frames — the report is deliberately frames latent; see
    /// [`crcbl::render::PassTimers`].
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// What the last [`Gpu::frame`] recorded: draws, instances and triangles,
    /// summed over the three renderers this bundle holds.
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.renderer
            .counters()
            .plus(self.menu.counters())
            .plus(self.ui.counters())
    }

    /// The UI geometry this frame handed over, for this crate's own tests.
    #[cfg(test)]
    #[must_use]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
    }

    /// The last frame's render-graph dump, for this crate's own tests.
    #[cfg(test)]
    #[must_use]
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
        // The extent the swapchain answered with, not the one that was asked
        // for: they differ on the frame a resize lands.
        let extent = acquired.extent;

        let light = key_light(&self.camera);
        self.renderer
            .begin_frame(self.ctx.device(), &self.camera, &light, extent)?;
        self.menu
            .begin_frame(self.ctx.device(), extent)
            .map_err(GpuError::Hal)?;
        // Upload this frame's UI geometry: the debug overlay, and only that —
        // the viewer has no HUD of its own.
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
            // **Between the scene and the text, and that order is the whole
            // join.** The menu's scrim dims what is already in the target, so it
            // has to come after the tonemap; the panel is opaque and the labels
            // are UI-pass text, so it has to come before the UI or the frame
            // paints over its own words.
            self.menu.add_pass(&mut graph, target);
            // Composited on top of the tonemapped scene, so the overlay is
            // readable over whatever the document drew.
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        #[cfg(test)]
        {
            self.last_dump = compiled.dump();
        }
        // "The graph must be able to explain itself" — §2.4's debug-tools
        // principle.
        if !self.dumped {
            crcbl::log::debug!("render graph for the viewer frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("viewer frame"),
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
        // Only after the submit's retire, so nothing the pool destroys can
        // still be referenced by a submission that has not completed.
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
    /// [`GpuError`] if the reconfigure failed.
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

// The nine forwards `crcbl::engine` calls this bundle through. Every one of
// them is a method above; the macro is what stops a sample forgetting one.
crcbl::impl_game_gpu!(Gpu);

/// The key light for a frame drawn from `camera`.
///
/// Behind the eye, above it and to its left, on the camera's *own* axes — so
/// the model is lit from the same relative angle however the turntable is
/// turned, and the side the user brought round to look at is the lit one. The
/// basis is built the way [`Camera::view`] builds its own, which is what makes
/// "the camera's right" mean one thing in this file and in the matrix the frame
/// is drawn with.
///
/// [`DirectionalLight::direction`] points *towards* the light, which is why
/// nothing here is negated twice.
#[must_use]
pub fn key_light(camera: &Camera) -> DirectionalLight {
    let forward = (camera.target - camera.eye).normalize_or_zero();
    // Degenerate only if the eye is on the pivot, which `OrbitCamera` clamps
    // against — `normalize_or_zero` is what keeps a `NaN` out of the uniform
    // block if some other caller ever does it anyway.
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward);
    DirectionalLight {
        direction: (-forward * KEY_BEHIND + up * KEY_ABOVE - right * KEY_ASIDE).normalize_or_zero(),
        color: Vec3::splat(KEY_INTENSITY),
        ambient: Vec3::splat(AMBIENT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::render::{OrbitCamera, Projection};
    use crcbl::screenshot::{ForwardScene, OffscreenSetup};

    /// The frame the grid proof below is drawn at.
    ///
    /// Small on purpose — the claim is about which pixels moved, not about how
    /// many — but tall enough that "the top eighth of the frame" is several rows
    /// rather than one.
    const PROOF_EXTENT: (u32, u32) = (160, 120);

    /// One frame of an otherwise **empty** scene, with the ground grid switched
    /// on or off, read back off the GPU.
    ///
    /// Empty because the grid is the subject: with no instance placed, every
    /// pixel that differs between the two frames is one the grid pass wrote, and
    /// nothing has to be assumed about where a mesh landed.
    ///
    /// The camera stands two metres up looking **horizontally**, so the ground
    /// plane fills the lower half of the frame and the horizon is across the
    /// middle. That is what makes the top of the frame a control: a ray leaving
    /// the upper half never reaches `y = 0`, so nothing the grid draws can land
    /// there.
    fn grid_proof_frame(on: bool) -> (u32, u32, Vec<u8>) {
        let mut setup = OffscreenSetup::open_forward(
            PROOF_EXTENT.0,
            PROOF_EXTENT.1,
            move |device, queue, format| {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                if on {
                    renderer.set_ground_grid(device, Some(GridStyle::default()))?;
                }
                Ok(ForwardScene {
                    camera: Camera {
                        eye: Vec3::new(0.0, 2.0, 8.0),
                        target: Vec3::new(0.0, 2.0, 0.0),
                        up: Vec3::Y,
                        projection: Projection::Perspective {
                            fov_y: std::f32::consts::FRAC_PI_3,
                            near: 0.01,
                        },
                    },
                    sun: DirectionalLight::default(),
                    renderer: Box::new(renderer),
                })
            },
        )
        .expect("the pinned backend opens");
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame lands");
        setup.finish().expect("teardown");
        (width, height, pixels)
    }

    /// Whether this run has a driver pinned, which is what
    /// [`grid_proof_frame`] needs and what `CRCBL_GPU=vk cargo test -p viewer`
    /// supplies.
    ///
    /// The null backend reads back nothing a pixel claim could be made about, so
    /// the proof below is skipped without a pin rather than made against zeroes.
    /// CI pins it — see the workflow's "Run the viewer's suite against
    /// lavapipe", which is the run this test exists for.
    fn a_driver_is_pinned() -> bool {
        match std::env::var(crcbl::backend::BACKEND_ENV_VAR) {
            Err(_) => false,
            Ok(name) => match GpuBackend::from_name(&name) {
                None => panic!(
                    "{} names {name:?}, which is not a backend",
                    crcbl::backend::BACKEND_ENV_VAR
                ),
                Some(backend) => backend != GpuBackend::Null,
            },
        }
    }

    /// **The grid reaches pixels.** Two readbacks of the same frame, one with
    /// the grid switched on and one without, and they are not the same picture.
    ///
    /// This is the claim `crate::app`'s dump assertion cannot make: a pass in
    /// the graph that bound a pipeline and drew nothing produces exactly that
    /// dump. Only the bytes can tell the two apart.
    ///
    /// Three assertions, and each rules out a different way of passing for the
    /// wrong reason:
    ///
    /// * the **top eighth** of the frame is byte-identical — what the pass drew
    ///   is a plane the camera's upper rays never reach, rather than something
    ///   sprayed over the whole target;
    /// * a real share of the lower half moved — the pass is not a no-op that
    ///   bound a pipeline and drew nothing;
    /// * and not *all* of the lower half moved — lines, not a flood fill.
    #[test]
    fn the_grid_draws_pixels_the_frame_does_not_have_without_it() {
        if !a_driver_is_pinned() {
            return;
        }
        let (width, height, off) = grid_proof_frame(false);
        let (on_width, on_height, on) = grid_proof_frame(true);
        assert_eq!((width, height), (on_width, on_height));
        assert_eq!(off.len(), on.len());

        let row = width as usize * 4;
        let sky = (height / 8) as usize * row;
        assert_eq!(
            &off[..sky],
            &on[..sky],
            "the grid changed pixels above the horizon, where no ray reaches the ground plane",
        );

        let half = (height / 2) as usize * row;
        let ground_pixels = (off.len() - half) / 4;
        let moved = off[half..]
            .chunks_exact(4)
            .zip(on[half..].chunks_exact(4))
            .filter(|(before, after)| before != after)
            .count();
        assert!(
            moved > ground_pixels / 100,
            "only {moved} of {ground_pixels} ground pixels moved, which is not a grid",
        );
        assert!(
            moved < ground_pixels,
            "every one of {ground_pixels} ground pixels moved, which is a wash rather than lines",
        );
    }

    /// **The light stays over the viewer's shoulder wherever the turntable
    /// goes.** A sun fixed in world space would put half of every orbit in
    /// silhouette, which for this application is the bug — see the [module
    /// docs](self).
    ///
    /// The claim is made against the camera's own forward vector, at several
    /// poses, so it is about the relationship and not about one lucky angle.
    #[test]
    fn the_key_light_follows_the_camera_round_the_model() {
        let orbit = OrbitCamera::new(Vec3::new(1.0, 2.0, -3.0), 5.0, Projection::default());
        let mut seen = Vec::new();
        for (yaw, pitch) in [(0.0f32, 0.0f32), (1.7, 0.3), (-2.6, -0.5), (3.1, 0.9)] {
            let mut turned = orbit;
            turned.orbit(yaw, pitch);
            let camera = turned.camera();
            let light = key_light(&camera);

            let forward = (camera.target - camera.eye).normalize();
            assert!(
                light.direction.dot(-forward) > 0.5,
                "at ({yaw}, {pitch}) the light points {:?}, which is not behind an eye \
                 looking {forward:?}",
                light.direction,
            );
            assert!(
                (light.direction.length() - 1.0).abs() < 1e-5,
                "the direction must be a unit vector, and is {:?}",
                light.direction,
            );
            seen.push(light.direction);
        }

        // Anti-vacuity: a light that ignored the camera entirely would satisfy
        // nothing above for the right reason only because every dot product
        // would be computed against the same vector.
        assert!(
            seen.windows(2).any(|pair| pair[0].distance(pair[1]) > 0.1),
            "the light never moved: {seen:?}",
        );
    }

    /// The bundle asks for the engine's own feature set rather than a subset
    /// spelled out here — the check every other sample carries, since one of
    /// them shipped a hand-written list that went stale.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engines_own() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, "viewer");
        assert_eq!(
            asked.optional_features,
            GpuContextDesc::default().optional_features,
            "a subset spelled out here is a copy, and a copy goes stale",
        );
    }
}
