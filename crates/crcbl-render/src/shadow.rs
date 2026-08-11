//! The sun's cascaded shadow map: where each cascade looks, and how far.
//!
//! `docs/plan/18-render-features.md`'s shadow section, arithmetic half. This
//! module owns the matrices; [`crate::forward`] owns the atlas, the depth-only
//! pipeline and the per-cascade cull that fills them. The split is where the
//! testable part is: a cascade matrix is a pure function of a camera and a light
//! direction, so every claim below — that the cascades are stable, that they
//! cover what they say they cover, that the snap is a whole number of texels —
//! is checked here without a device in the room.
//!
//! # One atlas, [`CASCADES`] tiles wide
//!
//! The cascades live side by side in a single `D32Float` image rather than as
//! layers of an array. That is a consequence of the render graph, not a
//! preference: a [`crate::graph`] render pass attaches an *image*, and there is
//! no way to attach layer `i` of one. Tiles need nothing the graph does not
//! already have — the shadow pass sets a viewport over tile `i` and draws — so
//! the cascade count is a constant either side of the seam and not a feature
//! request. `mesh.slang`'s `shadow_atlas` says the same from the sampling side.
//!
//! # Stability: a sphere around the eye, snapped to texels
//!
//! The classic cascade shimmer is a shadow edge crawling along a static surface
//! while the camera moves. It has two causes and this module removes both:
//!
//! * **The cascade's extent changing with the camera's orientation.** A box
//!   fitted to the split's view frustum is tight but rotates with the camera, so
//!   every frame resamples the same geometry at a different scale. [`Cascades`]
//!   uses a *sphere* instead, and its radius is [`Cascades::far`] — a number that
//!   depends on the split alone. Rotating the camera cannot change it.
//! * **The cascade's origin moving by a fraction of a texel.** Fixed by
//!   quantising the light-space origin to whole texels, which is what
//!   [`snap_to_texel`] does and what the ortho box below is built around.
//!
//! The cost of the sphere is resolution: a sphere of radius `r` centred on the
//! eye covers everything within `r`, including the half of it behind the camera
//! that will never be shaded. A frustum-fitted box would be roughly twice as
//! dense for the same tile. That is the trade, taken deliberately: the tight fit
//! is the version that has to branch on [`Projection`] — an orthographic camera
//! has no field of view to build corners from — and a shadow pass that is
//! correct on one projection and wrong on the other is worse than one that is
//! uniformly coarser.

use glam::{Mat4, Vec3, Vec4Swizzles};

use crate::camera::Camera;

/// How many cascades the sun's shadow map is split into.
///
/// The shader's number, not a second one: `crcbl_shaders::mesh` is where it
/// lives and where it is checked against the `.slang` sources, because the
/// uniform block's layout depends on it.
pub const CASCADES: usize = crcbl_shaders::mesh::SHADOW_CASCADES;

/// The side of one cascade's tile in the shadow atlas, in texels.
///
/// The atlas is therefore `TILE * CASCADES` by `TILE`. One number for every
/// cascade — a per-cascade resolution is what a shadow *atlas* with a packing
/// policy would buy, and topic 18 puts atlases post-MVP.
pub const TILE: u32 = 1024;

/// How far from the eye the sun's shadows reach, in world units.
///
/// Past this every surface is lit: the last cascade's box ends, the sampling
/// code takes its out-of-bounds path, and a distant hillside is simply not
/// shadowed. That is the honest failure for a shadow map, and the alternative —
/// extending the last cascade to the far plane — spends the whole atlas on
/// geometry too far away to see the result on.
///
/// Not a field on [`DirectionalLight`](crate::DirectionalLight): it is a
/// property of how much shadow detail a *view* is willing to pay for, which
/// topic 18 puts on the per-camera stack when there is one to put it on.
pub const DISTANCE: f32 = 24.0;

/// How much of the practical split scheme is logarithmic rather than uniform.
///
/// The two extremes are both wrong in the usual way: a uniform split gives the
/// nearest cascade — the one filling most of the screen — almost no resolution,
/// and a fully logarithmic one gives the far cascades so little that their
/// texels are visible as steps. `0.7` is the conventional compromise and is
/// where every reference implementation of this scheme sits.
const SPLIT_LAMBDA: f32 = 0.7;

/// Distance in front of the light's near plane that still writes depth, in world
/// units.
///
/// A caster between the sun and the cascade's sphere is outside the sphere and
/// must still darken what is inside it, so the light's box is pulled back this
/// far and its near plane put at zero. Too small and a tall object stops casting
/// as the camera approaches its base; too large and the depth range — and with
/// it the meaning of [`CONSTANT_BIAS`] — stretches for nothing.
const CASTER_REACH: f32 = 40.0;

/// The constant part of the shadow comparison's depth bias, in shadow-clip
/// depth.
///
/// Reversed-Z, so this is *added* to the receiver's depth: it moves the
/// reference towards the light, which is the direction that stops a surface
/// shadowing itself. See `sun_visibility` in `shaders/mesh.slang`.
const CONSTANT_BIAS: f32 = 0.0015;

/// The slope-scaled part, per unit of `tan(acos(N·L))`.
///
/// A surface nearly edge-on to the light spans many times its own depth across
/// one shadow texel, so a constant bias that suits a face pointing at the sun is
/// nowhere near enough there. The shader clamps how far this can go, because the
/// unbounded version detaches a shadow from its caster.
const SLOPE_BIAS: f32 = 0.0025;

/// The cascade matrices a frame shades and culls with.
///
/// One [`Mat4`] per cascade plus the split distances the fragment stage selects
/// between them with. Everything here is what goes into
/// `crcbl_shaders::mesh::FrameUniforms`; nothing in it names a GPU resource.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascades {
    /// World → cascade `i`'s shadow clip. Orthographic and **reversed-Z**, like
    /// every other projection in the engine.
    pub view_proj: [Mat4; CASCADES],
    /// How far from the eye cascade `i` reaches, in world units, and therefore
    /// the radius of the sphere it is fitted to.
    ///
    /// A `[f32; 4]` rather than a `[f32; CASCADES]` because that is the shape
    /// the uniform block's `float4` takes; components past [`CASCADES`] repeat
    /// the last real split, so an out-of-range read — which cannot happen —
    /// would select the widest cascade rather than a zero that shadows
    /// everything.
    pub far: [f32; 4],
}

impl Cascades {
    /// The cascade split distances, nearest first.
    ///
    /// The practical split scheme: a blend of a logarithmic and a uniform
    /// division of `near .. DISTANCE`, weighted by [`SPLIT_LAMBDA`].
    fn splits(near: f32) -> [f32; CASCADES] {
        let mut out = [DISTANCE; CASCADES];
        for (index, far) in out.iter_mut().enumerate() {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a cascade index is at most four"
            )]
            let ratio = (index + 1) as f32 / CASCADES as f32;
            let logarithmic = near * (DISTANCE / near).powf(ratio);
            let uniform = near.mul_add(1.0 - ratio, DISTANCE * ratio);
            *far = SPLIT_LAMBDA.mul_add(logarithmic, (1.0 - SPLIT_LAMBDA) * uniform);
        }
        out
    }

    /// Builds this frame's cascades for `camera` under a light shining *from*
    /// `to_light`.
    ///
    /// `to_light` is the same "towards the light" convention
    /// [`DirectionalLight::direction`](crate::DirectionalLight::direction) uses,
    /// and it does not have to be normalised.
    ///
    /// # Panics
    ///
    /// If `to_light` has no direction — a zero or non-finite vector — or if the
    /// camera is degenerate, on [`Camera::view`]'s terms. Both produce a matrix
    /// of `NaN`s that reaches the shader as an empty shadow map, which is the
    /// failure mode this whole slice can hide behind.
    #[must_use]
    pub fn new(camera: &Camera, to_light: Vec3) -> Self {
        let to_light = to_light.normalize_or_zero();
        assert!(
            to_light.length_squared() > 0.0,
            "the sun must point somewhere: {to_light:?} has no direction"
        );
        // Only the eye is read, so an orthographic camera needs no special case:
        // the sphere is centred on the eye whatever the projection is.
        let eye = camera.eye;
        assert!(eye.is_finite(), "the camera's eye is not finite: {eye:?}");

        let near = match camera.projection {
            crate::camera::Projection::Perspective { near, .. }
            | crate::camera::Projection::Orthographic { near, .. } => near.max(f32::MIN_POSITIVE),
        };
        let splits = Self::splits(near);

        let mut view_proj = [Mat4::IDENTITY; CASCADES];
        for (index, matrix) in view_proj.iter_mut().enumerate() {
            *matrix = cascade_matrix(eye, to_light, splits[index]);
        }
        // The last real split fills the unused components; see `far`.
        let mut far = [splits[CASCADES - 1]; 4];
        far[..CASCADES].copy_from_slice(&splits);
        Self { view_proj, far }
    }

    /// The `shadow_params` vector the fragment stage reads: the atlas's two
    /// texel sizes, then the constant and slope-scaled biases.
    ///
    /// The two texel sizes are not equal and deriving one from the other is the
    /// bug this returns both to prevent — the atlas is [`CASCADES`] tiles wide
    /// and one tall.
    #[must_use]
    pub fn params() -> [f32; 4] {
        let (width, height) = atlas_extent();
        #[expect(
            clippy::cast_precision_loss,
            reason = "an atlas extent is a few thousand texels"
        )]
        let inverse = [1.0 / width as f32, 1.0 / height as f32];
        [inverse[0], inverse[1], CONSTANT_BIAS, SLOPE_BIAS]
    }
}

/// The shadow atlas's extent in texels: [`CASCADES`] tiles side by side.
#[must_use]
pub const fn atlas_extent() -> (u32, u32) {
    (TILE * CASCADES as u32, TILE)
}

/// Where cascade `index`'s tile starts in the atlas, in texels.
#[must_use]
pub const fn tile_origin(index: usize) -> (u32, u32) {
    (TILE * index as u32, 0)
}

/// One cascade's world → shadow-clip matrix: an orthographic box around a
/// sphere of `radius` centred on `eye`, snapped to whole texels.
fn cascade_matrix(eye: Vec3, to_light: Vec3, radius: f32) -> Mat4 {
    // A basis that is not parallel to the light. `Vec3::Y` is the usual up and
    // is wrong for exactly one light — one straight overhead, which is the
    // commonest light there is — so the fallback is picked rather than asserted
    // against.
    let up = if to_light.dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    // **The light's view is anchored at the world origin, not at the camera.**
    // That is what makes the snap below mean anything: a view built from the
    // eye moves whenever the eye does, so quantising a position *inside* it
    // quantises a coordinate whose own frame is already sliding — the matrix
    // changes for a sub-texel step and the shimmer the snap exists to remove is
    // still there. Anchored, the frame is a pure function of the light, and
    // every cascade of every frame measures against the same one.
    let view = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, -to_light, up);

    // The snap. The sphere's centre is expressed in that fixed light frame and
    // quantised to whole texels, so a camera that moves by less than one
    // texel's worth of world produces a byte-identical matrix and the shadow
    // edges on a static surface do not move at all.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a tile is a few thousand texels"
    )]
    let texel = 2.0 * radius / TILE as f32;
    let centre = snap_to_texel((view * eye.extend(1.0)).xyz(), texel);

    // Right-handed view space looks down `-z`, so a light-space depth is `-z`.
    // The near end is pulled back past the sphere so a caster standing between
    // the sun and it still writes depth; the far end is the back of the sphere
    // and nothing beyond it can shadow anything inside.
    let farthest = radius - centre.z;
    let nearest = -centre.z - radius - CASTER_REACH;

    glam::camera::rh::proj::directx::orthographic(
        centre.x - radius,
        centre.x + radius,
        centre.y - radius,
        centre.y + radius,
        // **Swapped, and that is the whole reversal** — the same trick, for the
        // same reason, as `Projection::Orthographic`'s. The constructor maps its
        // fifth argument to depth 0 and its sixth to depth 1, so handing it the
        // far distance and then the near one puts 1.0 at the light. That is
        // reversed-Z, and it is what `CompareOp::Greater` everywhere else
        // expects.
        farthest,
        nearest,
    ) * view
}

/// Quantises a light-space position to whole `texel`-sized steps.
///
/// Separate from [`cascade_matrix`] because it is the half that is checkable on
/// its own: "the snapped value is a whole number of texels" is an assertion, and
/// "the shadow does not shimmer" is not.
///
/// All three components, not just the two across the map: the depth range is
/// derived from `z`, so leaving it unquantised would move the near and far
/// planes every frame and with them the depth a given caster stores — which is
/// the same shimmer one axis further in, and it changes what [`CONSTANT_BIAS`]
/// is worth.
fn snap_to_texel(position: Vec3, texel: f32) -> Vec3 {
    if texel <= 0.0 || !texel.is_finite() {
        return position;
    }
    (position / texel).floor() * texel
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Projection;

    fn camera_at(eye: Vec3) -> Camera {
        Camera {
            eye,
            target: eye + Vec3::NEG_Z,
            ..Camera::default()
        }
    }

    const SUN: Vec3 = Vec3::new(0.4, 0.8, 0.6);

    /// The splits must be strictly increasing and must end exactly at the
    /// distance shadows are declared to reach.
    ///
    /// A scheme that overshot would leave the last cascade covering geometry the
    /// fragment stage has already decided is out of range; one that undershot
    /// would leave a lit gap before it.
    #[test]
    fn the_splits_increase_and_end_at_the_shadow_distance() {
        let splits = Cascades::splits(0.1);
        assert!(
            (splits[CASCADES - 1] - DISTANCE).abs() < 1e-4,
            "the last split is the shadow distance: {splits:?}"
        );
        for window in splits.windows(2) {
            assert!(
                window[1] > window[0],
                "the splits must increase: {splits:?}"
            );
        }
        assert!(splits[0] > 0.0, "a cascade with no reach: {splits:?}");
    }

    /// **The cascade must actually contain the split it names.**
    ///
    /// This is the assertion that fails if the sphere, the pull-back or the
    /// reversed-Z swap is wrong, and it is deliberately about points rather than
    /// about the matrix: a point at the edge of the cascade's reach, in every
    /// direction, must land inside the unit box with a depth in `0..1`. A
    /// projection that mapped everything outside the box would render an empty
    /// shadow map, which is the exact failure a golden image cannot see.
    #[test]
    fn every_point_within_a_cascades_reach_lands_inside_its_box() {
        let camera = camera_at(Vec3::new(3.0, 1.0, -2.0));
        let cascades = Cascades::new(&camera, SUN);
        for (index, matrix) in cascades.view_proj.iter().enumerate() {
            let radius = cascades.far[index];
            for direction in [
                Vec3::X,
                Vec3::NEG_X,
                Vec3::Y,
                Vec3::NEG_Y,
                Vec3::Z,
                Vec3::NEG_Z,
                Vec3::ONE.normalize(),
            ] {
                // Just inside the sphere, so the texel snap's half-texel of
                // slack cannot be what carries the point.
                let point = camera.eye + direction * (radius * 0.98);
                let clip = *matrix * point.extend(1.0);
                let ndc = clip.xyz() / clip.w;
                assert!(
                    ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0,
                    "cascade {index} does not cover {direction:?}: ndc {ndc:?}"
                );
                assert!(
                    (0.0..=1.0).contains(&ndc.z),
                    "cascade {index} puts {direction:?} outside its depth range: ndc {ndc:?}"
                );
            }
        }
    }

    /// Reversed-Z, stated as the only thing that distinguishes it: a point
    /// nearer the light gets the **larger** depth.
    ///
    /// The whole shadow comparison is `Greater`, so a matrix that mapped depth
    /// the conventional way would invert every shadow in the scene — lighting
    /// exactly what it should darken, which reads as a bug in the light
    /// direction rather than in the projection.
    #[test]
    fn a_caster_nearer_the_light_gets_the_larger_depth() {
        let camera = camera_at(Vec3::ZERO);
        let cascades = Cascades::new(&camera, SUN);
        let to_light = SUN.normalize();
        let matrix = cascades.view_proj[0];
        let near_light = (matrix * (to_light * 2.0).extend(1.0)).z;
        let far_light = (matrix * (to_light * -2.0).extend(1.0)).z;
        assert!(
            near_light > far_light,
            "reversed-Z: {near_light} must exceed {far_light}"
        );
    }

    /// **Rotating the camera must not change a cascade at all**, which is the
    /// property the sphere buys and the reason it is a sphere.
    ///
    /// A box fitted to the split's view frustum would fail this outright, and
    /// the visible consequence is a shadow edge crawling across a wall while the
    /// player turns on the spot.
    #[test]
    fn turning_on_the_spot_leaves_every_cascade_alone() {
        let eye = Vec3::new(-2.0, 3.0, 5.0);
        let looking_north = Cascades::new(&camera_at(eye), SUN);
        let looking_east = Cascades::new(
            &Camera {
                eye,
                target: eye + Vec3::X,
                ..Camera::default()
            },
            SUN,
        );
        assert_eq!(
            looking_north, looking_east,
            "a cascade must not depend on where the camera looks"
        );
        // And the projection type is not a rotation either: an orthographic
        // camera at the same place gets the same cascades, because only the eye
        // and the near plane are read.
        let orthographic = Cascades::new(
            &camera_at(eye).with_projection(Projection::Orthographic {
                half_height: 4.0,
                near: 0.1,
                far: 50.0,
            }),
            SUN,
        );
        assert_eq!(looking_north, orthographic);
    }

    /// Sub-texel camera motion must produce a **byte-identical** matrix.
    ///
    /// This is the snap, and it is asserted as equality rather than as "close"
    /// on purpose: the shimmer it removes is caused by a difference far smaller
    /// than any tolerance a test would pick, so anything short of equality would
    /// pass with the snap deleted.
    #[test]
    fn moving_less_than_one_texel_changes_nothing() {
        let eye = Vec3::new(1.0, 2.0, 3.0);
        let splits = Cascades::splits(0.1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a tile is a few thousand texels"
        )]
        let texel = 2.0 * splits[0] / TILE as f32;
        let still = Cascades::new(&camera_at(eye), SUN);
        // A tenth of a texel, along an axis the light basis is not parallel to.
        let nudged = Cascades::new(&camera_at(eye + Vec3::X * texel * 0.1), SUN);
        assert_eq!(
            still.view_proj[0], nudged.view_proj[0],
            "a sub-texel move must snap to the same origin"
        );
        // And a move of many texels must *not* — or the assertion above would
        // be passing because the matrix ignores the eye entirely.
        let moved = Cascades::new(&camera_at(eye + Vec3::X * texel * 40.0), SUN);
        assert_ne!(
            still.view_proj[0], moved.view_proj[0],
            "the cascade must follow the camera at all"
        );
    }

    /// A light straight overhead is the commonest light there is, and it is the
    /// one the obvious up vector is parallel to.
    #[test]
    fn a_light_straight_overhead_still_produces_a_basis() {
        let cascades = Cascades::new(&camera_at(Vec3::ZERO), Vec3::Y);
        for (index, matrix) in cascades.view_proj.iter().enumerate() {
            assert!(
                matrix.to_cols_array().iter().all(|value| value.is_finite()),
                "cascade {index} is not finite under an overhead sun: {matrix:?}"
            );
        }
        // And it is a real projection, not a degenerate one: the origin is
        // inside the box.
        let clip = cascades.view_proj[0] * Vec3::ZERO.extend(1.0);
        let ndc = clip.xyz() / clip.w;
        assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z));
    }

    /// The atlas is tiles side by side, so its two texel sizes differ — and the
    /// shader derives its PCF kernel from both.
    #[test]
    fn the_atlas_is_wider_than_it_is_tall_and_says_so() {
        let (width, height) = atlas_extent();
        assert_eq!(width, TILE * CASCADES as u32);
        assert_eq!(height, TILE);
        let params = Cascades::params();
        #[expect(clippy::cast_precision_loss, reason = "an extent is a few thousand")]
        let expected = [1.0 / width as f32, 1.0 / height as f32];
        assert!((params[0] - expected[0]).abs() < f32::EPSILON);
        assert!((params[1] - expected[1]).abs() < f32::EPSILON);
        assert!(params[2] > 0.0 && params[3] > 0.0, "the bias must bias");
        for index in 0..CASCADES {
            assert_eq!(tile_origin(index), (TILE * index as u32, 0));
        }
    }
}
