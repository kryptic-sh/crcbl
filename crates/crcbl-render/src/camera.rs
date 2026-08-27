//! Cameras, and the reversed-Z projection matrices that are the engine's whole
//! depth convention.
//!
//! ```text
//! Camera { view, Projection::Perspective { .. } }  ──▶ view_proj  ──▶ FrameUniforms
//!          ^                Projection::Orthographic { .. }
//!          └── the only thing milestone 5 changes
//! ```
//!
//! # Reversed-Z is produced *here*, and nowhere else
//!
//! `docs/plan/02-vulkan-backend.md` locks the convention — "`D32_SFLOAT`,
//! projection uses an **infinite far plane with reversed depth**, compare op
//! `GREATER`, clear to 0.0" — and `crcbl-hal` bakes three of those four into its
//! defaults: [`DepthStencilState::default`](crcbl_hal::DepthStencilState) is
//! `Greater` on `D32Float`, [`ClearValue::default`](crcbl_hal::ClearValue)
//! clears to `0.0`, and [`Viewport::default`](crcbl_hal::Viewport) keeps the
//! depth range `0.0..1.0` *unreversed* because reversing the viewport is the
//! well-known trap that looks right and throws the precision away.
//!
//! The fourth is the projection matrix, and it lives in this module. Everything
//! else in the engine inherits the convention by using what this produces.
//!
//! ## Why it is worth the trouble at all
//!
//! Not aesthetics: **precision**. A conventional `0..1` depth buffer spends most
//! of a float's mantissa near the near plane, where nothing needs it, and runs
//! out at distance. `docs/plan/02-vulkan-backend.md`'s stated case is "a
//! sector-tiled world with 300 m+ sightlines z-fights immediately on a
//! conventional `0..1` buffer".
//!
//! That is a *measurable* claim, and this module's
//! `reversed_z_resolves_a_centimetre_at_300_metres_and_standard_z_does_not`
//! measures it: two surfaces one centimetre apart at 300 m quantise to the same
//! `f32` under a standard projection and to different ones under this module's.
//! The test computes both, so it fails if this module ever stops being
//! reversed — a comment could not do that.

use core::f32::consts::FRAC_PI_4;

use glam::{Mat4, Vec3};

/// How a camera flattens the world into clip space.
///
/// Two variants and one code path: `docs/plan/02-vulkan-backend.md`'s milestone
/// 5 is "orthographic camera mode proving the 2D story (z = z-index) is just a
/// projection matrix swap", and this enum is that proof's shape. Nothing
/// downstream of [`Camera::view_projection`] — not the shader, not the pipeline,
/// not the render graph — can tell which variant produced the matrix it was
/// handed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// Infinite-far reversed-Z perspective.
    ///
    /// There is no `far` field, and its absence is the design: an infinite far
    /// plane is free under reversed-Z (it costs one matrix element, not
    /// precision) and it removes the "what should the far plane be?" question
    /// from every caller in a world measured in sectors.
    Perspective {
        /// Vertical field of view, in radians.
        fov_y: f32,
        /// Distance to the near plane. Must be positive; **this is the only
        /// number that controls depth precision** under reversed-Z, so it is
        /// worth raising rather than lowering.
        near: f32,
    },
    /// Reversed-Z orthographic.
    ///
    /// The 2D mode. `half_height` is half the visible world height; the width
    /// follows from the aspect ratio, so a resize changes what is visible
    /// horizontally and never squashes the picture.
    Orthographic {
        /// Half the visible height, in world units.
        half_height: f32,
        /// Near plane distance along the view direction.
        near: f32,
        /// Far plane distance along the view direction. Finite, unlike the
        /// perspective case: an orthographic projection is linear in depth, so
        /// an infinite far plane would map every finite z to 1.0.
        far: f32,
    },
}

impl Default for Projection {
    /// A 45° vertical field of view with a 10 cm near plane.
    fn default() -> Self {
        Self::Perspective {
            fov_y: FRAC_PI_4,
            near: 0.1,
        }
    }
}

impl Projection {
    /// The projection matrix for an `aspect` (width ÷ height) viewport.
    ///
    /// The result maps to **Y-up NDC with depth in `0..1`**. Y-up rather than
    /// Vulkan's native Y-down because `crcbl-vk` submits a negative-height
    /// viewport, so the seam's convention is +Y up and a backend that did not
    /// flip would be the one out of step. Depth `0..1` because that is what
    /// every backend this engine targets uses; only the *direction* is unusual.
    ///
    /// # Panics
    ///
    /// If `aspect` is not finite and positive, or if a distance is not positive
    /// — all of which are caller bugs that otherwise produce a NaN matrix and a
    /// blank screen with no error anywhere.
    #[must_use]
    pub fn matrix(self, aspect: f32) -> Mat4 {
        assert!(
            aspect.is_finite() && aspect > 0.0,
            "aspect ratio must be finite and positive, got {aspect}"
        );
        match self {
            Self::Perspective { fov_y, near } => {
                assert!(near > 0.0, "the near plane must be positive, got {near}");
                assert!(
                    fov_y > 0.0 && fov_y < core::f32::consts::PI,
                    "the field of view must be in (0, π), got {fov_y}"
                );
                // Maps `near` to depth 1 and infinity to depth 0.
                glam::camera::rh::proj::directx::perspective_infinite_reverse(fov_y, aspect, near)
            }
            Self::Orthographic {
                half_height,
                near,
                far,
            } => {
                assert!(
                    half_height > 0.0,
                    "the orthographic half-height must be positive, got {half_height}"
                );
                assert!(far > near, "far {far} must be beyond near {near}");
                let half_width = half_height * aspect;
                // **The `near`/`far` arguments are swapped, and that is the
                // whole reversal.** `directx::orthographic(l, r, b, t, n, f)`
                // maps view depth `n` to 0 and `f` to 1; handing it `(f, n)`
                // maps `f` to 0 and `n` to 1, which is reversed-Z. There is no
                // other trick — no negated viewport, no flipped compare op at
                // the call site — because a second mechanism is how a convention
                // gets applied twice and cancels.
                glam::camera::rh::proj::directx::orthographic(
                    -half_width,
                    half_width,
                    -half_height,
                    half_height,
                    far,
                    near,
                )
            }
        }
    }

    /// Whether this is the 2D mode.
    #[must_use]
    pub const fn is_orthographic(self) -> bool {
        matches!(self, Self::Orthographic { .. })
    }

    /// How many pixels one unit of length subtends one unit from the eye, in a
    /// viewport `height` pixels tall.
    ///
    /// `docs/plan/25-lod.md`'s scale factor: the number that carries a frame's
    /// size and field of view into a screen-space error, so a level's stored
    /// error in world units becomes the pixels its simplification would cost.
    /// [`ClusterSelect::is_drawn`] divides it by the distance to the group's
    /// sphere, which is where the perspective falloff comes from.
    ///
    /// **An orthographic projection has no falloff at all**, so for that variant
    /// this is the whole scale factor and dividing it by a distance is wrong.
    /// The value returned is still the right one — half the viewport over half
    /// the visible height — and [`ForwardRenderer`] is what deals with the rest:
    /// it pairs an orthographic camera with a budget nothing satisfies, so the
    /// descent expands every group and draws the base level. That is the honest
    /// answer for a 2D mode rather than a distance term invented for it.
    ///
    /// # Panics
    ///
    /// On the same arguments [`matrix`](Self::matrix) refuses: a field of view
    /// outside `(0, π)` or a non-positive half-height. `height` is not checked —
    /// a zero-height viewport gives zero pixels per unit, which is a budget
    /// nothing exceeds and a frame with no pixels in it.
    ///
    /// [`ClusterSelect::is_drawn`]: crcbl_shaders::cluster_select::ClusterSelect::is_drawn
    /// [`ForwardRenderer`]: crate::forward::ForwardRenderer
    #[must_use]
    pub fn pixels_per_unit(self, height: f32) -> f32 {
        match self {
            Self::Perspective { fov_y, .. } => {
                assert!(
                    fov_y > 0.0 && fov_y < core::f32::consts::PI,
                    "the field of view must be in (0, π), got {fov_y}"
                );
                0.5 * height / (0.5 * fov_y).tan()
            }
            Self::Orthographic { half_height, .. } => {
                assert!(
                    half_height > 0.0,
                    "the orthographic half-height must be positive, got {half_height}"
                );
                0.5 * height / half_height
            }
        }
    }
}

/// A camera: where it is, what it looks at, and how it projects.
///
/// Positions are plain [`Vec3`] in **camera-relative render space**, not
/// [`WorldPos`](crcbl_core::WorldPos). That is deliberate and is
/// `docs/plan/01-foundations.md` §1.2's rule: "all simulation positions use
/// `WorldPos` from day one; plain `Vec3` is only ever camera-relative render
/// space". Converting one to the other is the renderer's job at P2, when
/// something other than a hardcoded cube has a position at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Eye position in render space.
    pub eye: Vec3,
    /// Point the camera looks at.
    pub target: Vec3,
    /// World up. Must not be parallel to `target - eye`.
    pub up: Vec3,
    /// How the world flattens into clip space.
    pub projection: Projection,
}

impl Default for Camera {
    /// Two metres back along +Z, looking at the origin, Y up, default
    /// perspective.
    fn default() -> Self {
        Self {
            eye: Vec3::new(0.0, 0.0, 2.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection: Projection::default(),
        }
    }
}

impl Camera {
    /// The view matrix: world → right-handed Y-up view space.
    ///
    /// # Panics
    ///
    /// If the camera is degenerate: `eye` and `target` coincide, `up` is zero,
    /// or `up` is parallel to the view direction. Each of those makes
    /// `look_at` divide by a zero-length vector and hand back a matrix of
    /// `NaN`s — which propagates through the uniform block into the vertex
    /// shader and shows up as an empty screen with no error anywhere, the same
    /// failure [`Projection::matrix`] asserts against.
    #[must_use]
    pub fn view(&self) -> Mat4 {
        let direction = self.target - self.eye;
        assert!(
            direction.length_squared() > 0.0 && direction.is_finite(),
            "the camera must look somewhere: eye {:?} and target {:?}",
            self.eye,
            self.target
        );
        assert!(
            self.up.length_squared() > 0.0 && self.up.is_finite(),
            "the camera's up vector must be non-zero and finite, got {:?}",
            self.up
        );
        assert!(
            direction
                .normalize()
                .cross(self.up.normalize())
                .length_squared()
                > f32::EPSILON,
            "the camera's up vector {:?} is parallel to its view direction {direction:?}; there \
             is no basis to build",
            self.up
        );
        glam::camera::rh::view::look_at_mat4(self.eye, self.target, self.up.normalize())
    }

    /// The combined world → clip matrix for an `aspect` viewport.
    ///
    /// # Panics
    ///
    /// As [`Projection::matrix`].
    #[must_use]
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection.matrix(aspect) * self.view()
    }

    /// The same camera with a different projection — **milestone 5, entire**.
    #[must_use]
    pub fn with_projection(mut self, projection: Projection) -> Self {
        self.projection = projection;
        self
    }

    /// The depth value a point in render space projects to.
    ///
    /// The perspective divide, done on the CPU. Exists because it is what makes
    /// the depth convention *testable* without a GPU: this is the number that
    /// ends up in the `D32_SFLOAT` buffer, and reversed-Z is a claim about how
    /// it is distributed.
    ///
    /// Returns `None` for a point on or behind the eye plane, where the divide
    /// is undefined.
    ///
    /// `w > 0`, not `w.abs() > 0`: under a right-handed perspective projection
    /// `w` is the distance in front of the eye, so a point *behind* it has a
    /// negative `w` and dividing by it yields a positive-looking depth for
    /// geometry that is not in the frustum at all. Under
    /// [`Projection::Orthographic`] `w` is always `1`, so every point has a
    /// depth and this returns one — which is correct, an orthographic camera
    /// has no eye plane to be behind.
    ///
    /// # Panics
    ///
    /// As [`Projection::matrix`] and [`Camera::view`].
    #[must_use]
    pub fn depth_of(&self, point: Vec3, aspect: f32) -> Option<f32> {
        let clip = self.view_projection(aspect) * point.extend(1.0);
        (clip.w > f32::MIN_POSITIVE).then(|| clip.z / clip.w)
    }
}

/// A directional light: the whole of milestone 4's lighting model.
///
/// One light, no shadows, no attenuation. `docs/plan/02-vulkan-backend.md` rung
/// 4 is "single directional light, Lambert+Blinn — enough to see geometry
/// properly; real material model comes with stage 3/5", and cascaded shadow maps
/// are explicitly P7 (topic 18). The real material model arrived: `mesh.slang`
/// shades with a Cook-Torrance GGX lobe against a glTF metallic-roughness row,
/// and the rung's Blinn wording is what it replaced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    /// Direction **towards** the light, in render space. Normalised on the way
    /// into the uniform block.
    ///
    /// Towards, not "the direction the light travels": that is the vector both
    /// the Lambert term and the specular lobe's half-vector want, and storing
    /// the other one means every consumer negates it and one of them eventually
    /// forgets.
    pub direction: Vec3,
    /// Colour premultiplied by intensity. **May exceed 1.0** — the scene target
    /// is `Rgba16Float` precisely so that it can.
    pub color: Vec3,
    /// Flat ambient term. A real irradiance probe is P7's.
    pub ambient: Vec3,
}

impl Default for DirectionalLight {
    /// A warm key light over the viewer's **right** shoulder, with headroom.
    ///
    /// Two of the three numbers here are deliberate rather than aesthetic:
    ///
    /// * The direction has a **positive** X, so it comes from the same side as
    ///   the default camera rather than from behind the geometry that camera can
    ///   see. A light pointing away from every visible face renders a technically
    ///   correct picture of an unlit cube, which is a confusing first result and
    ///   was in fact P1.3's first result.
    /// * The intensity is **above 1.0**, so the brightest lit surface exceeds
    ///   what the swapchain can hold. That is what makes the `Rgba16Float` scene
    ///   target carry something an `Rgba8` one could not, and it is what
    ///   `crcbl-vk`'s e2e suite reads back to prove HDR-from-P1 is a pipeline
    ///   rather than a format enum. A key light brighter than the display is
    ///   also simply what daylight is.
    ///
    /// The ambient is small but non-zero, so a face turned away from the light
    /// is dark rather than black.
    fn default() -> Self {
        Self {
            direction: Vec3::new(0.4, 0.8, 0.6).normalize(),
            color: Vec3::new(1.60, 1.55, 1.45),
            ambient: Vec3::splat(0.08),
        }
    }
}

/// Exponential height fog: how thick the air is, and what colour it scatters.
///
/// `docs/plan/43-render-standards.md` §4's cheapest large win. Density falls
/// off exponentially with height above [`reference_height`](Self::reference_height),
/// and `mesh.slang`'s fragment stage integrates it along the ray from the eye
/// to each shaded point — so a distant surface fades towards
/// [`color`](Self::color) and a valley fills while a hilltop stays clear, from
/// four numbers and no extra pass.
///
/// **[`Fog::NONE`] is the default and is exactly no fog**, not nearly none:
/// a zero density gives a zero optical depth, whose transmittance is exactly
/// one, and the shader composites as two multiplies rather than an
/// interpolation so that one returns the unfogged radiance bit for bit. Every
/// golden blessed before fog existed still matches.
///
/// [`ForwardRenderer::set_fog`] is what a caller hands this to, and
/// [`crcbl_shaders::fog`] is the arithmetic both sides run.
///
/// [`ForwardRenderer::set_fog`]: crate::forward::ForwardRenderer::set_fog
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    /// Optical depth added per world unit travelled at the reference height.
    ///
    /// Zero is off. The useful range is small — a density of one puts a surface
    /// a single unit away behind more than half the fog — so this is tenths and
    /// hundredths for a scene measured in metres.
    pub density: f32,
    /// The height over which the density falls by a factor of `e`.
    ///
    /// Larger is a thicker atmosphere reaching further up; zero or less is fog
    /// that does not thin with height at all, which is the limit this
    /// approaches rather than a special case in the shader.
    pub falloff: f32,
    /// Where the density is [`density`](Self::density), in render-space `y`.
    ///
    /// Usually the ground the scene sits on, so the fog is thickest where the
    /// geometry is and thins above it.
    pub reference_height: f32,
    /// The radiance the fog scatters towards the eye.
    ///
    /// **May exceed 1.0**, like [`DirectionalLight::color`] and for the same
    /// reason: the scene target is `Rgba16Float` and this is a radiance rather
    /// than a display colour, so a bright sky's haze is allowed to bloom.
    pub color: Vec3,
    /// What fraction of the sun's radiance the medium scatters towards the eye,
    /// per unit length.
    ///
    /// **Read only by the froxel path** — [`RenderEffects::VOLUMETRIC_FOG`] —
    /// and zero everywhere else. `mesh.slang`'s closed form integrates
    /// absorption along the view ray and has no direction in it at all, so
    /// there is nowhere for a phase function to go; a caller who sets this
    /// without switching the effect on gets the frame they had.
    ///
    /// Zero is exactly off: the scattering source is a sum, so a zero term
    /// leaves [`color`](Self::color)'s environment term bit for bit.
    ///
    /// [`RenderEffects::VOLUMETRIC_FOG`]: crate::RenderEffects::VOLUMETRIC_FOG
    pub sun_scattering: f32,
    /// How the medium redistributes what it scatters: positive forward, zero
    /// evenly, negative back.
    ///
    /// The `g` of [`crcbl_shaders::volumetric::phase`], clamped there to
    /// [`MAX_ANISOTROPY`](crcbl_shaders::volumetric::MAX_ANISOTROPY). Mie
    /// scattering in fog is around `0.8`, which is what makes looking towards
    /// the sun through it bright and looking away from it flat; smoke is
    /// lower. Read only by the froxel path, on
    /// [`sun_scattering`](Self::sun_scattering)'s terms.
    pub anisotropy: f32,
}

impl Fog {
    /// No fog, which is what a renderer nobody has called
    /// [`ForwardRenderer::set_fog`] on carries.
    ///
    /// The colour is black and unobservable: it is read only through the
    /// density, which is zero here.
    ///
    /// [`ForwardRenderer::set_fog`]: crate::forward::ForwardRenderer::set_fog
    pub const NONE: Self = Self {
        density: 0.0,
        falloff: 0.0,
        reference_height: 0.0,
        color: Vec3::ZERO,
        sun_scattering: 0.0,
        anisotropy: 0.0,
    };
}

impl Default for Fog {
    /// [`Fog::NONE`].
    fn default() -> Self {
        Self::NONE
    }
}

/// A gradient sky: three radiances, and the environment they light with.
///
/// `docs/plan/43-render-standards.md` §8. The sky is a smoothstep blend from
/// [`ground`](Self::ground) through [`horizon`](Self::horizon) to
/// [`zenith`](Self::zenith) in a direction's `y`, and what reaches the scene is
/// that field projected onto the L1 spherical-harmonic basis the irradiance
/// grid already speaks — so a surface facing up is lit by the sky and one
/// facing down by the ground, from three colours and no extra pass.
///
/// **It is added to the flat ambient and to the probe grid, not chosen between
/// them**, because all three are the same term: what a diffuse surface receives
/// from an environment it is not looking at directly. An author who wants the
/// sky to be the whole of the environment sets [`DirectionalLight::ambient`] to
/// zero.
///
/// **[`Sky::NONE`] is the default and is exactly no sky.** A black gradient
/// projects to coefficients that are all zero, so the three dot products the
/// fragment stage adds are zero, and every golden blessed before a sky existed
/// still matches.
///
/// Nothing draws the sky yet — this lights with it, and the background is still
/// the scene target's clear colour. [`ForwardRenderer::set_sky`] is what a
/// caller hands this to, and [`crcbl_shaders::sky`] is the projection.
///
/// [`ForwardRenderer::set_sky`]: crate::forward::ForwardRenderer::set_sky
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    /// Radiance straight up.
    ///
    /// **May exceed 1.0**, like [`DirectionalLight::color`] and [`Fog::color`]
    /// and for their reason: the scene target is `Rgba16Float` and this is a
    /// radiance rather than a display colour.
    pub zenith: Vec3,
    /// Radiance at the horizon, where a direction's `y` is zero.
    ///
    /// The band both hemispheres blend away from, so it carries more weight in
    /// the ambient than either pole does.
    pub horizon: Vec3,
    /// Radiance straight down — the ground's bounce, not the ground itself.
    ///
    /// This engine traces nothing to find it, so it is an author's estimate of
    /// what the surface below sends back up.
    pub ground: Vec3,
}

impl Sky {
    /// No sky, which is what a renderer nobody has called
    /// [`ForwardRenderer::set_sky`] on carries.
    ///
    /// Black in every direction, and its projection is every coefficient zero,
    /// so it adds nothing rather than nearly nothing.
    ///
    /// [`ForwardRenderer::set_sky`]: crate::forward::ForwardRenderer::set_sky
    pub const NONE: Self = Self {
        zenith: Vec3::ZERO,
        horizon: Vec3::ZERO,
        ground: Vec3::ZERO,
    };

    /// This sky in the form [`crcbl_shaders::sky`] projects.
    pub(crate) fn gradient(self) -> crcbl_shaders::sky::SkyGradient {
        crcbl_shaders::sky::SkyGradient {
            zenith: self.zenith.to_array(),
            horizon: self.horizon.to_array(),
            ground: self.ground.to_array(),
        }
    }
}

impl Default for Sky {
    /// [`Sky::NONE`].
    fn default() -> Self {
        Self::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASPECT: f32 = 16.0 / 9.0;

    /// A conventional (non-reversed) projection, built only so the tests below
    /// have something to compare against.
    ///
    /// This is what the engine would use if the decision had gone the other way,
    /// and it is the control in every experiment here.
    fn standard_z(near: f32, far: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(FRAC_PI_4, ASPECT, near, far)
    }

    fn reversed() -> Camera {
        Camera {
            eye: Vec3::ZERO,
            target: -Vec3::Z,
            up: Vec3::Y,
            projection: Projection::Perspective {
                fov_y: FRAC_PI_4,
                near: 0.1,
            },
        }
    }

    /// The convention itself: the near plane holds 1.0 and infinity holds 0.0.
    #[test]
    fn perspective_depth_runs_from_one_at_the_near_plane_to_zero_at_infinity() {
        let camera = reversed();
        let near = camera
            .depth_of(Vec3::new(0.0, 0.0, -0.1), ASPECT)
            .expect("in front of the eye");
        assert!(
            (near - crcbl_hal::depth::NEAR).abs() < 1e-5,
            "a point on the near plane must have depth {}, got {near}",
            crcbl_hal::depth::NEAR
        );

        let far = camera
            .depth_of(Vec3::new(0.0, 0.0, -1.0e9), ASPECT)
            .expect("in front of the eye");
        assert!(
            far.abs() < 1e-6,
            "a point at effectively infinity must approach {}, got {far}",
            crcbl_hal::depth::FAR
        );

        // And it is monotonic the *reversed* way: nearer is larger.
        let mut previous = f32::INFINITY;
        for distance in [0.1f32, 0.5, 1.0, 10.0, 100.0, 1000.0, 100_000.0] {
            let depth = camera
                .depth_of(Vec3::new(0.0, 0.0, -distance), ASPECT)
                .expect("in front of the eye");
            assert!(
                depth < previous,
                "depth must decrease with distance under reversed-Z: {distance} m gave \
                 {depth}, which is not below {previous}"
            );
            assert!((0.0..=1.0).contains(&depth), "{distance} m gave {depth}");
            previous = depth;
        }
    }

    /// **The reason reversed-Z is locked**, measured rather than asserted.
    ///
    /// `docs/plan/02-vulkan-backend.md`: "a sector-tiled world with 300 m+
    /// sightlines z-fights immediately on a conventional 0..1 buffer". This test
    /// puts two surfaces one centimetre apart at 300 m and quantises both depths
    /// to `f32` — which is what a `D32_SFLOAT` attachment stores — under *both*
    /// projections.
    ///
    /// Under the engine's reversed projection they are distinguishable. Under
    /// the conventional one they are the *same float*, which is z-fighting with
    /// no dither and no fix. If this module ever stopped being reversed, the
    /// first assertion would fail, and if the control were wrong the second
    /// would.
    #[test]
    fn reversed_z_resolves_a_centimetre_at_300_metres_and_standard_z_does_not() {
        let camera = reversed();
        let near_surface = Vec3::new(0.0, 0.0, -300.0);
        let far_surface = Vec3::new(0.0, 0.0, -300.01);

        let reversed_near = camera.depth_of(near_surface, ASPECT).expect("in front");
        let reversed_far = camera.depth_of(far_surface, ASPECT).expect("in front");
        assert_ne!(
            reversed_near, reversed_far,
            "reversed-Z must resolve 1 cm at 300 m; both quantised to {reversed_near}"
        );
        assert!(
            reversed_near > reversed_far,
            "the nearer surface must hold the larger value under reversed-Z"
        );

        // The control: the same two points through a conventional projection
        // with a generous far plane, which is what this engine deliberately does
        // not use.
        let conventional = standard_z(0.1, 10_000.0);
        let depth = |point: Vec3| {
            let clip = conventional * point.extend(1.0);
            clip.z / clip.w
        };
        let (standard_near, standard_far) = (depth(near_surface), depth(far_surface));
        assert_eq!(
            standard_near, standard_far,
            "the control is only meaningful if conventional depth genuinely \
             collapses here; it gave {standard_near} and {standard_far}, which \
             differ — re-derive the distances rather than relaxing the test"
        );
    }

    /// The seam's clear value and compare op only make sense against *this*
    /// matrix, so the agreement is worth pinning in one place.
    #[test]
    fn the_projection_agrees_with_the_seams_depth_defaults() {
        use crcbl_hal::{ClearValue, CompareOp, DepthStencilState, Format};

        let state = DepthStencilState::default();
        assert_eq!(state.depth_compare, CompareOp::Greater);
        assert_eq!(state.format, Format::D32Float);
        assert_eq!(ClearValue::default().depth, crcbl_hal::depth::CLEAR);

        // Geometry anywhere in front of the camera beats the cleared buffer
        // under `Greater`. Under a conventional projection it would not: a
        // fragment at depth 0.003 fails `Greater` against a clear of 0.0 only
        // marginally, and the *far* geometry would win instead — which is the
        // bug this arrangement exists to make impossible.
        let camera = reversed();
        for distance in [0.11f32, 1.0, 50.0, 5_000.0] {
            let depth = camera
                .depth_of(Vec3::new(0.0, 0.0, -distance), ASPECT)
                .expect("in front");
            assert!(
                depth > ClearValue::default().depth,
                "geometry at {distance} m must beat the cleared far plane, got {depth}"
            );
        }
    }

    /// Milestone 5's claim, checked: **nothing but the projection changes**.
    #[test]
    fn switching_to_orthographic_changes_only_the_projection() {
        let perspective = Camera::default();
        let ortho = perspective.with_projection(Projection::Orthographic {
            half_height: 1.0,
            near: 0.1,
            far: 100.0,
        });

        assert_eq!(perspective.eye, ortho.eye);
        assert_eq!(perspective.target, ortho.target);
        assert_eq!(perspective.up, ortho.up);
        assert_eq!(
            perspective.view(),
            ortho.view(),
            "the view matrix is a property of where the camera is, not of how it projects"
        );
        assert!(!perspective.projection.is_orthographic());
        assert!(ortho.projection.is_orthographic());
        assert_ne!(
            perspective.view_projection(ASPECT),
            ortho.view_projection(ASPECT)
        );
    }

    /// The 2D story: **z is a z-index**, and a larger z draws on top.
    ///
    /// With the engine's `Greater` depth test, "on top" means "larger depth
    /// value", so a sprite at z = 5 must produce a larger depth than one at
    /// z = 1. Under an unreversed orthographic projection it would produce a
    /// *smaller* one and the layering would silently invert — which is exactly
    /// the bug a 2D sample would spend an afternoon on.
    #[test]
    fn orthographic_depth_makes_a_larger_z_draw_on_top() {
        let camera = Camera {
            eye: Vec3::new(0.0, 0.0, 100.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection: Projection::Orthographic {
                half_height: 10.0,
                near: 0.1,
                far: 200.0,
            },
        };

        let mut previous = f32::NEG_INFINITY;
        for z_index in [0.0f32, 1.0, 2.0, 5.0, 20.0] {
            let depth = camera
                .depth_of(Vec3::new(0.0, 0.0, z_index), ASPECT)
                .expect("orthographic w is always 1");
            assert!(
                (0.0..=1.0).contains(&depth),
                "z-index {z_index} projected outside the depth range: {depth}"
            );
            assert!(
                depth > previous,
                "z-index {z_index} gave depth {depth}, which does not beat {previous}; \
                 a larger z must draw on top under the seam's `Greater` test"
            );
            previous = depth;
        }
    }

    /// Orthographic depth is *linear*, which is what makes a z-index usable as
    /// an integer layer number at all.
    #[test]
    fn orthographic_depth_is_linear_in_z() {
        let camera = Camera {
            eye: Vec3::new(0.0, 0.0, 100.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection: Projection::Orthographic {
                half_height: 10.0,
                near: 0.0,
                far: 100.0,
            },
        };
        let depth = |z: f32| {
            camera
                .depth_of(Vec3::new(0.0, 0.0, z), ASPECT)
                .expect("orthographic w is always 1")
        };
        let step = depth(1.0) - depth(0.0);
        for z in [1.0f32, 2.0, 10.0, 50.0] {
            let measured = depth(z + 1.0) - depth(z);
            assert!(
                (measured - step).abs() < 1e-6,
                "the depth step at z={z} is {measured}, not the constant {step}"
            );
        }
        assert!(step > 0.0, "successive layers must increase in depth");
    }

    /// The aspect ratio changes what is visible horizontally and never squashes
    /// the picture — the property a resize depends on.
    #[test]
    fn a_wider_viewport_shows_more_world_and_does_not_stretch_it() {
        for projection in [
            Projection::default(),
            Projection::Orthographic {
                half_height: 1.0,
                near: 0.1,
                far: 10.0,
            },
        ] {
            let square = projection.matrix(1.0);
            let wide = projection.matrix(2.0);
            // The vertical scale is untouched; only the horizontal one changes.
            assert!(
                (square.y_axis.y - wide.y_axis.y).abs() < 1e-6,
                "{projection:?}"
            );
            assert!(wide.x_axis.x < square.x_axis.x, "{projection:?}");
        }
    }

    /// A camera that cannot see is a caller bug, and a NaN matrix is a blank
    /// screen with no error. Both are panics rather than silent output.
    #[test]
    #[should_panic(expected = "aspect ratio")]
    fn a_zero_aspect_ratio_is_refused() {
        let _ = Projection::default().matrix(0.0);
    }

    #[test]
    #[should_panic(expected = "near plane")]
    fn a_zero_near_plane_is_refused() {
        let _ = Projection::Perspective {
            fov_y: FRAC_PI_4,
            near: 0.0,
        }
        .matrix(ASPECT);
    }

    #[test]
    #[should_panic(expected = "must be beyond near")]
    fn an_inverted_orthographic_range_is_refused() {
        let _ = Projection::Orthographic {
            half_height: 1.0,
            near: 10.0,
            far: 1.0,
        }
        .matrix(ASPECT);
    }

    /// The default light points *at* something rather than away from it, is
    /// unit length so the shader's `dot` is a cosine, and is bright enough that
    /// the HDR target has something to carry.
    #[test]
    fn the_default_light_is_normalised_and_has_headroom() {
        let light = DirectionalLight::default();
        assert!((light.direction.length() - 1.0).abs() < 1e-6);
        assert!(
            light.ambient.min_element() > 0.0,
            "an unlit face must not be black"
        );
        assert!(
            light.ambient.max_element() < 0.5,
            "ambient must not flatten the shading"
        );

        // **HDR from P1 needs something above 1.0 to carry.** The brightest
        // albedo in the engine's demo cube is 0.9, so an intensity below
        // `1 / 0.9` could never push any surface past what an `Rgba8` target
        // holds, and the e2e suite's HDR assertion would be checking a format
        // enum rather than a pipeline.
        assert!(
            light.color.max_element() > 1.0 / 0.9,
            "the key light must have HDR headroom, got {:?}",
            light.color
        );

        // And it comes from the camera's side, so the faces the default camera
        // can see are the faces it lights.
        let to_camera = Camera::default().eye.normalize();
        assert!(
            light.direction.dot(to_camera) > 0.0,
            "a key light pointing away from every visible face renders an \
             unlit cube: light {:?} against camera {to_camera:?}",
            light.direction
        );
    }

    /// A point behind the eye is not in front of the camera, and `depth_of`
    /// says so. The guard used to be `w.abs()`, which handed back a plausible
    /// negative depth for geometry the frustum does not contain — and the doc
    /// comment above it said the opposite.
    #[test]
    fn a_point_behind_the_eye_has_no_depth() {
        let camera = reversed();
        // The camera sits at the origin looking down -Z, so anything at
        // positive Z is behind it and the eye itself is on the eye plane.
        assert_eq!(camera.depth_of(Vec3::new(0.0, 0.0, 10.0), ASPECT), None);
        assert_eq!(camera.depth_of(camera.eye, ASPECT), None);
        assert!(camera.depth_of(Vec3::new(0.0, 0.0, -1.0), ASPECT).is_some());
    }

    /// An orthographic camera has no eye plane, so every point projects.
    #[test]
    fn an_orthographic_camera_projects_everything() {
        let camera = reversed().with_projection(Projection::Orthographic {
            half_height: 2.0,
            near: 0.1,
            far: 100.0,
        });
        assert!(
            camera.depth_of(Vec3::new(0.0, 0.0, 10.0), ASPECT).is_some(),
            "an orthographic camera has no eye plane to be behind"
        );
    }

    /// A degenerate basis makes `look_at` divide by a zero-length vector and
    /// return `NaN`s, which reach the shader as a blank screen and no error.
    /// Every other degenerate input to this module is asserted; so is this one.
    #[test]
    #[should_panic(expected = "parallel to its view direction")]
    fn an_up_vector_along_the_view_direction_panics() {
        let camera = Camera {
            eye: Vec3::new(0.0, 0.0, 2.0),
            target: Vec3::ZERO,
            up: Vec3::Z,
            projection: Projection::default(),
        };
        let _ = camera.view();
    }

    #[test]
    #[should_panic(expected = "up vector must be non-zero")]
    fn a_zero_up_vector_panics() {
        let camera = Camera {
            up: Vec3::ZERO,
            ..Camera::default()
        };
        let _ = camera.view();
    }

    #[test]
    #[should_panic(expected = "must look somewhere")]
    fn a_camera_looking_at_its_own_eye_panics() {
        let camera = Camera {
            target: Vec3::new(0.0, 0.0, 2.0),
            ..Camera::default()
        };
        let _ = camera.view();
    }
}
