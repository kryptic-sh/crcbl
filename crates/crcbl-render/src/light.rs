//! The lights a frame carries, and the rows they become.
//!
//! ```text
//!  DirectionalLight ──┐
//!  Light::Point    ───┤
//!  Light::Spot     ───┼──▶ [GpuLight] ──▶ light_cluster.slang ──▶ froxel grid
//!  Light::Rect     ───┘                                              │
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
/// Three variants and one row: the shader loop shades all four kinds with one
/// BRDF, which is `docs/plan/18-render-features.md`'s "one material model, one
/// BRDF, one set of inputs" holding by construction rather than by four copies
/// of it agreeing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Light {
    /// Radiates equally in every direction, out to its radius.
    Point(PointLight),
    /// A point light with a cone over it.
    Spot(SpotLight),
    /// A rectangle that radiates from one of its faces.
    Rect(RectLight),
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
    /// Whether this is a **fill** light: one that lights but casts no shadow and
    /// adds no specular.
    ///
    /// `docs/plan/44-lighting.md`'s rung 5 asked for it beside the area lights.
    /// It is how a stack with no baked bounce lights the far end of a room
    /// without paying for a shadow map or leaving a highlight where no fixture
    /// is. `crcbl_shaders::light::FLAG_FILL` is the row's bit;
    /// [`shadow::Selection`](crate::shadow::Selection) refuses such a light a
    /// tile and `mesh.slang` refuses it a lobe.
    ///
    /// **Every kind carries it**, on this field's terms exactly, and
    /// [`Light::is_fill`] is where one question is asked of the three.
    pub fill: bool,
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
    /// Whether this is a **fill** light, on [`PointLight::fill`]'s terms
    /// exactly: it lights, casts no shadow and adds no specular.
    pub fill: bool,
}

/// A rectangle that radiates from one of its faces — `docs/plan/44-lighting.md`'s
/// rung 5, shaded through the linearly transformed cosine fit in
/// [`crcbl_shaders::ltc`].
///
/// # What a rectangle is here
///
/// A centre, a plane, two half-extents and a side. The plane is
/// [`direction`](Self::direction) — the way the panel faces, so a surface behind
/// it is not lit at all — and [`tangent`](Self::tangent), which names the `u`
/// axis inside it; the `v` axis is their cross product and is not stored.
/// Neither vector has to arrive unit or perpendicular: [`Light::row`] normalises
/// and orthogonalises them on the way into the row, because a row that broke
/// either would be a rectangle that is not one and no picture would say so.
///
/// # The winding, which is what makes it one-sided
///
/// `v` is `cross(tangent, direction)`, and the shader walks the corners
/// `-u-v, +u-v, +u+v, -u+v`. That order makes the spherical polygon's integral
/// **positive** for a receiver on the side [`direction`](Self::direction) points
/// at and negative behind it, and the shader takes the positive half. So a panel
/// lights the room it faces and nothing on the other side of its own wall —
/// which is what a window is, and what saves the far half of a scene from being
/// lit through the floor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectLight {
    /// The centre of the rectangle, in render space.
    pub position: Vec3,
    /// How far its influence reaches **from that centre**, in world units.
    ///
    /// [`PointLight::radius`]'s terms, and the same hard bound: the shading
    /// window reaches exactly zero here and `light_cluster.slang` culls against
    /// this same sphere, so there is no radius at which the two disagree.
    /// Measured from the centre rather than from the nearest corner, which is
    /// what makes those two the same number — so a large panel wants a radius
    /// comfortably past its own half-diagonal or it will fade out before its own
    /// edge does.
    pub radius: f32,
    /// Colour premultiplied by intensity — the radiance leaving the face.
    ///
    /// **A radiance rather than a power**, which is what makes a rectangle
    /// behave: doubling its extent doubles the light in the room, because it is
    /// twice the emitter. A caller holding a lumen figure divides by the area.
    pub color: Vec3,
    /// The direction the face radiates along, away from the panel.
    ///
    /// [`SpotLight::direction`]'s convention — a fixture points *at* something —
    /// and normalised on the way into the row.
    pub direction: Vec3,
    /// The `u` axis of the rectangle's plane, which fixes how it is turned about
    /// its own normal.
    ///
    /// Orthogonalised against [`direction`](Self::direction) and normalised on
    /// the way into the row, so a caller may hand over any vector that is not
    /// parallel to the normal — a world axis, usually. One exactly parallel to
    /// it leaves the rectangle without a plane, and [`Light::row`] picks a
    /// perpendicular rather than emitting a row of nothings.
    pub tangent: Vec3,
    /// Half the rectangle's extent along `u`, in world units.
    pub half_width: f32,
    /// Half its extent along `v`, which is `cross(tangent, direction)`.
    pub half_height: f32,
    /// Whether this is a **fill** light, on [`PointLight::fill`]'s terms
    /// exactly: it lights, casts no shadow and adds no specular.
    ///
    /// A rectangle is refused a shadow map for a second reason as well — it
    /// radiates from a surface, so there is no centre of projection to render
    /// one from — and on this kind the flag is the specular half alone.
    pub fill: bool,
}

impl RectLight {
    /// The rectangle's plane, as a unit normal and a unit `u` axis inside it.
    ///
    /// The whole of the orthogonalisation the row needs, in one place because
    /// [`Light::row`] and [`Light::sphere`] would otherwise each have an opinion
    /// about a degenerate rectangle. A normal that is not a direction at all
    /// becomes `+Z`, and a tangent parallel to the normal becomes whichever
    /// world axis is least like it — both of which are arbitrary, and both of
    /// which are a rectangle rather than a row of `NaN`s.
    #[must_use]
    pub fn frame(&self) -> (Vec3, Vec3) {
        let normal = self.direction.normalize_or_zero();
        let normal = if normal.length_squared() > 0.0 {
            normal
        } else {
            Vec3::Z
        };
        let across = self.tangent - normal * self.tangent.dot(normal);
        let tangent = across.normalize_or_zero();
        if tangent.length_squared() > 0.0 {
            return (normal, tangent);
        }
        // The tangent was parallel to the normal, so it names no plane. Any
        // perpendicular will do — the rectangle is then turned arbitrarily about
        // its own axis, which is the only thing the caller left unsaid.
        let seed = if normal.z.abs() < 0.9 {
            Vec3::Z
        } else {
            Vec3::X
        };
        (normal, seed.cross(normal).normalize())
    }
}

impl Light {
    /// Whether this light is a **fill** light: one that lights, casts no shadow
    /// and adds no specular.
    ///
    /// A method on the enum rather than a match at each caller, so
    /// [`shadow::Selection`](crate::shadow::Selection) asks one question of a
    /// light rather than three. Every kind carries the field and
    /// [`PointLight::fill`] is where it is described.
    #[must_use]
    pub const fn is_fill(&self) -> bool {
        match self {
            Self::Point(point) => point.fill,
            Self::Spot(spot) => spot.fill,
            Self::Rect(rect) => rect.fill,
        }
    }

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
        let shadow_tile = base_tile.map_or(light::NO_SHADOW_TILE, |base| {
            u32::try_from(base).unwrap_or(light::NO_SHADOW_TILE)
        });
        // One reading of the flag for all three kinds, rather than an arm that
        // could be the one to forget it: `is_fill` is the same question
        // `shadow::Selection` asked before it refused this light a tile.
        let flags = if self.is_fill() { light::FLAG_FILL } else { 0 };
        match self {
            Self::Point(point) => GpuLight {
                position: point.position.extend(point.radius).to_array(),
                color: point.color.extend(0.0).to_array(),
                direction: [0.0; 4],
                tangent: [0.0; 4],
                kind: light::KIND_POINT,
                cos_inner: 0.0,
                shadow_tile,
                flags,
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
                    tangent: [0.0; 4],
                    kind: light::KIND_SPOT,
                    cos_inner: spot.inner_angle.cos(),
                    shadow_tile,
                    flags,
                }
            }
            Self::Rect(rect) => {
                let (normal, tangent) = rect.frame();
                GpuLight {
                    position: rect.position.extend(rect.radius).to_array(),
                    color: rect.color.extend(0.0).to_array(),
                    // The `w`s are the two half-extents, each beside the axis it
                    // is measured along — `crcbl_shaders::light::KIND_RECT` is
                    // the map.
                    direction: normal.extend(rect.half_height).to_array(),
                    tangent: tangent.extend(rect.half_width).to_array(),
                    kind: light::KIND_RECT,
                    cos_inner: 0.0,
                    shadow_tile,
                    flags,
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
            // The rectangle's own centre and reach, and deliberately not a
            // sphere grown to hold its corners: `light_cluster.slang` culls
            // against exactly the radius `mesh.slang`'s window goes to zero at,
            // and a bound that disagreed with the window is the seam
            // `RectLight::radius` exists to rule out.
            Self::Rect(rect) => (rect.position, rect.radius),
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
        tangent: [0.0; 4],
        kind: light::KIND_DIRECTIONAL,
        cos_inner: 0.0,
        // **The sun occludes through the cascades, not through a light tile.**
        // A directional row naming a tile would sample a spot's map with the
        // sun's geometry, so it names none — and `mesh.slang` reaches the
        // cascades by `kind` rather than by this field, which is why the two
        // cannot be confused.
        shadow_tile: light::NO_SHADOW_TILE,
        flags: 0,
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
            fill: false,
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
            fill: false,
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
            fill: false,
        })
        .row(None);
        assert!((row.cos_inner - 0.2_f32.cos()).abs() < 1e-6);
        assert!((row.direction[3] - 0.6_f32.cos()).abs() < 1e-6);
    }

    /// **Every kind's row carries the fill bit**, which is the half of the flag
    /// `mesh.slang` enforces — it drops the specular lobe on `FLAG_FILL` and
    /// asks nothing about the kind.
    ///
    /// Each light is written twice, filled and not, so a row that set the bit
    /// unconditionally fails here rather than reading as a light that is always
    /// matte.
    #[test]
    fn a_fill_light_of_any_kind_carries_the_flag_into_its_row() {
        let point = PointLight {
            position: Vec3::ZERO,
            radius: 5.0,
            color: Vec3::ONE,
            fill: false,
        };
        let spot = SpotLight {
            position: Vec3::ZERO,
            radius: 5.0,
            color: Vec3::ONE,
            direction: -Vec3::Y,
            inner_angle: 0.2,
            outer_angle: 0.6,
            fill: false,
        };
        let rect = RectLight {
            position: Vec3::ZERO,
            radius: 5.0,
            color: Vec3::ONE,
            direction: -Vec3::Y,
            tangent: Vec3::X,
            half_width: 0.5,
            half_height: 0.25,
            fill: false,
        };
        for (kind, light) in [
            ("point", Light::Point(point)),
            ("spot", Light::Spot(spot)),
            ("rect", Light::Rect(rect)),
        ] {
            assert_eq!(
                light.row(None).flags,
                0,
                "an ordinary {kind} light's row must carry no flags"
            );
            let filled = match light {
                Light::Point(point) => Light::Point(PointLight {
                    fill: true,
                    ..point
                }),
                Light::Spot(spot) => Light::Spot(SpotLight { fill: true, ..spot }),
                Light::Rect(rect) => Light::Rect(RectLight { fill: true, ..rect }),
            };
            assert!(filled.is_fill(), "a filled {kind} light must answer yes");
            assert_eq!(
                filled.row(None).flags,
                light::FLAG_FILL,
                "a fill {kind} light's row must carry the bit `mesh.slang` drops \
                 the specular on"
            );
        }
    }
}
