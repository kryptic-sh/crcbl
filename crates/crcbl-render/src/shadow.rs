//! Where each shadow map looks, and how far: the sun's cascades and a spot
//! light's cone.
//!
//! `docs/plan/18-render-features.md`'s shadow section, arithmetic half. This
//! module owns the matrices and the tile budget; [`crate::forward`] owns the
//! atlas, the depth-only pipeline and the per-tile cull that fills them. The
//! split is where the testable part is: a cascade matrix is a pure function of a
//! camera and a light direction, a spot's is a pure function of the light, and
//! the tile budget is a pure function of the light list — so every claim below
//! is checked here without a device in the room.
//!
//! # One atlas, a fixed grid of [`TILES`] tiles
//!
//! The maps live side by side in a single `D32Float` image rather than as layers
//! of an array. That is a consequence of the render graph, not a preference: a
//! [`crate::graph`] render pass attaches an *image*, and there is no way to
//! attach layer `i` of one. Tiles need nothing the graph does not already have —
//! the shadow pass sets a viewport over tile `i` and draws — so the tile count is
//! a constant either side of the seam and not a feature request.
//! `mesh.slang`'s `shadow_atlas` says the same from the sampling side.
//!
//! The grid is [`ATLAS_COLUMNS`] by [`ATLAS_ROWS`] and the split of it is topic
//! 18's 2026-08-13 decision: **the sun's cascades take the first [`CASCADES`]
//! tiles and the rest are handed out one per shadowed spot**, with
//! [`SHADOW_LIGHTS`] of them. A light that gets no tile still lights and simply
//! does not occlude, which is what makes the budget a quality knob rather than a
//! correctness cliff — see [`Selection`].
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
//!   `snap_to_texel` does and what the ortho box below is built around.
//!
//! The cost of the sphere is resolution: a sphere of radius `r` centred on the
//! eye covers everything within `r`, including the half of it behind the camera
//! that will never be shaded. A frustum-fitted box would be roughly twice as
//! dense for the same tile. That is the trade, taken deliberately: the tight fit
//! is the version that has to branch on [`Projection`](crate::Projection) — an
//! orthographic camera has no field of view to build corners from — and a shadow
//! pass that is correct on one projection and wrong on the other is worse than
//! one that is uniformly coarser.

use glam::{Mat4, Vec3, Vec4Swizzles};

use crate::camera::Camera;
use crate::light::{Light, SpotLight};

/// How many cascades the sun's shadow map is split into.
///
/// The shader's number, not a second one: `crcbl_shaders::mesh` is where it
/// lives and where it is checked against the `.slang` sources, because the
/// uniform block's layout depends on it.
pub const CASCADES: usize = crcbl_shaders::mesh::SHADOW_CASCADES;

/// How many shadowed lights the atlas has room for beside the cascades.
///
/// The shader's number for the same reason [`CASCADES`] is: the frame block
/// carries one matrix per slot, so a block sized differently on the two sides
/// puts every member after it at the wrong offset.
pub const SHADOW_LIGHTS: usize = crcbl_shaders::mesh::SHADOW_LIGHTS;

/// The side of one tile in the shadow atlas, in texels.
///
/// One number for every tile — a per-map resolution is what a shadow atlas with
/// a packing policy would buy, and topic 18 puts packing post-MVP.
pub const TILE: u32 = 1024;

/// Tiles across the atlas.
pub const ATLAS_COLUMNS: u32 = 2;

/// Tiles down it.
///
/// A grid rather than one row, and that is what the row above is for: a point
/// light is six tiles of exactly this kind, so the next slice widens the grid
/// rather than reshaping the atlas. The addressing below is already written in
/// terms of both extents.
pub const ATLAS_ROWS: u32 = 2;

/// Tiles in the whole atlas: [`CASCADES`] for the sun and [`SHADOW_LIGHTS`] for
/// the lights that fit.
pub const TILES: usize = (ATLAS_COLUMNS * ATLAS_ROWS) as usize;

const _: () = assert!(
    TILES == CASCADES + SHADOW_LIGHTS,
    "every tile of the grid is either a cascade's or a light's; a grid with a \
     tile nothing owns is atlas nothing writes and nothing samples"
);

/// The widest half-angle a spot can have and still be given a tile, in radians.
///
/// A spot's map is one perspective projection whose field of view is twice the
/// cone's outer half-angle, and a projection's field of view cannot reach 180°:
/// the tangent runs to infinity and the matrix stops being one. So a cone wider
/// than this is **refused a tile** rather than given a map that covers part of
/// it — [`Selection`] is where that happens, and the light then lights without
/// occluding, which is the same honest degradation as running out of tiles.
///
/// 80° leaves `tan` at under six, so the near plane's footprint stays a sane
/// multiple of the tile it is rendered into.
pub const MAX_SPOT_HALF_ANGLE: f32 = 80.0 * std::f32::consts::PI / 180.0;

/// A spot's near plane, in world units.
///
/// Everything nearer than this to the light is inside it and casts nothing. It
/// is the only knob a perspective shadow map's depth distribution really has —
/// under reversed-Z the precision piles up at the near plane, so this being
/// small is what makes a caster far from the light cheap in depth resolution and
/// not the other way round.
const SPOT_NEAR: f32 = 0.05;

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
    /// **Both texel sizes, even where the grid is square and they are equal.**
    /// The shader's PCF kernel steps in *tile* space and scales by
    /// [`ATLAS_COLUMNS`] and [`ATLAS_ROWS`] to get back to one shadow-map texel,
    /// so a grid that is not square — which it stops being the moment a point
    /// light's six tiles arrive — needs the two apart. Deriving one from the
    /// other is how that kernel ends up sampling a rectangle.
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

/// The shadow atlas's extent in texels: an [`ATLAS_COLUMNS`] by [`ATLAS_ROWS`]
/// grid of [`TILE`]-sided tiles.
#[must_use]
pub const fn atlas_extent() -> (u32, u32) {
    (TILE * ATLAS_COLUMNS, TILE * ATLAS_ROWS)
}

/// Where tile `index` starts in the atlas, in texels.
///
/// Row-major, so the sun's cascades are tiles `0..CASCADES` and land in the top
/// row while [`ATLAS_COLUMNS`] is at least [`CASCADES`]. That is what keeps a
/// cascade's rasterised tile byte-identical across a change to the grid's shape
/// — the cascade goldens are what say so.
#[must_use]
pub const fn tile_origin(index: usize) -> (u32, u32) {
    let index = index as u32;
    (
        TILE * (index % ATLAS_COLUMNS),
        TILE * (index / ATLAS_COLUMNS),
    )
}

/// Which tile shadowed-light slot `slot` renders into.
///
/// The one place the "cascades first, then the lights" split is written down.
/// Both the viewport the shadow pass sets and the tile `mesh.slang` samples come
/// through it — the shader's own `light_tile` is the same arithmetic, and
/// `crcbl_shaders::mesh`'s drift test is what holds the two together.
#[must_use]
pub const fn light_tile(slot: usize) -> usize {
    CASCADES + slot
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

/// A spot's world → shadow-clip matrix: a **perspective** projection down the
/// cone, reversed-Z, covering the cone exactly.
///
/// # Why there is no texel snap here
///
/// The cascades are snapped to whole texels because their box follows the
/// camera, so a sub-texel step of the eye would otherwise resample a static
/// surface at a new offset and crawl its shadow edges. Nothing about a spot's
/// map depends on the camera: it is a pure function of the light's own position,
/// axis and cone, so a still light produces a byte-identical matrix frame after
/// frame and there is nothing to quantise away. `a_still_spot_produces_the_same_matrix`
/// is the assertion, and it is the same guarantee the snap buys the sun rather
/// than a weaker one.
///
/// The snap could not transfer even if it were wanted: it works by quantising a
/// position in a *fixed light frame* whose texel size is constant across the
/// whole map, and a perspective map's texel covers a world footprint that grows
/// linearly with depth. There is no one step to round to.
///
/// # Panics
///
/// If the light's axis has no direction or its position is not finite. Both
/// produce a matrix of `NaN`s, which reaches the shader as a map that shadows
/// nothing — the failure this whole slice can hide behind.
#[must_use]
pub fn spot_matrix(spot: &SpotLight) -> Mat4 {
    let axis = spot.direction.normalize_or_zero();
    assert!(
        axis.length_squared() > 0.0,
        "a spot must point somewhere: {:?} has no direction",
        spot.direction
    );
    assert!(
        spot.position.is_finite(),
        "the spot's position is not finite: {:?}",
        spot.position
    );

    // A basis that is not parallel to the axis, on `cascade_matrix`'s terms and
    // for its reason: a spot pointing straight down is the commonest spot there
    // is, and it is exactly the one `Vec3::Y` cannot be the up vector of.
    let up = if axis.dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let view = glam::camera::rh::view::look_at_mat4(spot.position, spot.position + axis, up);

    // The cone's outer half-angle is the projection's *half* field of view, so
    // the map covers the cone and no more: every texel of the tile is somewhere
    // the light can reach. `SpotLight::outer_angle` may be narrower than the
    // inner one on the way in — `Light::row` widens it rather than trusting it
    // — so the same widening happens here, or the map would be narrower than the
    // cone it is sampled for.
    let outer = spot.outer_angle.max(spot.inner_angle);
    let far = spot.radius.max(SPOT_NEAR * 2.0);

    glam::camera::rh::proj::directx::perspective(
        2.0 * outer,
        // Square: the tile is, and a cone has no other aspect ratio to have.
        1.0,
        // **Swapped, and that is the whole reversal** — the same trick, for the
        // same reason, as `cascade_matrix`'s orthographic pair and
        // `Projection::Perspective`'s. The constructor maps its third argument
        // to depth 0 and its fourth to depth 1, so handing it the far distance
        // and then the near one puts 1.0 at the light. That is reversed-Z, and
        // it is what `CompareOp::Greater` everywhere else expects.
        far,
        SPOT_NEAR,
    ) * view
}

/// Whether a spot can be given a tile at all.
///
/// Split out so the reason a light was refused is one predicate rather than a
/// condition buried in a fold: a zero radius has no frustum, and a cone at or
/// past [`MAX_SPOT_HALF_ANGLE`] has no projection. Both light without occluding.
fn can_be_shadowed(light: &Light) -> bool {
    match light {
        // Point lights are six tiles and a face selection, which is the next
        // slice. Refused by name rather than by falling through a match, so a
        // point light in the list is a light that lights and does not occlude
        // rather than one that silently takes a tile it cannot fill.
        Light::Point(_) => false,
        Light::Spot(spot) => {
            spot.radius > 0.0
                && spot.direction.normalize_or_zero().length_squared() > 0.0
                && spot.position.is_finite()
                && spot.outer_angle.max(spot.inner_angle) < MAX_SPOT_HALF_ANGLE
        }
    }
}

/// How much of the screen a light is worth, as topic 18's rule states it:
/// **radius over distance to the eye**.
///
/// The same metric family `docs/plan/25-lod.md`'s level selection uses, so there
/// is one notion of "how much does this matter on screen" in the engine rather
/// than two. The distance is floored so a light the eye is standing inside is
/// the most important light there is rather than an infinity.
fn influence(light: &Light, eye: Vec3) -> f32 {
    let (position, radius) = light.sphere();
    radius / eye.distance(position).max(1e-4)
}

/// How much better a challenger must be before it takes a tile off the light
/// holding it.
///
/// The selection's hysteresis, and it is owed for the reason
/// `docs/plan/25-lod.md`'s was: two lights either side of the cutoff would
/// otherwise swap tiles whenever the camera drifts, and a shadow appearing and
/// disappearing frame to frame is far worse than the wrong one of two being
/// shadowed. Applied as a bonus on the incumbent's score, which is the same
/// shape as `ForwardRenderer::lod_hold_ratio` — a level is held until the error
/// clearly drops.
const HOLD_RATIO: f32 = 1.25;

/// Which lights hold the atlas's light tiles, and its memory of last frame.
///
/// Topic 18's 2026-08-13 rule, whole: eligible lights are ranked by
/// projected screen influence, ties break by index so a frame's answer does not
/// depend on the
/// order a caller happened to build the list in, the first [`SHADOW_LIGHTS`] of
/// them get tiles, and an incumbent keeps its tile until a challenger clearly
/// beats it.
///
/// **A light that gets no tile still lights.** Nothing here removes a light from
/// the frame; it decides which of them also occlude.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// Which light index holds slot `i`, or `None` if the slot is free.
    slots: [Option<usize>; SHADOW_LIGHTS],
}

impl Selection {
    /// Re-runs the selection over `lights` for an eye at `eye`.
    ///
    /// `lights` is the caller's own list and the indices below are into it; a
    /// caller whose shader rows are offset from it — the sun is row 0 — maps
    /// them itself.
    pub fn update(&mut self, lights: &[Light], eye: Vec3) {
        // An incumbent that has dropped off the end of the list, or stopped
        // being eligible, holds nothing: the list is rebuilt every frame and an
        // index is only meaningful against the list it was taken from.
        for slot in &mut self.slots {
            if !slot.is_some_and(|index| lights.get(index).is_some_and(can_be_shadowed)) {
                *slot = None;
            }
        }

        // Score every eligible light, with the incumbents' scores boosted. That
        // ordering *is* the hysteresis: a challenger enters the top
        // `SHADOW_LIGHTS` only by beating the incumbent it displaces by
        // `HOLD_RATIO`, which is the statement this constant is written as.
        let mut ranked: Vec<(usize, f32)> = lights
            .iter()
            .enumerate()
            .filter(|(_, light)| can_be_shadowed(light))
            .map(|(index, light)| {
                let held = self.slots.contains(&Some(index));
                let score = influence(light, eye) * if held { HOLD_RATIO } else { 1.0 };
                (index, score)
            })
            .collect();
        // Descending by score, ties by index. `total_cmp` rather than
        // `partial_cmp`: a `NaN` score would otherwise make the comparator
        // inconsistent and the sort's output arbitrary, and this is a frame's
        // *stable* answer or it is nothing.
        ranked.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(&right.0)));
        ranked.truncate(SHADOW_LIGHTS);

        // Incumbents keep the slot they had, so a tile's contents do not jump
        // between slots while both lights are still selected. Whatever is left
        // fills the free slots in rank order.
        for slot in &mut self.slots {
            if !slot.is_some_and(|index| ranked.iter().any(|(ranked, _)| *ranked == index)) {
                *slot = None;
            }
        }
        for (index, _) in ranked {
            if self.slots.contains(&Some(index)) {
                continue;
            }
            if let Some(free) = self.slots.iter_mut().find(|slot| slot.is_none()) {
                *free = Some(index);
            }
        }
    }

    /// Which light index holds each slot, in slot order.
    #[must_use]
    pub const fn slots(&self) -> &[Option<usize>; SHADOW_LIGHTS] {
        &self.slots
    }

    /// Which slot light `index` holds, if any.
    #[must_use]
    pub fn slot_of(&self, index: usize) -> Option<usize> {
        self.slots.iter().position(|slot| *slot == Some(index))
    }
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

    /// The atlas is a grid of equal tiles, and every tile has one of its own.
    ///
    /// **Two tiles sharing an origin is the failure this is here for**: the
    /// shadow pass sets a viewport per tile out of `tile_origin` and the shader
    /// addresses one out of the same arithmetic, so a collision is two maps
    /// written over each other and read as one — a picture, and a plausible one.
    #[test]
    fn every_tile_of_the_grid_has_an_origin_of_its_own() {
        let (width, height) = atlas_extent();
        assert_eq!(width, TILE * ATLAS_COLUMNS);
        assert_eq!(height, TILE * ATLAS_ROWS);
        let params = Cascades::params();
        #[expect(clippy::cast_precision_loss, reason = "an extent is a few thousand")]
        let expected = [1.0 / width as f32, 1.0 / height as f32];
        assert!((params[0] - expected[0]).abs() < f32::EPSILON);
        assert!((params[1] - expected[1]).abs() < f32::EPSILON);
        assert!(params[2] > 0.0 && params[3] > 0.0, "the bias must bias");

        let origins: Vec<(u32, u32)> = (0..TILES).map(tile_origin).collect();
        for (index, origin) in origins.iter().enumerate() {
            assert!(
                origin.0 + TILE <= width && origin.1 + TILE <= height,
                "tile {index} at {origin:?} runs off a {width}x{height} atlas"
            );
            assert_eq!(
                origins.iter().filter(|other| *other == origin).count(),
                1,
                "tile {index}'s origin {origin:?} is shared with another tile"
            );
        }

        // **The cascades are the first tiles and they are in the top row**,
        // which is the arrangement the sun's goldens were blessed under: cascade
        // `i` rasterises into exactly the texels it did before the grid gained a
        // second row, so a change to the grid's shape leaves them alone.
        for cascade in 0..CASCADES {
            assert_eq!(tile_origin(cascade), (TILE * cascade as u32, 0));
        }
        // And a light's slot is past every cascade's, which is the whole of the
        // split.
        for slot in 0..SHADOW_LIGHTS {
            assert!(light_tile(slot) >= CASCADES && light_tile(slot) < TILES);
        }
    }

    fn spot_at(position: Vec3, direction: Vec3) -> SpotLight {
        SpotLight {
            position,
            radius: 4.0,
            color: Vec3::ONE,
            direction,
            inner_angle: 0.2,
            outer_angle: 0.5,
        }
    }

    /// **Reversed-Z, stated as the only thing that distinguishes it**: a caster
    /// nearer the light gets the larger depth, the near plane is 1 and the
    /// radius is 0.
    ///
    /// The comparison sampler is `Greater` everywhere in this engine, so a
    /// matrix built the conventional way round would light exactly what it
    /// should darken — which reads as a bug in the cone rather than in the
    /// projection, and which no golden can tell from a scene with no occluder.
    #[test]
    fn a_caster_nearer_a_spot_gets_the_larger_depth() {
        let spot = spot_at(Vec3::new(0.0, 2.0, 0.0), Vec3::NEG_Y);
        let matrix = spot_matrix(&spot);
        let depth_at = |distance: f32| {
            let point = spot.position + Vec3::NEG_Y * distance;
            let clip = matrix * point.extend(1.0);
            (clip.xyz() / clip.w).z
        };
        let near = depth_at(0.5);
        let far = depth_at(3.0);
        assert!(
            near > far,
            "reversed-Z: a caster at 0.5 stores {near} and one at 3.0 stores {far}"
        );
        // And the two ends land where reversed-Z says: the near plane at 1 and
        // the radius at 0. An assertion about the *order* alone would pass on a
        // matrix whose range was 0.4..0.6, which is a map with almost no
        // precision in it.
        assert!(
            (depth_at(SPOT_NEAR) - 1.0).abs() < 1e-4,
            "the near plane must store 1.0, got {}",
            depth_at(SPOT_NEAR)
        );
        assert!(
            depth_at(spot.radius).abs() < 1e-4,
            "the radius must store 0.0, got {}",
            depth_at(spot.radius)
        );
        // Every depth in between is inside the range the comparison runs over.
        for step in 1..40u8 {
            let depth = depth_at(SPOT_NEAR + (spot.radius - SPOT_NEAR) * f32::from(step) / 40.0);
            assert!(
                (0.0..=1.0).contains(&depth),
                "a caster inside the cone stored {depth}, outside 0..1"
            );
        }
    }

    /// The map covers the cone and stops there: a point on the cone's outer edge
    /// is at the frustum's edge, and one well outside it is out of the box.
    ///
    /// A projection wider than the cone spends texels where the light has
    /// already fallen to zero; one narrower leaves a ring of the lit pool with
    /// no map behind it, which reads as a shadow that stops short of the pool's
    /// edge.
    #[test]
    fn a_spots_map_covers_its_cone_and_no_more() {
        let spot = spot_at(Vec3::new(1.0, 2.0, -1.0), Vec3::new(0.0, -1.0, 0.0));
        let matrix = spot_matrix(&spot);
        let ndc_of = |point: Vec3| {
            let clip = matrix * point.extend(1.0);
            clip.xyz() / clip.w
        };
        // A metre below the light, the cone's outer edge is `tan(outer)` out.
        let drop = 1.0;
        let edge = spot.outer_angle.tan() * drop;
        let on_edge = ndc_of(spot.position + Vec3::new(edge, -drop, 0.0));
        assert!(
            (on_edge.x.abs() - 1.0).abs() < 1e-3,
            "the cone's edge must land on the frustum's, got {on_edge:?}"
        );
        let inside = ndc_of(spot.position + Vec3::new(edge * 0.5, -drop, 0.0));
        assert!(inside.x.abs() < 1.0 && inside.y.abs() < 1.0, "{inside:?}");
        let outside = ndc_of(spot.position + Vec3::new(edge * 1.5, -drop, 0.0));
        assert!(outside.x.abs() > 1.0, "{outside:?}");
        // And behind the light is behind the light: the shader's `clip.w <= 0`
        // guard is what turns this into "lit" rather than a mirrored sample.
        let behind = matrix * (spot.position + Vec3::Y).extend(1.0);
        assert!(
            behind.w <= 0.0,
            "a point behind the spot has w {}",
            behind.w
        );
    }

    /// A spot pointing straight down is the commonest spot there is, and it is
    /// the one the obvious up vector is parallel to.
    #[test]
    fn a_spot_pointing_straight_down_still_produces_a_basis() {
        let matrix = spot_matrix(&spot_at(Vec3::new(0.0, 3.0, 0.0), Vec3::NEG_Y));
        assert!(
            matrix.to_cols_array().iter().all(|value| value.is_finite()),
            "a spot pointing down produced {matrix:?}"
        );
        let clip = matrix * Vec3::ZERO.extend(1.0);
        let ndc = clip.xyz() / clip.w;
        assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z));
    }

    /// **A still spot produces a byte-identical matrix**, which is the stability
    /// the cascades need a texel snap for and a spot gets for free.
    ///
    /// Asserted as equality rather than as "close", exactly as the cascades'
    /// snap is: shimmer is caused by a difference far below any tolerance a test
    /// would pick, so anything short of equality would pass on a matrix that
    /// moved every frame. The camera is not in the arithmetic at all, so this is
    /// a statement about `spot_matrix`'s inputs and nothing else.
    #[test]
    fn a_still_spot_produces_the_same_matrix() {
        let spot = spot_at(Vec3::new(0.5, 2.0, -1.5), Vec3::new(0.2, -1.0, 0.1));
        assert_eq!(spot_matrix(&spot), spot_matrix(&spot));
        // And a spot that *moved* does not, or the equality above would be
        // passing because the matrix ignores the light.
        let mut moved = spot;
        moved.position.x += 0.5;
        assert_ne!(spot_matrix(&spot), spot_matrix(&moved));
    }

    fn spot_light_at(position: Vec3, radius: f32) -> Light {
        Light::Spot(SpotLight {
            position,
            radius,
            color: Vec3::ONE,
            direction: Vec3::NEG_Y,
            inner_angle: 0.2,
            outer_angle: 0.5,
        })
    }

    /// The budget goes to the lights with the largest projected influence, and a
    /// light that misses out gets no slot at all.
    ///
    /// **Radius over distance, not distance alone**: a small light close to the
    /// eye and a large one further off are the pair that tells the two rules
    /// apart, and the list below is built so that the nearest light is *not* the
    /// most influential one.
    #[test]
    fn the_tiles_go_to_the_largest_projected_influence() {
        let eye = Vec3::ZERO;
        let lights = [
            // 0.5 / 1 = 0.5 — nearest, and the least influential.
            spot_light_at(Vec3::new(0.0, 0.0, -1.0), 0.5),
            // 8 / 4 = 2.0 — furthest, and the most.
            spot_light_at(Vec3::new(0.0, 0.0, -4.0), 8.0),
            // 2 / 2 = 1.0.
            spot_light_at(Vec3::new(0.0, 0.0, -2.0), 2.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, eye);
        assert_eq!(
            selection.slots(),
            &[Some(1), Some(2)],
            "the two largest influences take the tiles, in rank order"
        );
        assert_eq!(selection.slot_of(1), Some(0));
        assert_eq!(selection.slot_of(2), Some(1));
        assert_eq!(
            selection.slot_of(0),
            None,
            "the light past the budget gets no tile — it still lights, it just \
             does not occlude"
        );
    }

    /// Two lights of exactly equal influence break by index, so a frame's answer
    /// does not depend on the order a caller happened to build the list in.
    #[test]
    fn an_exact_tie_breaks_by_index() {
        let lights = [
            spot_light_at(Vec3::new(3.0, 0.0, 0.0), 1.0),
            spot_light_at(Vec3::new(-3.0, 0.0, 0.0), 1.0),
            spot_light_at(Vec3::new(0.0, 3.0, 0.0), 1.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, Vec3::ZERO);
        assert_eq!(selection.slots(), &[Some(0), Some(1)]);
    }

    /// **A light drifting across the cutoff must not flicker its shadow on and
    /// off**, which is the hysteresis and the reason it is owed.
    ///
    /// The incumbent is overtaken by a hair and keeps its tile; overtaken
    /// clearly, it loses it. Both halves, because a rule that never yields is
    /// not hysteresis — it is a first-come allocation, and the more important
    /// light would never get a map.
    #[test]
    fn an_incumbent_keeps_its_tile_until_a_challenger_clearly_beats_it() {
        // One slot's worth of contention: two lights and `SHADOW_LIGHTS` tiles
        // means nothing is contended, so the list is one longer than the budget.
        let contender = |influence: f32| spot_light_at(Vec3::new(0.0, 0.0, -1.0), influence);
        let mut lights = [contender(3.0), contender(2.0), contender(1.0)];
        let mut selection = Selection::default();
        selection.update(&lights, Vec3::ZERO);
        assert_eq!(selection.slots(), &[Some(0), Some(1)]);

        // The outsider edges past the incumbent by a few per cent. It does not
        // take the tile, and — this is the half that matters — the incumbent
        // stays in the *same* slot rather than being reshuffled.
        lights[2] = contender(2.1);
        selection.update(&lights, Vec3::ZERO);
        assert_eq!(
            selection.slots(),
            &[Some(0), Some(1)],
            "a challenger 5% ahead must not take a held tile"
        );

        // Clearly past it, and the tile changes hands.
        lights[2] = contender(2.0 * HOLD_RATIO + 0.1);
        selection.update(&lights, Vec3::ZERO);
        assert_eq!(
            selection.slots(),
            &[Some(0), Some(2)],
            "a challenger past the hold ratio must take the tile"
        );
    }

    /// A light this slice cannot map is refused a tile rather than given one it
    /// cannot fill — and the tile goes to the next light instead.
    ///
    /// Point lights are six tiles and a face selection, which is the next slice;
    /// a cone at or past a right angle has no perspective projection at all. Both
    /// still light.
    #[test]
    fn a_light_with_no_map_is_refused_a_tile_and_the_next_one_takes_it() {
        let mut wide = spot_light_at(Vec3::new(0.0, 0.0, -1.0), 10.0);
        if let Light::Spot(spot) = &mut wide {
            spot.outer_angle = MAX_SPOT_HALF_ANGLE + 0.01;
        }
        let lights = [
            Light::Point(crate::light::PointLight {
                position: Vec3::new(0.0, 0.0, -1.0),
                radius: 20.0,
                color: Vec3::ONE,
            }),
            wide,
            spot_light_at(Vec3::new(0.0, 0.0, -2.0), 1.0),
            spot_light_at(Vec3::new(0.0, 0.0, -3.0), 1.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, Vec3::ZERO);
        assert_eq!(
            selection.slots(),
            &[Some(2), Some(3)],
            "the two brightest are ineligible, so the tiles go to the two that are"
        );
    }

    /// An incumbent that leaves the list — or stops being eligible — frees its
    /// tile rather than holding an index into a list that no longer has it.
    ///
    /// The list is rebuilt every frame, so an index is only meaningful against
    /// the list it came from. Carrying one across a shorter list is how a slot
    /// ends up naming a light that is not there.
    #[test]
    fn an_incumbent_that_leaves_the_list_frees_its_tile() {
        let lights = [
            spot_light_at(Vec3::new(0.0, 0.0, -1.0), 1.0),
            spot_light_at(Vec3::new(0.0, 0.0, -2.0), 1.0),
        ];
        let mut selection = Selection::default();
        selection.update(&lights, Vec3::ZERO);
        assert_eq!(selection.slots(), &[Some(0), Some(1)]);

        selection.update(&lights[..1], Vec3::ZERO);
        assert_eq!(selection.slots(), &[Some(0), None]);

        selection.update(&[], Vec3::ZERO);
        assert_eq!(selection.slots(), &[None; SHADOW_LIGHTS]);
    }
}
