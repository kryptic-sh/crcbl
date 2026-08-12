//! The lights a frame carries, and the rows they become.
//!
//! ```text
//!  DirectionalLight ──┐
//!  Light::Point    ───┼──▶ [GpuLight] ──▶ light_cluster.slang ──▶ froxel grid
//!  Light::Spot     ───┘                                              │
//!                                            mesh.slang's fragment ◀─┘
//! ```
//!
//! `docs/plan/18-render-features.md`'s "Many lights": a light is a row in a
//! storage buffer, the sun included. The sun keeps a type of its own on this
//! side — it is the light that owns the ambient term and the shadow cascades,
//! and [`ForwardRenderer::begin_frame`] has always taken one — but it stops
//! being a special case where it mattered, which is in the shader.
//!
//! [`ForwardRenderer::begin_frame`]: crate::forward::ForwardRenderer::begin_frame

use crcbl_shaders::light::{self, GpuLight};
use glam::Vec3;

use crate::camera::DirectionalLight;

/// A light that lives somewhere, as distinct from the sun, which does not.
///
/// Two variants and one row: the shader loop shades all three kinds with one
/// BRDF, which is `docs/plan/18-render-features.md`'s "one material model, one
/// BRDF, one set of inputs" holding by construction rather than by three copies
/// of it agreeing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Light {
    /// Radiates equally in every direction, out to its radius.
    Point(PointLight),
    /// A point light with a cone over it.
    Spot(SpotLight),
}

/// A light radiating equally in every direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    /// Where it is, in render space.
    pub position: Vec3,
    /// How far its influence reaches, in world units.
    ///
    /// **A hard bound, not a fade cut short.** The shader's falloff reaches
    /// exactly zero here, and the clustering pass culls against this same
    /// number — so a light is either in a froxel's list or contributing nothing
    /// to it, and there is no radius at which the two disagree and leave a seam.
    /// Larger costs more froxels, not a softer edge.
    pub radius: f32,
    /// Colour premultiplied by intensity. **May exceed 1.0**, like the sun's:
    /// the scene target is `Rgba16Float` precisely so that it can.
    pub color: Vec3,
}

/// A point light with a cone over it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotLight {
    /// Where it is, in render space.
    pub position: Vec3,
    /// How far its influence reaches, on [`PointLight::radius`]'s terms exactly.
    pub radius: f32,
    /// Colour premultiplied by intensity.
    pub color: Vec3,
    /// The direction the cone points **along**, away from the light.
    ///
    /// The opposite convention from [`DirectionalLight::direction`], and
    /// deliberately: a spot points *at* something and a sun comes *from*
    /// somewhere, so each field is the vector a caller has. The shader is what
    /// reconciles them, and its `spot_cone` says where the negation is.
    ///
    /// Normalised on the way into the row.
    pub direction: Vec3,
    /// Half-angle of the cone's bright core, in radians. Full brightness inside.
    pub inner_angle: f32,
    /// Half-angle at which the cone closes, in radians. Dark outside.
    ///
    /// Must be at least [`inner_angle`](Self::inner_angle); [`Light::row`]
    /// widens it to that if it is not, rather than emitting a row whose two
    /// cosines the shader would divide the wrong way round.
    pub outer_angle: f32,
}

impl Light {
    /// This light as the row the shaders read, occluding through the light tiles
    /// starting at `base_tile`.
    ///
    /// **The first tile, not the only one**: a spot occludes through that tile
    /// alone and a point light through the
    /// [`POINT_FACES`](crate::shadow::POINT_FACES) tiles from there, one per cube
    /// face, which the shader selects between. `shadow::Selection` is what hands
    /// the run out.
    ///
    /// `None` is a light with no map of its own, which is the ordinary case: the
    /// atlas holds [`shadow::LIGHT_TILES`](crate::shadow::LIGHT_TILES) light
    /// tiles and a scene may want more than they hold. Such a light still lights
    /// and simply does not occlude — `docs/plan/18-render-features.md`'s honest
    /// degradation, and the reason the budget is a quality knob rather than a
    /// correctness cliff.
    #[must_use]
    pub fn row(&self, base_tile: Option<usize>) -> GpuLight {
        let shadow_slot = base_tile.map_or(light::NO_SHADOW_SLOT, |base| {
            u32::try_from(base).unwrap_or(light::NO_SHADOW_SLOT)
        });
        match self {
            Self::Point(point) => GpuLight {
                position: point.position.extend(point.radius).to_array(),
                color: point.color.extend(0.0).to_array(),
                direction: [0.0; 4],
                kind: light::KIND_POINT,
                cos_inner: 0.0,
                shadow_slot,
                pad1: 0,
            },
            Self::Spot(spot) => {
                // **Widened rather than trusted.** The shader divides by
                // `cos_inner - cos_outer` and a caller who wrote the two angles
                // the wrong way round would get a lit cone exterior — a picture,
                // not an error. Clamping here is the one place that can be
                // enforced for every backend at once.
                let outer = spot.outer_angle.max(spot.inner_angle);
                GpuLight {
                    position: spot.position.extend(spot.radius).to_array(),
                    color: spot.color.extend(0.0).to_array(),
                    direction: spot
                        .direction
                        .normalize_or_zero()
                        .extend(outer.cos())
                        .to_array(),
                    kind: light::KIND_SPOT,
                    cos_inner: spot.inner_angle.cos(),
                    shadow_slot,
                    pad1: 0,
                }
            }
        }
    }

    /// Where this light is and how far it reaches, for a caller that has to
    /// bound it — the clustering pass's test, on the host.
    #[must_use]
    pub const fn sphere(&self) -> (Vec3, f32) {
        match self {
            Self::Point(point) => (point.position, point.radius),
            Self::Spot(spot) => (spot.position, spot.radius),
        }
    }
}

/// The sun as a row: `docs/plan/18-render-features.md`'s "a directional light is
/// a row too, flagged as affecting every cluster".
///
/// The direction is normalised here and nowhere else, which is what lets
/// `mesh.slang`'s `normalize` of it be the same arithmetic the single-light form
/// performed on `FrameUniforms::light_direction` — the whole reason the goldens
/// can be expected not to move across the conversion.
#[must_use]
pub fn sun_row(sun: &DirectionalLight) -> GpuLight {
    GpuLight {
        // No position and no radius: a directional light has neither, and the
        // clustering pass never asks — it puts every directional row in every
        // froxel before it looks at anything else.
        position: [0.0; 4],
        color: sun.color.extend(0.0).to_array(),
        direction: sun.direction.normalize_or_zero().extend(0.0).to_array(),
        kind: light::KIND_DIRECTIONAL,
        cos_inner: 0.0,
        // **The sun occludes through the cascades, not through a light slot.**
        // A directional row naming a slot would sample a spot's map with the
        // sun's geometry, so it names none — and `mesh.slang` reaches the
        // cascades by `kind` rather than by this field, which is why the two
        // cannot be confused.
        shadow_slot: light::NO_SHADOW_SLOT,
        pad1: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sun's row carries the same normalised direction and the same colour
    /// the frame block used to, which is the whole of the conversion's contract.
    #[test]
    fn the_sun_s_row_is_the_frame_block_s_two_fields() {
        let sun = DirectionalLight::default();
        let row = sun_row(&sun);
        assert_eq!(row.kind, light::KIND_DIRECTIONAL);
        let normalised = sun.direction.normalize_or_zero();
        assert_eq!(
            [row.direction[0], row.direction[1], row.direction[2]],
            normalised.to_array(),
            "the row must carry the same bits the uniform block carried, or the \
             shader's `normalize` is not the same arithmetic"
        );
        assert_eq!(
            [row.color[0], row.color[1], row.color[2]],
            sun.color.to_array()
        );
        assert_eq!(
            row.position, [0.0; 4],
            "a directional light has no position and no radius"
        );
    }

    #[test]
    fn a_point_light_carries_its_radius_in_the_position_s_w() {
        let row = Light::Point(PointLight {
            position: Vec3::new(1.0, 2.0, 3.0),
            radius: 7.0,
            color: Vec3::new(0.25, 0.5, 0.75),
        })
        .row(None);
        assert_eq!(row.kind, light::KIND_POINT);
        assert_eq!(row.position, [1.0, 2.0, 3.0, 7.0]);
        assert_eq!(row.color, [0.25, 0.5, 0.75, 0.0]);
    }

    /// A caller who swapped the two angles gets a cone that is merely hard-edged
    /// rather than one lit on the outside.
    #[test]
    fn a_spot_with_its_angles_the_wrong_way_round_is_widened_not_inverted() {
        let spot = SpotLight {
            position: Vec3::ZERO,
            radius: 5.0,
            color: Vec3::ONE,
            direction: Vec3::new(0.0, -2.0, 0.0),
            inner_angle: 0.6,
            outer_angle: 0.2,
        };
        let row = Light::Spot(spot).row(None);
        assert_eq!(row.kind, light::KIND_SPOT);
        assert!(
            row.cos_inner >= row.direction[3],
            "the inner cosine must not be smaller than the outer one, or the \
             shader divides by a negative and lights the outside of the cone: \
             inner {} outer {}",
            row.cos_inner,
            row.direction[3]
        );
        assert_eq!(
            [row.direction[0], row.direction[1], row.direction[2]],
            [0.0, -1.0, 0.0],
            "the cone axis is normalised on the way in"
        );
    }

    #[test]
    fn a_spot_the_right_way_round_keeps_both_of_its_angles() {
        let row = Light::Spot(SpotLight {
            position: Vec3::ZERO,
            radius: 5.0,
            color: Vec3::ONE,
            direction: -Vec3::Y,
            inner_angle: 0.2,
            outer_angle: 0.6,
        })
        .row(None);
        assert!((row.cos_inner - 0.2_f32.cos()).abs() < 1e-6);
        assert!((row.direction[3] - 0.6_f32.cos()).abs() < 1e-6);
    }
}
