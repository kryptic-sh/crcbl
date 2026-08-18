//! The device, the swapchain and the one pass this milestone draws.
//!
//! Everything that is not this application's — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — lives in [`crcbl::engine::GpuContext`], the same join every
//! sample's `gpu.rs` uses.
//!
//! # One pass, and what is deliberately not beside it
//!
//! `docs/plan/sample/05-viewer.md`'s milestone 1 is "load + orbit + grid", and
//! this is the load-and-orbit half. There is **no grid floor** — a grid is a
//! second resident mesh and a second material row in a
//! [`SceneDesc`](crcbl::render::scene::SceneDesc) the glTF
//! conversion sized for the document alone, so it is a change to how the scene
//! is assembled rather than a pass to add here. There is no menu pass and no UI
//! pass, so no debug overlay: `apps/hud` and `apps/sandbox` get theirs from
//! [`crcbl::engine::Loop`], and this application owns its loop (see
//! [`crate::app`]). Both are in `docs/backlog.md`.
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
use crcbl::render::{TransientPool, scene::InstanceDesc};
use crcbl::shell::{Shell, WindowId};

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
    /// Whether the graph dump has been logged since the graph last changed
    /// shape. Once per shape rather than once per frame, because a dump every
    /// frame is a log nobody reads.
    dumped: bool,
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

        Ok(Self {
            ctx,
            renderer,
            pool: TransientPool::new(),
            camera: Camera::default(),
            dumped: false,
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

        let format = self.ctx.format();
        let compiled = {
            let mut graph = RenderGraph::new(self.ctx.queue());
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, format, extent),
            );
            let _hdr = self.renderer.add_passes(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

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
        compiled.execute(self.ctx.device(), &mut self.pool, encoder.as_mut(), None)?;
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
        self.pool.destroy(self.ctx.device());
        self.renderer.destroy(self.ctx.device());
        self.ctx.destroy()
    }
}

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
